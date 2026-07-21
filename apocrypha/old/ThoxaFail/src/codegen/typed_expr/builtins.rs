use crate::ast::TypedExpr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr};
use crate::codegen::tagged_values::extract_value;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle builtin expressions
pub fn translate_builtins<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::Print { value, ty } => {
            // Evaluate the value to print
            let val_tagged = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual value from tagged pointer
            let val = extract_value(builder, val_tagged);
            
            // Call the print_i64 function
            if let Some(print_func_id) = func_env.get_function("print_i64") {
                let print_fn = module.declare_func_in_func(print_func_id, builder.func);
                let args = vec![val];
                let call = builder.ins().call(print_fn, &args);
                let results = builder.inst_results(call);
                
                // Return the result
                let result = if results.is_empty() {
                    builder.ins().iconst(types::I64, 0)
                } else {
                    results[0]
                };
                
                result
            } else {
                eprintln!("ERROR: print_i64 function not found");
                let zero = builder.ins().iconst(types::I64, 0);
                zero
            }
        }
        
        TypedExpr::PrintStr { value, ty } => {
            // Evaluate the string value
            let str_val_tagged = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual string pointer from tagged pointer
            let str_val = extract_value(builder, str_val_tagged);
            
            // Call the print_string function
            if let Some(print_func_id) = func_env.get_function("print_string") {
                let print_fn = module.declare_func_in_func(print_func_id, builder.func);
                let args = vec![str_val];
                let call = builder.ins().call(print_fn, &args);
                let results = builder.inst_results(call);
                
                // Return the result
                let result = if results.is_empty() {
                    builder.ins().iconst(types::I64, 0)
                } else {
                    results[0]
                };
                
                result
            } else {
                eprintln!("ERROR: print_string function not found");
                let zero = builder.ins().iconst(types::I64, 0);
                zero
            }
        }
        
        TypedExpr::Syscall { number, arguments, ty } => {
            // Evaluate syscall number
            let syscall_num_tagged = translate_typed_expr(number, builder, var_env, func_env, module_registry, module, symbol_table);
            let syscall_num = extract_value(builder, syscall_num_tagged);
            
            // Evaluate arguments and extract values
            let mut arg_values = vec![syscall_num];
            for arg in arguments {
                let arg_tagged = translate_typed_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table);
                let arg_val = extract_value(builder, arg_tagged);
                arg_values.push(arg_val);
            }
            
            // Call the syscall function
            if let Some(syscall_func_id) = func_env.get_function("syscall") {
                let syscall_fn = module.declare_func_in_func(syscall_func_id, builder.func);
                let call = builder.ins().call(syscall_fn, &arg_values);
                let results = builder.inst_results(call);
                let result = if results.is_empty() {
                    builder.ins().iconst(types::I64, 0)
                } else {
                    results[0]
                };
                
                result
            } else {
                eprintln!("ERROR: syscall function not found");
                let zero = builder.ins().iconst(types::I64, 0);
                zero
            }
        }
        
        TypedExpr::Strlen { value, ty } => {
            // Evaluate string value
            let str_val_tagged = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual string pointer
            let str_ptr = extract_value(builder, str_val_tagged);
            
            // Load the length field (first 8 bytes of string)
            let length = builder.ins().load(types::I64, MemFlags::new(), str_ptr, 0);
            
            length
        }
        
        TypedExpr::Cstr { value, ty } => {
            // Evaluate string value
            let str_val_tagged = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual string pointer
            let str_ptr = extract_value(builder, str_val_tagged);
            
            // Calculate pointer to the data (skip length field)
            let cstr_ptr = builder.ins().iadd_imm(str_ptr, 8);
            
            cstr_ptr
        }
        
        TypedExpr::IntToString { value, ty } => {
            // Evaluate integer value
            let int_val_tagged = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual integer value
            let int_val = extract_value(builder, int_val_tagged);
            
            // Call int_to_string function
            if let Some(int_to_string_func_id) = func_env.get_function("int_to_string") {
                let func_ref = module.declare_func_in_func(int_to_string_func_id, builder.func);
                let args = vec![int_val];
                let call = builder.ins().call(func_ref, &args);
                let results = builder.inst_results(call);
                let result = if results.is_empty() {
                    builder.ins().iconst(types::I64, 0)
                } else {
                    results[0]
                };
                
                result
            } else {
                eprintln!("ERROR: int_to_string function not found");
                let zero = builder.ins().iconst(types::I64, 0);
                zero
            }
        }
        
        _ => panic!("Not a builtin expression"),
    }
}