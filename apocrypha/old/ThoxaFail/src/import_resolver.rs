use std::collections::HashMap;
use crate::ast::{Expr, ImportSpec, TypeMember};
use crate::modules::CompiledModule;
use crate::symbols::SymbolTable;

/// Handles import resolution and binding
pub struct ImportResolver {
    /// Compiled modules by path
    modules: HashMap<String, CompiledModule>,
    /// Module instances (singleton per module)
    module_instances: HashMap<String, TypeInstance>,
}

/// A runtime module instance
#[derive(Debug, Clone)]
pub struct TypeInstance {
    pub module_name: String,
    pub exports: HashMap<String, ExportedValue>,
}

/// An exported value from a module
#[derive(Debug, Clone)]
pub enum ExportedValue {
    Variable(String),      // Name of the variable
    Function(String),      // Name of the function
    Module(TypeInstance), // Nested module
}

impl ImportResolver {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            module_instances: HashMap::new(),
        }
    }
    
    /// Register a compiled module
    pub fn register_type(&mut self, path: String, module: CompiledModule) {
        self.modules.insert(path, module);
    }
    
    /// Resolve imports for a module and update the symbol table
    pub fn resolve_imports(
        &mut self,
        module: &CompiledModule,
        symbol_table: &mut SymbolTable,
    ) -> Result<(), String> {
        for import in &module.imports {
            let imported_path = import.resolved_path.to_str()
                .ok_or_else(|| format!("Invalid path: {:?}", import.resolved_path))?
                .to_string();
            
            // Get or create the module instance
            let instance = self.get_or_create_instance(&imported_path)?;
            
            // Bind the imports according to the import spec
            match &import.spec {
                ImportSpec::Default(name) => {
                    // import name from "path" - bind the default instance
                    self.bind_default_import(name, &instance, symbol_table)?;
                }
                ImportSpec::TypeAndInstance { type_name, instance_name } => {
                    // import Type, instance from "path"
                    self.bind_type_import(type_name, &instance, symbol_table)?;
                    self.bind_default_import(instance_name, &instance, symbol_table)?;
                }
                ImportSpec::Destructured(names) => {
                    // import { name1, name2 } from "path"
                    for name in names {
                        self.bind_destructured_import(name, &instance, symbol_table)?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Get or create a module instance
    fn get_or_create_instance(&mut self, path: &str) -> Result<TypeInstance, String> {
        if let Some(instance) = self.module_instances.get(path) {
            return Ok(instance.clone());
        }
        
        // Create new instance from compiled module
        let module = self.modules.get(path)
            .ok_or_else(|| format!("Module not found: {}", path))?;
        
        let instance = self.create_instance_from_ast(&module.ast)?;
        self.module_instances.insert(path.to_string(), instance.clone());
        
        Ok(instance)
    }
    
    /// Create a module instance from AST
    fn create_instance_from_ast(&self, ast: &Expr) -> Result<TypeInstance, String> {
        match ast {
            Expr::TypeDef { name, members } => {
                let mut exports = HashMap::new();
                
                for member in members {
                    match member {
                        TypeMember::LetBinding { name, exported, .. } => {
                            if *exported {
                                exports.insert(
                                    name.clone(),
                                    ExportedValue::Variable(name.clone())
                                );
                            }
                        }
                        TypeMember::FunctionDef { name, exported, .. } => {
                            if *exported {
                                exports.insert(
                                    name.clone(),
                                    ExportedValue::Function(name.clone())
                                );
                            }
                        }
                        TypeMember::Conversion { .. } => {
                            // Conversions are not exported as named values
                            // They are part of the type's interface
                        }
                    }
                }
                
                Ok(TypeInstance {
                    module_name: name.clone(),
                    exports,
                })
            }
            _ => Err("Expected module definition".to_string()),
        }
    }
    
    /// Bind a default import
    fn bind_default_import(
        &self,
        name: &str,
        instance: &TypeInstance,
        symbol_table: &mut SymbolTable,
    ) -> Result<(), String> {
        use crate::symbols::ImportedBinding;
        
        // Register the imported module instance
        let binding = ImportedBinding::TypeInstance {
            module_name: instance.module_name.clone(),
            source_path: String::new(), // TODO: track source path
        };
        
        symbol_table.register_import(name, binding)?;
        Ok(())
    }
    
    /// Bind a type import
    fn bind_type_import(
        &self,
        name: &str,
        instance: &TypeInstance,
        symbol_table: &mut SymbolTable,
    ) -> Result<(), String> {
        use crate::symbols::ImportedBinding;
        
        // Register the imported module type
        let binding = ImportedBinding::TypeInfo {
            module_name: instance.module_name.clone(),
            source_path: String::new(), // TODO: track source path
        };
        
        symbol_table.register_import(name, binding)?;
        Ok(())
    }
    
    /// Bind a destructured import
    fn bind_destructured_import(
        &self,
        name: &str,
        instance: &TypeInstance,
        symbol_table: &mut SymbolTable,
    ) -> Result<(), String> {
        use crate::symbols::ImportedBinding;
        
        // Check if the name is exported
        if !instance.exports.contains_key(name) {
            return Err(format!(
                "Module '{}' does not export '{}'",
                instance.module_name, name
            ));
        }
        
        // Get the member info from the module type
        let module_type = symbol_table.get_type(&instance.module_name)
            .ok_or_else(|| format!("Module type '{}' not found", instance.module_name))?;
        
        let member_info = module_type.members.get(name)
            .ok_or_else(|| format!("Member '{}' not found in module '{}'", name, instance.module_name))?
            .clone();
        
        // Register the destructured import
        let binding = ImportedBinding::DestructuredMember {
            member_name: name.to_string(),
            module_name: instance.module_name.clone(),
            source_path: String::new(), // TODO: track source path
            member_info,
        };
        
        symbol_table.register_import(name, binding)?;
        Ok(())
    }
}