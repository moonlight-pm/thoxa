use crate::ast::{Expr, TypedExpr, BinaryOp};
use crate::types::{Type, TypeEnv};
use crate::symbols::SymbolTable;
use super::resolution::{resolve_type_expr, types_compatible};

/// Type check basic expressions that don't require complex logic or sub-modules
/// Returns None if the expression doesn't match any of the basic cases
pub fn type_check_basic_expr(
    expr: &Expr,
    env: &mut TypeEnv,
    symbols: &SymbolTable,
    type_check_expr: &dyn Fn(&Expr, &mut TypeEnv, &SymbolTable) -> Result<TypedExpr, String>
) -> Option<Result<TypedExpr, String>> {
    match expr {
        Expr::Number(value) => {
            // Numbers default to Int64
            Some(Ok(TypedExpr::Number {
                value: *value,
                ty: Type::int(64),
            }))
        }
        
        Expr::StringLiteral(value) => {
            Some(Ok(TypedExpr::StringLiteral {
                value: value.clone(),
                ty: Type::String,
            }))
        }
        
        Expr::Void => {
            // For now, use Untyped fallback for Void
            Some(Ok(TypedExpr::Untyped {
                expr: expr.clone(),
                ty: Type::Void,
            }))
        }
        
        Expr::Identifier(name) => {
            // Check if this is an import that needs special handling
            if symbols.get_import(name).is_some() {
                // Let the main handler deal with imports
                None
            } else if let Some(ty) = env.lookup_var(name) {
                Some(Ok(TypedExpr::Identifier {
                    name: name.clone(),
                    ty,
                }))
            } else if let Some(ty) = env.lookup_type(name) {
                Some(Ok(TypedExpr::Identifier {
                    name: name.clone(),
                    ty,
                }))
            } else if let Some(ty) = env.lookup_func(name) {
                Some(Ok(TypedExpr::Identifier {
                    name: name.clone(),
                    ty,
                }))
            } else {
                Some(Err(format!("Unknown identifier: {}", name)))
            }
        }
        
        Expr::IntType { signed: _, bits: _ } => {
            // The expression itself is of type Type
            Some(Ok(TypedExpr::Untyped {
                expr: expr.clone(),
                ty: Type::Type,
            }))
        }
        
        Expr::Binary { left, op, right } => {
            let typed_left = match type_check_expr(left, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            let typed_right = match type_check_expr(right, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Get types from the typed expressions
            let left_ty = typed_left.get_type();
            let right_ty = typed_right.get_type();
            
            // For now, assume all binary operations work on Int64 and return Int64
            // TODO: Proper type checking for binary operations
            match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    if !left_ty.is_numeric() || !right_ty.is_numeric() {
                        return Some(Err(format!(
                            "Binary operation {:?} requires numeric operands, got {} and {}",
                            op, left_ty.name(), right_ty.name()
                        )));
                    }
                    Some(Ok(TypedExpr::Binary {
                        left: Box::new(typed_left),
                        op: op.clone(),
                        right: Box::new(typed_right),
                        ty: Type::int(64),
                    }))
                }
                BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Less | 
                BinaryOp::Greater | BinaryOp::LessEq | BinaryOp::GreaterEq => {
                    // Comparison operations return Int64 (0 or 1)
                    Some(Ok(TypedExpr::Binary {
                        left: Box::new(typed_left),
                        op: op.clone(),
                        right: Box::new(typed_right),
                        ty: Type::int(64),
                    }))
                }
            }
        }
        
        Expr::Sequence(exprs) => {
            if exprs.is_empty() {
                Some(Ok(TypedExpr::Sequence {
                    exprs: vec![],
                    ty: Type::Void,
                }))
            } else {
                // Type check all expressions, return type of last
                let mut typed_exprs = Vec::new();
                let mut last_type = Type::Void;
                for e in exprs {
                    let typed = match type_check_expr(e, env, symbols) {
                        Ok(typed) => typed,
                        Err(e) => return Some(Err(e)),
                    };
                    last_type = typed.get_type().clone();
                    typed_exprs.push(typed);
                }
                Some(Ok(TypedExpr::Sequence {
                    exprs: typed_exprs,
                    ty: last_type,
                }))
            }
        }
        
        Expr::Block(exprs) => {
            // Create a new scope for the block
            let mut block_env = TypeEnv::child(env.clone());
            
            if exprs.is_empty() {
                Some(Ok(TypedExpr::Block {
                    exprs: vec![],
                    ty: Type::Void,
                }))
            } else {
                let mut typed_exprs = Vec::new();
                let mut last_type = Type::Void;
                for e in exprs {
                    let typed = match type_check_expr(e, &mut block_env, symbols) {
                        Ok(typed) => typed,
                        Err(e) => return Some(Err(e)),
                    };
                    last_type = typed.get_type().clone();
                    typed_exprs.push(typed);
                }
                Some(Ok(TypedExpr::Block {
                    exprs: typed_exprs,
                    ty: last_type,
                }))
            }
        }
        
        Expr::LetBinding { name, type_expr, value, mutable } => {
            // Resolve the actual type from the type expression
            let actual_type = match resolve_type_expr(type_expr, env, symbols) {
                Ok(ty) => ty,
                Err(e) => return Some(Err(e)),
            };
            
            // Type check the value
            let value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Special case: automatic module instantiation
            // If the value is Void and the type is a struct, this is automatic instantiation
            let final_value_typed = if matches!(value_typed.get_type(), Type::Void) && matches!(&actual_type, Type::Struct { .. }) {
                // Create an identifier that will be resolved to the module instance at codegen time
                TypedExpr::Identifier {
                    name: actual_type.name(),
                    ty: actual_type.clone(),
                }
            } else {
                // Normal case: check type compatibility
                if !types_compatible(&value_typed.get_type(), &actual_type) {
                    return Some(Err(format!(
                        "Type mismatch in let binding '{}': expected {}, got {}",
                        name, actual_type.name(), value_typed.get_type().name()
                    )));
                }
                value_typed
            };
            
            // Add variable to environment
            env.insert_var(name.clone(), actual_type.clone());
            
            // Let bindings evaluate to Void
            Some(Ok(TypedExpr::LetBinding {
                name: name.clone(),
                type_expr: type_expr.clone(),
                value: Box::new(final_value_typed),
                mutable: *mutable,
                ty: Type::Void,
            }))
        }
        
        Expr::Assignment { name, value } => {
            // Look up variable type
            let var_type = if let Some(ty) = env.lookup_var(name) {
                ty
            } else {
                return Some(Err(format!("Unknown variable: {}", name)));
            };
            
            // Type check the value
            let value_typed = match type_check_expr(value, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Check type compatibility
            if !types_compatible(&value_typed.get_type(), &var_type) {
                return Some(Err(format!(
                    "Type mismatch in assignment to '{}': expected {}, got {}",
                    name, var_type.name(), value_typed.get_type().name()
                )));
            }
            
            // Assignments evaluate to Void
            Some(Ok(TypedExpr::Assignment {
                name: name.clone(),
                value: Box::new(value_typed),
                ty: Type::Void,
            }))
        }
        
        Expr::If { condition, then_expr, else_expr } => {
            // Type check condition
            let cond_typed = match type_check_expr(condition, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Type check branches
            let then_typed = match type_check_expr(then_expr, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            let else_typed = if let Some(else_expr) = else_expr {
                match type_check_expr(else_expr, env, symbols) {
                    Ok(typed) => Some(Box::new(typed)),
                    Err(e) => return Some(Err(e)),
                }
            } else {
                None
            };
            
            // Determine result type
            let result_type = if let Some(ref _else_typed) = else_typed {
                // TODO: Check that branch types are compatible
                // For now, use the then branch type
                then_typed.get_type().clone()
            } else {
                // If without else always returns Void
                Type::Void
            };
            
            Some(Ok(TypedExpr::If {
                condition: Box::new(cond_typed),
                then_expr: Box::new(then_typed),
                else_expr: else_typed,
                ty: result_type,
            }))
        }
        
        Expr::Loop { body } => {
            // Type check body
            let _body_typed = match type_check_expr(body, env, symbols) {
                Ok(typed) => typed,
                Err(e) => return Some(Err(e)),
            };
            
            // Loops that don't break with a value return Void
            // TODO: Track break values to determine actual return type
            Some(Ok(TypedExpr::Untyped {
                expr: expr.clone(),
                ty: Type::Void,
            }))
        }
        
        Expr::Break { value } => {
            // Type check value if present
            let break_type = if let Some(value) = value {
                match type_check_expr(value, env, symbols) {
                    Ok(typed) => typed.get_type().clone(),
                    Err(e) => return Some(Err(e)),
                }
            } else {
                Type::Void
            };
            
            // Break itself doesn't return (but affects loop type)
            Some(Ok(TypedExpr::Untyped {
                expr: expr.clone(),
                ty: break_type,
            }))
        }
        
        Expr::Continue => {
            // Continue doesn't return
            Some(Ok(TypedExpr::Continue {
                ty: Type::Void,
            }))
        }
        
        // Return None for expressions that are not basic and need special handling
        _ => None,
    }
}