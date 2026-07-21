use crate::ast::{BinaryOp, Expr};
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, ModuleInfo, value_type};
use crate::codegen::tagged_values::{tag_int_value, extract_value};
use cranelift::prelude::*;
use cranelift_module::{Module, DataDescription};

pub fn translate_basic_values<M: cranelift_module::Module>(
    expr: &Expr, 
    builder: &mut FunctionBuilder, 
    var_env: &mut VarEnv, 
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable
) -> Option<Value> {
    match expr {
        Expr::Number(n) => {
            let raw_value = builder.ins().iconst(types::I64, *n);
            // Tag the integer value
            Some(tag_int_value(builder, raw_value))
        },
        Expr::Binary { left, op, right } => {
            let left_tagged = crate::codegen::expr::translate_expr(left, builder, var_env, func_env, module_registry, module, symbol_table);
            let right_tagged = crate::codegen::expr::translate_expr(right, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Extract values from tagged pointers
            let left_val = extract_value(builder, left_tagged);
            let right_val = extract_value(builder, right_tagged);
            
            let result = match op {
                BinaryOp::Add => builder.ins().iadd(left_val, right_val),
                BinaryOp::Sub => builder.ins().isub(left_val, right_val),
                BinaryOp::Mul => builder.ins().imul(left_val, right_val),
                BinaryOp::Div => builder.ins().sdiv(left_val, right_val),
                // Comparison operators - return 1 for true, 0 for false
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
                BinaryOp::Greater => {
                    let cmp = builder.ins().icmp(IntCC::SignedGreaterThan, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::LessEq => {
                    let cmp = builder.ins().icmp(IntCC::SignedLessThanOrEqual, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
                BinaryOp::GreaterEq => {
                    let cmp = builder.ins().icmp(IntCC::SignedGreaterThanOrEqual, left_val, right_val);
                    builder.ins().uextend(types::I64, cmp)
                }
            };
            
            // Tag the result as an integer
            Some(tag_int_value(builder, result))
        }
        Expr::Identifier(name) => {
            // Check local variables (which now includes imported modules)
            if let Some(var) = var_env.get_variable(name) {
                Some(builder.use_var(var))
            } else if let Some(import) = symbol_table.get_import(name) {
                // This shouldn't happen if imports are properly instantiated
                // Return error value
                use crate::symbols::ImportedBinding;
                match import {
                    ImportedBinding::TypeInstance { .. } => {
                        // Imported modules should be in var_env after instantiation
                        Some(builder.ins().iconst(types::I64, 0))
                    }
                    ImportedBinding::TypeInfo { module_name, .. } => {
                        // Module types evaluate to their hash
                        let hash = module_name.chars().fold(0i64, |acc, c| acc.wrapping_mul(31).wrapping_add(c as i64));
                        Some(builder.ins().iconst(types::I64, hash))
                    }
                    ImportedBinding::DestructuredMember {  member_info, .. } => {
                        // Handle destructured member imports
                        use crate::symbols::TypeMemberInfo;
                        match member_info {
                            TypeMemberInfo::LetBinding { value, exported, .. } => {
                                if *exported {
                                    // For destructured field imports, we need the module instance
                                    // For now, we'll evaluate the init expression directly
                                    // TODO: This should reference the actual module instance
                                    Some(crate::codegen::expr::translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table))
                                } else {
                                    Some(builder.ins().iconst(types::I64, 0))
                                }
                            }
                            TypeMemberInfo::FunctionDef { .. } => {
                                // Functions as values not yet supported
                                // This would return a function pointer in the future
                                Some(builder.ins().iconst(types::I64, 0))
                            }
                        }
                    }
                }
            } else {
                // If not found, return hash of identifier (for type names, etc.)
                let hash = name.chars().fold(0i64, |acc, c| acc.wrapping_mul(31).wrapping_add(c as i64));
                Some(builder.ins().iconst(types::I64, hash))
            }
        }
        Expr::IntType { signed, bits } => {
            // Encode type information as a single integer
            // Format: [signed bit][7-bit size]
            let type_id = if *signed { 0x80 } else { 0x00 } | (*bits as i64);
            Some(builder.ins().iconst(types::I64, type_id))
        }
        Expr::Sequence(exprs) => {
            // Evaluate all expressions, return the last one
            let mut result = builder.ins().iconst(types::I64, 0); // Default value
            for expr in exprs {
                result = crate::codegen::expr::translate_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
            }
            Some(result)
        }
        Expr::Block(exprs) => {
            // Evaluate all expressions in the block, return the last one
            if exprs.is_empty() {
                // Empty block returns 0 (could be unit type later)
                Some(builder.ins().iconst(types::I64, 0))
            } else {
                // Create a result value that we'll update as we go
                let mut result = builder.ins().iconst(types::I64, 0);
                
                for (_i, expr) in exprs.iter().enumerate() {
                    // Check if we still have a current block before translating
                    if builder.current_block().is_none() {
                        // Block has been terminated (e.g., by break), skip remaining expressions
                        break;
                    }
                    
                    let val = crate::codegen::expr::translate_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table);
                    
                    // Only update result if we're still in a valid block
                    if builder.current_block().is_some() {
                        result = val;
                    }
                }
                
                Some(result)
            }
        }
        Expr::LetBinding { name, type_expr, value, mutable } => {
            // Check if this is a module instantiation (let p: ModuleName;)
            if let Expr::Identifier(type_name) = type_expr.as_ref() {
                if let Some(module_info) = module_registry.get_type(type_name).cloned() {
                    // This is a module instantiation
                    // Allocate stack space for the module instance
                    let module_size = module_info.size as u32;
                    let stack_slot = builder.create_sized_stack_slot(StackSlotData {
                        kind: StackSlotKind::ExplicitSlot,
                        size: module_size,
                    });
                    
                    // Get the address of the stack slot
                    let base_addr = builder.ins().stack_addr(types::I64, stack_slot, 0);
                    
                    // Initialize each field
                    for (field_name, _, init_expr) in module_info.fields.iter() {
                        // Execute the field's initialization expression
                        let init_value = crate::codegen::expr::translate_expr(init_expr, builder, var_env, func_env, module_registry, module, symbol_table);
                        
                        // Get the field offset
                        if let Some(offset) = module_info.get_field_offset(field_name) {
                            let offset_val = (offset * 8) as i32; // Each field is 8 bytes
                            // Store the value at the field offset
                            builder.ins().store(MemFlags::new(), init_value, base_addr, offset_val);
                        }
                    }
                    
                    // Store the module instance variable
                    let var = var_env.next_variable();
                    builder.declare_var(var, value_type());
                    
                    // Initialize with default value to avoid SSA issues
                    let default_val = builder.ins().iconst(types::I64, 0);
                    builder.def_var(var, default_val);
                    
                    let module_value = {
                        // Module value is just the pointer (tagged with 000)
                        base_addr
                    };
                    builder.def_var(var, module_value);
                    var_env.declare_variable(name.clone(), var, *mutable);
                    
                    return Some(module_value);
                }
            }
            
            // Regular let binding (not a module instantiation)
            let var = var_env.next_variable();
            builder.declare_var(var, value_type());
            
            // Initialize with default value to avoid SSA issues
            let default_val = builder.ins().iconst(types::I64, 0);
            builder.def_var(var, default_val);
            
            // Evaluate the value expression
            let value_result = crate::codegen::expr::translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            
            // Store the value in the variable
            builder.def_var(var, value_result);
            
            // Register the variable in our environment
            var_env.declare_variable(name.clone(), var, *mutable);
            
            // Let binding evaluates to the value that was bound
            Some(value_result)
        }
        Expr::TypeDef { name, members } => {
            // Process module definition and register it
            let mut module_info = ModuleInfo::new(name.clone());
            
            // Process module members
            for member in members {
                match member {
                    crate::ast::TypeMember::LetBinding { name: field_name, exported, value, .. } => {
                        module_info.add_field(field_name.clone(), *exported, value.clone());
                    }
                    crate::ast::TypeMember::FunctionDef { .. } => {
                        // Functions are compiled separately, not stored as fields
                        // Skip function definitions here
                    }
                    crate::ast::TypeMember::Conversion { .. } => {
                        // Conversions are compiled separately, not stored as fields
                        // Skip conversion definitions here
                    }
                }
            }
            
            // Register the module type
            module_registry.register_module(name.clone(), module_info);
            
            // Module definition itself evaluates to a type ID (hash for now)
            let hash = name.chars().fold(0i64, |acc, c| acc.wrapping_mul(31).wrapping_add(c as i64));
            Some(builder.ins().iconst(types::I64, hash))
        }
        Expr::Assignment { name, value } => {
            eprintln!("DEBUG: Untyped Assignment to variable '{}' started", name);
            // First check if the variable exists and is mutable
            if let Some(var) = var_env.get_variable(name) {
                if let Some(is_mutable) = var_env.is_mutable(name) {
                    if !is_mutable {
                        eprintln!("DEBUG: Variable '{}' is not mutable", name);
                        // Error: attempting to assign to immutable variable
                        // In a proper implementation, this would be a compile error
                        // For now, we'll just evaluate the value and return it
                        return Some(crate::codegen::expr::translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table));
                    }
                    
                    eprintln!("DEBUG: Variable '{}' is mutable, proceeding with assignment", name);
                    // Evaluate the new value
                    let new_value = crate::codegen::expr::translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
                    eprintln!("DEBUG: New value computed for '{}', calling def_var", name);
                    eprintln!("DEBUG: Variable index for '{}': {:?}", name, var);
                    
                    // Update the variable
                    builder.def_var(var, new_value);
                    
                    // Assignment expressions evaluate to the assigned value
                    Some(new_value)
                } else {
                    // Variable not found in mutability tracking (shouldn't happen)
                    Some(builder.ins().iconst(types::I64, 0))
                }
            } else {
                // Variable not found
                // In a proper implementation, this would be caught during parsing/validation
                Some(builder.ins().iconst(types::I64, 0))
            }
        }
        Expr::Import { .. } => {
            // Import statements should be handled at a higher level during compilation
            // They shouldn't reach code generation
            // For now, return a dummy value
            Some(builder.ins().iconst(types::I64, 0))
        }
        Expr::Void => {
            // Void evaluates to 0
            Some(builder.ins().iconst(types::I64, 0))
        }
        Expr::StringLiteral(s) => {
            // String layout (Zig-inspired):
            // [0..8]   length (u64) - actual string length, NOT including null
            // [8..8+N] data (bytes) - the string content  
            // [8+N]    null (u8)    - always '\0' for C compatibility
            
            let string_bytes = s.as_bytes();
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
            
            // 3. Null terminator (separate from data)
            string_data.push(0);
            
            // Define the data
            let mut data_desc = DataDescription::new();
            data_desc.define(string_data.into_boxed_slice());
            module.define_data(data_id, &data_desc).unwrap();
            
            // Get a reference to the string structure in the current function
            let string_ref = module.declare_data_in_func(data_id, builder.func);
            
            // Return the address of the string structure
            // This points to the length field, so string operations can access all parts
            let string_ptr = builder.ins().global_value(types::I64, string_ref);
            
            // String pointer is already a tagged pointer
            // String pointer is already properly tagged (000)
            Some(string_ptr)
        }
        _ => None // Return None for cases this function doesn't handle
    }
}