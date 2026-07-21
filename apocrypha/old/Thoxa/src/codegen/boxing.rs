//! Unified boxing system for value types
//! 
//! This module provides systematic boxing and unboxing of primitive values
//! for use in unions, Trial/May types, and other heap-allocated structures.

use crate::codegen::{CodeGen, types::TypeId};
use crate::codegen::builder::QbeType;
use crate::codegen::expr::ExprResult;
use crate::error::Result;

impl CodeGen {
    /// Box a value based on its type, using the unified boxing system
    pub fn box_value(&mut self, value: &ExprResult) -> Result<ExprResult> {
        let boxed_temp = self.symbol_table.gen_temp();
        
        match value.ty {
            TypeId::INT | TypeId::INT64 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_int64_unified",
                    &[(&value.temp, QbeType::L)]
                );
            }
            TypeId::INT32 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_int32_unified",
                    &[(&value.temp, QbeType::W)]
                );
            }
            TypeId::INT16 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_int16_unified",
                    &[(&value.temp, QbeType::H)]
                );
            }
            TypeId::INT8 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_int8_unified",
                    &[(&value.temp, QbeType::B)]
                );
            }
            TypeId::BOOL => {
                // Bool values are already stored as L but need to be passed as B
                // We'll just pass the L value and let the C function handle it
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_bool_unified",
                    &[(&value.temp, QbeType::L)]
                );
            }
            TypeId::FLOAT | TypeId::FLOAT64 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_float64_unified",
                    &[(&value.temp, QbeType::D)]
                );
            }
            TypeId::FLOAT32 => {
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_box_float32_unified",
                    &[(&value.temp, QbeType::S)]
                );
            }
            TypeId::STRING => {
                // Strings are already heap-allocated, just need proper wrapping
                self.qbe.call(
                    Some(&boxed_temp),
                    QbeType::L,
                    "thoxa_string_new",
                    &[(&value.temp, QbeType::L)]
                );
            }
            TypeId::POINTER | TypeId::TRIAL | TypeId::MAY => {
                // Already heap-allocated, no boxing needed
                return Ok(value.clone());
            }
            _ => {
                // For user-defined types, they're already heap-allocated
                return Ok(value.clone());
            }
        }
        
        Ok(ExprResult {
            temp: boxed_temp,
            ty: TypeId::POINTER,  // Boxed values are pointers
        })
    }
    
    /// Unbox a value to its original type
    pub fn unbox_value(&mut self, boxed: &ExprResult, target_type: TypeId) -> Result<ExprResult> {
        let unboxed_temp = self.symbol_table.gen_temp();
        
        match target_type {
            TypeId::INT | TypeId::INT64 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::L,
                    "thoxa_unbox_int64_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::INT32 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::W,
                    "thoxa_unbox_int32_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::INT16 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::H,
                    "thoxa_unbox_int16_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::INT8 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::B,
                    "thoxa_unbox_int8_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::BOOL => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::B,
                    "thoxa_unbox_bool_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::FLOAT | TypeId::FLOAT64 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::D,
                    "thoxa_unbox_float64_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            TypeId::FLOAT32 => {
                self.qbe.call(
                    Some(&unboxed_temp),
                    QbeType::S,
                    "thoxa_unbox_float32_unified",
                    &[(&boxed.temp, QbeType::L)]
                );
            }
            _ => {
                // For non-primitive types, just return the pointer
                return Ok(boxed.clone());
            }
        }
        
        Ok(ExprResult {
            temp: unboxed_temp,
            ty: target_type,
        })
    }
    
    /// Check if a type needs boxing when stored in a union or Trial/May
    pub fn needs_boxing(ty: TypeId) -> bool {
        match ty {
            // Primitive types need boxing
            TypeId::INT | TypeId::INT64 | TypeId::INT32 | TypeId::INT16 | TypeId::INT8 |
            TypeId::BOOL | TypeId::FLOAT | TypeId::FLOAT64 | TypeId::FLOAT32 => true,
            
            // Already heap-allocated types don't need boxing
            TypeId::STRING | TypeId::POINTER | TypeId::TRIAL | TypeId::MAY => false,
            
            // User-defined types are heap-allocated
            _ => false,
        }
    }
}