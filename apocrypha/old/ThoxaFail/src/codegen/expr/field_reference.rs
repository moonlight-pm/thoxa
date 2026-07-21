use crate::ast::Expr;
use crate::codegen::{VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;
use cranelift_module::Module;

use super::main::translate_expr;

pub fn translate_field_reference<M: Module>(
    expr: &Expr,
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &crate::symbols::SymbolTable
) -> Option<Value> {
    match expr {
        Expr::FieldAccess { object, field } => {
            // Handle field access for module instances (non-method calls only)
            match object.as_ref() {
                Expr::Identifier(obj_name) => {
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
                                            return Some(translate_expr(value, builder, var_env, func_env, module_registry, module, symbol_table));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Get the module instance base address
                    if let Some(var) = var_env.get_variable(obj_name) {
                        let base_addr = builder.use_var(var);
                        
                        // Try to determine the module type from the variable name
                        // This is a simplified approach - in a full implementation,
                        // we'd track variable types properly
                        for (_, module_info) in &module_registry.modules {
                            if let Some(offset) = module_info.get_field_offset(field) {
                                let offset_val = (offset * 8) as i32; // Each field is 8 bytes
                                // Load the value from the field offset
                                return Some(builder.ins().load(types::I64, MemFlags::new(), base_addr, offset_val));
                            }
                        }
                        
                        // Field not found, return 0
                        Some(builder.ins().iconst(types::I64, 0))
                    } else {
                        // Object not found, return 0
                        Some(builder.ins().iconst(types::I64, 0))
                    }
                }
                _ => {
                    // For complex expressions, evaluate the object first
                    let obj_addr = translate_expr(object, builder, var_env, func_env, module_registry, module, symbol_table);
                    
                    // Try to find the field in any module type
                    // This is simplified - we'd need type information in practice
                    for (_, module_info) in &module_registry.modules {
                        if let Some(offset) = module_info.get_field_offset(field) {
                            let offset_val = (offset * 8) as i32;
                            return Some(builder.ins().load(types::I64, MemFlags::new(), obj_addr, offset_val));
                        }
                    }
                    
                    // Field not found
                    Some(builder.ins().iconst(types::I64, 0))
                }
            }
        }
        Expr::Reference { expr } => {
            // Generate address for the referenced expression
            match expr.as_ref() {
                Expr::Identifier(name) => {
                    // For identifiers, we need to distinguish between different types
                    if let Some(var) = var_env.get_variable(name) {
                        // Check if this is a module instance (already an address)
                        // For now, we'll assume module instances store addresses
                        // and regular variables need their address taken
                        // In a full implementation, we'd track variable types
                        Some(builder.use_var(var))
                    } else {
                        // Unknown identifier
                        Some(builder.ins().iconst(types::I64, 0))
                    }
                }
                Expr::FieldAccess { object, field } => {
                    // For field access, compute the address of the field
                    match object.as_ref() {
                        Expr::Identifier(obj_name) => {
                            if let Some(var) = var_env.get_variable(obj_name) {
                                let base_addr = builder.use_var(var);
                                
                                // Find the field offset
                                for (_, module_info) in &module_registry.modules {
                                    if let Some(offset) = module_info.get_field_offset(field) {
                                        let offset_val = (offset * 8) as i64;
                                        // Return the address of the field
                                        return Some(builder.ins().iadd_imm(base_addr, offset_val));
                                    }
                                }
                                
                                // Field not found
                                Some(builder.ins().iconst(types::I64, 0))
                            } else {
                                Some(builder.ins().iconst(types::I64, 0))
                            }
                        }
                        _ => {
                            // For complex expressions, evaluate to get base address
                            let obj_addr = translate_expr(object, builder, var_env, func_env, module_registry, module, symbol_table);
                            
                            // Find the field offset
                            for (_, module_info) in &module_registry.modules {
                                if let Some(offset) = module_info.get_field_offset(field) {
                                    let offset_val = (offset * 8) as i64;
                                    return Some(builder.ins().iadd_imm(obj_addr, offset_val));
                                }
                            }
                            
                            // Field not found
                            Some(builder.ins().iconst(types::I64, 0))
                        }
                    }
                }
                _ => {
                    // For other expressions, we can't take their address
                    // This should be caught by validation
                    Some(builder.ins().iconst(types::I64, 0))
                }
            }
        }
        _ => None
    }
}