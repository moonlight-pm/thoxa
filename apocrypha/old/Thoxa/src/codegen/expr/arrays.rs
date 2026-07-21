//! Code generation for array operations

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId, builder};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for an array literal
    pub(super) fn gen_array_literal(&mut self, elements: &[Expr]) -> Result<ExprResult> {
        // Determine element type by scanning all non-Not() elements
        let element_type = if elements.is_empty() {
            // For now, default to Int for empty arrays
            // TODO: Require type annotation for empty arrays
            TypeId::INT
        } else {
            // Find the first non-Not() element to determine the type
            let mut determined_type = None;
            for element in elements {
                if let Expr::EnumVariant { variant, .. } = element {
                    if variant == "Not" {
                        continue; // Skip Not() variants for type determination
                    }
                }
                // Generate this element to get its type
                let elem_result = self.gen_expr(element)?;
                determined_type = Some(elem_result.ty);
                break;
            }
            
            // If all elements are Not(), we can't determine the type
            determined_type.ok_or_else(|| {
                Error::Codegen("Cannot determine array type from only Not() values".to_string())
            })?
        };
        
        // Determine element size based on type
        let element_size = if self.is_may_instance(element_type) {
            16 // May types are 16 bytes (4-byte tag + padding + 8-byte pointer)
        } else {
            match element_type {
                TypeId::INT | TypeId::INT64 => 8,
                TypeId::INT32 | TypeId::FLOAT32 => 4,
                TypeId::INT16 => 2,
                TypeId::INT8 | TypeId::BOOL | TypeId::CHAR => 1,
                TypeId::FLOAT | TypeId::FLOAT64 => 8,
                TypeId::STRING | TypeId::POINTER => 8, // Pointers
                _ => 8, // Default to pointer size for user types
            }
        };
        
        // Create array with initial capacity
        let capacity = if elements.is_empty() { 8 } else { elements.len() };
        let array_temp = self.symbol_table.gen_temp();
        
        // Call thoxa_array_new(capacity, element_size, element_type)
        self.qbe.call(
            Some(&array_temp),
            builder::QbeType::L,
            "thoxa_array_new",
            &[
                (&format!("{}", capacity), builder::QbeType::L),
                (&format!("{}", element_size), builder::QbeType::L),
                (&format!("{}", element_type.to_u16()), builder::QbeType::L),
            ]
        );
        
        // Add each element to the array
        for element in elements {
            let elem_result = if let Expr::EnumVariant { variant, value, .. } = element {
                if variant == "Not" {
                    // Special handling for Not() - create with the expected May type
                    if self.is_may_instance(element_type) {
                        // Get the type parameter from the expected May type
                        let inner_type = if let Some(type_info) = self.type_registry.get(element_type) {
                            if let Some(ref params) = type_info.type_params {
                                if !params.is_empty() {
                                    params[0]
                                } else {
                                    TypeId::VOID
                                }
                            } else {
                                TypeId::VOID
                            }
                        } else {
                            TypeId::VOID
                        };
                        
                        // Create Not with the correct type parameter
                        let may_instance = self.type_registry.register_generic_instance(
                            "May".to_string(),
                            vec![inner_type]
                        );
                        
                        // Generate the Not variant
                        let result_temp = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&result_temp),
                            crate::codegen::builder::QbeType::L,
                            "thoxa_may_not",
                            &[]  // No arguments needed for Not
                        );
                        
                        ExprResult {
                            temp: result_temp,
                            ty: may_instance,
                        }
                    } else {
                        // Not a May array, generate normally
                        self.gen_expr(element)?
                    }
                } else {
                    // Not a Not() variant, generate normally
                    self.gen_expr(element)?
                }
            } else {
                // Not an enum variant at all
                self.gen_expr(element)?
            };
            
            // Type check - all elements must be same type
            if elem_result.ty != element_type {
                return Err(Error::Codegen(format!(
                    "Array element type mismatch: expected {:?}, got {:?}",
                    element_type, elem_result.ty
                )));
            }
            
            // For primitive types, we need to pass address of the value
            // Note: May types are passed by value but thoxa_array_push expects a pointer
            let elem_ptr = if Self::is_primitive_type(elem_result.ty) || self.is_may_instance(elem_result.ty) {
                // Allocate stack space for the value
                let ptr_temp = self.symbol_table.gen_temp();
                self.qbe.alloc(&ptr_temp, element_size, 8); // 8-byte alignment
                
                // Store the value
                let qbe_type = self.get_qbe_type(elem_result.ty)?;
                self.qbe.store(qbe_type, &elem_result.temp, &ptr_temp);
                
                ptr_temp
            } else {
                // For heap types, the temp already contains a pointer
                elem_result.temp.clone()
            };
            
            // Call thoxa_array_push(array, &element)
            self.qbe.call(
                None,
                builder::QbeType::W,
                "thoxa_array_push",
                &[
                    (&array_temp, builder::QbeType::L),
                    (&elem_ptr, builder::QbeType::L),
                ]
            );
        }
        
        // Register the array type with its element type
        let array_type = self.type_registry.register_array_type(element_type);
        
        Ok(ExprResult {
            temp: array_temp,
            ty: array_type, // Return the proper array type
        })
    }
    
    /// Generate code for array indexing
    pub(super) fn gen_array_index(&mut self, array: &Expr, index: &Expr) -> Result<ExprResult> {
        let array_result = self.gen_expr(array)?;
        let index_result = self.gen_expr(index)?;
        
        // Get the element type from the array type
        let element_type = self.type_registry.get_array_element_type(array_result.ty)
            .unwrap_or(TypeId::INT);  // Default fallback
        
        // Check that index is an integer type
        if !matches!(index_result.ty, TypeId::INT | TypeId::INT32 | TypeId::INT16 | TypeId::INT8) {
            return Err(Error::Codegen(format!(
                "Array index must be integer, got {:?}",
                index_result.ty
            )));
        }
        
        // Get array length for bounds checking
        let length_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&length_temp),
            builder::QbeType::L,
            "thoxa_array_length",
            &[(&array_result.temp, builder::QbeType::L)]
        );
        
        // Bounds check: thoxa_check_bounds(index, length, "array")
        let array_str = self.qbe.add_string_literal("array");
        self.qbe.call(
            None,
            builder::QbeType::W,
            "thoxa_check_bounds",
            &[
                (&index_result.temp, builder::QbeType::L),
                (&length_temp, builder::QbeType::L),
                (&array_str, builder::QbeType::L),
            ]
        );
        
        // Get element pointer: thoxa_array_get(array, index)
        let elem_ptr = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&elem_ptr),
            builder::QbeType::L,
            "thoxa_array_get",
            &[
                (&array_result.temp, builder::QbeType::L),
                (&index_result.temp, builder::QbeType::L),
            ]
        );
        
        // Load the value from the pointer
        // TODO: Need to know element type to load correctly
        // For now, assume pointer-sized elements
        let value_temp = self.symbol_table.gen_temp();
        self.qbe.load(&value_temp, builder::QbeType::L, &elem_ptr);
        
        Ok(ExprResult {
            temp: value_temp,
            ty: element_type, // Return the actual element type
        })
    }
    
    /// Check if a type is primitive (stored inline vs heap pointer)
    fn is_primitive_type(ty: TypeId) -> bool {
        matches!(
            ty,
            TypeId::INT | TypeId::INT64 | TypeId::INT32 | TypeId::INT16 | TypeId::INT8 |
            TypeId::UINT | TypeId::UINT64 | TypeId::UINT32 | TypeId::UINT16 | TypeId::UINT8 |
            TypeId::FLOAT | TypeId::FLOAT64 | TypeId::FLOAT32 |
            TypeId::BOOL | TypeId::CHAR | TypeId::VOID
        )
    }
    
    /// Generate code for array method calls
    pub(super) fn gen_array_method_call(&mut self, array: &ExprResult, method: &str, args: &[Expr]) -> Result<ExprResult> {
        match method {
            "push" => {
                // push() requires exactly one argument
                if args.len() != 1 {
                    return Err(Error::Codegen(format!(
                        "Array push() expects 1 argument, got {}",
                        args.len()
                    )));
                }
                
                // Generate code for the element to push
                let elem_result = self.gen_expr(&args[0])?;
                
                // Get element size based on type
                let element_size = if self.is_may_instance(elem_result.ty) {
                    16 // May types are 16 bytes
                } else {
                    match elem_result.ty {
                        TypeId::INT | TypeId::INT64 => 8,
                        TypeId::INT32 | TypeId::FLOAT32 => 4,
                        TypeId::INT16 => 2,
                        TypeId::INT8 | TypeId::BOOL | TypeId::CHAR => 1,
                        TypeId::FLOAT | TypeId::FLOAT64 => 8,
                        TypeId::STRING | TypeId::POINTER => 8,
                        _ => 8,
                    }
                };
                
                // For primitive types, we need to pass address of the value
                let elem_ptr = if Self::is_primitive_type(elem_result.ty) || self.is_may_instance(elem_result.ty) {
                    // Allocate stack space for the value
                    let ptr_temp = self.symbol_table.gen_temp();
                    self.qbe.alloc(&ptr_temp, element_size, 8);
                    
                    // Store the value
                    let qbe_type = self.get_qbe_type(elem_result.ty)?;
                    self.qbe.store(qbe_type, &elem_result.temp, &ptr_temp);
                    
                    ptr_temp
                } else {
                    // For heap types, the temp already contains a pointer
                    elem_result.temp.clone()
                };
                
                // Call thoxa_array_push(array, &element)
                self.qbe.call(
                    None,
                    builder::QbeType::W,
                    "thoxa_array_push",
                    &[
                        (&array.temp, builder::QbeType::L),
                        (&elem_ptr, builder::QbeType::L),
                    ]
                );
                
                // push() returns void
                let void_temp = self.symbol_table.gen_temp();
                self.qbe.copy(&void_temp, "0");
                
                Ok(ExprResult {
                    temp: void_temp,
                    ty: TypeId::VOID,
                })
            }
            
            "pop" => {
                // pop() takes no arguments
                if !args.is_empty() {
                    return Err(Error::Codegen(format!(
                        "Array pop() expects 0 arguments, got {}",
                        args.len()
                    )));
                }
                
                // Allocate space for the popped element
                let out_buffer = self.symbol_table.gen_temp();
                self.qbe.alloc(&out_buffer, 8, 8); // Assuming 8-byte elements for now
                
                // Call thoxa_array_pop(array, buffer) - returns int (1 if success, 0 if empty)
                let success_temp = self.symbol_table.gen_temp();
                self.qbe.call(
                    Some(&success_temp),
                    builder::QbeType::W,
                    "thoxa_array_pop",
                    &[
                        (&array.temp, builder::QbeType::L),
                        (&out_buffer, builder::QbeType::L)
                    ]
                );
                
                // Create a May<T> based on success
                // We need to check if success == 1
                let cond_label = self.qbe.gen_label("pop_has");
                let else_label = self.qbe.gen_label("pop_not");
                let end_label = self.qbe.gen_label("pop_end");
                
                // Check if success == 1
                self.qbe.jnz(&success_temp, &cond_label, &else_label);
                
                // Has branch: we got a value
                self.qbe.label(&cond_label);
                
                // Load the value from the buffer
                let value_temp = self.symbol_table.gen_temp();
                self.qbe.load(&value_temp, builder::QbeType::L, &out_buffer);
                
                // Box the value for May<T>
                let boxed_temp = self.symbol_table.gen_temp();
                self.qbe.call(
                    Some(&boxed_temp),
                    builder::QbeType::L,
                    "thoxa_box_int",  // TODO: Use appropriate box function based on element type
                    &[(&value_temp, builder::QbeType::L)]
                );
                
                // Create Has(value)
                let has_temp = self.symbol_table.gen_temp();
                self.qbe.call(
                    Some(&has_temp),
                    builder::QbeType::L,
                    "thoxa_may_has",
                    &[(&boxed_temp, builder::QbeType::L)]
                );
                
                self.qbe.jmp(&end_label);
                
                // Not branch: array was empty
                self.qbe.label(&else_label);
                
                // Create Not()
                let not_temp = self.symbol_table.gen_temp();
                self.qbe.call(
                    Some(&not_temp),
                    builder::QbeType::L,
                    "thoxa_may_not",
                    &[]
                );
                
                self.qbe.jmp(&end_label);
                
                // End: phi to get the result
                self.qbe.label(&end_label);
                let result_temp = self.symbol_table.gen_temp();
                self.qbe.phi(
                    &result_temp,
                    builder::QbeType::L,
                    &[
                        (&has_temp, &cond_label),
                        (&not_temp, &else_label),
                    ]
                );
                
                Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::MAY,  // Returns May<T>
                })
            }
            
            "clear" => {
                // clear() takes no arguments
                if !args.is_empty() {
                    return Err(Error::Codegen(format!(
                        "Array clear() expects 0 arguments, got {}",
                        args.len()
                    )));
                }
                
                // Call thoxa_array_clear(array)
                self.qbe.call(
                    None,
                    builder::QbeType::W,
                    "thoxa_array_clear",
                    &[(&array.temp, builder::QbeType::L)]
                );
                
                // clear() returns void
                let void_temp = self.symbol_table.gen_temp();
                self.qbe.copy(&void_temp, "0");
                
                Ok(ExprResult {
                    temp: void_temp,
                    ty: TypeId::VOID,
                })
            }
            
            _ => Err(Error::Codegen(format!("Unknown array method: {}", method)))
        }
    }
}