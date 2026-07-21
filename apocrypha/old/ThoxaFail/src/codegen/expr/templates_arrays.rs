use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use crate::codegen::tagged_values::{tag_int_value, extract_value};
use cranelift::prelude::*;
use cranelift_module::{Module, DataDescription};

pub fn translate_templates_arrays<M: Module>(
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
        Expr::TemplateLiteral { parts, expressions } => {
            // Template literal code generation
            // For now, we'll implement a simplified static version for literals only
            
            // Check if all expressions are compile-time constants
            let mut all_static = true;
            let mut static_values = Vec::new();
            
            for expr in expressions {
                match expr {
                    Expr::Number(n) => {
                        static_values.push(n.to_string());
                    }
                    Expr::StringLiteral(s) => {
                        static_values.push(s.clone());
                    }
                    _ => {
                        all_static = false;
                        break;
                    }
                }
            }
            
            if all_static {
                // Build the final string at compile time
                let mut final_string = String::new();
                let mut expr_index = 0;
                
                for (_i, part) in parts.iter().enumerate() {
                    final_string.push_str(part);
                    if expr_index < static_values.len() {
                        final_string.push_str(&static_values[expr_index]);
                        expr_index += 1;
                    }
                }
                
                // Now create a string literal with the final value
                let string_bytes = final_string.as_bytes();
                let length = string_bytes.len() as u64;
                
                // Create a data object for the string structure
                let data_id = module
                    .declare_anonymous_data(true, false)
                    .unwrap();
                
                // Build the string structure in memory
                let mut string_data = Vec::new();
                
                // 1. Length (8 bytes, little-endian)
                string_data.extend_from_slice(&length.to_le_bytes());
                
                // 2. String data
                string_data.extend_from_slice(string_bytes);
                
                // 3. Null terminator
                string_data.push(0);
                
                // Define the data
                let mut data_desc = DataDescription::new();
                data_desc.define(string_data.into_boxed_slice());
                module.define_data(data_id, &data_desc).unwrap();
                
                // Get a reference to the string structure in the current function
                let string_ref = module.declare_data_in_func(data_id, builder.func);
                
                // Return the address of the string structure
                Some(builder.ins().global_value(types::I64, string_ref))
            } else {
                // For dynamic expressions, we need runtime string building
                // This is complex and requires:
                // 1. Calling toString on each expression
                // 2. Calculating total length
                // 3. Allocating memory
                // 4. Copying all parts
                // For now, return a placeholder
                let placeholder = "[template with expressions]";
                let string_bytes = placeholder.as_bytes();
                let length = string_bytes.len() as u64;
                
                let data_id = module
                    .declare_anonymous_data(true, false)
                    .unwrap();
                
                let mut string_data = Vec::new();
                string_data.extend_from_slice(&length.to_le_bytes());
                string_data.extend_from_slice(string_bytes);
                string_data.push(0);
                
                let mut data_desc = DataDescription::new();
                data_desc.define(string_data.into_boxed_slice());
                module.define_data(data_id, &data_desc).unwrap();
                
                let string_ref = module.declare_data_in_func(data_id, builder.func);
                Some(builder.ins().global_value(types::I64, string_ref))
            }
        }
        
        Expr::MethodCall { object, method, arguments: _ } => {
            // Evaluate the object
            let obj_tagged = translate_expr(object, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual object address from fat/tagged pointer if needed
            let obj_val = if true {
                extract_value(builder, obj_tagged)
            } else {
                obj_tagged
            };
            
            // Handle array methods
            match object.as_ref() {
                Expr::Identifier(_) | Expr::ArrayLiteral { .. } | Expr::ArrayIndex { .. } => {
                    // Could be an array
                    if method == "length" {
                        // Load the length field (first 8 bytes) - already tagged
                        let length_val = builder.ins().load(types::I64, MemFlags::new(), obj_val, 0);
                        // Length is already tagged, just return it
                        return Some(length_val);
                    } else if method == "toString" {
                        // For arrays, return a simple representation
                        let placeholder = "[array]";
                        let string_bytes = placeholder.as_bytes();
                        let length = string_bytes.len() as u64;
                        
                        let data_id = module
                            .declare_anonymous_data(true, false)
                            .unwrap();
                        
                        let mut string_data = Vec::new();
                        string_data.extend_from_slice(&length.to_le_bytes());
                        string_data.extend_from_slice(string_bytes);
                        string_data.push(0);
                        
                        let mut data_desc = DataDescription::new();
                        data_desc.define(string_data.into_boxed_slice());
                        module.define_data(data_id, &data_desc).unwrap();
                        
                        let string_ref = module.declare_data_in_func(data_id, builder.func);
                        return Some(builder.ins().global_value(types::I64, string_ref));
                    }
                }
                _ => {}
            }
            
            // Handle built-in toString methods for other types
            if method == "toString" {
                // For now, just return a placeholder
                // TODO: Implement proper toString methods
                let placeholder = "[toString not implemented]";
                let string_bytes = placeholder.as_bytes();
                let length = string_bytes.len() as u64;
                
                let data_id = module
                    .declare_anonymous_data(true, false)
                    .unwrap();
                
                let mut string_data = Vec::new();
                string_data.extend_from_slice(&length.to_le_bytes());
                string_data.extend_from_slice(string_bytes);
                string_data.push(0);
                
                let mut data_desc = DataDescription::new();
                data_desc.define(string_data.into_boxed_slice());
                module.define_data(data_id, &data_desc).unwrap();
                
                let string_ref = module.declare_data_in_func(data_id, builder.func);
                Some(builder.ins().global_value(types::I64, string_ref))
            } else {
                panic!("Unknown method: {}", method);
            }
        }
        
        Expr::ArrayLiteral { elements } => {
            // Array memory layout:
            // [8 bytes: length][8 bytes: capacity][8 bytes * length: elements]
            // For now, we'll use a simplified layout with just length and elements
            
            let element_count = elements.len();
            
            if element_count == 0 {
                // Empty array - just return null pointer for now
                // TODO: Proper empty array handling
                let null_ptr = builder.ins().iconst(types::I64, 0);
                // Array pointer is already properly tagged (000)
                Some(null_ptr)
            } else {
                // Calculate total size: 8 bytes (length) + 8 bytes per element
                let total_size = 8 + (element_count * 8);
                
                // Allocate stack space for the array
                let array_slot = builder.create_sized_stack_slot(StackSlotData {
                    kind: StackSlotKind::ExplicitSlot,
                    size: total_size as u32,
                });
                
                // Get pointer to the array
                let array_ptr = builder.ins().stack_addr(types::I64, array_slot, 0);
                
                // Store the length at offset 0 as a TAGGED integer
                let length_val = builder.ins().iconst(types::I64, element_count as i64);
                let tagged_length = 
                    tag_int_value(builder, length_val)
                ;
                builder.ins().store(MemFlags::new(), tagged_length, array_ptr, 0);
                
                // Store each element
                let mut offset = 8; // Start after length field
                for element in elements {
                    let elem_val = translate_expr(element, builder, var_env, func_env, module_registry, module, symbol_table);
                    builder.ins().store(MemFlags::new(), elem_val, array_ptr, offset);
                    offset += 8;
                }
                
                // Return pointer to the array
                // Array pointer is already properly tagged (000)
                Some(array_ptr)
            }
        }
        
        Expr::ArrayIndex { array, index } => {
            // Get array pointer (tagged pointer)
            let array_tagged = translate_expr(array, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract actual array pointer from tagged pointer
            let array_ptr = extract_value(builder, array_tagged);
            
            // Get index value (tagged pointer)
            let index_tagged = translate_expr(index, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract actual index value from tagged pointer
            let index_val = extract_value(builder, index_tagged);
            
            // Load array length for bounds checking
            let array_length = builder.ins().load(types::I64, MemFlags::new(), array_ptr, 0);
            
            // Bounds check: index >= 0 && index < length
            // For now, we'll do a simple check for index < length (assuming unsigned)
            let in_bounds = builder.ins().icmp(IntCC::UnsignedLessThan, index_val, array_length);
            
            // Create blocks for bounds check
            let _current_block = builder.current_block().unwrap();
            let bounds_ok_block = builder.create_block();
            let bounds_fail_block = builder.create_block();
            let merge_block = builder.create_block();
            
            // Add merge block parameter for the result
            let result_type = types::I64; // Tagged pointers are 64-bit
            builder.append_block_param(merge_block, result_type);
            
            // Branch based on bounds check
            builder.ins().brif(in_bounds, bounds_ok_block, &[], bounds_fail_block, &[]);
            
            // Bounds OK: load the element
            builder.switch_to_block(bounds_ok_block);
            builder.seal_block(bounds_ok_block);
            
            // Calculate element offset: 8 (skip length) + index * element_size
            let element_size = 8; // Tagged pointers are 8 bytes
            let size_const = builder.ins().iconst(types::I64, element_size);
            let element_offset = builder.ins().imul(index_val, size_const);
            let eight = builder.ins().iconst(types::I64, 8);
            let total_offset = builder.ins().iadd(element_offset, eight); // Add 8 to skip length field
            
            // Calculate element address
            let element_addr = builder.ins().iadd(array_ptr, total_offset);
            
            // Load the element (64-bit tagged value)
            let element = builder.ins().load(types::I64, MemFlags::new(), element_addr, 0);
            builder.ins().jump(merge_block, &[element]);
            
            // Bounds fail: return 0 for now (should panic in the future)
            builder.switch_to_block(bounds_fail_block);
            builder.seal_block(bounds_fail_block);
            let zero = {
                // Create a tagged integer value 0
                let zero_val = builder.ins().iconst(types::I64, 0);
                tag_int_value(builder, zero_val)
            };
            // TODO: Call panic function here
            builder.ins().jump(merge_block, &[zero]);
            
            // Merge block
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            
            // Return the phi value from the merge block
            Some(builder.block_params(merge_block)[0])
        }
        
        _ => None,
    }
}