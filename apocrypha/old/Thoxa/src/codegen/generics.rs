//! Generic type instantiation and monomorphization

use std::collections::HashMap;
use crate::ast::{Type, UnitDef, UnionDef};
use crate::codegen::types::{TypeId, TypeRegistry};
use crate::error::{Error, Result};
use crate::lexer::Span;

/// Represents a generic type definition before instantiation
#[derive(Debug, Clone)]
pub struct GenericTypeDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub kind: GenericTypeKind,
}

#[derive(Debug, Clone)]
pub enum GenericTypeKind {
    Unit(UnitDef),
    Union(UnionDef),
}

/// Represents an instantiated generic type (after monomorphization)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeInstantiation {
    pub generic_name: String,           // e.g., "Box"
    pub type_args: Vec<TypeId>,         // e.g., [TypeId::INT]
    pub monomorphized_name: String,     // e.g., "Box_Int"
}

/// Manages generic types and their instantiations
pub struct GenericRegistry {
    /// Generic type definitions (not yet instantiated)
    generic_types: HashMap<String, GenericTypeDef>,
    
    /// Maps instantiations to their generated TypeIds
    instantiations: HashMap<TypeInstantiation, TypeId>,
    
    /// Tracks which generic functions have been instantiated
    pub function_instantiations: HashMap<(String, Vec<TypeId>), String>,
}

impl GenericRegistry {
    pub fn new() -> Self {
        GenericRegistry {
            generic_types: HashMap::new(),
            instantiations: HashMap::new(),
            function_instantiations: HashMap::new(),
        }
    }
    
    /// Register a generic type definition
    pub fn register_generic_type(&mut self, def: GenericTypeDef) {
        self.generic_types.insert(def.name.clone(), def);
    }
    
    /// Check if a type is generic
    pub fn is_generic_type(&self, name: &str) -> bool {
        self.generic_types.contains_key(name)
    }
    
    /// Get or create an instantiation of a generic type
    pub fn get_or_create_instantiation(
        &mut self,
        generic_name: &str,
        type_args: Vec<TypeId>,
        type_registry: &mut TypeRegistry,
    ) -> Result<TypeId> {
        // Check if this instantiation already exists
        let monomorphized_name = Self::generate_monomorphized_name(generic_name, &type_args);
        
        let instantiation = TypeInstantiation {
            generic_name: generic_name.to_string(),
            type_args: type_args.clone(),
            monomorphized_name: monomorphized_name.clone(),
        };
        
        if let Some(&type_id) = self.instantiations.get(&instantiation) {
            return Ok(type_id);
        }
        
        // Create new instantiation
        let generic_def = self.generic_types.get(generic_name)
            .ok_or_else(|| Error::Codegen(format!("Unknown generic type: {}", generic_name)))?
            .clone();
        
        // Verify correct number of type arguments
        if type_args.len() != generic_def.type_params.len() {
            return Err(Error::Codegen(format!(
                "Type {} expects {} type arguments, got {}",
                generic_name,
                generic_def.type_params.len(),
                type_args.len()
            )));
        }
        
        // Perform the instantiation based on the kind of generic type
        let type_id = match generic_def.kind {
            GenericTypeKind::Unit(unit_def) => {
                self.instantiate_unit(unit_def, &generic_def.type_params, &type_args, &monomorphized_name, type_registry)?
            }
            GenericTypeKind::Union(union_def) => {
                self.instantiate_union(union_def, &generic_def.type_params, &type_args, &monomorphized_name, type_registry)?
            }
        };
        
        // Cache the instantiation
        self.instantiations.insert(instantiation, type_id);
        
        Ok(type_id)
    }
    
    /// Instantiate a generic unit type
    fn instantiate_unit(
        &self,
        unit_def: UnitDef,
        type_params: &[String],
        type_args: &[TypeId],
        monomorphized_name: &str,
        type_registry: &mut TypeRegistry,
    ) -> Result<TypeId> {
        // Extract fields from the body statements
        let mut fields = Vec::new();
        for stmt in &unit_def.body {
            if let crate::ast::Stmt::Let { name, ty, is_mut, .. } = stmt {
                if let Some(field_type_ast) = ty {
                    // Substitute the type before converting to TypeId
                    let substituted_type = self.substitute_type(field_type_ast, type_params, type_args)?;
                    // For now, we need to resolve the substituted type
                    // This is a bit circular, but we'll handle it by using the type names
                    let field_type = Self::resolve_substituted_type(&substituted_type, type_registry)?;
                    fields.push((name.clone(), field_type, *is_mut));
                }
            }
        }
        
        // Extract constructor parameters
        let mut constructor_params = Vec::new();
        for param in &unit_def.constructor_params {
            let substituted_type = self.substitute_type(&param.ty, type_params, type_args)?;
            let param_type = Self::resolve_substituted_type(&substituted_type, type_registry)?;
            constructor_params.push((param.name.clone(), param_type));
        }
        
        // Register the monomorphized type
        Ok(type_registry.register_unit_type(
            monomorphized_name.to_string(),
            fields,
            constructor_params,
        ))
    }
    
    /// Resolve a substituted type to a TypeId
    fn resolve_substituted_type(ty: &Type, type_registry: &TypeRegistry) -> Result<TypeId> {
        match ty {
            Type::Named { name, args, .. } => {
                if !args.is_empty() {
                    // This would be a nested generic type, not supported yet
                    return Err(Error::Codegen(format!("Nested generic types not yet supported: {}", name)));
                }
                
                // Map the type name to TypeId
                match name.as_str() {
                    "Void" => Ok(TypeId::VOID),
                    "Bool" => Ok(TypeId::BOOL),
                    "Int" => Ok(TypeId::INT),
                    "Float" => Ok(TypeId::FLOAT),
                    "Char" => Ok(TypeId::CHAR),
                    "String" => Ok(TypeId::STRING),
                    _ => {
                        // Look up in the type registry
                        type_registry.get_type_id(name)
                            .ok_or_else(|| Error::Codegen(format!("Unknown type: {}", name)))
                    }
                }
            }
            _ => Err(Error::Codegen("Complex types in generics not yet supported".to_string())),
        }
    }
    
    /// Instantiate a generic union type
    fn instantiate_union(
        &self,
        union_def: UnionDef,
        type_params: &[String],
        type_args: &[TypeId],
        monomorphized_name: &str,
        type_registry: &mut TypeRegistry,
    ) -> Result<TypeId> {
        // Substitute types in all variants
        let mut variant_type_ids = Vec::new();
        for variant_type in &union_def.variants {
            let substituted_variant = self.substitute_type(variant_type, type_params, type_args)?;
            // For union variants, we need to ensure each variant is a registered type
            // For now, we'll just resolve the type names
            let variant_id = Self::resolve_substituted_type(&substituted_variant, type_registry)?;
            variant_type_ids.push(variant_id);
        }
        
        // Register the monomorphized union type
        Ok(type_registry.register_union_type(
            monomorphized_name.to_string(),
            variant_type_ids,
            None, // No type params since it's monomorphized
        ))
    }
    
    /// Generate a unique name for a monomorphized type
    pub fn generate_monomorphized_name(base_name: &str, type_args: &[TypeId]) -> String {
        if type_args.is_empty() {
            return base_name.to_string();
        }
        
        let mut name = base_name.to_string();
        for arg in type_args {
            name.push('_');
            name.push_str(&Self::type_id_to_suffix(*arg));
        }
        name
    }
    
    /// Convert a TypeId to a string suffix for monomorphized names
    fn type_id_to_suffix(type_id: TypeId) -> String {
        match type_id {
            TypeId::VOID => "Void".to_string(),
            TypeId::BOOL => "Bool".to_string(),
            TypeId::INT => "Int".to_string(),
            TypeId::FLOAT => "Float".to_string(),
            TypeId::CHAR => "Char".to_string(),
            TypeId::STRING => "String".to_string(),
            _ => format!("Type{}", type_id.0),
        }
    }
    
    /// Convert a TypeId to an AST Type
    fn type_id_to_type(&self, type_id: TypeId, span: Span) -> Type {
        Type::Named {
            name: Self::type_id_to_suffix(type_id),
            args: vec![],
            span,
        }
    }
    
    /// Perform type substitution on an AST type
    pub fn substitute_type(
        &self,
        ty: &Type,
        type_params: &[String],
        type_args: &[TypeId],
    ) -> Result<Type> {
        match ty {
            Type::Named { name, args, span } => {
                // Check if this is a type parameter
                if let Some(index) = type_params.iter().position(|p| p == name) {
                    // This is a type parameter, substitute it
                    if index >= type_args.len() {
                        return Err(Error::Codegen(format!(
                            "Type parameter {} not provided",
                            name
                        )));
                    }
                    
                    // Convert TypeId back to Type
                    Ok(self.type_id_to_type(type_args[index], *span))
                } else {
                    // Regular type, recursively substitute its arguments
                    let substituted_args = args.iter()
                        .map(|arg| self.substitute_type(arg, type_params, type_args))
                        .collect::<Result<Vec<_>>>()?;
                    
                    Ok(Type::Named {
                        name: name.clone(),
                        args: substituted_args,
                        span: *span,
                    })
                }
            }
            Type::Array { elem, span } => {
                let substituted_elem = self.substitute_type(elem, type_params, type_args)?;
                Ok(Type::Array {
                    elem: Box::new(substituted_elem),
                    span: *span,
                })
            }
            Type::Function { params, ret, span } => {
                let substituted_params = params.iter()
                    .map(|p| self.substitute_type(p, type_params, type_args))
                    .collect::<Result<Vec<_>>>()?;
                let substituted_ret = self.substitute_type(ret, type_params, type_args)?;
                
                Ok(Type::Function {
                    params: substituted_params,
                    ret: Box::new(substituted_ret),
                    span: *span,
                })
            }
            Type::Tuple { elems, span } => {
                let substituted_elems = elems.iter()
                    .map(|e| self.substitute_type(e, type_params, type_args))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(Type::Tuple {
                    elems: substituted_elems,
                    span: *span,
                })
            }
            Type::Union { variants, span } => {
                let substituted_variants = variants.iter()
                    .map(|v| self.substitute_type(v, type_params, type_args))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(Type::Union {
                    variants: substituted_variants,
                    span: *span,
                })
            }
        }
    }
    
    /// Instantiate a generic function with specific type arguments
    pub fn instantiate_function(
        &mut self,
        func_name: &str,
        type_args: Vec<TypeId>,
        ast_functions: &HashMap<String, crate::ast::Function>,
        codegen: &mut super::CodeGen,
    ) -> Result<String> {
        // Check if this instantiation already exists
        let key = (func_name.to_string(), type_args.clone());
        if let Some(instantiated_name) = self.function_instantiations.get(&key) {
            return Ok(instantiated_name.clone());
        }
        
        // Get the generic function definition
        let generic_func = ast_functions.get(func_name)
            .ok_or_else(|| Error::Codegen(format!("Unknown generic function: {}", func_name)))?;
        
        // Verify it's actually generic
        if generic_func.type_params.is_empty() {
            return Err(Error::Codegen(format!("Function {} is not generic", func_name)));
        }
        
        // Verify correct number of type arguments
        if type_args.len() != generic_func.type_params.len() {
            return Err(Error::Codegen(format!(
                "Function {} expects {} type arguments, got {}",
                func_name,
                generic_func.type_params.len(),
                type_args.len()
            )));
        }
        
        // Generate the monomorphized name
        let instantiated_name = Self::generate_monomorphized_name(func_name, &type_args);
        
        // Clone the function and substitute types
        let mut instantiated_func = generic_func.clone();
        instantiated_func.name = instantiated_name.clone();
        instantiated_func.type_params.clear(); // No longer generic
        
        // Create type parameter mapping
        let type_param_map: HashMap<String, TypeId> = generic_func.type_params
            .iter()
            .zip(type_args.iter())
            .map(|(param, &type_id)| (param.clone(), type_id))
            .collect();
        
        // Substitute types in parameters
        for param in &mut instantiated_func.params {
            param.ty = self.substitute_type_with_map(&param.ty, &type_param_map, &codegen.type_registry)?;
        }
        
        // Substitute types in return type
        if let Some(ref mut ret_ty) = instantiated_func.return_type {
            *ret_ty = self.substitute_type_with_map(ret_ty, &type_param_map, &codegen.type_registry)?;
        }
        
        // TODO: Also substitute types in the function body
        // For now, we'll generate the function with the substituted signature
        
        // Generate code for the instantiated function
        codegen.gen_function(&instantiated_func)?;
        
        // Store the instantiation for future use
        self.function_instantiations.insert(key, instantiated_name.clone());
        
        Ok(instantiated_name)
    }
    
    /// Substitute types in an AST type using a type parameter map
    fn substitute_type_with_map(
        &self,
        ty: &Type,
        type_param_map: &HashMap<String, TypeId>,
        type_registry: &TypeRegistry,
    ) -> Result<Type> {
        match ty {
            Type::Named { name, args, span } => {
                // Check if this is a type parameter that needs substitution
                if let Some(&type_id) = type_param_map.get(name) {
                    // Convert TypeId back to Type
                    let type_info = type_registry.get(type_id)
                        .ok_or_else(|| Error::Codegen(format!("Unknown type ID: {:?}", type_id)))?;
                    
                    Ok(Type::Named {
                        name: type_info.name.clone(),
                        args: vec![],
                        span: *span,
                    })
                } else {
                    // Recursively substitute in type arguments
                    let substituted_args = args.iter()
                        .map(|arg| self.substitute_type_with_map(arg, type_param_map, type_registry))
                        .collect::<Result<Vec<_>>>()?;
                    
                    Ok(Type::Named {
                        name: name.clone(),
                        args: substituted_args,
                        span: *span,
                    })
                }
            }
            Type::Array { elem, span } => {
                let substituted_elem = self.substitute_type_with_map(elem, type_param_map, type_registry)?;
                Ok(Type::Array {
                    elem: Box::new(substituted_elem),
                    span: *span,
                })
            }
            Type::Function { params, ret, span } => {
                let substituted_params = params.iter()
                    .map(|p| self.substitute_type_with_map(p, type_param_map, type_registry))
                    .collect::<Result<Vec<_>>>()?;
                let substituted_ret = self.substitute_type_with_map(ret, type_param_map, type_registry)?;
                
                Ok(Type::Function {
                    params: substituted_params,
                    ret: Box::new(substituted_ret),
                    span: *span,
                })
            }
            Type::Tuple { elems, span } => {
                let substituted_elems = elems.iter()
                    .map(|e| self.substitute_type_with_map(e, type_param_map, type_registry))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(Type::Tuple {
                    elems: substituted_elems,
                    span: *span,
                })
            }
            Type::Union { variants, span } => {
                let substituted_variants = variants.iter()
                    .map(|v| self.substitute_type_with_map(v, type_param_map, type_registry))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(Type::Union {
                    variants: substituted_variants,
                    span: *span,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_monomorphized_names() {
        let name = GenericRegistry::generate_monomorphized_name("Box", &[TypeId::INT]);
        assert_eq!(name, "Box_Int");
        
        let name = GenericRegistry::generate_monomorphized_name("Map", &[TypeId::STRING, TypeId::INT]);
        assert_eq!(name, "Map_String_Int");
        
        let name = GenericRegistry::generate_monomorphized_name("Simple", &[]);
        assert_eq!(name, "Simple");
    }
}