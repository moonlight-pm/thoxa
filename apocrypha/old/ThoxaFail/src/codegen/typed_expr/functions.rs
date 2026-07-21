use crate::ast::TypedExpr;
use crate::types::Type;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr, translate_expr};
use crate::codegen::tagged_values::{extract_value, tag_int_value};
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle function and method calls
pub fn translate_functions<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::MethodCall { object, method, arguments, ty } => {
            // Get the type of the object
            let object_type = object.get_type();
            
            // Evaluate the object
            let object_value = translate_typed_expr(object, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Handle array methods
            if let Type::Array { .. } = &object_type {
                if method == "length" {
                    // Extract the actual array pointer from tagged pointer
                    let array_ptr = extract_value(builder, object_value);
                    
                    // Load the length field (first 8 bytes) - already tagged
                    let length_val = builder.ins().load(types::I64, MemFlags::new(), array_ptr, 0);
                    
                    // Length is already tagged
                    return length_val;
                } else if method == "toString" {
                    // Array toString implementation
                    let placeholder = "[array]";
                    let string_bytes = placeholder.as_bytes();
                    let length = string_bytes.len() as u64;
                    
                    let data_id = module
                        .declare_anonymous_data(true, false)
                        .unwrap();
                    
                    let mut string_data = Vec::new();
                    string_data.extend_from_slice(&length.to_le_bytes());
                    string_data.extend_from_slice(string_bytes);
                    string_data.push(0); // null terminator
                    
                    let mut data_desc = cranelift_module::DataDescription::new();
                    data_desc.define(string_data.into_boxed_slice());
                    module.define_data(data_id, &data_desc).unwrap();
                    
                    let local_id = module.declare_data_in_func(data_id, builder.func);
                    let string_ptr = builder.ins().symbol_value(types::I64, local_id);
                    
                    return string_ptr;
                } else {
                    eprintln!("ERROR: Unknown array method: {}", method);
                    return builder.ins().iconst(types::I64, 0);
                }
            }
            
            // Find the module name from the object type for struct methods
            let module_name = match &object_type {
                Type::Struct { name, .. } => name.clone(),
                _ => {
                    eprintln!("ERROR: Method call on non-struct/non-array type: {:?}", object_type);
                    return builder.ins().iconst(types::I64, 0);
                }
            };
            
            // Build the fully qualified function name
            let func_name = format!("{}.{}", module_name, method);
            
            // Check if this is a variadic function
            let (is_variadic, fixed_param_count) = func_env.get_variadic_info(&func_name)
                .unwrap_or((false, arguments.len()));
            
            // Prepare arguments
            let mut arg_values = Vec::new();
            
            // Add the object as the first argument (self parameter)
            arg_values.push(object_value);
            
            if is_variadic && arguments.len() >= fixed_param_count {
                // Handle variadic function call
                // First, add fixed arguments
                for i in 0..fixed_param_count {
                    let arg_val = translate_typed_expr(&arguments[i], builder, var_env, func_env, module_registry, module, symbol_table);
                    arg_values.push(arg_val);
                }
                
                // Collect remaining arguments into an array
                if arguments.len() > fixed_param_count {
                    let variadic_args = &arguments[fixed_param_count..];
                    
                    // Create array directly from TypedExpr values to preserve tagging
                    let element_count = variadic_args.len();
                    let element_size = 8; // Tagged pointers are 8 bytes
                    let total_size = 8 + (element_count * element_size);
                    
                    // Allocate stack space for the array
                    let array_slot = builder.create_sized_stack_slot(StackSlotData {
                        kind: StackSlotKind::ExplicitSlot,
                        size: total_size as u32,
                    });
                    
                    // Get pointer to the array
                    let array_ptr = builder.ins().stack_addr(types::I64, array_slot, 0);
                    
                    // Store the length at offset 0 as a TAGGED integer
                    let length_val = builder.ins().iconst(types::I64, element_count as i64);
                    let tagged_length = tag_int_value(builder, length_val);
                    builder.ins().store(MemFlags::new(), tagged_length, array_ptr, 0);
                    
                    // Store each element (preserving tagged values)
                    let mut offset = 8;
                    for element in variadic_args {
                        let element_val = translate_typed_expr(element, builder, var_env, func_env, module_registry, module, symbol_table);
                        
                        // Store tagged value directly (8 bytes)
                        builder.ins().store(MemFlags::new(), element_val, array_ptr, offset);
                        offset += 8;
                    }
                    
                    // Arrays are pointers, so tag is 000 (pointer tag)
                    arg_values.push(array_ptr);
                } else {
                    // No variadic arguments - create empty array
                    let empty_array = crate::ast::Expr::ArrayLiteral {
                        elements: vec![],
                    };
                    let empty_array_val = translate_expr(&empty_array, builder, var_env, func_env, module_registry, module, symbol_table);
                    arg_values.push(empty_array_val);
                }
            } else {
                // Regular (non-variadic) function call
                for arg in arguments {
                    let arg_val = translate_typed_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table);
                    arg_values.push(arg_val);
                }
            }
            
            // Look up the function
            if let Some(func_id) = func_env.get_function(&func_name) {
                let func_ref = module.declare_func_in_func(func_id, builder.func);
                let call = builder.ins().call(func_ref, &arg_values);
                let results = builder.inst_results(call);
                if results.is_empty() {
                    // Void return
                    builder.ins().iconst(types::I64, 0)
                } else {
                    // Function already returns a tagged pointer
                    results[0]
                }
            } else {
                eprintln!("ERROR: Function not found: {}", func_name);
                builder.ins().iconst(types::I64, 0)
            }
        }
        
        TypedExpr::FunctionCall { function, arguments, ty: _ } => {
            // Evaluate the function expression to get the function ID
            let func_val = translate_typed_expr(function, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // For now, we only support direct function calls by name
            if let TypedExpr::Identifier { name, ty: _ } = function.as_ref() {
                if let Some(func_id) = func_env.get_function(name) {
                    // Evaluate arguments
                    let mut arg_values = Vec::new();
                    for arg in arguments {
                        let arg_val = translate_typed_expr(arg, builder, var_env, func_env, module_registry, module, symbol_table);
                        arg_values.push(arg_val);
                    }
                    
                    // Call the function
                    let func_ref = module.declare_func_in_func(func_id, builder.func);
                    let call = builder.ins().call(func_ref, &arg_values);
                    let results = builder.inst_results(call);
                    
                    // Return the result or 0 if void
                    if results.is_empty() {
                        builder.ins().iconst(types::I64, 0)
                    } else {
                        results[0]
                    }
                } else {
                    eprintln!("ERROR: Function '{}' not found", name);
                    builder.ins().iconst(types::I64, 0)
                }
            } else {
                eprintln!("ERROR: Indirect function calls not yet supported");
                builder.ins().iconst(types::I64, 0)
            }
        }
        
        _ => panic!("Not a function expression"),
    }
}