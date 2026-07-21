use crate::ast::{BinaryOp, TypedExpr};
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr};
use crate::codegen::tagged_values::{tag_int_value, extract_value};
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle binary operations and assignments
pub fn translate_operators<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::Binary { left, op, right, ty: _ } => {
            // Evaluate operands
            let left_tagged = translate_typed_expr(left, builder, var_env, func_env, module_registry, module, symbol_table);
            let right_tagged = translate_typed_expr(right, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract actual values from tagged pointers for arithmetic
            let left_val = extract_value(builder, left_tagged);
            let right_val = extract_value(builder, right_tagged);
            
            // Perform the operation
            let result_val = match op {
                BinaryOp::Add => builder.ins().iadd(left_val, right_val),
                BinaryOp::Sub => builder.ins().isub(left_val, right_val),
                BinaryOp::Mul => builder.ins().imul(left_val, right_val),
                BinaryOp::Div => builder.ins().sdiv(left_val, right_val),
                BinaryOp::Eq => {
                    let cmp = builder.ins().icmp(IntCC::Equal, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::NotEq => {
                    let cmp = builder.ins().icmp(IntCC::NotEqual, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::Less => {
                    let cmp = builder.ins().icmp(IntCC::SignedLessThan, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::LessEq => {
                    let cmp = builder.ins().icmp(IntCC::SignedLessThanOrEqual, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::Greater => {
                    let cmp = builder.ins().icmp(IntCC::SignedGreaterThan, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::GreaterEq => {
                    let cmp = builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
            };
            
            // Tag the result as an integer
            tag_int_value(builder, result_val)
        }
        
        TypedExpr::Assignment { name, value, ty: _ } => {
            // Look up the variable
            if let Some(var) = var_env.get_variable(name) {
                // Check if it's mutable
                if let Some(is_mutable) = var_env.is_mutable(name) {
                    if !is_mutable {
                        eprintln!("ERROR: Cannot assign to immutable variable '{}'", name);
                        // Continue anyway for now
                    }
                }
                
                // Evaluate the value
                let val = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
                
                // Store the value
                builder.def_var(var, val);
                
                // Return the value
                val
            } else {
                eprintln!("ERROR: Unknown variable '{}' in assignment", name);
                builder.ins().iconst(types::I64, 0)
            }
        }
        
        _ => panic!("Not an operator expression"),
    }
}