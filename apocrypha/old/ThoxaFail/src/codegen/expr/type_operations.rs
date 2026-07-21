use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;

pub fn translate_type_operations<M: cranelift_module::Module>(
    expr: &Expr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
    translate_expr: &dyn Fn(&Expr, &mut FunctionBuilder, &mut VarEnv, &FuncEnv, &mut ModuleRegistry, &mut M, &crate::symbols::SymbolTable) -> Value,
) -> Option<Value> {
    match expr {
        Expr::Conversion { expr, target_type } => {
            // Evaluate the expression to convert
            let expr_val = translate_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // For now, handle built-in conversions directly
            // In the future, we'll call user-defined conversion functions
            match (expr.as_ref(), target_type.as_ref()) {
                // Int to String conversion
                (_, Expr::Identifier(target)) if target == "String" => {
                    // Call @intToString builtin
                    if let Some(int_to_string) = func_env.get_function("@intToString") {
                        let int_to_string_ref = module.declare_func_in_func(int_to_string, builder.func);
                        let call = builder.ins().call(int_to_string_ref, &[expr_val]);
                        Some(builder.inst_results(call)[0])
                    } else {
                        panic!("@intToString builtin not found");
                    }
                }
                // String to String is identity
                (Expr::StringLiteral(_), Expr::Identifier(target)) if target == "String" => {
                    Some(expr_val)
                }
                // For other conversions, just return the value for now
                // TODO: Implement user-defined conversions
                _ => Some(expr_val)
            }
        }
        
        Expr::CanConvert { expr, target_type } => {
            // Check if conversion exists at runtime
            // For now, handle built-in conversions
            match (expr.as_ref(), target_type.as_ref()) {
                // Int can convert to String
                (Expr::Number(_), Expr::Identifier(target)) if target == "String" => {
                    Some(builder.ins().iconst(types::I64, 1))
                }
                (Expr::Identifier(_), Expr::Identifier(target)) if target == "String" => {
                    // Would need runtime type checking, for now assume it can
                    Some(builder.ins().iconst(types::I64, 1))
                }
                // String can convert to String
                (Expr::StringLiteral(_), Expr::Identifier(target)) if target == "String" => {
                    Some(builder.ins().iconst(types::I64, 1))
                }
                // Other conversions don't exist yet
                _ => Some(builder.ins().iconst(types::I64, 0))
            }
        }
        
        Expr::TypeGuard { expr, type_expr: _ } => {
            // Use translate_expr for now, which won't give us type info
            // In the future, this should be type checked first
            let _expr_val = translate_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Without type checking, we can't do much here
            // Just return false (0) for now
            eprintln!("WARNING: TypeGuard in untyped context not fully supported");
            Some(builder.ins().iconst(types::I64, 0))
        }
        
        _ => None,
    }
}