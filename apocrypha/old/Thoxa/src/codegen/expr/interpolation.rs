//! Code generation for string interpolation

use crate::ast::StringSegment;
use crate::codegen::{CodeGen, types::TypeId, builder};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for an interpolated string
    pub(super) fn gen_interpolated_string(&mut self, segments: &[StringSegment]) -> Result<ExprResult> {
        // If there's only one text segment with no interpolations, treat as regular string
        if segments.len() == 1 {
            if let StringSegment::Text(text) = &segments[0] {
                return self.gen_string_literal(text);
            }
        }
        
        // We'll build the string incrementally
        // Start with an empty string builder
        let mut result_temp: Option<String> = None;
        
        for segment in segments {
            match segment {
                StringSegment::Text(text) => {
                    // Create a string from the text
                    let text_label = self.qbe.add_string_literal(text);
                    let text_str = self.symbol_table.gen_temp();
                    self.qbe.call(
                        Some(&text_str),
                        builder::QbeType::L,
                        "thoxa_string_new",
                        &[(&text_label, builder::QbeType::L)]
                    );
                    
                    // Concatenate with result if we have one
                    if let Some(prev) = result_temp {
                        let new_result = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&new_result),
                            builder::QbeType::L,
                            "thoxa_string_concat",
                            &[(&prev, builder::QbeType::L), (&text_str, builder::QbeType::L)]
                        );
                        result_temp = Some(new_result);
                    } else {
                        result_temp = Some(text_str);
                    }
                }
                
                StringSegment::Interpolation(expr) => {
                    // Generate code for the expression
                    let expr_result = self.gen_expr(expr)?;
                    
                    // Convert the expression result to a string
                    let expr_str = match expr_result.ty {
                        TypeId::STRING => {
                            // Already a string
                            expr_result.temp.clone()
                        }
                        TypeId::INT64 | TypeId::INT32 | TypeId::INT16 | TypeId::INT8 |
                        TypeId::INT => {
                            // Convert integer to string
                            let str_temp = self.symbol_table.gen_temp();
                            self.qbe.call(
                                Some(&str_temp),
                                builder::QbeType::L,
                                "thoxa_int_to_string",
                                &[(&expr_result.temp, builder::QbeType::L)]
                            );
                            str_temp
                        }
                        TypeId::BOOL => {
                            // Convert bool to string
                            let str_temp = self.symbol_table.gen_temp();
                            self.qbe.call(
                                Some(&str_temp),
                                builder::QbeType::L,
                                "thoxa_bool_to_string",
                                &[(&expr_result.temp, builder::QbeType::W)]
                            );
                            str_temp
                        }
                        TypeId::FLOAT64 | TypeId::FLOAT32 | TypeId::FLOAT => {
                            // Convert float to string
                            let str_temp = self.symbol_table.gen_temp();
                            self.qbe.call(
                                Some(&str_temp),
                                builder::QbeType::L,
                                "thoxa_float_to_string",
                                &[(&expr_result.temp, builder::QbeType::D)]
                            );
                            str_temp
                        }
                        TypeId::CHAR => {
                            // Convert char to string
                            let str_temp = self.symbol_table.gen_temp();
                            self.qbe.call(
                                Some(&str_temp),
                                builder::QbeType::L,
                                "thoxa_char_to_string",
                                &[(&expr_result.temp, builder::QbeType::B)]
                            );
                            str_temp
                        }
                        _ => {
                            // For other types, we'll need type-specific conversion
                            // For now, error
                            return Err(Error::Codegen(format!(
                                "Cannot interpolate type {:?} in string",
                                expr_result.ty
                            )));
                        }
                    };
                    
                    // Concatenate with result if we have one
                    if let Some(prev) = result_temp {
                        let new_result = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&new_result),
                            builder::QbeType::L,
                            "thoxa_string_concat",
                            &[(&prev, builder::QbeType::L), (&expr_str, builder::QbeType::L)]
                        );
                        result_temp = Some(new_result);
                    } else {
                        result_temp = Some(expr_str);
                    }
                }
            }
        }
        
        // If we somehow have no result, return empty string
        let final_result = result_temp.unwrap_or_else(|| {
            let empty = self.symbol_table.gen_temp();
            let empty_label = self.qbe.add_string_literal("");
            self.qbe.call(
                Some(&empty),
                builder::QbeType::L,
                "thoxa_string_new",
                &[(&empty_label, builder::QbeType::L)]
            );
            empty
        });
        
        Ok(ExprResult {
            temp: final_result,
            ty: TypeId::STRING,
        })
    }
}