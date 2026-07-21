use crate::ast::{Expr, TypeMember};
use crate::symbols::{SymbolTable, ValueCategory};

/// Recursively walk the AST and populate the symbol table
pub fn populate_symbol_table(expr: &Expr, symbol_table: &mut SymbolTable) -> Result<(), String> {
    match expr {
        Expr::LetBinding { name, type_expr, value, mutable } => {
            // For now, all let bindings are stack values (local variables)
            // This will be refined when we add imports and global constants
            symbol_table.register_let_binding(name, (**type_expr).clone(), (**value).clone(), ValueCategory::Stack)?;
            
            // TODO: Track mutability in the symbol table
            let _ = mutable; // Suppress unused warning for now
            
            // Recursively process type and value expressions
            populate_symbol_table(type_expr, symbol_table)?;
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::TypeDef { name, members } => {
            // Register module in symbol table
            symbol_table.register_type(name, members)?;
            
            // Process member expressions recursively
            for member in members {
                match member {
                    TypeMember::LetBinding { type_expr, value, .. } => {
                        populate_symbol_table(type_expr, symbol_table)?;
                        populate_symbol_table(value, symbol_table)?;
                    }
                    TypeMember::FunctionDef { params, return_type, body, .. } => {
                        for param in params {
                            populate_symbol_table(&param.type_expr, symbol_table)?;
                        }
                        populate_symbol_table(return_type, symbol_table)?;
                        populate_symbol_table(body, symbol_table)?;
                    }
                    TypeMember::Conversion { target_type, body } => {
                        populate_symbol_table(target_type, symbol_table)?;
                        populate_symbol_table(body, symbol_table)?;
                    }
                }
            }
        }
        Expr::Binary { left, right, .. } => {
            populate_symbol_table(left, symbol_table)?;
            populate_symbol_table(right, symbol_table)?;
        }
        Expr::Sequence(exprs) => {
            for expr in exprs {
                populate_symbol_table(expr, symbol_table)?;
            }
        }
        Expr::Block(exprs) => {
            for expr in exprs {
                populate_symbol_table(expr, symbol_table)?;
            }
        }
        Expr::FieldAccess { object, .. } => {
            populate_symbol_table(object, symbol_table)?;
        }
        Expr::FunctionCall { function, arguments } => {
            populate_symbol_table(function, symbol_table)?;
            for arg in arguments {
                populate_symbol_table(arg, symbol_table)?;
            }
        }
        Expr::Reference { expr } => {
            // First, recursively process the referenced expression
            populate_symbol_table(expr, symbol_table)?;
            
            // Then validate that the reference is memory-safe
            symbol_table.validate_reference(expr)?;
        }
        // Base cases that don't need recursive processing
        Expr::Number(_) | Expr::Identifier(_) | 
        Expr::IntType { .. } | Expr::Void | Expr::StringLiteral(_) => {
            // Nothing to do for these leaf nodes
        }
        Expr::Print { value } => {
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::PrintStr { value } => {
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::Syscall { number, arguments } => {
            populate_symbol_table(number, symbol_table)?;
            for arg in arguments {
                populate_symbol_table(arg, symbol_table)?;
            }
        }
        Expr::Strlen { value } => {
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::Cstr { value } => {
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::IntToString { value } => {
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::If { condition, then_expr, else_expr } => {
            populate_symbol_table(condition, symbol_table)?;
            populate_symbol_table(then_expr, symbol_table)?;
            if let Some(else_expr) = else_expr {
                populate_symbol_table(else_expr, symbol_table)?;
            }
        }
        Expr::Loop { body } => {
            populate_symbol_table(body, symbol_table)?;
        }
        Expr::Break { value } => {
            if let Some(value) = value {
                populate_symbol_table(value, symbol_table)?;
            }
        }
        Expr::Continue => {
            // Continue has no sub-expressions to populate
        }
        Expr::Assignment { name, value } => {
            // TODO: Verify that the variable exists and is mutable
            let _ = name; // Suppress unused warning for now
            populate_symbol_table(value, symbol_table)?;
        }
        Expr::Import { imports, path } => {
            // TODO: Handle import resolution and symbol registration
            let _ = imports; // Suppress unused warning for now
            let _ = path; // Suppress unused warning for now
        }
        Expr::TemplateLiteral { parts: _, expressions } => {
            // Process all embedded expressions
            for expr in expressions {
                populate_symbol_table(expr, symbol_table)?;
            }
        }
        Expr::MethodCall { object, arguments, .. } => {
            populate_symbol_table(object, symbol_table)?;
            for arg in arguments {
                populate_symbol_table(arg, symbol_table)?;
            }
        }
        
        Expr::ArrayLiteral { elements } => {
            for elem in elements {
                populate_symbol_table(elem, symbol_table)?;
            }
        }
        
        Expr::ArrayIndex { array, index } => {
            populate_symbol_table(array, symbol_table)?;
            populate_symbol_table(index, symbol_table)?;
        }
        
        Expr::Conversion { expr, target_type } => {
            populate_symbol_table(expr, symbol_table)?;
            populate_symbol_table(target_type, symbol_table)?;
        }
        
        Expr::CanConvert { expr, target_type } => {
            populate_symbol_table(expr, symbol_table)?;
            populate_symbol_table(target_type, symbol_table)?;
        }
        
        Expr::TypeGuard { expr, type_expr } => {
            populate_symbol_table(expr, symbol_table)?;
            populate_symbol_table(type_expr, symbol_table)?;
        }
    }
    Ok(())
}