use crate::ast::TypedExpr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr};
use crate::codegen::tagged_values::{tag_int_value, extract_value};
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle array expressions
pub fn translate_arrays<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::ArrayLiteral { elements, ty } => {
            let element_count = elements.len();
            
            // Special handling for empty arrays
            let array_ptr = if element_count == 0 {
                // Create a minimal array with just the length field
                let array_slot = builder.create_sized_stack_slot(StackSlotData {
                    kind: StackSlotKind::ExplicitSlot,
                    size: 8, // Just the length field
                });
                
                let array_ptr = builder.ins().stack_addr(types::I64, array_slot, 0);
                
                // Store length as 0 (tagged)
                let zero = builder.ins().iconst(types::I64, 0);
                let tagged_zero = tag_int_value(builder, zero);
                builder.ins().store(MemFlags::new(), tagged_zero, array_ptr, 0);
                
                array_ptr
            } else {
                // Calculate total size: 8 bytes (length) + element size
                let element_size = 8; // Tagged pointers are 8 bytes
                let total_size = 8 + (element_count * element_size);
                
                // Allocate stack space for the array
                let array_slot = builder.create_sized_stack_slot(StackSlotData {
                    kind: StackSlotKind::ExplicitSlot,
                    size: total_size as u32,
                });
                
                // Get pointer to the array
                let array_ptr = builder.ins().stack_addr(types::I64, array_slot, 0);
                
                // Store the length at offset 0 as a tagged integer
                let length_val = builder.ins().iconst(types::I64, element_count as i64);
                let tagged_length = tag_int_value(builder, length_val);
                builder.ins().store(MemFlags::new(), tagged_length, array_ptr, 0);
                
                // Store each element
                let mut offset = 8;
                for element in elements {
                    let element_val = translate_typed_expr(element, builder, var_env, func_env, module_registry, module, symbol_table);
                    
                    // Store tagged value directly (8 bytes)
                    builder.ins().store(MemFlags::new(), element_val, array_ptr, offset);
                    offset += 8;
                }
                
                // Return pointer to the array
                array_ptr
            };
            
            // Arrays are already pointers, so tag is 000 (pointer tag)
            array_ptr
        }
        
        TypedExpr::ArrayIndex { array, index, ty } => {
            // Get array pointer (tagged pointer)
            let array_tagged = translate_typed_expr(array, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual array pointer
            let array_ptr = extract_value(builder, array_tagged);
            
            // Get index value (tagged pointer)
            let index_tagged = translate_typed_expr(index, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract the actual index value
            let index_val = extract_value(builder, index_tagged);
            
            // Load array length for bounds checking
            let array_length = builder.ins().load(types::I64, MemFlags::new(), array_ptr, 0);
            
            // Extract the actual length value (it's tagged)
            let length_val = extract_value(builder, array_length);
            
            // Bounds check: 0 <= index < length
            let zero = builder.ins().iconst(types::I64, 0);
            let index_gte_zero = builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, index_val, zero);
            let index_lt_length = builder.ins().icmp(IntCC::SignedLessThan, index_val, length_val);
            let in_bounds = builder.ins().band(index_gte_zero, index_lt_length);
            
            // Create blocks for bounds check
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
            
            // Calculate offset: 8 (length) + index * 8 (tagged pointer size)
            let offset_base = builder.ins().iconst(types::I64, 8);
            let index_offset = builder.ins().imul_imm(index_val, 8);
            let total_offset = builder.ins().iadd(offset_base, index_offset);
            
            // Load tagged value at calculated offset
            let element_ptr = builder.ins().iadd(array_ptr, total_offset);
            let element_val = builder.ins().load(types::I64, MemFlags::new(), element_ptr, 0);
            
            builder.ins().jump(merge_block, &[element_val]);
            
            // Bounds fail: return error value
            builder.switch_to_block(bounds_fail_block);
            builder.seal_block(bounds_fail_block);
            
            let error_val = builder.ins().iconst(types::I64, -1);
            
            builder.ins().jump(merge_block, &[error_val]);
            
            // Merge block
            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            builder.block_params(merge_block)[0]
        }
        
        _ => panic!("Not an array expression"),
    }
}