//! Type inference for generic functions

use std::collections::HashMap;
use crate::ast::Type as AstType;
use crate::codegen::types::TypeId;
use crate::error::{Error, Result};

/// Infer type arguments for a generic function call based on the provided arguments
pub fn infer_type_arguments(
    type_params: &[String],
    param_types: &[AstType],
    arg_types: &[TypeId],
    type_registry: &crate::codegen::types::TypeRegistry,
) -> Result<Vec<TypeId>> {
    if type_params.is_empty() {
        return Ok(Vec::new());
    }
    
    // Build a mapping from type parameters to their inferred types
    let mut type_map: HashMap<String, TypeId> = HashMap::new();
    
    // Iterate through parameters and arguments to infer types
    for (param_type, &arg_type_id) in param_types.iter().zip(arg_types.iter()) {
        infer_from_type(param_type, arg_type_id, &mut type_map, type_registry)?;
    }
    
    // Check that all type parameters were inferred
    let mut result = Vec::new();
    for type_param in type_params {
        let inferred_type = type_map.get(type_param)
            .ok_or_else(|| Error::Codegen(format!(
                "Could not infer type for type parameter '{}'",
                type_param
            )))?;
        result.push(*inferred_type);
    }
    
    Ok(result)
}

/// Recursively infer type parameters from a type pattern
fn infer_from_type(
    param_type: &AstType,
    arg_type_id: TypeId,
    type_map: &mut HashMap<String, TypeId>,
    type_registry: &crate::codegen::types::TypeRegistry,
) -> Result<()> {
    match param_type {
        AstType::Named { name, args, .. } => {
            // Check if this is a type parameter (single uppercase letter for now)
            if name.len() == 1 && name.chars().next().unwrap().is_uppercase() {
                // This is a type parameter
                if let Some(&existing) = type_map.get(name) {
                    // Check consistency
                    if existing != arg_type_id {
                        return Err(Error::Codegen(format!(
                            "Type parameter '{}' has conflicting inferences",
                            name
                        )));
                    }
                } else {
                    // Record the inference
                    type_map.insert(name.clone(), arg_type_id);
                }
            } else if !args.is_empty() {
                // This is a generic type like Box<T>
                // For now, we can't decompose generic instantiations
                // TODO: Enhance type system to track type arguments in instantiated types
                if let Some(type_info) = type_registry.get(arg_type_id) {
                    // Check if the base matches
                    if !type_info.name.starts_with(name) {
                        return Err(Error::Codegen(format!(
                            "Type mismatch: expected {}, got {}",
                            name, type_info.name
                        )));
                    }
                }
            }
            // Otherwise it's a concrete type, no inference needed
        }
        AstType::Array { .. } => {
            // Arrays are represented as Ptr in the type system
            // We can't currently infer the element type from a Ptr
            // TODO: Enhance type system to track array element types
        }
        AstType::Function { .. } => {
            // Function types are not yet fully supported
            // TODO: Implement when function types are added
        }
        AstType::Tuple { .. } => {
            // Tuple types need special handling
            // TODO: Implement tuple type inference
        }
        AstType::Union { .. } => {
            // Union types need special handling
            // TODO: Implement union type inference
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_type_inference() {
        use crate::lexer::Span;
        let type_registry = crate::codegen::types::TypeRegistry::new();
        let span = Span { start: 0, end: 0, line: 1, column: 1 };
        
        // identity<T>(x: T) -> T called with Int argument
        let type_params = vec!["T".to_string()];
        let param_types = vec![AstType::Named { 
            name: "T".to_string(),
            args: vec![],
            span: span.clone(),
        }];
        let arg_types = vec![TypeId::INT64];
        
        let result = infer_type_arguments(
            &type_params,
            &param_types,
            &arg_types,
            &type_registry
        ).unwrap();
        
        assert_eq!(result, vec![TypeId::INT64]);
    }
    
    #[test]
    fn test_multiple_param_inference() {
        use crate::lexer::Span;
        let type_registry = crate::codegen::types::TypeRegistry::new();
        let span = Span { start: 0, end: 0, line: 1, column: 1 };
        
        // swap<A, B>(a: A, b: B) -> (B, A) called with (Int, String)
        let type_params = vec!["A".to_string(), "B".to_string()];
        let param_types = vec![
            AstType::Named { 
                name: "A".to_string(),
                args: vec![],
                span: span.clone(),
            },
            AstType::Named { 
                name: "B".to_string(),
                args: vec![],
                span: span.clone(),
            },
        ];
        let arg_types = vec![TypeId::INT64, TypeId::STRING];
        
        let result = infer_type_arguments(
            &type_params,
            &param_types,
            &arg_types,
            &type_registry
        ).unwrap();
        
        assert_eq!(result, vec![TypeId::INT64, TypeId::STRING]);
    }
    
    #[test]
    fn test_conflicting_inference() {
        use crate::lexer::Span;
        let type_registry = crate::codegen::types::TypeRegistry::new();
        let span = Span { start: 0, end: 0, line: 1, column: 1 };
        
        // equals<T>(a: T, b: T) -> Bool called with (Int, String)
        let type_params = vec!["T".to_string()];
        let param_types = vec![
            AstType::Named { 
                name: "T".to_string(),
                args: vec![],
                span: span.clone(),
            },
            AstType::Named { 
                name: "T".to_string(),
                args: vec![],
                span: span.clone(),
            },
        ];
        let arg_types = vec![TypeId::INT64, TypeId::STRING];
        
        let result = infer_type_arguments(
            &type_params,
            &param_types,
            &arg_types,
            &type_registry
        );
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("conflicting"));
    }
}