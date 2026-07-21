//! Function call code generation

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::{TypeId, TypeKind}, builder};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for a function call
    pub(super) fn gen_call(&mut self, func: &Expr, type_args: &[crate::ast::Type], args: &[Expr]) -> Result<ExprResult> {
        // Check if this is a method call on an array
        if let Expr::FieldAccess { object, field, .. } = func {
            // For mutating methods, check if the object is mutable
            let mutating_methods = ["push", "pop", "clear"];
            if mutating_methods.contains(&field.as_str()) {
                // Check if the object is a simple variable (not an expression)
                if let Expr::Ident { name, .. } = &**object {
                    // Only check if the variable has already been defined
                    // This allows us to catch most cases while avoiding the timing issue
                    // with variables being used in their own initialization
                    if let Some(symbol) = self.symbol_table.lookup(name) {
                        if !symbol.is_mut {
                            return Err(Error::Codegen(format!(
                                "Cannot call mutating method '{}' on immutable array '{}'",
                                field, name
                            )));
                        }
                    }
                    // If the variable doesn't exist yet, it will be caught later
                    // when we try to generate code for it
                }
                // For non-identifier expressions (like array literals), 
                // we can't check mutability at compile time
            }
            
            let obj_result = self.gen_expr(object)?;
            
            // Check if this is an array type (pointer)
            let type_info = self.type_registry.get(obj_result.ty)
                .ok_or_else(|| Error::Codegen(format!("Unknown type ID: {:?}", obj_result.ty)))?;
            
            if type_info.name == "Ptr" {
                // Handle array methods
                return self.gen_array_method_call(&obj_result, field, args);
            }
            
            return Err(Error::Codegen(format!("Method calls on {} not supported yet", type_info.name)));
        }
        
        // Check if we're calling a function value (first-class function)
        if !matches!(func, Expr::Ident { .. }) {
            // This could be a function value - generate code for it
            let func_result = self.gen_expr(func)?;
            
            // Extract type info we need
            let (is_function, return_type, param_types) = {
                if let Some(type_info) = self.type_registry.get(func_result.ty) {
                    if type_info.kind == TypeKind::Function {
                        if let Some(ref type_params) = type_info.type_params {
                            let return_type = type_params.last().copied().unwrap_or(TypeId::VOID);
                            let param_types: Vec<TypeId> = type_params[..type_params.len() - 1].to_vec();
                            (true, return_type, param_types)
                        } else {
                            (false, TypeId::VOID, Vec::new())
                        }
                    } else {
                        (false, TypeId::VOID, Vec::new())
                    }
                } else {
                    (false, TypeId::VOID, Vec::new())
                }
            };
            
            if is_function {
                // Generate code for arguments
                let mut arg_results = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let arg_result = self.gen_expr(arg)?;
                    
                    // Type check
                    if i < param_types.len() && arg_result.ty != param_types[i] {
                        return Err(Error::Codegen(format!(
                            "Type mismatch in function call: parameter {} expects {:?}, got {:?}",
                            i + 1, param_types[i], arg_result.ty
                        )));
                    }
                    
                    arg_results.push(arg_result);
                }
                
                // Generate indirect call through function pointer
                let result_temp = if return_type != TypeId::VOID {
                    self.symbol_table.gen_temp()
                } else {
                    String::new()
                };
                
                // Build argument list for QBE
                let qbe_args: Vec<(&str, builder::QbeType)> = arg_results.iter()
                    .map(|arg| (arg.temp.as_str(), builder::QbeType::L))
                    .collect();
                
                // Generate indirect call instruction
                if return_type != TypeId::VOID {
                    let qbe_type = self.get_qbe_type(return_type)?;
                    self.qbe.call_indirect(
                        Some(&result_temp),
                        qbe_type,
                        &func_result.temp,
                        &qbe_args
                    );
                } else {
                    self.qbe.call_indirect(
                        None,
                        builder::QbeType::W,
                        &func_result.temp,
                        &qbe_args
                    );
                }
                
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: return_type,
                });
            }
        }
        
        // Otherwise, try to resolve as a direct function call by name
        let func_name = match func {
            Expr::Ident { name, .. } => name,
            _ => return Err(Error::Codegen("Expression is not callable".to_string())),
        };
        
        // Check if this is a unit constructor call
        if let Some(type_id) = self.type_registry.find_by_name(func_name) {
            return self.gen_unit_constructor(type_id, func_name, args);
        }
        
        // Check if this is a generic function (with or without explicit type args)
        let (func_name_to_use, _inferred_arg_types) = if let Some(ast_func) = self.ast_functions.get(func_name) {
            if !ast_func.type_params.is_empty() {
                // This is a generic function
                let (type_ids, arg_types): (Vec<TypeId>, Option<Vec<TypeId>>) = if !type_args.is_empty() {
                    // Explicit type arguments provided
                    let type_ids: Result<Vec<TypeId>> = type_args.iter()
                        .map(|ty| self.resolve_type(ty))
                        .collect();
                    (type_ids?, None)
                } else {
                    // No explicit type args - attempt type inference with recursion protection
                    if !self.inference_context.can_infer(func_name) {
                        return Err(Error::Codegen(format!(
                            "Cannot infer types for '{}': recursive inference detected. \
                             Please provide explicit type arguments.",
                            func_name
                        )));
                    }
                    
                    // Start inference with guard
                    let _guard = self.inference_context.start_inference(func_name)?;
                    
                    // Get argument types by type-checking (not generating) the expressions
                    let arg_types: Result<Vec<TypeId>> = args.iter()
                        .map(|arg| self.get_expr_type(arg))
                        .collect();
                    let arg_types = arg_types?;
                    
                    // Infer type arguments from the argument types
                    let inferred = crate::codegen::type_inference::infer_type_arguments(
                        &ast_func.type_params,
                        &ast_func.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>(),
                        &arg_types,
                        &self.type_registry
                    )?;
                    
                    (inferred.clone(), Some(arg_types))
                };
                
                // Check if instantiation already exists
                let key = (func_name.to_string(), type_ids.clone());
                let instantiated_name = if let Some(name) = self.generic_registry.function_instantiations.get(&key) {
                    name.clone()
                } else {
                    // Need to create the instantiation
                    self.instantiate_generic_function(func_name, type_ids)?
                };
                (instantiated_name, arg_types)
            } else {
                // Not a generic function
                (func_name.to_string(), None)
            }
        } else if !type_args.is_empty() {
            // Type args provided but function not found in AST (shouldn't happen)
            return Err(Error::Codegen(format!("Unknown function: {}", func_name)));
        } else {
            // Regular non-generic function
            (func_name.to_string(), None)
        };
        
        // Look up the function - first try the exact name, then try with parent function prefix
        let func_info = if let Some(info) = self.functions.get(&func_name_to_use) {
            info.clone()
        } else if let Some(parent_fn) = &self.current_function {
            // Try nested function name with the original func_name (not func_name_to_use which might be mangled)
            let nested_name = format!("{}__{}", parent_fn.name, func_name);
            self.functions.get(&nested_name)
                .ok_or_else(|| Error::Codegen(format!("Unknown function: {}", func_name)))?
                .clone()
        } else {
            return Err(Error::Codegen(format!("Unknown function: {}", func_name_to_use)));
        };
        
        // Check argument count
        if args.len() != func_info.param_types.len() {
            return Err(Error::Codegen(format!(
                "Function '{}' expects {} arguments, got {}",
                func_name, func_info.param_types.len(), args.len()
            )));
        }
        
        // Generate code for arguments
        let mut arg_temps = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let arg_result = self.gen_expr(arg)?;
            
            // Type check arguments
            if arg_result.ty != func_info.param_types[i] {
                return Err(Error::Codegen(format!(
                    "Type mismatch in argument {} of function '{}': expected {:?}, got {:?}",
                    i + 1, func_name, func_info.param_types[i], arg_result.ty
                )));
            }
            
            // Retain heap-allocated arguments (the callee will release them)
            if self.type_registry.needs_refcount(arg_result.ty) {
                // Check for null before retaining
                let null_check = self.symbol_table.gen_temp();
                self.qbe.instruction(format!("{} =w ceql {}, 0", null_check, arg_result.temp));
                
                let retain_label = self.qbe.gen_label("retain_arg");
                let skip_retain = self.qbe.gen_label("skip_retain_arg");
                self.qbe.jnz(&null_check, &skip_retain, &retain_label);
                
                self.qbe.label(&retain_label);
                self.qbe.call_void("thoxa_retain", vec![(arg_result.temp.clone(), "l")]);
                self.qbe.jmp(&skip_retain);
                
                self.qbe.label(&skip_retain);
            }
            
            // For function arguments, use param type (promotes small types to word)
            let qbe_type = self.get_qbe_param_type(arg_result.ty)?;
            arg_temps.push((arg_result.temp, qbe_type));
        }
        
        // Generate the call
        // Convert args to references
        let arg_refs: Vec<(&str, _)> = arg_temps.iter()
            .map(|(temp, ty)| (temp.as_str(), *ty))
            .collect();
        
        // Use external name if provided, otherwise use the function name
        let call_name = func_info.external_name.as_ref().unwrap_or(&func_info.name);
            
        if let Some(return_type) = func_info.return_type {
            // Function returns a value
            let result_temp = self.symbol_table.gen_temp();
            let qbe_type = self.get_qbe_type(return_type)?;
            self.qbe.call(Some(&result_temp), qbe_type, call_name, &arg_refs);
            
            Ok(ExprResult {
                temp: result_temp,
                ty: return_type,
            })
        } else {
            // Void function
            self.qbe.call(None, self.get_qbe_type(TypeId::VOID)?, call_name, &arg_refs);
            
            // Return a dummy void value
            let void_temp = self.symbol_table.gen_temp();
            self.qbe.copy(&void_temp, "0");
            
            Ok(ExprResult {
                temp: void_temp,
                ty: TypeId::VOID,
            })
        }
    }
    
    /// Generate code for a unit constructor call
    fn gen_unit_constructor(&mut self, type_id: TypeId, type_name: &str, args: &[Expr]) -> Result<ExprResult> {
        // Get the type info and extract what we need
        let (constructor_params, fields) = {
            let type_info = self.type_registry.get(type_id)
                .ok_or_else(|| Error::Codegen(format!("Type info not found for {}", type_name)))?;
            
            // Verify this is a unit type
            if type_info.kind != TypeKind::Unit {
                return Err(Error::Codegen(format!("{} is not a unit type", type_name)))
            }
            
            // Clone the data we need to avoid borrowing issues
            (
                type_info.constructor_params.clone().unwrap_or_default(),
                type_info.fields.clone().unwrap_or_default()
            )
        };
        
        self.qbe.comment(&format!("Constructor call for {}", type_name));
        
        // Check argument count against constructor parameters
        if args.len() != constructor_params.len() {
            return Err(Error::Codegen(format!(
                "Constructor '{}' expects {} arguments, got {}",
                type_name, constructor_params.len(), args.len()
            )));
        }
        
        // Generate code for constructor arguments
        let mut arg_results = Vec::new();
        for (i, arg) in args.iter().enumerate() {
            let arg_result = self.gen_expr(arg)?;
            let (param_name, param_type) = &constructor_params[i];
            
            // Type check arguments
            if arg_result.ty != *param_type {
                return Err(Error::Codegen(format!(
                    "Type mismatch in constructor argument '{}' of '{}': expected {:?}, got {:?}",
                    param_name, type_name, param_type, arg_result.ty
                )));
            }
            
            arg_results.push((param_name.clone(), arg_result));
        }
        
        // Calculate the size needed for the unit
        // Each field is 16 bytes (8 for type pointer, 8 for value)
        // Plus 8 bytes for the unit's own type pointer
        let size = 8 + (fields.len() * 16); // Type pointer + fields
        
        // Allocate memory for the unit
        let instance_temp = self.symbol_table.gen_temp();
        let size_temp = self.symbol_table.gen_temp();
        
        // Generate allocation parameters
        self.qbe.copy(&size_temp, &size.to_string());
        
        // Type tag for the unit (use the TypeId value)
        let type_tag_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&type_tag_temp, &format!("{}", type_id.0));
        
        // Call thoxaAlloc
        self.qbe.call(
            Some(&instance_temp),
            builder::QbeType::L,
            "thoxa_alloc",
            &[(&size_temp, builder::QbeType::L), (&type_tag_temp, builder::QbeType::L)]
        );
        
        // Initialize fields from constructor parameters
        // Match constructor params to fields by name
        for (field_idx, (field_name, field_type, _is_mut)) in fields.iter().enumerate() {
            // Look for a matching constructor parameter
            if let Some((_, arg_result)) = arg_results.iter().find(|(name, _)| name == field_name) {
                self.qbe.comment(&format!("Initialize field {}", field_name));
                
                // Calculate offset: 8 bytes for type pointer, then 16 bytes per field
                let field_offset = 8 + (field_idx * 16);
                
                // Store the field's type pointer at offset + 0
                let field_type_temp = self.symbol_table.gen_temp();
                self.qbe.copy(&field_type_temp, &format!("{}", field_type.0));
                
                let field_type_ptr_temp = self.symbol_table.gen_temp();
                self.qbe.add(&field_type_ptr_temp, builder::QbeType::L, &instance_temp, &field_offset.to_string());
                self.qbe.store(builder::QbeType::L, &field_type_temp, &field_type_ptr_temp);
                
                // Store the field's value at offset + 8
                let field_value_ptr_temp = self.symbol_table.gen_temp();
                self.qbe.add(&field_value_ptr_temp, builder::QbeType::L, &instance_temp, &(field_offset + 8).to_string());
                
                // Use the appropriate store instruction based on field type
                let store_type = self.get_qbe_type(*field_type)?;
                self.qbe.store(store_type, &arg_result.temp, &field_value_ptr_temp);
            }
        }
        
        // Return the allocated and initialized pointer
        Ok(ExprResult {
            temp: instance_temp,
            ty: type_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use crate::lexer::{Span, IntSuffix};
    
    fn make_span() -> Span {
        Span { start: 0, end: 0, line: 1, column: 1 }
    }
    
    #[test]
    fn test_function_call() {
        let mut codegen = CodeGen::new();
        
        // First, register a function manually
        codegen.functions.insert("add".to_string(), crate::codegen::FunctionInfo {
            name: "add".to_string(),
            param_types: vec![TypeId::INT64, TypeId::INT64],
            return_type: Some(TypeId::INT64),
            external_name: None,
        });
        
        // Start a test function
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a call expression: add(10, 20)
        let call_expr = ast::Expr::Call {
            func: Box::new(ast::Expr::Ident {
                name: "add".to_string(),
                span: make_span(),
            }),
            type_args: vec![],
            args: vec![
                ast::Expr::IntLiteral {
                    value: 10,
                    suffix: IntSuffix::None,
                    span: make_span(),
                },
                ast::Expr::IntLiteral {
                    value: 20,
                    suffix: IntSuffix::None,
                    span: make_span(),
                },
            ],
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&call_expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        println!("QBE output:\n{}", output);
        assert!(output.contains("call") && output.contains("$add"));
    }
    
    #[test]
    fn test_void_function_call() {
        let mut codegen = CodeGen::new();
        
        // Register a void function
        codegen.functions.insert("print_hello".to_string(), crate::codegen::FunctionInfo {
            name: "print_hello".to_string(),
            param_types: vec![],
            return_type: None,
            external_name: None,
        });
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Call the void function
        let call_expr = ast::Expr::Call {
            func: Box::new(ast::Expr::Ident {
                name: "print_hello".to_string(),
                span: make_span(),
            }),
            type_args: vec![],
            args: vec![],
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&call_expr).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("call"));
        assert!(output.contains("$print_hello"));
    }
    
    #[test]
    fn test_unknown_function_error() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to call an unknown function
        let call_expr = ast::Expr::Call {
            func: Box::new(ast::Expr::Ident {
                name: "unknown_func".to_string(),
                span: make_span(),
            }),
            type_args: vec![],
            args: vec![],
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&call_expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown function"));
    }
    
    #[test]
    fn test_argument_count_mismatch() {
        let mut codegen = CodeGen::new();
        
        // Register a function that expects 2 arguments
        codegen.functions.insert("add".to_string(), crate::codegen::FunctionInfo {
            name: "add".to_string(),
            param_types: vec![TypeId::INT64, TypeId::INT64],
            return_type: Some(TypeId::INT64),
            external_name: None,
        });
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to call with only 1 argument
        let call_expr = ast::Expr::Call {
            func: Box::new(ast::Expr::Ident {
                name: "add".to_string(),
                span: make_span(),
            }),
            type_args: vec![],
            args: vec![
                ast::Expr::IntLiteral {
                    value: 10,
                    suffix: IntSuffix::None,
                    span: make_span(),
                },
            ],
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&call_expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expects 2 arguments, got 1"));
    }
}