//! Pattern matching code generation

use crate::ast::{Expr, MatchArm, Pattern};
use crate::codegen::{CodeGen, expr::ExprResult, builder::QbeType};
use crate::codegen::types::TypeId;
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for a match expression
    pub fn gen_match(&mut self, expr: &Expr, arms: &[MatchArm]) -> Result<ExprResult> {
        // Generate code for the value being matched
        let matched = self.gen_expr(expr)?;
        
        // Support matching on Trial types (Pass/Fail), May types (Has/Not), and union types
        // The discriminant tells us which variant it is
        let is_union = if let Some(type_info) = self.type_registry.get(matched.ty) {
            type_info.kind == crate::codegen::types::TypeKind::Union
        } else {
            false
        };
        
        let is_may = self.is_may_instance(matched.ty);
        let is_trial = self.is_trial_instance(matched.ty);
        
        
        if !is_trial && !is_may && !is_union && matched.ty != TypeId::POINTER {
            return Err(Error::Codegen(format!(
                "Match expression currently only supports Trial, May, and union values, got {:?}",
                matched.ty
            )));
        }
        
        // Create labels for each arm and the end
        let end_label = self.qbe.gen_label("match_end");
        let mut arm_labels = Vec::new();
        for i in 0..arms.len() {
            arm_labels.push(self.qbe.gen_label(&format!("match_arm_{}", i)));
        }
        
        // Also create test labels for the comparison chain
        let mut test_labels = Vec::new();
        for i in 0..arms.len() {
            test_labels.push(self.qbe.gen_label(&format!("match_test_{}", i)));
        }
        
        // Result will be stored in a temporary
        // We'll determine the result type from the first arm later
        let result_temp = self.symbol_table.gen_temp();
        let result_slot = self.symbol_table.gen_temp();
        let mut result_ty = TypeId::VOID; // Will be set by first arm
        
        // Allocate stack slot for result
        self.qbe.alloc(&result_slot, 8, 8);
        
        // Initialize with a default value to avoid QBE complaints
        self.qbe.store(QbeType::L, "0", &result_slot);
        
        // Load the discriminant from the value
        let disc_value = self.symbol_table.gen_temp();
        
        if is_union {
            // For union types, get the variant type ID
            // Union layout: [union_type_id (16-bit)][variant_type_id (16-bit)][size (32-bit)][value_ptr (64-bit)]
            let variant_ptr = self.symbol_table.gen_temp();
            self.qbe.add(&variant_ptr, QbeType::L, &matched.temp, "2");  // Offset to variant_type_id (skip union_type_id)
            
            // Load as halfword (QBE automatically zero-extends to word)
            self.qbe.load(&disc_value, QbeType::H, &variant_ptr);  // Load 16-bit halfword as word
        } else {
            // For Trial/May types, load discriminant directly from the pointer (4 bytes at offset 0)
            self.qbe.load(&disc_value, QbeType::W, &matched.temp);
        }
        
        // Jump to first test
        if !test_labels.is_empty() {
            self.qbe.jmp(&test_labels[0]);
        }
        
        // Generate test chain for each pattern
        for (i, arm) in arms.iter().enumerate() {
            self.qbe.label(&test_labels[i]);
            
            match &arm.pattern {
                Pattern::Constructor { name, .. } => {
                    // For Pass/Fail and Has/Not patterns
                    let expected_disc = match name.as_str() {
                        "Pass" | "Has" => 0,
                        "Fail" | "Not" => 1,
                        _ => return Err(Error::Codegen(format!("Unknown constructor: {}", name))),
                    };
                    
                    // Compare discriminant
                    let cmp_temp = self.symbol_table.gen_temp();
                    self.qbe.cmp(&cmp_temp, "eq", QbeType::W, &disc_value, &expected_disc.to_string());
                    
                    // Jump to arm if match, otherwise go to next test
                    let next_label = if i + 1 < arms.len() {
                        test_labels[i + 1].clone()
                    } else {
                        end_label.clone()  // No more patterns, no match found
                    };
                    
                    self.qbe.jnz(&cmp_temp, &arm_labels[i], &next_label);
                }
                Pattern::Type { name, .. } => {
                    // For union type patterns like `c: Circle`
                    // Get the type ID for the named type
                    let expected_type_id = self.type_registry.get_by_name(name)
                        .ok_or_else(|| Error::Codegen(format!("Unknown type in pattern: {}", name)))?
                        .id;
                    
                    // Compare discriminant (which is the variant type ID for unions)
                    let cmp_temp = self.symbol_table.gen_temp();
                    self.qbe.cmp(&cmp_temp, "eq", QbeType::W, &disc_value, &format!("{}", expected_type_id.0));
                    
                    // Jump to arm if match, otherwise go to next test
                    let next_label = if i + 1 < arms.len() {
                        test_labels[i + 1].clone()
                    } else {
                        end_label.clone()  // No more patterns, no match found
                    };
                    
                    self.qbe.jnz(&cmp_temp, &arm_labels[i], &next_label);
                }
                Pattern::Ident { .. } => {
                    // Catch-all pattern - always matches
                    // Jump directly to this arm
                    self.qbe.jmp(&arm_labels[i]);
                }
                _ => {
                    return Err(Error::Codegen(format!(
                        "Pattern type not yet supported: {:?}",
                        arm.pattern
                    )))
                }
            }
        }
        
        // Generate code for each arm body
        for (i, arm) in arms.iter().enumerate() {
            self.qbe.label(&arm_labels[i]);
            
            // Enter new scope for pattern bindings
            self.symbol_table.push_scope();
            
            // Bind pattern variables
            match &arm.pattern {
                Pattern::Constructor { fields, .. } => {
                    // Extract the value from the Trial
                    if let Some(fields) = fields {
                        if fields.len() == 1 {
                            // Get the value pointer from the Trial
                            let value_ptr_offset = self.symbol_table.gen_temp();
                            let value_ptr = self.symbol_table.gen_temp();
                            
                            // Value pointer is after discriminant (4 bytes + 4 bytes padding = offset 8)
                            self.qbe.add(&value_ptr_offset, QbeType::L, &matched.temp, "8");
                            self.qbe.load(&value_ptr, QbeType::L, &value_ptr_offset);
                            
                            // Bind the pattern variable (as borrowed, not owned)
                            if let Some(field) = fields.first() {
                                if let Some(Pattern::Ident { name, .. }) = &field.pattern {
                                    // Extract the inner type from Trial<T,E> or May<T>
                                    let inner_type = if self.is_trial_instance(matched.ty) || self.is_may_instance(matched.ty) {
                                        // Get the first type parameter (T in Trial<T,E> or T in May<T>)
                                        if let Some(type_info) = self.type_registry.get(matched.ty) {
                                            if let Some(ref params) = type_info.type_params {
                                                if !params.is_empty() {
                                                    params[0]  // The T in Trial<T,E> or May<T>
                                                } else {
                                                    TypeId::POINTER  // Fallback
                                                }
                                            } else {
                                                TypeId::POINTER  // Fallback
                                            }
                                        } else {
                                            TypeId::POINTER  // Fallback
                                        }
                                    } else {
                                        TypeId::POINTER  // Default for non-enum types
                                    };
                                    
                                    // Unbox value types when extracting from Trial
                                    let final_value = match inner_type {
                                        TypeId::INT | TypeId::INT64 => {
                                            // Unbox 64-bit integer
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::L,
                                                "thoxa_unbox_int64_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::INT32 => {
                                            // Unbox 32-bit integer
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::W,
                                                "thoxa_unbox_int32_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::INT16 => {
                                            // Unbox 16-bit integer
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::H,
                                                "thoxa_unbox_int16_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::INT8 => {
                                            // Unbox 8-bit integer
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::B,
                                                "thoxa_unbox_int8_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::BOOL => {
                                            // Unbox boolean
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::W,
                                                "thoxa_unbox_bool_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::FLOAT | TypeId::FLOAT64 => {
                                            // Unbox 64-bit float
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::D,
                                                "thoxa_unbox_float64_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        TypeId::FLOAT32 => {
                                            // Unbox 32-bit float
                                            let unboxed_temp = self.symbol_table.gen_temp();
                                            self.qbe.call(
                                                Some(&unboxed_temp),
                                                crate::codegen::builder::QbeType::S,
                                                "thoxa_unbox_float32_unified",
                                                &[(&value_ptr, crate::codegen::builder::QbeType::L)]
                                            );
                                            unboxed_temp
                                        }
                                        _ => {
                                            // Already a pointer or other type, no unboxing needed
                                            value_ptr.clone()
                                        }
                                    };
                                    
                                    // Insert as a borrowed reference that doesn't need cleanup
                                    let symbol = self.symbol_table.insert_borrowed(name.clone(), inner_type)?;
                                    self.qbe.copy(&symbol.qbe_name, &final_value);
                                }
                            }
                        }
                    }
                }
                Pattern::Type { name, pattern, .. } => {
                    // For union type patterns, extract the value from the union
                    if is_union {
                        // Get the expected type ID
                        let expected_type_id = self.type_registry.get_by_name(name)
                            .ok_or_else(|| Error::Codegen(format!("Unknown type in pattern: {}", name)))?
                            .id;
                        
                        // Extract the value from the union
                        let extracted = self.extract_from_union(&matched, expected_type_id)?;
                        
                        // Bind the pattern variable if there is one
                        if let Pattern::Ident { name, .. } = pattern.as_ref() {
                            let symbol = self.symbol_table.insert_borrowed(name.clone(), expected_type_id)?;
                            self.qbe.copy(&symbol.qbe_name, &extracted.temp);
                        }
                    }
                }
                Pattern::Ident { name, .. } => {
                    // Bind the entire matched value
                    let symbol = self.symbol_table.insert(name.clone(), matched.ty, false)?;
                    self.qbe.copy(&symbol.qbe_name, &matched.temp);
                }
                _ => {}
            }
            
            // Generate arm body
            let arm_result = self.gen_block(&arm.body)?;
            
            // Set result type from first arm
            if i == 0 {
                result_ty = arm_result.ty;
            } else if arm_result.ty != result_ty {
                // Verify all arms have the same type
                return Err(Error::Codegen(format!(
                    "Match arms have different types: first arm has {:?}, arm {} has {:?}",
                    result_ty, i + 1, arm_result.ty
                )));
            }
            
            // Exit pattern scope and get cleanup vars
            let cleanup_vars = self.symbol_table.pop_scope()?;
            
            // Store result and jump if not terminated
            if !self.qbe.is_terminated() {
                self.qbe.store(QbeType::L, &arm_result.temp, &result_slot);
                
                // Do cleanup before jumping
                for var in &cleanup_vars {
                    self.qbe.call_void("thoxa_decref", vec![(var.qbe_name.clone(), "l")]);
                }
                
                self.qbe.jmp(&end_label);
            }
        }
        
        // End label
        self.qbe.label(&end_label);
        
        // Only load result if we're not already terminated
        if !self.qbe.is_terminated() {
            // Load result
            self.qbe.load(&result_temp, QbeType::L, &result_slot);
        }
        
        Ok(ExprResult {
            temp: result_temp,
            ty: result_ty,
        })
    }
}