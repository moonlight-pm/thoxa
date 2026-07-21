use crate::symbols::{SymbolTable, ImportedBinding};
use crate::ast::Expr;
use cranelift::prelude::*;
use cranelift_module::Module;

/// Extended environment that includes symbol table for import resolution
pub struct SymbolEnv<'a> {
    pub symbol_table: &'a SymbolTable,
    pub var_env: &'a mut VarEnv,
    pub module_registry: &'a mut ModuleRegistry,
    pub func_env: &'a FuncEnv,
}

/// Resolve an identifier through local variables and imports
pub fn resolve_identifier<M: Module>(
    name: &str,
    builder: &mut FunctionBuilder,
    symbol_env: &mut SymbolEnv,
) -> Option<Value> {
    // First check local variables
    if let Some(var) = symbol_env.var_env.get_variable(name) {
        return Some(builder.use_var(var));
    }
    
    // Then check imported bindings
    if let Some(import) = symbol_env.symbol_table.get_import(name) {
        match import {
            ImportedBinding::TypeInstance { module_name, .. } => {
                // For module instances, we need to create or retrieve the instance
                // This is similar to module instantiation in let bindings
                if let Some(module_info) = symbol_env.module_registry.get_module(module_name).cloned() {
                    // Create a new instance of the module
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        module_info.size as u32,
                    ));
                    
                    let base_addr = builder.ins().stack_addr(types::I64, slot, 0);
                    
                    // TODO: Initialize the module fields
                    // For now, just return the base address
                    return Some(base_addr);
                }
            }
            ImportedBinding::TypeInfo { .. } => {
                // Module types can't be used as values
                // This would be a type error in a full implementation
                return None;
            }
            ImportedBinding::DestructuredMember {  .. } => {
                // For destructured members, we need to evaluate the member
                // This is more complex and would require accessing the module's exports
                // For now, return None
                return None;
            }
        }
    }
    
    None
}

/// Resolve a function call (either local or imported)
pub fn resolve_function_call(
    object: &Expr,
    function_name: &str,
    symbol_env: &SymbolEnv,
) -> Option<(String, String)> {
    match object {
        Expr::Identifier(obj_name) => {
            // Check if this is an imported module
            if let Some(import) = symbol_env.symbol_table.get_import(obj_name) {
                match import {
                    ImportedBinding::TypeInstance { module_name, .. } => {
                        // Return the module and function names for lookup
                        return Some((module_name.clone(), function_name.to_string()));
                    }
                    _ => return None,
                }
            }
            
            // Otherwise, check local module instances
            // TODO: Track module types for local variables
            // For now, we can't resolve local module instance function calls through this path
        }
        _ => {}
    }
    
    None
}

// Re-export the necessary types from codegen
pub use crate::codegen::{VarEnv, ModuleRegistry, FuncEnv};