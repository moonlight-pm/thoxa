//! Code generation for union type operations

use crate::codegen::{CodeGen, types::TypeId};
use crate::codegen::builder::QbeType;
use crate::codegen::expr::ExprResult;
use crate::error::Result;

impl CodeGen {
    /// Convert a value to a union type if needed
    pub fn convert_to_union(&mut self, value: &ExprResult, target_type: TypeId) -> Result<ExprResult> {
        // Check if the target is a union type
        if let Some(target_info) = self.type_registry.get(target_type) {
            if target_info.kind == crate::codegen::types::TypeKind::Union {
                // Check if the value type is a variant of the union
                if self.type_registry.is_variant_of(value.ty, target_type) {
                    // Perform the conversion
                    let union_temp = self.symbol_table.gen_temp();
                    
                    // Box the value if it's a primitive
                    let boxed_value = if CodeGen::needs_boxing(value.ty) {
                        self.box_value(value)?
                    } else {
                        value.clone()
                    };
                    
                    // Call the union creation function
                    self.qbe.call(
                        Some(&union_temp),
                        QbeType::L,
                        "thoxa_union_create",
                        &[
                            (&format!("{}", target_type.0), QbeType::L),  // Union type ID
                            (&format!("{}", value.ty.0), QbeType::L),     // Variant type ID
                            (&boxed_value.temp, QbeType::L),              // Value pointer
                        ]
                    );
                    
                    return Ok(ExprResult {
                        temp: union_temp,
                        ty: target_type,
                    });
                }
            }
        }
        
        // No conversion needed or possible
        Ok(value.clone())
    }
    
    /// Extract a value from a union
    pub fn extract_from_union(&mut self, union: &ExprResult, expected_type: TypeId) -> Result<ExprResult> {
        let extracted_temp = self.symbol_table.gen_temp();
        
        // Call the union extraction function
        self.qbe.call(
            Some(&extracted_temp),
            QbeType::L,
            "thoxa_union_extract",
            &[
                (&union.temp, QbeType::L),                     // Union pointer
                (&format!("{}", expected_type.0), QbeType::L), // Expected type ID
            ]
        );
        
        // Unbox if needed
        let result = if CodeGen::needs_boxing(expected_type) {
            self.unbox_value(&ExprResult {
                temp: extracted_temp,
                ty: TypeId::POINTER,
            }, expected_type)?
        } else {
            ExprResult {
                temp: extracted_temp,
                ty: expected_type,
            }
        };
        
        Ok(result)
    }
    
    /// Check if a union contains a specific variant type
    pub fn check_union_variant(&mut self, union: &ExprResult, variant_type: TypeId) -> Result<String> {
        let check_temp = self.symbol_table.gen_temp();
        
        // Call the union variant check function
        self.qbe.call(
            Some(&check_temp),
            QbeType::W,
            "thoxa_union_is_variant",
            &[
                (&union.temp, QbeType::L),                    // Union pointer
                (&format!("{}", variant_type.0), QbeType::L), // Variant type ID to check
            ]
        );
        
        Ok(check_temp)
    }
    
    /// Get the variant type ID from a union
    pub fn get_union_variant_type(&mut self, union: &ExprResult) -> Result<String> {
        let type_temp = self.symbol_table.gen_temp();
        
        // Call the function to get variant type
        self.qbe.call(
            Some(&type_temp),
            QbeType::L,
            "thoxa_union_get_variant_type",
            &[(&union.temp, QbeType::L)]
        );
        
        Ok(type_temp)
    }
}