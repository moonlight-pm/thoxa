//! Code generation module for Thoxa
//!
//! This module handles the translation from AST to QBE IR.

pub mod ast_substitution;
pub mod boxing;
pub mod builder;
pub mod control_flow;
pub mod expr;
pub mod function;
pub mod generics;
pub mod inference;
pub mod inference_context;
pub mod match_expr;
pub mod stmt;
pub mod symbol;
pub mod type_inference;
pub mod types;
pub mod unions;

use expr::ExprResult;

#[cfg(test)]
pub mod test_utils;

use self::builder::Builder;
use self::generics::GenericRegistry;
use self::inference::TypeInferenceEngine;
use self::inference_context::InferenceContext;
use self::symbol::SymbolTable;
use self::types::TypeRegistry;
use crate::ast;
use crate::error::{Error, Result};
use std::collections::HashMap;

/// Context for the current function being compiled
#[derive(Clone)]
pub(crate) struct FunctionContext {
    /// Name of the function
    pub name: String,
    /// Return type (if any)
    pub return_type: Option<types::TypeId>,
}

/// Context for the current loop being compiled
pub(crate) struct LoopContext {
    /// Label to jump to for continue
    pub continue_label: String,
    /// Label to jump to for break
    pub break_label: String,
    /// Slot to store break value (if any)
    pub break_value_slot: Option<String>,
    /// Type of the break value
    pub break_value_type: Option<types::TypeId>,
}

/// Information about a defined function
#[derive(Debug, Clone)]
pub(crate) struct FunctionInfo {
    /// Name of the function
    pub name: String,
    /// Parameter types
    pub param_types: Vec<types::TypeId>,
    /// Return type (if any)
    pub return_type: Option<types::TypeId>,
    /// External C name (if different from name)
    pub external_name: Option<String>,
}

/// The main code generator
pub struct CodeGen {
    /// Registry of all types
    type_registry: TypeRegistry,
    /// Registry of generic types and instantiations
    generic_registry: GenericRegistry,
    /// Symbol table for variables
    symbol_table: SymbolTable,
    /// QBE output builder
    qbe: Builder,
    /// Current function context (if any)
    current_function: Option<FunctionContext>,
    /// Current loop context (if any)
    current_loop: Option<LoopContext>,
    /// Registry of defined functions
    functions: HashMap<String, FunctionInfo>,
    /// AST functions (for generic instantiation)
    ast_functions: HashMap<String, ast::Function>,
    /// Pending generic function instantiations
    pending_instantiations: Vec<(String, Vec<types::TypeId>)>,
    /// Inference context for preventing recursion
    inference_context: InferenceContext,
    /// Unit constructors to generate synthetic functions for
    unit_constructors: HashMap<String, (ast::UnitDef, types::TypeId)>,
}

impl CodeGen {
    /// Create a new code generator
    pub fn new() -> Self {
        CodeGen {
            type_registry: TypeRegistry::new(),
            generic_registry: GenericRegistry::new(),
            symbol_table: SymbolTable::new(),
            qbe: Builder::new(),
            current_function: None,
            current_loop: None,
            functions: HashMap::new(),
            ast_functions: HashMap::new(),
            pending_instantiations: Vec::new(),
            inference_context: InferenceContext::new(),
            unit_constructors: HashMap::new(),
        }
    }

    /// Generate QBE IR for a complete module
    pub fn generate(mut self, module: &ast::Module) -> Result<String> {
        // Run type inference on the module
        let mut inference_engine = TypeInferenceEngine::new();
        match inference_engine.infer_module(module, &mut self) {
            Ok(substitution) => {
                // Apply inferred types back to the module
                let mut module_copy = module.clone();
                inference_engine.apply_inference(&mut module_copy, &substitution)?;
                // Continue with the modified module
                return self.generate_with_inferred_types(&module_copy);
            }
            Err(e) => {
                // If inference fails, fall back to normal generation
            }
        }
        
        // Register built-in functions
        self.register_builtins();
        
        // Emit external function declarations for built-ins
        self.emit_builtin_declarations();
        
        // TODO: Generate code for imports

        // First pass: register all types (Unit and Union)
        for item in &module.items {
            match item {
                ast::Item::Type(_typedef) => {
                    // Type is a type alias, we might need to handle this differently
                    // For now, skip it as we don't have type aliases yet
                }
                ast::Item::Unit(unit_def) => {
                    if !unit_def.type_params.is_empty() {
                        // Register as generic type
                        let generic_def = generics::GenericTypeDef {
                            name: unit_def.name.clone(),
                            type_params: unit_def.type_params.clone(),
                            kind: generics::GenericTypeKind::Unit(unit_def.clone()),
                        };
                        self.generic_registry.register_generic_type(generic_def);
                    } else {
                        // Register the unit type without generating code yet
                        self.register_unit_type(unit_def)?;
                    }
                }
                ast::Item::Union(union_def) => {
                    if !union_def.type_params.is_empty() {
                        // Register as generic type
                        let generic_def = generics::GenericTypeDef {
                            name: union_def.name.clone(),
                            type_params: union_def.type_params.clone(),
                            kind: generics::GenericTypeKind::Union(union_def.clone()),
                        };
                        self.generic_registry.register_generic_type(generic_def);
                    } else {
                        // Register the union type
                        self.register_union_type(union_def)?;
                    }
                }
                _ => {} // Skip other items in this pass
            }
        }

        // Second pass: collect all function signatures
        for item in &module.items {
            if let ast::Item::Function(func) = item {
                // Store generic functions for later instantiation
                if !func.type_params.is_empty() {
                    self.ast_functions.insert(func.name.clone(), func.clone());
                    continue;
                }
                
                // Resolve return type
                let return_type = if let Some(ref ty) = func.return_type {
                    Some(self.resolve_type(ty)?)
                } else {
                    None
                };
                
                // Process parameters
                let mut param_types = Vec::new();
                for param in &func.params {
                    let param_type = self.resolve_type(&param.ty)?;
                    param_types.push(param_type);
                }
                
                // Register the function
                self.functions.insert(func.name.clone(), FunctionInfo {
                    name: func.name.clone(),
                    param_types,
                    return_type,
                    external_name: None,
                });
            }
        }

        // Third pass: generate code for each item
        for item in &module.items {
            self.gen_item(item)?;
        }
        
        // Process any pending generic function instantiations
        self.process_pending_instantiations()?;
        
        // Generate synthetic constructor functions for units
        self.generate_unit_constructors()?;

        // Generate type initialization function
        self.gen_type_init()?;

        // If there's no main function, we need to add a default one
        // (This check would be better done by examining if we generated a main)
        let has_main = module.items.iter().any(|item| {
            matches!(item, ast::Item::Function(f) if f.name == "main")
        });
        
        if !has_main {
            // Generate a default main that returns 0
            self.qbe.function_start("main", &[], builder::QbeType::L, true)?;
            self.qbe.label("start");
            // Call type initialization
            self.qbe.call_void("thoxa_init_runtime", vec![]);
            self.qbe.ret(Some("0"));
            self.qbe.function_end()?;
        } else {
            // We need to ensure type init is called in user's main
            // For now, we'll handle this by modifying main generation
        }

        Ok(self.qbe.finish())
    }
    
    /// Generate QBE IR for a module with already-inferred types
    pub fn generate_with_inferred_types(mut self, module: &ast::Module) -> Result<String> {
        // Register built-in functions
        self.register_builtins();
        
        // Emit external function declarations for built-ins
        self.emit_builtin_declarations();
        
        // First pass: register all types (Unit and Union)
        for item in &module.items {
            match item {
                ast::Item::Type(_typedef) => {
                    // Type is a type alias, we might need to handle this differently
                    // For now, skip it as we don't have type aliases yet
                }
                ast::Item::Unit(unit_def) => {
                    if !unit_def.type_params.is_empty() {
                        // Register as generic type
                        let generic_def = generics::GenericTypeDef {
                            name: unit_def.name.clone(),
                            type_params: unit_def.type_params.clone(),
                            kind: generics::GenericTypeKind::Unit(unit_def.clone()),
                        };
                        self.generic_registry.register_generic_type(generic_def);
                    } else {
                        // Register the unit type without generating code yet
                        self.register_unit_type(unit_def)?;
                    }
                }
                ast::Item::Union(union_def) => {
                    if !union_def.type_params.is_empty() {
                        // Register as generic type
                        let generic_def = generics::GenericTypeDef {
                            name: union_def.name.clone(),
                            type_params: union_def.type_params.clone(),
                            kind: generics::GenericTypeKind::Union(union_def.clone()),
                        };
                        self.generic_registry.register_generic_type(generic_def);
                    } else {
                        // Register the union type
                        self.register_union_type(union_def)?;
                    }
                }
                _ => {} // Skip other items in this pass
            }
        }

        // Second pass: collect all function signatures
        for item in &module.items {
            if let ast::Item::Function(func) = item {
                // Store generic functions for later instantiation
                if !func.type_params.is_empty() {
                    self.ast_functions.insert(func.name.clone(), func.clone());
                    continue;
                }
                
                // Resolve return type
                let return_type = if let Some(ref ty) = func.return_type {
                    Some(self.resolve_type(ty)?)
                } else {
                    None
                };
                
                // Process parameters
                let mut param_types = Vec::new();
                for param in &func.params {
                    let param_type = self.resolve_type(&param.ty)?;
                    param_types.push(param_type);
                }
                
                // Register the function
                self.functions.insert(func.name.clone(), FunctionInfo {
                    name: func.name.clone(),
                    param_types,
                    return_type,
                    external_name: None,
                });
            }
        }

        // Third pass: generate code for each item
        for item in &module.items {
            self.gen_item(item)?;
        }
        
        // Process any pending generic function instantiations
        self.process_pending_instantiations()?;
        
        // Generate synthetic constructor functions for units
        self.generate_unit_constructors()?;

        // Generate type initialization function
        self.gen_type_init()?;

        // If there's no main function, we need to add a default one
        let has_main = module.items.iter().any(|item| {
            matches!(item, ast::Item::Function(f) if f.name == "main")
        });
        
        if !has_main {
            // Generate a default main that returns 0
            self.qbe.function_start("main", &[], builder::QbeType::L, true)?;
            self.qbe.label("start");
            // Call type initialization
            self.qbe.call_void("thoxa_init_runtime", vec![]);
            self.qbe.ret(Some("0"));
            self.qbe.function_end()?;
        }

        Ok(self.qbe.finish())
    }
    
    /// Get the current QBE output (useful for debugging/testing)
    pub fn current_qbe(&self) -> String {
        self.qbe.current_output()
    }
    
    /// Queue a generic function for instantiation
    pub fn instantiate_generic_function(&mut self, func_name: &str, type_args: Vec<types::TypeId>) -> Result<String> {
        // Check if already instantiated
        let key = (func_name.to_string(), type_args.clone());
        if let Some(name) = self.generic_registry.function_instantiations.get(&key) {
            return Ok(name.clone());
        }
        
        // Get the generic function definition to create the placeholder
        let generic_func = self.ast_functions.get(func_name)
            .ok_or_else(|| Error::Codegen(format!("Unknown generic function: {}", func_name)))?
            .clone();
        
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
        let instantiated_name = generics::GenericRegistry::generate_monomorphized_name(func_name, &type_args);
        
        // Create type parameter mapping for signature
        let type_param_map: HashMap<String, types::TypeId> = generic_func.type_params
            .iter()
            .zip(type_args.iter())
            .map(|(param, &type_id)| (param.clone(), type_id))
            .collect();
        
        // Resolve parameter types
        let mut param_types = Vec::new();
        for param in &generic_func.params {
            let substituted_ty = self.substitute_type_in_ast(&param.ty, &type_param_map)?;
            param_types.push(self.resolve_type(&substituted_ty)?);
        }
        
        // Resolve return type
        let return_type = if let Some(ref ret_ty) = generic_func.return_type {
            let substituted_ty = self.substitute_type_in_ast(ret_ty, &type_param_map)?;
            Some(self.resolve_type(&substituted_ty)?)
        } else {
            None
        };
        
        // Register the function info so it can be called
        self.functions.insert(instantiated_name.clone(), FunctionInfo {
            name: instantiated_name.clone(),
            param_types,
            return_type,
            external_name: None,
        });
        
        // Store the instantiation mapping BEFORE queueing (to avoid infinite loops)
        self.generic_registry.function_instantiations.insert(key, instantiated_name.clone());
        
        // Queue the instantiation to be processed later
        self.pending_instantiations.push((func_name.to_string(), type_args));
        
        Ok(instantiated_name)
    }
    
    /// Process all pending generic function instantiations
    fn process_pending_instantiations(&mut self) -> Result<()> {
        // Process pending instantiations iteratively until none remain
        // This handles nested generic function calls
        while !self.pending_instantiations.is_empty() {
            // Take the pending instantiations to avoid borrow issues
            let pending = std::mem::take(&mut self.pending_instantiations);
            
            for (func_name, type_args) in pending {
            let key = (func_name.clone(), type_args.clone());
            let instantiated_name = generics::GenericRegistry::generate_monomorphized_name(&func_name, &type_args);
            
            // Get the generic function definition
            let generic_func = self.ast_functions.get(&func_name)
                .ok_or_else(|| Error::Codegen(format!("Unknown generic function: {}", func_name)))?
                .clone();
            
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
            
            // Clone the function and substitute types
            let mut instantiated_func = generic_func.clone();
            instantiated_func.name = instantiated_name.clone();
            instantiated_func.type_params.clear(); // No longer generic
            
            // Create type parameter mapping
            let type_param_map: HashMap<String, types::TypeId> = generic_func.type_params
                .iter()
                .zip(type_args.iter())
                .map(|(param, &type_id)| (param.clone(), type_id))
                .collect();
            
            // Substitute types in parameters
            for param in &mut instantiated_func.params {
                param.ty = self.substitute_type_in_ast(&param.ty, &type_param_map)?;
            }
            
            // Substitute types in return type
            if let Some(ref mut ret_ty) = instantiated_func.return_type {
                *ret_ty = self.substitute_type_in_ast(ret_ty, &type_param_map)?;
            }
            
            // Substitute types in the function body using the new AST substitutor
            let substitutor = ast_substitution::AstTypeSubstitutor::new(&type_param_map, &self.type_registry);
            substitutor.substitute_function_body(&mut instantiated_func.body)?;
            
            // Generate code for the instantiated function
            self.gen_function(&instantiated_func)?;
        }
        } // End of while loop
        
        Ok(())
    }
    
    /// Generate synthetic constructor functions for units with constructor parameters
    fn generate_unit_constructors(&mut self) -> Result<()> {
        // Take the unit constructors to avoid borrow issues
        let constructors = std::mem::take(&mut self.unit_constructors);
        
        for (constructor_name, (unit_def, type_id)) in constructors {
            // Prepare QBE parameters
            let mut qbe_params = Vec::new();
            let mut param_type_ids = Vec::new();
            
            for param in &unit_def.constructor_params {
                let param_type_id = self.resolve_type(&param.ty)?;
                param_type_ids.push(param_type_id);
                let qbe_type = self.get_qbe_param_type(param_type_id)?;
                qbe_params.push((param.name.as_str(), qbe_type));
            }
            
            // Generate the constructor function
            self.qbe.function_start(&constructor_name, &qbe_params, builder::QbeType::L, false)?;
            self.qbe.label("start");
            
            // Enter a new scope for the function
            self.symbol_table.enter_scope();
            
            // Add constructor parameters to symbol table
            let mut param_symbols = Vec::new();
            for (i, param) in unit_def.constructor_params.iter().enumerate() {
                let symbol = self.symbol_table.insert_param(
                    param.name.clone(),
                    param_type_ids[i],
                    false,
                    self.type_registry.needs_refcount(param_type_ids[i])
                )?;
                param_symbols.push((param.name.clone(), symbol.qbe_name.clone()));
            }
            
            // Get the type info for field information
            let type_info = self.type_registry.get(type_id)
                .ok_or_else(|| Error::Codegen(format!("Type info not found for unit")))?;
            
            let fields = type_info.fields.as_ref()
                .ok_or_else(|| Error::Codegen(format!("Unit type has no fields")))?
                .clone();
            
            // Calculate allocation size
            let alloc_size = 16 + fields.len() * 8;
            
            // Allocate memory for the unit
            let unit_ptr = self.symbol_table.gen_temp();
            let size_temp = self.symbol_table.gen_temp();
            self.qbe.copy(&size_temp, &format!("{}", alloc_size));
            self.qbe.call(
                Some(&unit_ptr),
                builder::QbeType::L,
                "thoxa_alloc",
                &[(&size_temp, builder::QbeType::L)]
            );
            
            // Store constructor parameters into fields
            // Constructor parameters map directly to fields in order
            for (i, (param_name, param_qbe_name)) in param_symbols.iter().enumerate() {
                if i < fields.len() {
                    let field_offset = 16 + i * 8;
                    let field_ptr = self.symbol_table.gen_temp();
                    self.qbe.add(&field_ptr, builder::QbeType::L, &unit_ptr, &format!("{}", field_offset));
                    
                    // Get the field type
                    let field_type = fields[i].1;
                    
                    // Box value types if needed
                    let value_to_store = if CodeGen::needs_boxing(field_type) {
                        let boxed = self.box_value(&ExprResult {
                            temp: param_qbe_name.clone(),
                            ty: field_type,
                        })?;
                        boxed.temp
                    } else {
                        param_qbe_name.clone()
                    };
                    
                    self.qbe.store(builder::QbeType::L, &value_to_store, &field_ptr);
                }
            }
            
            // Return the unit pointer
            self.qbe.ret(Some(&unit_ptr));
            
            // Exit the function scope
            self.symbol_table.exit_scope();
            
            self.qbe.function_end()?;
        }
        
        Ok(())
    }
    
    /// Substitute types in an AST type using a type parameter map
    fn substitute_type_in_ast(
        &self,
        ty: &ast::Type,
        type_param_map: &HashMap<String, types::TypeId>,
    ) -> Result<ast::Type> {
        match ty {
            ast::Type::Named { name, args, span } => {
                // Check if this is a type parameter that needs substitution
                if let Some(&type_id) = type_param_map.get(name) {
                    // Convert TypeId back to Type
                    let type_info = self.type_registry.get(type_id)
                        .ok_or_else(|| Error::Codegen(format!("Unknown type ID: {:?}", type_id)))?;
                    
                    Ok(ast::Type::Named {
                        name: type_info.name.clone(),
                        args: vec![],
                        span: *span,
                    })
                } else {
                    // Recursively substitute in type arguments
                    let substituted_args = args.iter()
                        .map(|arg| self.substitute_type_in_ast(arg, type_param_map))
                        .collect::<Result<Vec<_>>>()?;
                    
                    Ok(ast::Type::Named {
                        name: name.clone(),
                        args: substituted_args,
                        span: *span,
                    })
                }
            }
            ast::Type::Array { elem, span } => {
                let substituted_elem = self.substitute_type_in_ast(elem, type_param_map)?;
                Ok(ast::Type::Array {
                    elem: Box::new(substituted_elem),
                    span: *span,
                })
            }
            ast::Type::Function { params, ret, span } => {
                let substituted_params = params.iter()
                    .map(|p| self.substitute_type_in_ast(p, type_param_map))
                    .collect::<Result<Vec<_>>>()?;
                let substituted_ret = self.substitute_type_in_ast(ret, type_param_map)?;
                
                Ok(ast::Type::Function {
                    params: substituted_params,
                    ret: Box::new(substituted_ret),
                    span: *span,
                })
            }
            ast::Type::Tuple { elems, span } => {
                let substituted_elems = elems.iter()
                    .map(|e| self.substitute_type_in_ast(e, type_param_map))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(ast::Type::Tuple {
                    elems: substituted_elems,
                    span: *span,
                })
            }
            ast::Type::Union { variants, span } => {
                let substituted_variants = variants.iter()
                    .map(|v| self.substitute_type_in_ast(v, type_param_map))
                    .collect::<Result<Vec<_>>>()?;
                
                Ok(ast::Type::Union {
                    variants: substituted_variants,
                    span: *span,
                })
            }
        }
    }
    
    /// Get the QBE type for a Thoxa type
    pub(crate) fn get_qbe_type(&self, ty: types::TypeId) -> Result<builder::QbeType> {
        use builder::QbeType;
        // For now, map types to QBE types based on size
        // In the future, this will need to consider the actual type info
        match ty {
            types::TypeId::INT8 | types::TypeId::UINT8 => Ok(QbeType::B),
            types::TypeId::INT16 | types::TypeId::UINT16 => Ok(QbeType::H),
            types::TypeId::INT32 | types::TypeId::UINT32 | types::TypeId::BOOL => Ok(QbeType::W),
            types::TypeId::INT64 | types::TypeId::UINT64 => Ok(QbeType::L),
            types::TypeId::FLOAT32 => Ok(QbeType::S),
            types::TypeId::FLOAT64 => Ok(QbeType::D),
            _ => Ok(QbeType::L), // Default to long for pointers/unknown
        }
    }
    
    /// Get the QBE type for a function parameter (must be at least word-sized)
    pub(crate) fn get_qbe_param_type(&self, ty: types::TypeId) -> Result<builder::QbeType> {
        use builder::QbeType;
        match ty {
            // Small types are promoted to word for parameters
            types::TypeId::INT8 | types::TypeId::UINT8 | types::TypeId::BOOL |
            types::TypeId::INT16 | types::TypeId::UINT16 |
            types::TypeId::INT32 | types::TypeId::UINT32 => Ok(QbeType::W),
            types::TypeId::INT64 | types::TypeId::UINT64 => Ok(QbeType::L),
            types::TypeId::FLOAT32 => Ok(QbeType::S),
            types::TypeId::FLOAT64 => Ok(QbeType::D),
            _ => Ok(QbeType::L), // Default to long for pointers/unknown
        }
    }
    
    /// Check if a type is a Trial instance
    pub(crate) fn is_trial_instance(&self, type_id: types::TypeId) -> bool {
        if let Some(info) = self.type_registry.get(type_id) {
            // Check if it's a generic instance of Trial
            info.name.starts_with("Trial<") || type_id == types::TypeId::TRIAL
        } else {
            false
        }
    }
    
    /// Check if a type is a May instance
    pub(crate) fn is_may_instance(&self, type_id: types::TypeId) -> bool {
        if let Some(info) = self.type_registry.get(type_id) {
            // Check if it's a generic instance of May
            info.name.starts_with("May<")
        } else {
            false
        }
    }
    
    /// Resolve a type AST node to a TypeId
    pub(crate) fn resolve_type(&mut self, ty: &ast::Type) -> Result<types::TypeId> {
        match ty {
            ast::Type::Named { name, args, .. } => {
                // Check if this is a generic type with arguments
                if !args.is_empty() {
                    // First resolve the type arguments
                    let mut type_args = Vec::new();
                    for arg in args {
                        type_args.push(self.resolve_type(arg)?);
                    }
                    
                    // Check if this is a registered generic type
                    if self.generic_registry.is_generic_type(name) {
                        // Get or create the instantiation
                        let type_id = self.generic_registry.get_or_create_instantiation(
                            name,
                            type_args,
                            &mut self.type_registry,
                        )?;
                        return Ok(type_id);
                    }
                    
                    // Handle built-in generic types (Trial, May)
                    if name == "Trial" {
                        if args.len() != 2 {
                            return Err(Error::Codegen(format!(
                                "Trial requires exactly 2 type parameters, got {}",
                                args.len()
                            )));
                        }
                        
                        // Register this specific instance of Trial<T,E>
                        let type_id = self.type_registry.register_generic_instance(
                            "Trial".to_string(),
                            type_args
                        );
                        
                        return Ok(type_id);
                    } else if name == "May" {
                        if args.len() != 1 {
                            return Err(Error::Codegen(format!(
                                "May requires exactly 1 type parameter, got {}",
                                args.len()
                            )));
                        }
                        
                        // Register this specific instance of May<T>
                        let type_id = self.type_registry.register_generic_instance(
                            "May".to_string(),
                            type_args
                        );
                        
                        return Ok(type_id);
                    } else {
                        return Err(Error::Codegen(format!(
                            "Generic type {} not yet supported",
                            name
                        )));
                    }
                }
                
                // Check for user-defined types first
                if let Some(type_id) = self.type_registry.find_by_name(name) {
                    return Ok(type_id);
                }
                
                // Simple named types
                match name.as_str() {
                    "Int" => Ok(types::TypeId::INT64),
                    "Int8" => Ok(types::TypeId::INT8),
                    "Int16" => Ok(types::TypeId::INT16),
                    "Int32" => Ok(types::TypeId::INT32),
                    "Int64" => Ok(types::TypeId::INT64),
                    "UInt8" => Ok(types::TypeId::UINT8),
                    "UInt16" => Ok(types::TypeId::UINT16),
                    "UInt32" => Ok(types::TypeId::UINT32),
                    "UInt64" => Ok(types::TypeId::UINT64),
                    "Float" => Ok(types::TypeId::FLOAT64),
                    "Float32" => Ok(types::TypeId::FLOAT32),
                    "Float64" => Ok(types::TypeId::FLOAT64),
                    "Bool" => Ok(types::TypeId::BOOL),
                    "Char" => Ok(types::TypeId::CHAR),
                    "String" => Ok(types::TypeId::STRING),
                    "Void" => Ok(types::TypeId::VOID),
                    "Trial" => Ok(types::TypeId::TRIAL),  // Base Trial type
                    "i8" => Ok(types::TypeId::INT8),
                    "i16" => Ok(types::TypeId::INT16),
                    "i32" => Ok(types::TypeId::INT32),
                    "i64" => Ok(types::TypeId::INT64),
                    "u8" => Ok(types::TypeId::UINT8),
                    "u16" => Ok(types::TypeId::UINT16),
                    "u32" => Ok(types::TypeId::UINT32),
                    "u64" => Ok(types::TypeId::UINT64),
                    "f32" => Ok(types::TypeId::FLOAT32),
                    "f64" => Ok(types::TypeId::FLOAT64),
                    _ => Err(Error::Codegen(format!("Unknown type: {}", name))),
                }
            }
            ast::Type::Tuple { elems, .. } => {
                // Resolve element types
                let mut elem_types = Vec::new();
                for elem in elems {
                    elem_types.push(self.resolve_type(elem)?);
                }
                
                // Register the tuple type
                Ok(self.type_registry.register_tuple_type(elem_types))
            }
            ast::Type::Array { elem, .. } => {
                // For now, arrays are just pointers
                // TODO: Track element type for better type checking
                let _elem_type = self.resolve_type(elem)?;
                Ok(types::TypeId::POINTER)
            }
            ast::Type::Function { .. } => {
                // Function types are pointers to functions
                // TODO: Track parameter and return types
                Ok(types::TypeId::POINTER)
            }
            ast::Type::Union { .. } => {
                // Union types need special handling
                Err(Error::Codegen("Union types not yet implemented".to_string()))
            }
        }
    }
    
    /// Register built-in functions
    fn register_builtins(&mut self) {
        // Register print for strings (takes a ThoxaString pointer, returns void)
        self.functions.insert("print".to_string(), FunctionInfo {
            name: "print".to_string(),
            param_types: vec![types::TypeId::STRING],
            return_type: None,
            external_name: Some("thoxa_string_print".to_string()),
        });
        
        // Register println for strings (takes a ThoxaString pointer, returns void)
        self.functions.insert("println".to_string(), FunctionInfo {
            name: "println".to_string(),
            param_types: vec![types::TypeId::STRING],
            return_type: None,
            external_name: Some("thoxa_string_println".to_string()),
        });
        
        // Register printInt for integers (takes an Int, returns void)
        self.functions.insert("printInt".to_string(), FunctionInfo {
            name: "printInt".to_string(),
            param_types: vec![types::TypeId::INT64],
            return_type: None,
            external_name: None,
        });
        
        // Register printChar for characters (takes a Char, returns void)
        self.functions.insert("printChar".to_string(), FunctionInfo {
            name: "printChar".to_string(),
            param_types: vec![types::TypeId::CHAR],
            return_type: None,
            external_name: None,
        });
        
        // Memory management functions
        // thoxaAlloc(size: Int, typeTag: Int) -> Ptr
        self.functions.insert("thoxaAlloc".to_string(), FunctionInfo {
            name: "thoxaAlloc".to_string(),
            param_types: vec![types::TypeId::INT64, types::TypeId::INT64],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_alloc".to_string()),
        });
        
        // thoxaAllocRaw(size: Int) -> Ptr (backwards compat)
        self.functions.insert("thoxaAllocRaw".to_string(), FunctionInfo {
            name: "thoxaAllocRaw".to_string(),
            param_types: vec![types::TypeId::INT64],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_alloc_raw".to_string()),
        });
        
        // thoxaFree(ptr: Ptr) -> Void
        self.functions.insert("thoxaFree".to_string(), FunctionInfo {
            name: "thoxaFree".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_free".to_string()),
        });
        
        // thoxaRetain(ptr: Ptr) -> Void
        self.functions.insert("thoxaRetain".to_string(), FunctionInfo {
            name: "thoxaRetain".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_retain".to_string()),
        });
        
        // thoxaRelease(ptr: Ptr) -> Void
        self.functions.insert("thoxaRelease".to_string(), FunctionInfo {
            name: "thoxaRelease".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_release".to_string()),
        });
        
        // thoxaIncref(ptr: Ptr) -> Void
        self.functions.insert("thoxaIncref".to_string(), FunctionInfo {
            name: "thoxaIncref".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_incref".to_string()),
        });
        
        // thoxaDecref(ptr: Ptr) -> Void
        self.functions.insert("thoxaDecref".to_string(), FunctionInfo {
            name: "thoxaDecref".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_decref".to_string()),
        });
        
        // thoxaRefcount(ptr: Ptr) -> Int
        self.functions.insert("thoxaRefcount".to_string(), FunctionInfo {
            name: "thoxaRefcount".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: Some(types::TypeId::INT64),
            external_name: Some("thoxa_refcount".to_string()),
        });
        
        // String functions
        // thoxaStringNew(cstr: String) -> Ptr
        self.functions.insert("thoxaStringNew".to_string(), FunctionInfo {
            name: "thoxaStringNew".to_string(),
            param_types: vec![types::TypeId::STRING],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_string_new".to_string()),
        });
        
        // thoxaStringCstr(str: Ptr) -> String
        self.functions.insert("thoxaStringCstr".to_string(), FunctionInfo {
            name: "thoxaStringCstr".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: Some(types::TypeId::STRING),
            external_name: Some("thoxa_string_cstr".to_string()),
        });
        
        // thoxaStringLength(str: Ptr) -> Int
        self.functions.insert("thoxaStringLength".to_string(), FunctionInfo {
            name: "thoxaStringLength".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: Some(types::TypeId::INT64),
            external_name: Some("thoxa_string_length".to_string()),
        });
        
        // thoxaStringConcat(a: Ptr, b: Ptr) -> Ptr
        self.functions.insert("thoxaStringConcat".to_string(), FunctionInfo {
            name: "thoxaStringConcat".to_string(),
            param_types: vec![types::TypeId::POINTER, types::TypeId::POINTER],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_string_concat".to_string()),
        });
        
        // thoxaStringStatic(cstr: String) -> Ptr
        self.functions.insert("thoxaStringStatic".to_string(), FunctionInfo {
            name: "thoxaStringStatic".to_string(),
            param_types: vec![types::TypeId::STRING],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_string_static".to_string()),
        });
        
        // Array functions
        // thoxaArrayNew(capacity: Int, elemSize: Int, elemType: Int) -> Ptr
        self.functions.insert("thoxaArrayNew".to_string(), FunctionInfo {
            name: "thoxaArrayNew".to_string(),
            param_types: vec![types::TypeId::INT64, types::TypeId::INT64, types::TypeId::INT64],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_array_new".to_string()),
        });
        
        // thoxaArrayLength(arr: Ptr) -> Int
        self.functions.insert("thoxaArrayLength".to_string(), FunctionInfo {
            name: "thoxaArrayLength".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: Some(types::TypeId::INT64),
            external_name: Some("thoxa_array_length".to_string()),
        });
        
        // thoxaArrayCapacity(arr: Ptr) -> Int
        self.functions.insert("thoxaArrayCapacity".to_string(), FunctionInfo {
            name: "thoxaArrayCapacity".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: Some(types::TypeId::INT64),
            external_name: Some("thoxa_array_capacity".to_string()),
        });
        
        // thoxaArrayPush(arr: Ptr, elem: Ptr) -> Void
        self.functions.insert("thoxaArrayPush".to_string(), FunctionInfo {
            name: "thoxaArrayPush".to_string(),
            param_types: vec![types::TypeId::POINTER, types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_array_push".to_string()),
        });
        
        // thoxaArrayGet(arr: Ptr, index: Int) -> Ptr
        self.functions.insert("thoxaArrayGet".to_string(), FunctionInfo {
            name: "thoxaArrayGet".to_string(),
            param_types: vec![types::TypeId::POINTER, types::TypeId::INT64],
            return_type: Some(types::TypeId::POINTER),
            external_name: Some("thoxa_array_get".to_string()),
        });
        
        // thoxaArraySet(arr: Ptr, index: Int, elem: Ptr) -> Void
        self.functions.insert("thoxaArraySet".to_string(), FunctionInfo {
            name: "thoxaArraySet".to_string(),
            param_types: vec![types::TypeId::POINTER, types::TypeId::INT64, types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_array_set".to_string()),
        });
        
        // thoxaArrayPop(arr: Ptr, outElem: Ptr) -> Int
        self.functions.insert("thoxaArrayPop".to_string(), FunctionInfo {
            name: "thoxaArrayPop".to_string(),
            param_types: vec![types::TypeId::POINTER, types::TypeId::POINTER],
            return_type: Some(types::TypeId::INT64),
            external_name: Some("thoxa_array_pop".to_string()),
        });
        
        // thoxaArrayClear(arr: Ptr) -> Void
        self.functions.insert("thoxaArrayClear".to_string(), FunctionInfo {
            name: "thoxaArrayClear".to_string(),
            param_types: vec![types::TypeId::POINTER],
            return_type: None,
            external_name: Some("thoxa_array_clear".to_string()),
        });
    }
    
    /// Emit external function declarations for built-ins
    fn emit_builtin_declarations(&mut self) {
        // Declare runtime functions that we'll call
        // For now, just the essential ones
    }
    
    /// Generate type initialization function
    fn gen_type_init(&mut self) -> Result<()> {
        // Generate a function that initializes all types
        self.qbe.function_start("thoxa_init_runtime", &[], builder::QbeType::W, false)?;
        self.qbe.label("start");
        
        // Call the runtime's type system initializer
        self.qbe.call_void("thoxa_init_types", vec![]);
        
        // Register each user-defined type
        // For now, we'll keep it simple - the runtime already registers built-in types
        // TODO: Register user-defined unit types
        
        self.qbe.ret(Some("0"));
        self.qbe.function_end()?;
        Ok(())
    }
    
    /// Register a unit type in the type registry
    pub fn register_unit_type(&mut self, unit_def: &ast::UnitDef) -> Result<()> {
        // Extract fields from the body statements
        let mut fields = Vec::new();
        
        // For now, process the temporary let statements we create
        // TODO: Once we parse proper export let syntax, extract fields properly
        for stmt in &unit_def.body {
            if let ast::Stmt::Let { name, ty, is_mut, .. } = stmt {
                if let Some(field_type_ast) = ty {
                    let field_type = self.resolve_type(field_type_ast)?;
                    fields.push((name.clone(), field_type, *is_mut));
                }
            }
        }
        
        // Extract constructor parameters
        let mut constructor_params = Vec::new();
        let mut param_types = Vec::new();
        for param in &unit_def.constructor_params {
            let param_type = self.resolve_type(&param.ty)?;
            constructor_params.push((param.name.clone(), param_type));
            param_types.push(param_type);
        }
        
        // Register the unit type with its fields and constructor params
        let type_id = self.type_registry.register_unit_type(
            unit_def.name.clone(),
            fields.clone(),
            constructor_params.clone(),
        );
        
        // If this unit has constructor parameters, register a synthetic constructor function
        if !constructor_params.is_empty() {
            let constructor_name = format!("thoxa_constructor_{}", unit_def.name);
            
            // Register the constructor function info
            self.functions.insert(constructor_name.clone(), FunctionInfo {
                name: constructor_name.clone(),
                param_types,
                return_type: Some(type_id),
                external_name: Some(constructor_name.clone()),
            });
            
            // Store unit definition for later code generation
            self.unit_constructors.insert(constructor_name, (unit_def.clone(), type_id));
        }
        
        Ok(())
    }
    
    /// Register a union type in the type registry
    pub fn register_union_type(&mut self, union_def: &ast::UnionDef) -> Result<()> {
        // Resolve variant types
        let mut variant_type_ids = Vec::new();
        for variant_type in &union_def.variants {
            let type_id = self.resolve_type(variant_type)?;
            variant_type_ids.push(type_id);
        }
        
        // Register the union type with its variants
        let _type_id = self.type_registry.register_union_type(
            union_def.name.clone(),
            variant_type_ids,
            if union_def.type_params.is_empty() { None } else { Some(union_def.type_params.clone()) },
        );
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_codegen() {
        let codegen = CodeGen::new();
        let module = ast::Module {
            imports: vec![],
            items: vec![],
            span: crate::lexer::Span {
                start: 0,
                end: 0,
                line: 1,
                column: 1,
            },
        };

        let output = codegen.generate(&module).unwrap();

        // Should generate a basic main function
        assert!(output.contains("export function l $main()"));
        assert!(output.contains("ret 0"));
    }
    
    #[test]
    fn test_module_with_main() {
        use crate::lexer::{Span, IntSuffix};
        
        fn make_span() -> Span {
            Span { start: 0, end: 0, line: 1, column: 1 }
        }
        
        let codegen = CodeGen::new();
        
        // Create a module with a main function
        let module = ast::Module {
            imports: vec![],
            items: vec![
                ast::Item::Function(ast::Function {
                    name: "main".to_string(),
                    type_params: vec![],
                    params: vec![],
                    return_type: Some(ast::Type::Named {
                        name: "Int".to_string(),
                        args: vec![],
                        span: make_span(),
                    }),
                    body: ast::Block {
                        stmts: vec![
                            ast::Stmt::Let {
                                name: "x".to_string(),
                                ty: None,
                                value: Some(ast::Expr::IntLiteral {
                                    value: 10,
                                    suffix: IntSuffix::None,
                                    span: make_span(),
                                }),
                                is_mut: false,
                                is_export: false,
                                span: make_span(),
                            },
                            ast::Stmt::Let {
                                name: "y".to_string(),
                                ty: None,
                                value: Some(ast::Expr::Binary {
                                    op: ast::BinaryOp::Add,
                                    left: Box::new(ast::Expr::Ident {
                                        name: "x".to_string(),
                                        span: make_span(),
                                    }),
                                    right: Box::new(ast::Expr::IntLiteral {
                                        value: 5,
                                        suffix: IntSuffix::None,
                                        span: make_span(),
                                    }),
                                    span: make_span(),
                                }),
                                is_mut: false,
                                is_export: false,
                                span: make_span(),
                            },
                        ],
                        expr: Some(Box::new(ast::Expr::Ident {
                            name: "y".to_string(),
                            span: make_span(),
                        })),
                        span: make_span(),
                    },
                    is_export: true,
                    span: make_span(),
                }),
            ],
            span: make_span(),
        };
        
        let output = codegen.generate(&module).unwrap();
        
        // Should have our main function
        assert!(output.contains("export function l $main()"));
        // Should have the arithmetic
        assert!(output.contains("add"));
        // Should return y
        assert!(output.contains("ret %y"));
    }
}