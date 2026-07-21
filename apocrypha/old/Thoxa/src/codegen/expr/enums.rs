//! Enum and Trial type code generation

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for enum variant constructor (Pass/Fail/Has/Not)
    pub fn gen_enum_variant(&mut self, variant: &str, value: Option<&Expr>) -> Result<ExprResult> {
        match variant {
            "Pass" => self.gen_pass_variant(value),
            "Fail" => self.gen_fail_variant(value),
            "Has" => self.gen_has_variant(value),
            "Not" => self.gen_not_variant(),
            _ => Err(Error::Codegen(format!("Unknown enum variant: {}", variant))),
        }
    }
    
    /// Generate code for Pass(value) constructor
    fn gen_pass_variant(&mut self, value: Option<&Expr>) -> Result<ExprResult> {
        // Generate code for the value
        let (value_result, value_type) = if let Some(v) = value {
            let expr_result = self.gen_expr(v)?;
            let original_ty = expr_result.ty;
            
            // Use unified boxing system
            let boxed = if CodeGen::needs_boxing(expr_result.ty) {
                self.box_value(&expr_result)?
            } else {
                expr_result
            };
            (boxed, original_ty)  // Use original type, not boxed type
        } else {
            // Pass() with no value - use null/0
            let temp = self.symbol_table.gen_temp();
            self.qbe.copy(&temp, "0");
            (ExprResult {
                temp,
                ty: TypeId::POINTER,
            }, TypeId::VOID)
        };
        
        // Create generic Trial<T, E> instance
        let trial_instance = self.type_registry.register_generic_instance(
            "Trial".to_string(),
            vec![value_type, TypeId::STRING]  // Default error type to String
        );
        
        // Call thoxa_trial_pass to create the Trial
        let result_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&result_temp),
            crate::codegen::builder::QbeType::L,
            "thoxa_trial_pass",
            &[(&value_result.temp, crate::codegen::builder::QbeType::L)]
        );
        
        Ok(ExprResult {
            temp: result_temp,
            ty: trial_instance,  // Return generic Trial<T, E> instance
        })
    }
    
    /// Generate code for Fail(error) constructor
    fn gen_fail_variant(&mut self, value: Option<&Expr>) -> Result<ExprResult> {
        // Generate code for the error value
        let (value_result, error_type) = if let Some(v) = value {
            let expr_result = self.gen_expr(v)?;
            let original_ty = expr_result.ty;
            
            // Use unified boxing system
            let boxed = if CodeGen::needs_boxing(expr_result.ty) {
                self.box_value(&expr_result)?
            } else {
                expr_result
            };
            (boxed, original_ty)  // Use original type, not boxed type
        } else {
            // Fail() with no value - use null/0
            let temp = self.symbol_table.gen_temp();
            self.qbe.copy(&temp, "0");
            (ExprResult {
                temp,
                ty: TypeId::POINTER,
            }, TypeId::STRING)  // Default error type to String
        };
        
        // Create generic Trial<T, E> instance
        let trial_instance = self.type_registry.register_generic_instance(
            "Trial".to_string(),
            vec![TypeId::VOID, error_type]  // Value type is Void for Fail
        );
        
        // Call thoxa_trial_fail to create the Trial
        let result_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&result_temp),
            crate::codegen::builder::QbeType::L,
            "thoxa_trial_fail",
            &[(&value_result.temp, crate::codegen::builder::QbeType::L)]
        );
        
        Ok(ExprResult {
            temp: result_temp,
            ty: trial_instance,  // Return generic Trial<T, E> instance
        })
    }
    
    /// Generate code for Has(value) constructor (for May<T>)
    fn gen_has_variant(&mut self, value: Option<&Expr>) -> Result<ExprResult> {
        // Generate code for the value
        let (value_result, original_type) = if let Some(v) = value {
            let expr_result = self.gen_expr(v)?;
            let original_ty = expr_result.ty;
            
            // Use unified boxing system
            let boxed = if CodeGen::needs_boxing(expr_result.ty) {
                self.box_value(&expr_result)?
            } else {
                expr_result
            };
            (boxed, original_ty)
        } else {
            return Err(Error::Codegen("Has() requires a value".to_string()));
        };
        
        // Create generic May<T> instance using the original type (before boxing)
        let may_instance = self.type_registry.register_generic_instance(
            "May".to_string(),
            vec![original_type]  // Use the original type, not the boxed type
        );
        
        // Call thoxa_may_has to create the May
        let result_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&result_temp),
            crate::codegen::builder::QbeType::L,
            "thoxa_may_has",
            &[(&value_result.temp, crate::codegen::builder::QbeType::L)]
        );
        
        Ok(ExprResult {
            temp: result_temp,
            ty: may_instance,  // Return generic May<T> instance
        })
    }
    
    /// Generate code for Not constructor (for May<T>)
    fn gen_not_variant(&mut self) -> Result<ExprResult> {
        // Not() has no value - but we still need to determine the type
        // For now, we'll create May<Void> as a placeholder
        // In practice, type inference should determine the actual type
        let may_instance = self.type_registry.register_generic_instance(
            "May".to_string(),
            vec![TypeId::VOID]
        );
        
        // Not() has no value - use null/0
        let null_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&null_temp, "0");
        
        // Call thoxa_may_not to create the May
        let result_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&result_temp),
            crate::codegen::builder::QbeType::L,
            "thoxa_may_not",
            &[]  // No arguments needed for Not
        );
        
        Ok(ExprResult {
            temp: result_temp,
            ty: may_instance,  // Return generic May<T> instance
        })
    }
}