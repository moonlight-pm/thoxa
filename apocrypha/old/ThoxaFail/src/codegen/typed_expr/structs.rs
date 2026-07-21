use crate::ast::{Expr, TypedExpr};
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry, translate_typed_expr, translate_expr, value_type};
use crate::codegen::tagged_values::extract_value;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Handle struct-related expressions (field access, let bindings with struct instantiation)
pub fn translate_structs<M: Module>(
    typed_expr: &TypedExpr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable,
) -> Value {
    match typed_expr {
        TypedExpr::FieldAccess { object, field, ty } => {
            // Handle field access for module instances
            match object.as_ref() {
                TypedExpr::Identifier { name: obj_name, .. } => {
                    // First check if this is an imported module
                    if let Some(import) = symbol_table.get_import(obj_name) {
                        use crate::symbols::ImportedBinding;
                        if let ImportedBinding::TypeInstance { module_name, .. } = import {
                            // Look up the field in the module's symbol table
                            if let Some(module_type) = symbol_table.get_type(module_name) {
                                if let Some(member_info) = module_type.members.get(field) {
                                    use crate::symbols::TypeMemberInfo;
                                    if let TypeMemberInfo::LetBinding { value, exported, .. } = member_info {
                                        if *exported {
                                            // Evaluate the field's initialization expression
                                            return translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Get the module instance base address
                    if let Some(var) = var_env.get_variable(obj_name) {
                        let base_addr_tagged = builder.use_var(var);
                        
                        // Extract the actual address from tagged pointer if needed
                        let base_addr = extract_value(builder, base_addr_tagged);
                        
                        // Try to determine the module type from the variable name
                        // This is a simplified approach - in a full implementation,
                        // we'd track variable types properly
                        for (_, module_info) in &module_registry.modules {
                            if let Some(offset) = module_info.get_field_offset(field) {
                                let offset_val = (offset * 8) as i32; // Each field is 8 bytes
                                // Load the value from the field offset
                                let loaded_value = builder.ins().load(types::I64, MemFlags::new(), base_addr, offset_val);
                                
                                // Return the loaded value
                                return loaded_value;
                            }
                        }
                        
                        // Field not found, return 0
                        builder.ins().iconst(types::I64, 0)
                    } else {
                        // Object not found, return 0
                        builder.ins().iconst(types::I64, 0)
                    }
                }
                _ => {
                    // For complex expressions, evaluate the object first
                    let obj_addr_tagged = translate_typed_expr(object, builder, var_env, func_env, module_registry, module, symbol_table);
                    
                    // Extract the actual address from tagged pointer if needed
                    let obj_addr = extract_value(builder, obj_addr_tagged);
                    
                    // Try to find the field in any module type
                    // This is simplified - we'd need type information in practice
                    for (_, module_info) in &module_registry.modules {
                        if let Some(offset) = module_info.get_field_offset(field) {
                            let offset_val = (offset * 8) as i32;
                            let loaded_value = builder.ins().load(types::I64, MemFlags::new(), obj_addr, offset_val);
                            
                            // Return the loaded value
                            return loaded_value;
                        }
                    }
                    
                    // Field not found
                    builder.ins().iconst(types::I64, 0)
                }
            }
        }
        
        TypedExpr::LetBinding { name, type_expr, value, mutable, ty: _, .. } => {
            // First resolve the actual type from type_expr
            let actual_type = if let Expr::Identifier(type_name) = type_expr.as_ref() {
                // Look up the type name
                if let Some(module_info) = module_registry.get_type(type_name) {
                    // This is a struct type
                    Some((type_name.clone(), module_info.clone()))
                } else {
                    None
                }
            } else {
                None
            };
            
            // Check if this is a module instantiation
            if let Some((_type_name, module_info)) = actual_type {
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
                    let init_value = translate_expr(init_expr, builder, var_env, func_env, module_registry, module, symbol_table);
                    
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
                
                let module_value = base_addr;
                
                builder.def_var(var, module_value);
                var_env.declare_variable(name.clone(), var, *mutable);
                
                return module_value;
            }
            
            // Regular let binding (not a module instantiation)
            let var = var_env.next_variable();
            builder.declare_var(var, value_type());
            
            // Initialize with default value to avoid SSA issues
            let default_val = builder.ins().iconst(types::I64, 0);
            builder.def_var(var, default_val);
            
            // Evaluate the value expression
            let value_result = translate_typed_expr(value, builder, var_env, func_env, module_registry, module, symbol_table);
            
            // Store the value in the variable
            builder.def_var(var, value_result);
            
            // Register the variable in our environment
            var_env.declare_variable(name.clone(), var, *mutable);
            
            // Let binding evaluates to the value that was bound
            value_result
        }
        
        TypedExpr::Reference { expr, ty: _ } => {
            // For now, just evaluate the expression
            // In the future, this might create actual references
            translate_typed_expr(expr, builder, var_env, func_env, module_registry, module, symbol_table)
        }
        
        _ => panic!("Not a struct-related expression"),
    }
}