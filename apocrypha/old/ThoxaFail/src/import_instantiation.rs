use crate::symbols::{SymbolTable, ImportedBinding};
use crate::codegen::{translate_expr, VarEnv, FuncEnv, ModuleRegistry};
use cranelift::prelude::*;
use cranelift_module::Module;
use std::collections::HashMap;

/// Instantiate all imported modules at the beginning of the main function
pub fn instantiate_imports<M: Module>(
    builder: &mut FunctionBuilder,
    var_env: &mut VarEnv,
    func_env: &FuncEnv,
    module_registry: &mut ModuleRegistry,
    module: &mut M,
    symbol_table: &SymbolTable,
) -> Result<HashMap<String, Variable>, String> {
    let mut import_vars = HashMap::new();
    
    // Iterate through all imported bindings
    for (binding_name, import) in &symbol_table.imported_bindings {
        match import {
            ImportedBinding::TypeInstance { module_name, .. } => {
                // Get module info from registry
                if let Some(module_info) = module_registry.get_module(module_name).cloned() {
                    // Allocate stack space for the module instance
                    let slot = builder.create_sized_stack_slot(StackSlotData {
                        kind: StackSlotKind::ExplicitSlot,
                        size: module_info.size as u32,
                    });
                    
                    let base_addr = builder.ins().stack_addr(types::I64, slot, 0);
                    
                    // Create a variable to store the module base address
                    let var = var_env.next_variable();
                    builder.declare_var(var, types::I64);
                    builder.def_var(var, base_addr);
                    
                    // Store the variable for this import (imports are immutable)
                    var_env.declare_variable(binding_name.clone(), var, false);
                    import_vars.insert(binding_name.clone(), var);
                    
                    // Initialize each field
                    for (field_name, _is_exported, init_expr) in module_info.fields.iter() {
                        let init_value = translate_expr(
                            init_expr, 
                            builder, 
                            var_env, 
                            func_env, 
                            module_registry, 
                            module, 
                            symbol_table
                        );
                        
                        if let Some(offset) = module_info.get_field_offset(field_name) {
                            let offset_val = (offset * 8) as i32;
                            builder.ins().store(MemFlags::new(), init_value, base_addr, offset_val);
                        }
                    }
                } else {
                    return Err(format!("Module '{}' not found in registry", module_name));
                }
            }
            ImportedBinding::TypeInfo { .. } => {
                // Module types don't need instantiation
            }
            ImportedBinding::DestructuredMember { .. } => {
                // TODO: Handle destructured imports
            }
        }
    }
    
    Ok(import_vars)
}