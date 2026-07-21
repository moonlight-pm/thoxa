use crate::ast::TypedExpr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;
use cranelift_module::Module;

mod literals;
mod operators;
mod control;
mod structs;
mod functions;
mod arrays;
mod types;
mod builtins;

pub use literals::translate_literals;
pub use operators::translate_operators;
pub use control::translate_control;
pub use structs::translate_structs;

/// Main dispatch function for typed expressions
pub fn translate_typed_expr<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        // Literals
        TypedExpr::Number { .. } |
        TypedExpr::StringLiteral { .. } |
        TypedExpr::Identifier { .. } => {
            literals::translate_literals(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Operators
        TypedExpr::Binary { .. } |
        TypedExpr::Assignment { .. } => {
            operators::translate_operators(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Control flow
        TypedExpr::If { .. } |
        TypedExpr::Loop { .. } |
        TypedExpr::Break { .. } |
        TypedExpr::Continue { .. } |
        TypedExpr::Block { .. } |
        TypedExpr::Sequence { .. } => {
            control::translate_control(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Structs
        TypedExpr::FieldAccess { .. } |
        TypedExpr::LetBinding { .. } |
        TypedExpr::Reference { .. } => {
            structs::translate_structs(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Functions
        TypedExpr::FunctionCall { .. } |
        TypedExpr::MethodCall { .. } => {
            functions::translate_functions(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Arrays
        TypedExpr::ArrayLiteral { .. } |
        TypedExpr::ArrayIndex { .. } => {
            arrays::translate_arrays(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Types
        TypedExpr::TypeGuard { .. } |
        TypedExpr::Conversion { .. } |
        TypedExpr::Untyped { .. } => {
            types::translate_types(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        // Builtins
        TypedExpr::Print { .. } |
        TypedExpr::PrintStr { .. } |
        TypedExpr::Syscall { .. } |
        TypedExpr::Strlen { .. } |
        TypedExpr::Cstr { .. } |
        TypedExpr::IntToString { .. } => {
            builtins::translate_builtins(typed_expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
    }
}