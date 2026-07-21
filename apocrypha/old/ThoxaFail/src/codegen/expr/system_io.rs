use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use crate::codegen::tagged_values::extract_value;
use cranelift::prelude::*;
use cranelift_module::Module;

pub fn translate_system_io<M: Module>(
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
        Expr::Print { value } => {
            // Evaluate the value to print
            let val_tagged = translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual value from fat/tagged pointer if needed
            let val = extract_value(builder, val_tagged);
            
            // Get or declare the print_i64 function
            let print_func = module
                .declare_function("print_i64", cranelift_module::Linkage::Import, &{
                    let mut sig = module.make_signature();
                    sig.params.push(cranelift::prelude::AbiParam::new(types::I64));
                    sig.returns.push(cranelift::prelude::AbiParam::new(types::I64)); // void functions return i64
                    sig
                })
                .unwrap();
            
            // Get a reference to the function in the current function
            let print_ref = module.declare_func_in_func(print_func, builder.func);
            
            // Call the print function
            let call = builder.ins().call(print_ref, &[val]);
            
            // Return the result (which will be 0 for void)
            let result = builder.inst_results(call)[0];
            
            // Result is already a proper value
            Some(result)
        }
        Expr::PrintStr { value } => {
            // Evaluate the value to get the string structure pointer
            let string_ptr_tagged = translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual pointer from tagged pointer
            let string_ptr = extract_value(builder, string_ptr_tagged);
            
            // Our string structure layout:
            // [0..8]   length (u64)
            // [8..8+N] data (bytes)
            // [8+N]    null terminator
            
            // Get or declare the print_string function (expects our string structure pointer)
            let print_func = module
                .declare_function("print_string", cranelift_module::Linkage::Import, &{
                    let mut sig = module.make_signature();
                    sig.params.push(cranelift::prelude::AbiParam::new(types::I64));
                    sig.returns.push(cranelift::prelude::AbiParam::new(types::I64)); // void functions return i64
                    sig
                })
                .unwrap();
            
            // Get a reference to the function in the current function
            let print_ref = module.declare_func_in_func(print_func, builder.func);
            
            // Call the print function with the string structure pointer (includes length)
            let call = builder.ins().call(print_ref, &[string_ptr]);
            
            // Return the result (which will be 0 for void)
            let result = builder.inst_results(call)[0];
            
            // Void is represented as zero (untagged)
            Some(result)
        }
        Expr::Syscall { number, arguments } => {
            // Evaluate syscall number
            let syscall_num = translate_expr(number, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Evaluate arguments (up to 6 on most systems)
            let mut arg_values = Vec::new();
            for arg in arguments {
                arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
            }
            
            // macOS uses different syscall numbers and convention than Linux
            // On macOS ARM64:
            // - Syscall number goes in X16
            // - Arguments in X0-X5
            // - Result in X0
            // - Use SVC #0x80 instruction
            
            // For now, we'll implement this as a call to a helper function
            // that does the actual syscall
            let syscall_func = module
                .declare_function("do_syscall", cranelift_module::Linkage::Import, &{
                    let mut sig = module.make_signature();
                    // syscall number
                    sig.params.push(cranelift::prelude::AbiParam::new(types::I64));
                    // up to 6 arguments
                    for _ in 0..6 {
                        sig.params.push(cranelift::prelude::AbiParam::new(types::I64));
                    }
                    sig.returns.push(cranelift::prelude::AbiParam::new(types::I64));
                    sig
                })
                .unwrap();
            
            // Get a reference to the function
            let syscall_ref = module.declare_func_in_func(syscall_func, builder.func);
            
            // Prepare arguments: syscall number + up to 6 args (pad with 0s)
            let mut call_args = vec![syscall_num];
            for i in 0..6 {
                if i < arg_values.len() {
                    call_args.push(arg_values[i]);
                } else {
                    call_args.push(builder.ins().iconst(types::I64, 0));
                }
            }
            
            // Call the syscall function
            let call = builder.ins().call(syscall_ref, &call_args);
            
            // Return the result
            Some(builder.inst_results(call)[0])
        }
        Expr::Strlen { value } => {
            // Evaluate the string structure pointer
            let string_ptr_or_tagged = translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract actual pointer from tagged pointer
            let string_ptr = extract_value(builder, string_ptr_or_tagged);
            
            // Our string structure layout:
            // [0..8]   length (u64) - this is what we want!
            // [8..8+N] data (bytes)
            // [8+N]    null terminator
            
            // Load the length field directly (first 8 bytes)
            let flags = cranelift::codegen::ir::MemFlags::new();
            let length = builder.ins().load(types::I64, flags, string_ptr, 0);
            
            // Return the length
            Some(length)
        }
        Expr::Cstr { value } => {
            // Evaluate the string structure pointer
            let string_ptr = translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Our string structure layout:
            // [0..8]   length (u64)
            // [8..8+N] data (bytes) - this is what we want!
            // [8+N]    null terminator
            
            // Calculate pointer to the actual string data (skip the length field)
            let offset = builder.ins().iconst(types::I64, 8); // Skip 8-byte length
            let data_ptr = builder.ins().iadd(string_ptr, offset);
            
            // Return the C-compatible string pointer
            Some(data_ptr)
        }
        Expr::IntToString { value } => {
            // Evaluate the integer value
            let int_val = translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Get or declare the int_to_string function
            let int_to_string_func = module
                .declare_function("int_to_string", cranelift_module::Linkage::Import, &{
                    let mut sig = module.make_signature();
                    sig.params.push(cranelift::prelude::AbiParam::new(types::I64));
                    sig.returns.push(cranelift::prelude::AbiParam::new(types::I64));
                    sig
                })
                .unwrap();
            
            // Get a reference to the function
            let int_to_string_ref = module.declare_func_in_func(int_to_string_func, builder.func);
            
            // Call the int_to_string function
            let call = builder.ins().call(int_to_string_ref, &[int_val]);
            
            // Return the string pointer
            Some(builder.inst_results(call)[0])
        }
        _ => None,
    }
}