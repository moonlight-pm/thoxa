//! Try operator (?) code generation
//!
//! The try operator desugars to a match expression:
//! ```text
//! expr?
//! ```
//! becomes:
//! ```text
//! match expr {
//!     Pass(value) -> value,
//!     Fail(error) -> return Fail(error)
//! }
//! ```

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId};
use crate::codegen::expr::ExprResult;
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for the try operator (?)
    pub fn gen_try_op(&mut self, expr: &Expr) -> Result<ExprResult> {
        // Check that we're in a function that returns Trial
        let _func_ctx = self.current_function.as_ref()
            .ok_or_else(|| Error::Codegen("Try operator used outside of function".to_string()))?;
        
        // TODO: Check that function return type is Trial<T,E>
        // For now, we assume it is
        
        // Evaluate the expression (should be a Trial)
        let trial_result = self.gen_expr(expr)?;
        
        // Generate labels for the check
        let check_label = self.qbe.gen_label("try_check");
        let pass_label = self.qbe.gen_label("try_pass");
        let fail_label = self.qbe.gen_label("try_fail");
        let continue_label = self.qbe.gen_label("try_continue");
        
        // Allocate temporary for the unwrapped value
        let result_temp = self.symbol_table.gen_temp();
        
        // Jump to check
        self.qbe.jmp(&check_label);
        
        // Check discriminant
        self.qbe.label(&check_label);
        let disc_temp = self.symbol_table.gen_temp();
        self.qbe.load(&disc_temp, crate::codegen::builder::QbeType::W, &trial_result.temp);
        
        // Compare with 0 (Pass)
        let cmp_temp = self.symbol_table.gen_temp();
        self.qbe.cmp(&cmp_temp, "eq", crate::codegen::builder::QbeType::W, &disc_temp, "0");
        
        // Branch based on result
        self.qbe.jnz(&cmp_temp, &pass_label, &fail_label);
        
        // Extract the success type from Trial<T,E> first (needed for unboxing)
        let unwrapped_type = if trial_result.ty == TypeId::TRIAL {
            // Base Trial type, default to INT for now
            TypeId::INT
        } else {
            // Get the type parameters from the Trial instance
            if let Some(type_info) = self.type_registry.get(trial_result.ty) {
                if let Some(ref params) = type_info.type_params {
                    if !params.is_empty() {
                        params[0]  // The T in Trial<T,E>
                    } else {
                        TypeId::INT  // Fallback
                    }
                } else {
                    TypeId::INT  // Fallback
                }
            } else {
                TypeId::INT  // Fallback
            }
        };
        
        // Pass case: extract the value
        self.qbe.label(&pass_label);
        let value_ptr_offset = self.symbol_table.gen_temp();
        let boxed_value = self.symbol_table.gen_temp();
        self.qbe.add(&value_ptr_offset, crate::codegen::builder::QbeType::L, &trial_result.temp, "8");
        self.qbe.load(&boxed_value, crate::codegen::builder::QbeType::L, &value_ptr_offset);
        
        // Unbox value types after extracting from Trial
        match unwrapped_type {
            TypeId::INT | TypeId::INT64 => {
                // Unbox 64-bit integer
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::L,
                    "thoxa_unbox_int",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::INT32 => {
                // Unbox 32-bit integer
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::W,
                    "thoxa_unbox_int32",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::INT16 => {
                // Unbox 16-bit integer
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::H,
                    "thoxa_unbox_int16",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::INT8 => {
                // Unbox 8-bit integer
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::B,
                    "thoxa_unbox_int8",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::BOOL => {
                // Unbox boolean
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::W,
                    "thoxa_unbox_bool",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::FLOAT | TypeId::FLOAT64 => {
                // Unbox 64-bit float
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::D,
                    "thoxa_unbox_float",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            TypeId::FLOAT32 => {
                // Unbox 32-bit float
                self.qbe.call(
                    Some(&result_temp),
                    crate::codegen::builder::QbeType::S,
                    "thoxa_unbox_float32",
                    &[(&boxed_value, crate::codegen::builder::QbeType::L)]
                );
            }
            _ => {
                // Already a pointer or other type, no unboxing needed
                self.qbe.copy(&result_temp, &boxed_value);
            }
        }
        
        // Clean up the Trial (decref)
        self.qbe.call_void("thoxa_decref", vec![(trial_result.temp.clone(), "l")]);
        
        // Continue with the unwrapped value
        self.qbe.jmp(&continue_label);
        
        // Fail case: return early with the Fail
        self.qbe.label(&fail_label);
        
        // Return the Trial as-is (it's already a Fail)
        self.qbe.ret(Some(&trial_result.temp));
        
        // Continue label
        self.qbe.label(&continue_label);
        
        Ok(ExprResult {
            temp: result_temp,
            ty: unwrapped_type,
        })
    }
}