use std::collections::{HashMap, HashSet};
use crate::ast::{Expr, TypeMember, Parameter};

/// Memory category for values - determines reference safety rules
#[derive(Debug, Clone, PartialEq)]
pub enum ValueCategory {
    /// Stack values - local variables, parameters (cannot be referenced)
    Stack,
    /// Static values - module instances, global constants (can be referenced)
    Static,
    /// Heap values - explicitly allocated (can be referenced, future)
    #[allow(dead_code)]
    Heap,
}

/// Represents a type definition with its members and export information
#[derive(Debug, Clone, PartialEq)]
pub struct TypeInfo {
    pub name: String,
    pub members: HashMap<String, TypeMemberInfo>,
    pub exported_members: HashSet<String>,
}

/// Information about a module member
#[derive(Debug, Clone, PartialEq)]
pub enum TypeMemberInfo {
    LetBinding {
        type_expr: Expr,
        value: Expr,
        exported: bool,
    },
    FunctionDef {
        params: Vec<Parameter>,
        return_type: Expr,
        body: Expr,
        exported: bool,
    },
}

/// Type of imported binding
#[derive(Debug, Clone, PartialEq)]
pub enum ImportedBinding {
    /// Type instance (import math from "./Math")
    TypeInstance {
        module_name: String,
        source_path: String,
    },
    /// Type type (import Math from "./Math" - for type usage)
    TypeInfo {
        module_name: String,
        source_path: String,
    },
    /// Destructured member (import { add } from "./Math")
    DestructuredMember {
        member_name: String,
        module_name: String,
        source_path: String,
        member_info: TypeMemberInfo,
    },
}

/// The main symbol table for managing modules and bindings
#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// Registered module types
    pub type_definitions: HashMap<String, TypeInfo>,
    /// Local let bindings (name -> (type, value, memory_category))
    pub let_bindings: HashMap<String, (Expr, Expr, ValueCategory)>,
    /// Imported bindings (name -> import info)
    pub imported_bindings: HashMap<String, ImportedBinding>,
}

impl SymbolTable {
    /// Create a new empty symbol table
    pub fn new() -> Self {
        Self {
            type_definitions: HashMap::new(),
            let_bindings: HashMap::new(),
            imported_bindings: HashMap::new(),
        }
    }

    /// Register a module type from a TypeDef expression
    pub fn register_type(&mut self, name: &str, members: &[TypeMember]) -> Result<(), String> {
        if self.type_definitions.contains_key(name) {
            return Err(format!("Type '{}' is already defined", name));
        }

        let mut module_members = HashMap::new();
        let mut exported_members = HashSet::new();

        for member in members {
            match member {
                TypeMember::LetBinding { name: member_name, type_expr, value, exported } => {
                    if module_members.contains_key(member_name) {
                        return Err(format!("Member '{}' is already defined in module '{}'", member_name, name));
                    }

                    let member_info = TypeMemberInfo::LetBinding {
                        type_expr: type_expr.clone(),
                        value: value.clone(),
                        exported: *exported,
                    };

                    module_members.insert(member_name.clone(), member_info);
                    
                    if *exported {
                        exported_members.insert(member_name.clone());
                    }
                }
                TypeMember::FunctionDef { name: member_name, params, return_type, body, exported } => {
                    if module_members.contains_key(member_name) {
                        return Err(format!("Member '{}' is already defined in module '{}'", member_name, name));
                    }

                    let member_info = TypeMemberInfo::FunctionDef {
                        params: params.clone(),
                        return_type: return_type.clone(),
                        body: body.clone(),
                        exported: *exported,
                    };

                    module_members.insert(member_name.clone(), member_info);
                    
                    if *exported {
                        exported_members.insert(member_name.clone());
                    }
                }
                TypeMember::Conversion { .. } => {
                    // Conversions are not tracked as regular members
                    // They are part of the type system's conversion mechanism
                }
            }
        }

        let module_type = TypeInfo {
            name: name.to_string(),
            members: module_members,
            exported_members,
        };

        self.type_definitions.insert(name.to_string(), module_type);
        Ok(())
    }

    /// Look up a module type by name
    pub fn get_type(&self, name: &str) -> Option<&TypeInfo> {
        self.type_definitions.get(name)
    }

    /// Check if a module member is exported
    pub fn is_member_exported(&self, module_name: &str, member_name: &str) -> Result<bool, String> {
        let module = self.get_type(module_name)
            .ok_or_else(|| format!("Type '{}' not found", module_name))?;
        
        if !module.members.contains_key(member_name) {
            return Err(format!("Member '{}' not found in module '{}'", member_name, module_name));
        }

        Ok(module.exported_members.contains(member_name))
    }

    /// Get information about a module member
    pub fn get_member_info(&self, module_name: &str, member_name: &str) -> Result<&TypeMemberInfo, String> {
        let module = self.get_type(module_name)
            .ok_or_else(|| format!("Type '{}' not found", module_name))?;
        
        module.members.get(member_name)
            .ok_or_else(|| format!("Member '{}' not found in module '{}'", member_name, module_name))
    }

    /// Register a let binding with memory category
    pub fn register_let_binding(&mut self, name: &str, type_expr: Expr, value: Expr, category: ValueCategory) -> Result<(), String> {
        if self.let_bindings.contains_key(name) {
            return Err(format!("Variable '{}' is already defined", name));
        }

        self.let_bindings.insert(name.to_string(), (type_expr, value, category));
        Ok(())
    }

    /// Look up a let binding
    pub fn get_let_binding(&self, name: &str) -> Option<&(Expr, Expr, ValueCategory)> {
        self.let_bindings.get(name)
    }
    
    /// Get the memory category of a let binding
    pub fn get_value_category(&self, name: &str) -> Option<ValueCategory> {
        self.let_bindings.get(name).map(|(_, _, category)| category.clone())
    }
    
    /// Check if a value can be referenced (not Stack category)
    pub fn can_reference(&self, name: &str) -> Result<bool, String> {
        match self.get_value_category(name) {
            Some(ValueCategory::Stack) => Ok(false),
            Some(ValueCategory::Static) | Some(ValueCategory::Heap) => Ok(true),
            None => Err(format!("Variable '{}' not found", name)),
        }
    }

    /// Get all exported members of a module
    pub fn get_exported_members(&self, module_name: &str) -> Result<Vec<String>, String> {
        let module = self.get_type(module_name)
            .ok_or_else(|| format!("Type '{}' not found", module_name))?;
        
        Ok(module.exported_members.iter().cloned().collect())
    }
    
    /// Register an imported binding
    pub fn register_import(&mut self, name: &str, binding: ImportedBinding) -> Result<(), String> {
        // Check for conflicts with existing bindings
        if self.let_bindings.contains_key(name) {
            return Err(format!("Cannot import '{}': conflicts with existing let binding", name));
        }
        
        if self.imported_bindings.contains_key(name) {
            return Err(format!("'{}' has already been imported", name));
        }
        
        self.imported_bindings.insert(name.to_string(), binding);
        Ok(())
    }
    
    /// Look up an imported binding
    pub fn get_import(&self, name: &str) -> Option<&ImportedBinding> {
        self.imported_bindings.get(name)
    }
    
    /// Check if a name is imported
    pub fn is_imported(&self, name: &str) -> bool {
        self.imported_bindings.contains_key(name)
    }
    
    /// Validate a reference expression for memory safety
    pub fn validate_reference(&self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Identifier(name) => {
                if !self.can_reference(name)? {
                    Err(format!(
                        "Cannot reference stack value '{}'. Stack values cannot be referenced to prevent dangling references.", 
                        name
                    ))
                } else {
                    Ok(())
                }
            }
            Expr::FieldAccess { object, field } => {
                // For field access, check if the base object can be referenced
                match object.as_ref() {
                    Expr::Identifier(obj_name) => {
                        if !self.can_reference(obj_name)? {
                            Err(format!(
                                "Cannot reference field '{}' of stack value '{}'. Stack values cannot be referenced to prevent dangling references.", 
                                field, obj_name
                            ))
                        } else {
                            Ok(())
                        }
                    }
                    _ => {
                        // For complex expressions, recursively validate
                        self.validate_reference(object)
                    }
                }
            }
            _ => {
                // For other expressions (literals, operations, etc.), referencing is generally safe
                // but we might want to add more specific rules later
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, TypeMember, Parameter};

    #[test]
    fn test_symbol_table_creation() {
        let symbol_table = SymbolTable::new();
        assert!(symbol_table.type_definitions.is_empty());
        assert!(symbol_table.let_bindings.is_empty());
        assert!(symbol_table.imported_bindings.is_empty());
    }

    #[test]
    fn test_register_simple_module() {
        let mut symbol_table = SymbolTable::new();
        
        let members = vec![
            TypeMember::LetBinding {
                name: "x".to_string(),
                type_expr: Expr::Identifier("I64".to_string()),
                value: Expr::Number(42),
                exported: true,
            }
        ];

        let result = symbol_table.register_type("Point", &members);
        assert!(result.is_ok());
        
        let module = symbol_table.get_type("Point").unwrap();
        assert_eq!(module.name, "Point");
        assert_eq!(module.members.len(), 1);
        assert!(module.exported_members.contains("x"));
    }

    #[test]
    fn test_register_duplicate_module() {
        let mut symbol_table = SymbolTable::new();
        
        let members = vec![];
        
        symbol_table.register_type("Point", &members).unwrap();
        let result = symbol_table.register_type("Point", &members);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already defined"));
    }

    #[test]
    fn test_module_member_access_control() {
        let mut symbol_table = SymbolTable::new();
        
        let members = vec![
            TypeMember::LetBinding {
                name: "public".to_string(),
                type_expr: Expr::Identifier("I64".to_string()),
                value: Expr::Number(1),
                exported: true,
            },
            TypeMember::LetBinding {
                name: "private".to_string(),
                type_expr: Expr::Identifier("I64".to_string()),
                value: Expr::Number(2),
                exported: false,
            }
        ];

        symbol_table.register_type("Test", &members).unwrap();
        
        assert_eq!(symbol_table.is_member_exported("Test", "public").unwrap(), true);
        assert_eq!(symbol_table.is_member_exported("Test", "private").unwrap(), false);
    }

    #[test]
    fn test_let_bindings() {
        let mut symbol_table = SymbolTable::new();
        
        let result = symbol_table.register_let_binding(
            "x", 
            Expr::Identifier("I64".to_string()), 
            Expr::Number(42),
            ValueCategory::Stack
        );
        assert!(result.is_ok());
        
        let binding = symbol_table.get_let_binding("x").unwrap();
        assert_eq!(binding.1, Expr::Number(42));
        assert_eq!(binding.2, ValueCategory::Stack);
        
        // Test duplicate binding
        let duplicate_result = symbol_table.register_let_binding(
            "x", 
            Expr::Identifier("I32".to_string()), 
            Expr::Number(10),
            ValueCategory::Stack
        );
        assert!(duplicate_result.is_err());
    }

    #[test]
    fn test_module_with_functions() {
        let mut symbol_table = SymbolTable::new();
        
        let members = vec![
            TypeMember::FunctionDef {
                name: "add".to_string(),
                params: vec![
                    Parameter {
                        name: "a".to_string(),
                        type_expr: Expr::Identifier("I64".to_string()),
                        variadic: false,
                    },
                    Parameter {
                        name: "b".to_string(),
                        type_expr: Expr::Identifier("I64".to_string()),
                        variadic: false,
                    },
                ],
                return_type: Expr::Identifier("I64".to_string()),
                body: Expr::Binary {
                    left: Box::new(Expr::Identifier("a".to_string())),
                    op: crate::ast::BinaryOp::Add,
                    right: Box::new(Expr::Identifier("b".to_string())),
                },
                exported: true,
            }
        ];

        symbol_table.register_type("Math", &members).unwrap();
        
        let member_info = symbol_table.get_member_info("Math", "add").unwrap();
        if let TypeMemberInfo::FunctionDef { params, exported, .. } = member_info {
            assert_eq!(params.len(), 2);
            assert_eq!(*exported, true);
        } else {
            panic!("Expected FunctionDef");
        }
    }
    
    #[test]
    fn test_memory_categories() {
        let mut symbol_table = SymbolTable::new();
        
        // Register stack value
        symbol_table.register_let_binding(
            "stack_var", 
            Expr::Identifier("I64".to_string()), 
            Expr::Number(42),
            ValueCategory::Stack
        ).unwrap();
        
        // Register static value
        symbol_table.register_let_binding(
            "static_var", 
            Expr::Identifier("I64".to_string()), 
            Expr::Number(100),
            ValueCategory::Static
        ).unwrap();
        
        // Test memory category retrieval
        assert_eq!(symbol_table.get_value_category("stack_var"), Some(ValueCategory::Stack));
        assert_eq!(symbol_table.get_value_category("static_var"), Some(ValueCategory::Static));
        assert_eq!(symbol_table.get_value_category("nonexistent"), None);
        
        // Test reference ability
        assert_eq!(symbol_table.can_reference("stack_var").unwrap(), false);
        assert_eq!(symbol_table.can_reference("static_var").unwrap(), true);
        assert!(symbol_table.can_reference("nonexistent").is_err());
    }
    
    #[test]
    fn test_reference_validation() {
        let mut symbol_table = SymbolTable::new();
        
        // Register test values
        symbol_table.register_let_binding(
            "stack_val", 
            Expr::Identifier("I64".to_string()), 
            Expr::Number(42),
            ValueCategory::Stack
        ).unwrap();
        
        symbol_table.register_let_binding(
            "static_val", 
            Expr::Identifier("I64".to_string()), 
            Expr::Number(100),
            ValueCategory::Static
        ).unwrap();
        
        // Test simple identifier references
        let stack_ref = Expr::Identifier("stack_val".to_string());
        let static_ref = Expr::Identifier("static_val".to_string());
        
        assert!(symbol_table.validate_reference(&stack_ref).is_err());
        assert!(symbol_table.validate_reference(&static_ref).is_ok());
        
        // Test field access references
        let stack_field_ref = Expr::FieldAccess {
            object: Box::new(Expr::Identifier("stack_val".to_string())),
            field: "x".to_string(),
        };
        
        let static_field_ref = Expr::FieldAccess {
            object: Box::new(Expr::Identifier("static_val".to_string())),
            field: "x".to_string(),
        };
        
        assert!(symbol_table.validate_reference(&stack_field_ref).is_err());
        assert!(symbol_table.validate_reference(&static_field_ref).is_ok());
        
        // Test references to literals (should be safe)
        let literal_ref = Expr::Number(42);
        assert!(symbol_table.validate_reference(&literal_ref).is_ok());
    }
    
    #[test]
    fn test_reference_error_messages() {
        let mut symbol_table = SymbolTable::new();
        
        symbol_table.register_let_binding(
            "p", 
            Expr::Identifier("Point".to_string()), 
            Expr::Number(0),
            ValueCategory::Stack
        ).unwrap();
        
        // Test error message for direct reference
        let stack_ref = Expr::Identifier("p".to_string());
        let error = symbol_table.validate_reference(&stack_ref).unwrap_err();
        assert!(error.contains("Cannot reference stack value 'p'"));
        assert!(error.contains("prevent dangling references"));
        
        // Test error message for field reference
        let field_ref = Expr::FieldAccess {
            object: Box::new(Expr::Identifier("p".to_string())),
            field: "x".to_string(),
        };
        let field_error = symbol_table.validate_reference(&field_ref).unwrap_err();
        assert!(field_error.contains("Cannot reference field 'x' of stack value 'p'"));
        assert!(field_error.contains("prevent dangling references"));
    }
}