//! Unit construction and field access code generation

use crate::ast::Expr;
use crate::codegen::CodeGen;
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for unit construction
    pub fn gen_unit_construction(&mut self, name: &str, fields: &[(String, Expr)]) -> Result<ExprResult> {
        // Look up the unit type and extract needed information
        let (unit_type_id, field_info, alloc_size) = {
            let unit_type = self.type_registry.get_by_name(name)
                .ok_or_else(|| Error::Codegen(format!("Unknown unit type: {}", name)))?;
            
            let field_info = unit_type.fields.as_ref()
                .ok_or_else(|| Error::Codegen(format!("{} is not a unit type", name)))?
                .clone();
            
            let alloc_size = 16 + field_info.len() * 8;
            (unit_type.id, field_info, alloc_size)
        };
        
        // Allocate memory for the unit
        let unit_ptr = self.symbol_table.gen_temp();
        let size_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&size_temp, &format!("{}", alloc_size));
        self.qbe.call(
            Some(&unit_ptr),
            crate::codegen::builder::QbeType::L,
            "thoxa_alloc",
            &[(&size_temp, crate::codegen::builder::QbeType::L)]
        );
        
        // Generate code for each field value
        let mut field_values = Vec::new();
        for (field_name, field_expr) in fields {
            // Find the field index
            let field_idx = field_info.iter()
                .position(|(name, _, _)| name == field_name)
                .ok_or_else(|| Error::Codegen(format!("Unknown field {} in {}", field_name, name)))?;
            
            // Generate code for the field value
            let field_value = self.gen_expr(field_expr)?;
            field_values.push((field_idx, field_value));
        }
        
        // Store field values in the unit
        for (idx, value) in field_values {
            let field_offset = 16 + idx * 8;  // Skip header
            let field_ptr = self.symbol_table.gen_temp();
            self.qbe.add(&field_ptr, crate::codegen::builder::QbeType::L, &unit_ptr, &format!("{}", field_offset));
            
            // Box value types if needed
            let final_value = if CodeGen::needs_boxing(value.ty) {
                let boxed_result = self.box_value(&value)?;
                boxed_result.temp
            } else {
                value.temp.clone()
            };
            
            self.qbe.store(crate::codegen::builder::QbeType::L, &final_value, &field_ptr);
        }
        
        Ok(ExprResult {
            temp: unit_ptr,
            ty: unit_type_id,
        })
    }
}