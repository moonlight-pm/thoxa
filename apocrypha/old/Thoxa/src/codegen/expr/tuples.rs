//! Tuple expression code generation

use crate::ast::Expr;
use crate::codegen::CodeGen;
use crate::codegen::expr::ExprResult;
use crate::codegen::types::TypeId;
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for a tuple literal
    pub(super) fn gen_tuple(&mut self, elements: &[Expr]) -> Result<ExprResult> {
        // Evaluate all elements
        let mut elem_results = Vec::new();
        let mut elem_types = Vec::new();
        
        for elem in elements {
            let result = self.gen_expr(elem)?;
            elem_types.push(result.ty);
            elem_results.push(result);
        }
        
        // Register or find the tuple type
        let tuple_type = self.type_registry.register_tuple_type(elem_types.clone());
        
        // Calculate size: header (8 bytes for type tag) + element pointers
        let size = 8 + elements.len() * 8;
        
        // Allocate memory for the tuple
        let tuple_ptr = self.symbol_table.gen_temp();
        let size_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&size_temp, &format!("{}", size));
        self.qbe.call(
            Some(&tuple_ptr),
            crate::codegen::builder::QbeType::L,
            "thoxa_alloc",
            &[(&size_temp, crate::codegen::builder::QbeType::L)]
        );
        
        // Store type tag
        let type_tag = tuple_type.to_u16();
        self.qbe.store(crate::codegen::builder::QbeType::W, &format!("{}", type_tag), &tuple_ptr);
        
        // Store each element
        for (i, result) in elem_results.iter().enumerate() {
            let offset = 8 + i * 8;  // Skip header
            let elem_ptr = self.symbol_table.gen_temp();
            self.qbe.add(&elem_ptr, crate::codegen::builder::QbeType::L, 
                        &tuple_ptr, &format!("{}", offset));
            
            // Box value types if needed (like units do)
            let final_value = if CodeGen::needs_boxing(result.ty) {
                let boxed_result = self.box_value(&result)?;
                boxed_result.temp
            } else {
                result.temp.clone()
            };
            
            // Store the element pointer
            self.qbe.store(crate::codegen::builder::QbeType::L, &final_value, &elem_ptr);
        }
        
        Ok(ExprResult {
            temp: tuple_ptr,
            ty: tuple_type,
        })
    }
    
    /// Generate code for tuple field access
    pub(super) fn gen_tuple_access(&mut self, tuple: &Expr, index: usize) -> Result<ExprResult> {
        let tuple_result = self.gen_expr(tuple)?;
        
        // Get tuple element types
        let elem_types = if let Some(info) = self.type_registry.get(tuple_result.ty) {
            if let Some(ref elems) = info.tuple_elements {
                elems.clone()
            } else {
                return Err(Error::Codegen("Type is not a tuple".to_string()));
            }
        } else {
            return Err(Error::Codegen(format!("Unknown type: {:?}", tuple_result.ty)));
        };
        
        // Check index bounds
        if index >= elem_types.len() {
            return Err(Error::Codegen(format!(
                "Tuple index {} out of bounds for tuple with {} elements",
                index, elem_types.len()
            )));
        }
        
        // Calculate offset: header (8 bytes) + index * 8
        let offset = 8 + index * 8;
        
        // Load the element
        let elem_ptr = self.symbol_table.gen_temp();
        self.qbe.add(&elem_ptr, crate::codegen::builder::QbeType::L,
                    &tuple_result.temp, &format!("{}", offset));
        
        let elem_value = self.symbol_table.gen_temp();
        self.qbe.load(&elem_value, crate::codegen::builder::QbeType::L, &elem_ptr);
        
        // Unbox value types if needed
        let result = ExprResult {
            temp: elem_value,
            ty: elem_types[index],
        };
        
        if CodeGen::needs_boxing(elem_types[index]) {
            // Unbox primitive values
            self.unbox_value(&result, elem_types[index])
        } else {
            Ok(result)
        }
    }
}