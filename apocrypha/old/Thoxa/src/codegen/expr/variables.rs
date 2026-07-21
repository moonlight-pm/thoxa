//! Variable and identifier expression code generation

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for an identifier (variable reference)
    pub(super) fn gen_ident(&mut self, name: &str) -> Result<ExprResult> {
        // First check if it's a variable in the symbol table
        if let Some(symbol) = self.symbol_table.lookup(name) {
            return Ok(ExprResult {
                temp: symbol.qbe_name.clone(),
                ty: symbol.type_id,
            });
        }
        
        // Check if it's a function name (either regular or generic)
        if let Some(func_info) = self.functions.get(name) {
            // Create a function type for this function
            let func_type = self.type_registry.register_function_type(
                func_info.param_types.clone(),
                func_info.return_type.unwrap_or(TypeId::VOID)
            );
            
            // For function references, we need to get the function pointer
            // In QBE, function names are global symbols prefixed with $
            let func_ptr = self.symbol_table.gen_temp();
            self.qbe.instruction(format!("{} =l copy ${}", func_ptr, func_info.external_name.as_ref().unwrap_or(&func_info.name)));
            
            return Ok(ExprResult {
                temp: func_ptr,
                ty: func_type,
            });
        }
        
        // Check if it's a generic function that needs a type variable
        if let Some(ast_func) = self.ast_functions.get(name) {
            // For generic functions without type arguments, we can't create a concrete function pointer
            // This would need full type inference or explicit type arguments
            // For now, return an error asking for type arguments
            return Err(Error::Codegen(format!(
                "Generic function '{}' requires type arguments when used as a value. \
                 Consider providing explicit type arguments or using it in a context where types can be inferred.",
                name
            )));
        }
        
        // Check if it's a unit constructor (a unit type with constructor params)
        if let Some(unit_type) = self.type_registry.get_by_name(name) {
            if let Some(ref constructor_params) = unit_type.constructor_params {
                // Create a function type for the constructor
                let param_types: Vec<TypeId> = constructor_params.iter().map(|(_, ty)| *ty).collect();
                let constructor_type = self.type_registry.register_function_type(
                    param_types,
                    unit_type.id
                );
                
                // Generate a synthetic constructor function name
                let constructor_name = format!("thoxa_constructor_{}", name);
                let func_ptr = self.symbol_table.gen_temp();
                self.qbe.instruction(format!("{} =l copy ${}", func_ptr, constructor_name));
                
                return Ok(ExprResult {
                    temp: func_ptr,
                    ty: constructor_type,
                });
            }
        }
        
        Err(Error::Codegen(format!("Unknown variable: {}", name)))
    }
    
    /// Generate code for field access (e.g., obj.field)
    pub(super) fn gen_field_access(&mut self, object: &crate::ast::Expr, field: &str) -> Result<ExprResult> {
        // Generate code for the object
        let obj_result = self.gen_expr(object)?;
        
        // Get the type information first to check the name
        let type_info = self.type_registry.get(obj_result.ty)
            .ok_or_else(|| Error::Codegen(format!("Unknown type ID: {:?}", obj_result.ty)))?;
        
        // Special handling for array properties
        if (type_info.kind == crate::codegen::types::TypeKind::Array || type_info.name == "Ptr") && field == "count" {
            // For arrays, call thoxa_array_length (runtime still uses 'length')
            let count_temp = self.symbol_table.gen_temp();
            self.qbe.call(
                Some(&count_temp),
                crate::codegen::builder::QbeType::L,
                "thoxa_array_length",
                &[(&obj_result.temp, crate::codegen::builder::QbeType::L)]
            );
            
            return Ok(ExprResult {
                temp: count_temp,
                ty: TypeId::INT,
            });
        }
        
        // Get field information
        let field_info = type_info.fields.as_ref()
            .ok_or_else(|| Error::Codegen(format!("{} is not a unit type", type_info.name)))?;
        
        // Find the field
        let (field_idx, field_type, _is_mutable) = field_info.iter()
            .enumerate()
            .find_map(|(idx, (name, ty, mutable))| {
                if name == field {
                    Some((idx, *ty, *mutable))
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::Codegen(format!("Unknown field {} in {}", field, type_info.name)))?;
        
        // Calculate field offset
        let field_offset = 16 + field_idx * 8;  // Skip header
        
        // Load the field value
        let field_ptr = self.symbol_table.gen_temp();
        self.qbe.add(&field_ptr, crate::codegen::builder::QbeType::L, &obj_result.temp, &format!("{}", field_offset));
        
        // Determine the correct QBE type for the field
        let qbe_type = self.get_qbe_type(field_type)?;
        
        let field_value = self.symbol_table.gen_temp();
        self.qbe.load(&field_value, qbe_type, &field_ptr);
        
        // Unbox value types if needed
        let final_result = if CodeGen::needs_boxing(field_type) {
            // The loaded value is a pointer to a box, unbox it
            self.unbox_value(&ExprResult {
                temp: field_value.clone(),
                ty: TypeId::POINTER,
            }, field_type)?
        } else {
            ExprResult {
                temp: field_value,
                ty: field_type,
            }
        };
        
        Ok(final_result)
    }
    
    /// Generate code for assignment
    pub(super) fn gen_assign(&mut self, target: &Expr, value: &Expr) -> Result<ExprResult> {
        // For now, only support simple identifier targets
        let target_name = match target {
            Expr::Ident { name, .. } => name,
            _ => return Err(Error::Codegen("Assignment target must be a simple identifier (for now)".to_string())),
        };
        
        // Look up the variable and extract needed information
        let (symbol_qbe_name, symbol_type_id, is_mut, needs_refcount) = {
            let symbol = self.symbol_table.lookup(target_name)
                .ok_or_else(|| Error::Codegen(format!("Undefined variable: {}", target_name)))?;
            (symbol.qbe_name.clone(), symbol.type_id, symbol.is_mut, symbol.needs_refcount)
        };
        
        // Check mutability
        if !is_mut {
            return Err(Error::Codegen(format!("Cannot assign to immutable variable: {}", target_name)));
        }
        
        // Generate code for the value expression
        let value_result = self.gen_expr(value)?;
        
        // Type check
        if value_result.ty != symbol_type_id {
            return Err(Error::Codegen(format!(
                "Type mismatch in assignment: expected {:?}, got {:?}",
                symbol_type_id, value_result.ty
            )));
        }
        
        // Handle reference counting for pointer types
        if needs_refcount {
            // Save the old value
            let old_val = self.symbol_table.gen_temp();
            self.qbe.copy(&old_val, &symbol_qbe_name);
            
            // Incref the new value (unless it's null)
            let null_check = self.symbol_table.gen_temp();
            self.qbe.instruction(format!("{} =w ceql {}, 0", null_check, value_result.temp));
            
            let incref_label = self.qbe.gen_label("incref");
            let skip_incref = self.qbe.gen_label("skip_incref");
            self.qbe.jnz(&null_check, &skip_incref, &incref_label);
            
            self.qbe.label(&incref_label);
            self.qbe.call_void("thoxa_retain", vec![(value_result.temp.clone(), "l")]);
            self.qbe.jmp(&skip_incref);
            
            self.qbe.label(&skip_incref);
            
            // Store the new value
            self.qbe.copy(&symbol_qbe_name, &value_result.temp);
            
            // Decref the old value (unless it's null)
            let old_null_check = self.symbol_table.gen_temp();
            self.qbe.instruction(format!("{} =w ceql {}, 0", old_null_check, old_val));
            
            let decref_label = self.qbe.gen_label("decref");
            let skip_decref = self.qbe.gen_label("skip_decref");
            self.qbe.jnz(&old_null_check, &skip_decref, &decref_label);
            
            self.qbe.label(&decref_label);
            self.qbe.call_void("thoxa_release", vec![(old_val, "l")]);
            self.qbe.jmp(&skip_decref);
            
            self.qbe.label(&skip_decref);
        } else {
            // Copy the value to the variable
            // For now, just copy directly. TODO: Implement proper 16-byte value structure
            self.qbe.copy(&symbol_qbe_name, &value_result.temp);
        }
        
        // Assignment expressions return void in Thoxa
        let void_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&void_temp, "0"); // Void is represented as 0
        
        Ok(ExprResult {
            temp: void_temp,
            ty: TypeId::VOID,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_undefined_variable() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to reference an undefined variable
        let result = codegen.gen_ident("undefined_var");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown variable"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_variable_lookup() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Manually insert a variable into the symbol table for testing
        let symbol = codegen.symbol_table.insert(
            "x".to_string(),
            TypeId::INT64,
            false
        ).unwrap();
        
        // Now try to reference it
        let result = codegen.gen_ident("x").unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        assert_eq!(result.temp, symbol.qbe_name);
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_assignment() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // First create a mutable variable
        let symbol = codegen.symbol_table.insert(
            "x".to_string(),
            TypeId::INT64,
            true // mutable
        ).unwrap();
        
        // Initialize it
        codegen.qbe.copy(&symbol.qbe_name, "10");
        
        // Create an assignment expression: x = 42
        let target = Expr::Ident {
            name: "x".to_string(),
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        let value = Expr::IntLiteral {
            value: 42,
            suffix: crate::lexer::IntSuffix::None,
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        
        let result = codegen.gen_assign(&target, &value).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy")); // Should have copy instructions
    }
    
    #[test]
    fn test_assignment_to_immutable() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create an immutable variable
        codegen.symbol_table.insert(
            "x".to_string(),
            TypeId::INT64,
            false // immutable
        ).unwrap();
        
        // Try to assign to it
        let target = Expr::Ident {
            name: "x".to_string(),
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        let value = Expr::IntLiteral {
            value: 42,
            suffix: crate::lexer::IntSuffix::None,
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        
        let result = codegen.gen_assign(&target, &value);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot assign to immutable variable"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_assignment_type_mismatch() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a mutable bool variable
        codegen.symbol_table.insert(
            "flag".to_string(),
            TypeId::BOOL,
            true // mutable
        ).unwrap();
        
        // Try to assign an integer to it
        let target = Expr::Ident {
            name: "flag".to_string(),
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        let value = Expr::IntLiteral {
            value: 42,
            suffix: crate::lexer::IntSuffix::None,
            span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
        };
        
        let result = codegen.gen_assign(&target, &value);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Type mismatch in assignment"));
        
        codegen.qbe.function_end().unwrap();
    }
}