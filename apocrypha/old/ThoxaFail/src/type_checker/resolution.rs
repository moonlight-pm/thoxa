use crate::ast::{Expr, Parameter};
use crate::types::{Type, TypeEnv};
use crate::symbols::{SymbolTable, ImportedBinding};

/// Resolve a type expression to an actual Type
pub fn resolve_type_expr(expr: &Expr, env: &TypeEnv, symbol_table: &SymbolTable) -> Result<Type, String> {
    match expr {
        Expr::Void => Ok(Type::Void),
        Expr::Identifier(name) => {
            // First check the type environment for types
            if let Some(ty) = env.lookup_type(name) {
                return Ok(ty);
            }
            
            // Then check for type variables (like Type itself)
            if let Some(ty) = env.lookup_var(name) {
                return Ok(ty);
            }
            
            // Then check if it's an imported type
            if let Some(import) = symbol_table.get_import(name) {
                match import {
                    ImportedBinding::DestructuredMember { member_info, .. } => {
                        // This is an imported type value (e.g., Int64 from std/Types)
                        match member_info {
                            crate::symbols::TypeMemberInfo::LetBinding {  value, .. } => {
                                // For type imports, the value IS the type
                                match value {
                                    Expr::IntType { signed, bits } => Ok(Type::Int { signed: *signed, bits: *bits }),
                                    Expr::Identifier(s) if s == "String" => Ok(Type::String),
                                    Expr::Identifier(s) if s == "Void" => Ok(Type::Void),
                                    _ => {
                                        // For type aliases, try to resolve them
                                        resolve_type_expr(value, env, symbol_table)
                                    }
                                }
                            }
                            crate::symbols::TypeMemberInfo::FunctionDef { .. } => {
                                Err(format!("'{}' is a function, not a type", name))
                            }
                        }
                    }
                    _ => Err(format!("'{}' is not a type", name))
                }
            } else {
                Err(format!("Unknown type: {}", name))
            }
        }
        Expr::IntType { signed, bits } => {
            Ok(Type::Int { signed: *signed, bits: *bits })
        }
        _ => Err(format!("Invalid type expression: {:?}", expr))
    }
}

/// Resolve a member type expression
pub fn resolve_member_type(expr: &Expr, env: &TypeEnv, symbol_table: &SymbolTable) -> Result<Type, String> {
    // For now, use the same logic as resolve_type_expr
    // In the future, this might handle more complex member types
    resolve_type_expr(expr, env, symbol_table)
}

/// Build a function type from parameters
#[allow(dead_code)]
pub fn build_function_type(
    params: &[Parameter], 
    return_type: &Type,
    env: &TypeEnv,
    symbol_table: &SymbolTable
) -> Result<Type, String> {
    let mut regular_params = Vec::new();
    let mut variadic_type = None;
    
    // Process parameters - variadic must be last
    let mut found_variadic = false;
    for param in params {
        let param_type = resolve_type_expr(&param.type_expr, env, symbol_table)?;
        
        if param.variadic {
            if found_variadic {
                return Err("Only one variadic parameter allowed".to_string());
            }
            found_variadic = true;
            variadic_type = Some(param_type);
        } else {
            if found_variadic {
                return Err("Variadic parameter must be last".to_string());
            }
            regular_params.push(param_type);
        }
    }
    
    Ok(Type::Function {
        params: regular_params,
        return_type: Box::new(return_type.clone()),
        variadic: variadic_type.map(Box::new),
    })
}

/// Check if two types are compatible (can be assigned)
pub fn types_compatible(from: &Type, to: &Type) -> bool {
    match (from, to) {
        // Same types are always compatible
        (a, b) if a == b => true,
        
        // Any type can accept any value
        (_, Type::Any) => true,
        
        // Any type can be assigned to any type (but with runtime checking)
        (Type::Any, _) => true,
        
        // Unknown type is compatible with anything (for error recovery)
        (Type::Unknown, _) | (_, Type::Unknown) => true,
        
        // Type variables are compatible with anything (for future inference)
        (Type::TypeVar(_), _) | (_, Type::TypeVar(_)) => true,
        
        // Integer coercion rules: allow widening conversions
        (Type::Int { signed: from_signed, bits: from_bits }, 
         Type::Int { signed: to_signed, bits: to_bits }) => {
            // Can't convert between signed and unsigned
            if from_signed != to_signed {
                return false;
            }
            
            // Allow widening conversions (smaller to larger)
            from_bits <= to_bits
        }
        
        // No other conversions are allowed
        _ => false,
    }
}