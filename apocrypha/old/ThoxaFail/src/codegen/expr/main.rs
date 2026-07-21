use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;

use super::{basic_values, control_flow, field_reference, system_io, templates_arrays, type_operations};
pub fn translate_expr<M: cranelift_module::Module>(
    expr: &Expr, 
    builder: &mut FunctionBuilder, 
    var_env: &mut VarEnv, 
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable
) -> Value {
    // Try basic values first
    if let Some(value) = basic_values::translate_basic_values(expr, builder, var_env, func_env, module_registry, module, symbol_table) {
        return value;
    }
    
    // Try field reference handling
    if let Some(value) = field_reference::translate_field_reference(expr, builder, var_env, func_env, module_registry, module, symbol_table) {
        return value;
    }
    
    // Try system I/O operations
    if let Some(value) = system_io::translate_system_io(expr, builder, var_env, func_env, module_registry, module, symbol_table, &translate_expr) {
        return value;
    }
    
    // Try control flow operations
    if let Some(value) = control_flow::translate_control_flow(expr, builder, var_env, func_env, module_registry, module, symbol_table, &translate_expr) {
        return value;
    }
    
    // Try templates and arrays operations
    if let Some(value) = templates_arrays::translate_templates_arrays(expr, builder, var_env, func_env, module_registry, module, symbol_table, &translate_expr) {
        return value;
    }
    
    // Try type operations
    if let Some(value) = type_operations::translate_type_operations(expr, builder, var_env, func_env, module_registry, module, symbol_table, &translate_expr) {
        return value;
    }
    
    match expr {
        Expr::FunctionCall { function, arguments } => {
            // Handle function calls
            match function.as_ref() {
                // Direct method call: obj.method(args)
                Expr::FieldAccess { object, field } => {
                    // This is a method call
                    // First, we need to determine the module type
                    match object.as_ref() {
                        Expr::Identifier(obj_name) => {
                            // First check if this is an imported module
                            if let Some(import) = symbol_table.get_import(obj_name) {
                                use crate::symbols::ImportedBinding;
                                if let ImportedBinding::TypeInstance { module_name, .. } = import {
                                    let func_name = format!("{}.{}", module_name, field);
                                    if let Some(_func_id) = func_env.get_function(&func_name) {
                                        // Found the function!
                                        // Evaluate arguments
                                        let mut arg_values = Vec::new();
                                        
                                        // Evaluate arguments
                                        for arg in arguments {
                                            arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
                                        }
                                        
                                        // Get the function reference and call it
                                        let func_id = func_env.get_function(&func_name).unwrap();
                                        let func_ref = module.declare_func_in_func(func_id, builder.func);
                                        let call = builder.ins().call(func_ref, &arg_values);
                                        let results = builder.inst_results(call);
                                        if results.is_empty() {
                                            return builder.ins().iconst(types::I64, 0);
                                        } else {
                                            return results[0];
                                        }
                                    }
                                }
                            }
                            
                            // Look up the module instance as a local variable
                            if let Some(var) = var_env.get_variable(obj_name) {
                                let var_value = builder.use_var(var);
                                
                                // Check if this is an array method
                                if field == "length" {
                                    // Extract the actual array pointer from tagged pointer if needed
                                    let array_ptr = var_value;
                                    
                                    // Load and return the length field (first 8 bytes)
                                    let length_val = builder.ins().load(types::I64, MemFlags::new(), array_ptr, 0);
                                    
                                    // Tag the length as an integer
                                    {
                                        return length_val;
                                    }
                                }
                                
                                // Try to find the function in any module
                                // TODO: This should use type information to find the right module
                                for (module_name, _module_info) in &module_registry.modules {
                                    let func_name = format!("{}.{}", module_name, field);
                                    if let Some(_func_id) = func_env.get_function(&func_name) {
                                        // Found the function!
                                        // Evaluate arguments
                                        let mut arg_values = Vec::new();
                                        
                                        // TODO: Check if this function needs &self as first parameter
                                        // For now, we don't support methods with self parameter
                                        
                                        // Evaluate arguments
                                        for arg in arguments {
                                            arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
                                        }
                                        
                                        // Get the function reference and call it
                                        let func_id = func_env.get_function(&func_name).unwrap();
                                        let func_ref = module.declare_func_in_func(func_id, builder.func);
                                        let call = builder.ins().call(func_ref, &arg_values);
                                        let results = builder.inst_results(call);
                                        if results.is_empty() {
                                            return builder.ins().iconst(types::I64, 0);
                                        } else {
                                            return results[0];
                                        }
                                    }
                                }
                                
                                // Function not found
                                builder.ins().iconst(types::I64, 0)
                            } else {
                                // Object not found
                                builder.ins().iconst(types::I64, 0)
                            }
                        }
                        _ => {
                            // Complex object expression - not supported yet
                            builder.ins().iconst(types::I64, 0)
                        }
                    }
                }
                // Direct function call by name
                Expr::Identifier(func_name) => {
                    // First check if it's a regular function
                    if let Some(_func_id) = func_env.get_function(func_name) {
                        let mut arg_values = Vec::new();
                        
                        // Check if function is variadic
                        if let Some((is_variadic, fixed_param_count)) = func_env.get_variadic_info(func_name) {
                            if is_variadic {
                                // Handle variadic function call
                                let arg_count = arguments.len();
                                
                                // Evaluate fixed arguments
                                for i in 0..fixed_param_count.min(arg_count) {
                                    arg_values.push(translate_expr(&arguments[i], builder, var_env, func_env, module_registry, module, symbol_table));
                                }
                                
                                // Create array from variadic arguments
                                if arg_count > fixed_param_count {
                                    let variadic_args = &arguments[fixed_param_count..];
                                    let array_expr = crate::ast::Expr::ArrayLiteral {
                                        elements: variadic_args.to_vec(),
                                    };
                                    let array_value = translate_expr(&array_expr, builder, var_env, func_env, module_registry, module, symbol_table);
                                    arg_values.push(array_value);
                                } else {
                                    // No variadic arguments - create empty array
                                    let empty_array = crate::ast::Expr::ArrayLiteral {
                                        elements: vec![],
                                    };
                                    let array_value = translate_expr(&empty_array, builder, var_env, func_env, module_registry, module, symbol_table);
                                    arg_values.push(array_value);
                                }
                            } else {
                                // Regular function - evaluate all arguments normally
                                for arg in arguments {
                                    arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
                                }
                            }
                        } else {
                            // No variadic info - treat as regular function
                            for arg in arguments {
                                arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
                            }
                        }
                        
                        // Get the function reference and call it
                        let func_id = func_env.get_function(func_name).unwrap();
                        let func_ref = module.declare_func_in_func(func_id, builder.func);
                        let call = builder.ins().call(func_ref, &arg_values);
                        let results = builder.inst_results(call);
                        if results.is_empty() {
                            builder.ins().iconst(types::I64, 0)
                        } else {
                            results[0]
                        }
                    } else if let Some(import) = symbol_table.get_import(func_name) {
                        // Check if it's a destructured function import
                        use crate::symbols::{ImportedBinding, TypeMemberInfo};
                        if let ImportedBinding::DestructuredMember { module_name, member_name, member_info, .. } = import {
                            if let TypeMemberInfo::FunctionDef { .. } = member_info {
                                // It's a destructured function - construct the full name
                                let full_func_name = format!("{}.{}", module_name, member_name);
                                if let Some(func_id) = func_env.get_function(&full_func_name) {
                                    // Evaluate arguments
                                    let mut arg_values = Vec::new();
                                    for arg in arguments {
                                        arg_values.push(translate_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table));
                                    }
                                    
                                    // Call the function
                                    let func_ref = module.declare_func_in_func(func_id, builder.func);
                                    let call = builder.ins().call(func_ref, &arg_values);
                                    let results = builder.inst_results(call);
                                    if results.is_empty() {
                                        builder.ins().iconst(types::I64, 0)
                                    } else {
                                        results[0]
                                    }
                                } else {
                                    // Function not found in registry
                                    builder.ins().iconst(types::I64, 0)
                                }
                            } else {
                                // Not a function
                                builder.ins().iconst(types::I64, 0)
                            }
                        } else {
                            // Not a destructured member
                            builder.ins().iconst(types::I64, 0)
                        }
                    } else {
                        // Function not found
                        builder.ins().iconst(types::I64, 0)
                    }
                }
                _ => {
                    // Other expression types as functions not supported yet
                    builder.ins().iconst(types::I64, 0)
                }
            }
        }
        _ => {
            // These cases should have been handled by basic_values
            panic!("Unhandled expression type: {:?}", expr)
        }
    }
}
