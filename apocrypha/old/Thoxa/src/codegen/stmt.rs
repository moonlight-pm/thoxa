//! Statement code generation

use crate::ast::Stmt;
use crate::codegen::CodeGen;
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for a statement
    pub fn gen_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        match stmt {
            Stmt::Let { name, ty, value, is_mut, is_export: _, .. } => {
                self.gen_let_binding_with_type(name, ty.as_ref(), value.as_ref(), *is_mut)
            }
            Stmt::Method { .. } => {
                // Nested functions are already processed during function generation
                // (see gen_function in function.rs)
                // They don't generate code in statement position
                Ok(())
            }
            Stmt::Expr(expr) => {
                // Generate the expression but discard the result
                self.gen_expr(expr)?;
                Ok(())
            }
            Stmt::Return { value, .. } => self.gen_return(value.as_ref()),
            Stmt::Break { value, .. } => self.gen_break(value.as_ref()),
            Stmt::Continue { .. } => self.gen_continue(),
        }
    }
    
    /// Generate code for a let/mut binding
    fn gen_let_binding(&mut self, name: &str, value: Option<&crate::ast::Expr>, is_mut: bool) -> Result<()> {
        // Get the initializer value (required for now)
        let init_expr = value.ok_or_else(|| 
            Error::Codegen("Variable declarations must have initializers (for now)".to_string())
        )?;
        
        // Generate code for the initializer expression
        let init_result = self.gen_expr(init_expr)?;
        
        // Use the type from the initializer
        let var_type = init_result.ty;
        
        // Add the variable to the symbol table with proper refcount tracking
        let needs_refcount = self.type_registry.needs_refcount(var_type);
        let symbol = self.symbol_table.insert_with_refcount(
            name.to_string(),
            var_type,
            is_mut,
            needs_refcount,
        )?;
        
        // Copy the value to the variable's QBE name with proper type
        let qbe_type = self.get_qbe_type(var_type)?;
        self.qbe.copy_typed(&symbol.qbe_name, qbe_type, &init_result.temp);
        
        // If this type needs reference counting, retain the value
        if needs_refcount {
            // Check for null before retaining
            let null_check = self.symbol_table.gen_temp();
            self.qbe.instruction(format!("{} =w ceql {}, 0", null_check, init_result.temp));
            
            let retain_label = self.qbe.gen_label("retain");
            let skip_retain = self.qbe.gen_label("skip_retain");
            self.qbe.jnz(&null_check, &skip_retain, &retain_label);
            
            self.qbe.label(&retain_label);
            self.qbe.call_void("thoxa_retain", vec![(init_result.temp.clone(), "l")]);
            self.qbe.jmp(&skip_retain);
            
            self.qbe.label(&skip_retain);
        }
        
        Ok(())
    }
    
    /// Generate code for a let/mut binding with type annotation
    fn gen_let_binding_with_type(&mut self, name: &str, ty: Option<&crate::ast::Type>, value: Option<&crate::ast::Expr>, is_mut: bool) -> Result<()> {
        // If no type annotation, delegate to the simpler version
        if ty.is_none() {
            return self.gen_let_binding(name, value, is_mut);
        }
        
        // Get the initializer value (required for now)
        let init_expr = value.ok_or_else(|| 
            Error::Codegen("Variable declarations must have initializers (for now)".to_string())
        )?;
        
        // Generate code for the initializer expression
        let mut init_result = self.gen_expr(init_expr)?;
        
        // Use the type annotation
        let type_annotation = ty.unwrap();
        let target_type = self.resolve_type(type_annotation)?;
        
        // Check if we need to convert to a union type
        if init_result.ty != target_type {
            // Try implicit conversion to union
            init_result = self.convert_to_union(&init_result, target_type)?;
        }
        
        // Add the variable to the symbol table with proper refcount tracking
        let needs_refcount = self.type_registry.needs_refcount(target_type);
        let symbol = self.symbol_table.insert_with_refcount(
            name.to_string(),
            target_type,
            is_mut,
            needs_refcount,
        )?;
        
        // Copy the value to the variable's QBE name with proper type
        let qbe_type = self.get_qbe_type(target_type)?;
        self.qbe.copy_typed(&symbol.qbe_name, qbe_type, &init_result.temp);
        
        // If this type needs reference counting, retain the value
        if needs_refcount {
            // Check for null before retaining
            let null_check = self.symbol_table.gen_temp();
            self.qbe.instruction(format!("{} =w ceql {}, 0", null_check, init_result.temp));
            
            let retain_label = self.qbe.gen_label("retain");
            let skip_retain = self.qbe.gen_label("skip_retain");
            self.qbe.jnz(&null_check, &skip_retain, &retain_label);
            
            self.qbe.label(&retain_label);
            self.qbe.call_void("thoxa_retain", vec![(init_result.temp.clone(), "l")]);
            self.qbe.jmp(&skip_retain);
            
            self.qbe.label(&skip_retain);
        }
        
        Ok(())
    }
    
    /// Generate code for a return statement
    /// 
    /// NOTE: Currently, return statements must be the last statement in a block.
    /// Early returns (with code after them) are not yet supported due to lack of
    /// proper control flow handling. This will be addressed in a future update.
    fn gen_return(&mut self, value: Option<&crate::ast::Expr>) -> Result<()> {
        // Extract return type from current function
        let expected_return_type = self.current_function.as_ref()
            .ok_or_else(|| Error::Codegen("Return statement outside of function".to_string()))?
            .return_type;
        
        // Generate return value first (if any) before cleanup
        let return_temp = if let Some(expr) = value {
            // Generate code for the return expression
            let result = self.gen_expr(expr)?;
            
            // Type check against function return type
            if let Some(expected_type) = expected_return_type {
                if result.ty != expected_type {
                    return Err(Error::Codegen(format!(
                        "Type mismatch in return: expected {:?}, got {:?}",
                        expected_type, result.ty
                    )));
                }
            }
            
            Some(result.temp)
        } else {
            // No return value - check that function returns void
            if let Some(return_type) = expected_return_type {
                if return_type != crate::codegen::types::TypeId::VOID {
                    return Err(Error::Codegen(format!(
                        "Function expects return type {:?}, but return has no value",
                        return_type
                    )));
                }
            }
            
            None
        };
        
        // For now, skip cleanup on return to avoid infinite loops
        // TODO: Fix cleanup generation for returns
        let all_cleanup_vars: Vec<crate::codegen::symbol::Symbol> = Vec::new(); // self.symbol_table.get_all_cleanup_vars();
        
        // Filter to only user-defined variables (not temporaries starting with %t.)
        let user_vars: Vec<_> = all_cleanup_vars.iter()
            .filter(|var| !var.qbe_name.starts_with("%t."))
            .filter(|var| {
                // Skip the return value itself if it's a variable
                if let Some(ref ret_temp) = return_temp {
                    var.qbe_name != *ret_temp
                } else {
                    true
                }
            })
            .collect();
        
        // Only generate cleanup if there are variables to clean up
        if !user_vars.is_empty() {
            for var in user_vars {
                // For now, just call release without null check to avoid label explosion
                // The runtime handles null gracefully
                self.qbe.call_void("thoxa_release", vec![(var.qbe_name.clone(), "l")]);
            }
        }
        
        // Now emit the actual return instruction
        if let Some(temp) = return_temp {
            self.qbe.ret(Some(&temp));
        } else {
            self.qbe.ret(None);
        }
        
        Ok(())
    }
    
    /// Generate code for a break statement
    fn gen_break(&mut self, value: Option<&crate::ast::Expr>) -> Result<()> {
        // Handle break with value
        if let Some(expr) = value {
            // Generate the value expression
            let result = self.gen_expr(expr)?;
            
            // Extract needed values from loop context before any mutations
            let (break_label, break_slot, current_type) = {
                let ctx = self.current_loop.as_ref()
                    .ok_or_else(|| Error::Codegen("Break statement outside of loop".to_string()))?;
                (ctx.break_label.clone(), ctx.break_value_slot.clone(), ctx.break_value_type)
            };
            
            // Store the value in the break slot
            if let Some(ref slot) = break_slot {
                // Box value types before storing
                let boxed_value = match result.ty {
                    crate::codegen::types::TypeId::INT | crate::codegen::types::TypeId::INT64 => {
                        let boxed_temp = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&boxed_temp),
                            crate::codegen::builder::QbeType::L,
                            "thoxa_box_int64_unified",
                            &[(&result.temp, crate::codegen::builder::QbeType::L)]
                        );
                        boxed_temp
                    }
                    crate::codegen::types::TypeId::INT32 => {
                        let boxed_temp = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&boxed_temp),
                            crate::codegen::builder::QbeType::L,
                            "thoxa_box_int32_unified",
                            &[(&result.temp, crate::codegen::builder::QbeType::W)]
                        );
                        boxed_temp
                    }
                    crate::codegen::types::TypeId::BOOL => {
                        let boxed_temp = self.symbol_table.gen_temp();
                        self.qbe.call(
                            Some(&boxed_temp),
                            crate::codegen::builder::QbeType::L,
                            "thoxa_box_bool_unified",
                            &[(&result.temp, crate::codegen::builder::QbeType::W)]
                        );
                        boxed_temp
                    }
                    _ => {
                        // Already a pointer or other type
                        result.temp.clone()
                    }
                };
                
                self.qbe.store(crate::codegen::builder::QbeType::L, &boxed_value, slot);
                
                // Update the break value type in the loop context
                if current_type.is_none() {
                    if let Some(ref mut ctx) = self.current_loop {
                        ctx.break_value_type = Some(result.ty);
                    }
                } else if current_type != Some(result.ty) {
                    return Err(Error::Codegen(format!(
                        "Break value type mismatch: expected {:?}, got {:?}",
                        current_type, result.ty
                    )));
                }
            }
            
            // Jump to the break label (loop end)
            self.qbe.jmp(&break_label);
        } else {
            // No value - simple break
            let loop_context = self.current_loop.as_ref()
                .ok_or_else(|| Error::Codegen("Break statement outside of loop".to_string()))?;
            self.qbe.jmp(&loop_context.break_label);
        }
        
        Ok(())
    }
    
    /// Generate code for a continue statement
    fn gen_continue(&mut self) -> Result<()> {
        // Check if we're in a loop
        let loop_context = self.current_loop.as_ref()
            .ok_or_else(|| Error::Codegen("Continue statement outside of loop".to_string()))?;
        
        // Jump to the continue label (loop start)
        self.qbe.jmp(&loop_context.continue_label);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use crate::lexer::{Span, IntSuffix};
    
    fn make_span() -> Span {
        Span { start: 0, end: 0, line: 1, column: 1 }
    }
    
    #[test]
    fn test_let_binding() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a let binding: let x = 42
        let stmt = ast::Stmt::Let {
            name: "x".to_string(),
            ty: None, // Type inference
            value: Some(ast::Expr::IntLiteral {
                value: 42,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            is_mut: false,
            is_export: false,
            span: make_span(),
        };
        
        // Generate code for the statement
        codegen.gen_stmt(&stmt).unwrap();
        
        // Verify the variable was added to the symbol table
        let symbol = codegen.symbol_table.lookup("x").unwrap();
        assert_eq!(symbol.type_id, crate::codegen::types::TypeId::INT64);
        assert!(!symbol.is_mut);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy")); // Should have copy instructions
    }
    
    #[test]
    fn test_mut_binding() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a mutable binding: mut y = true
        let stmt = ast::Stmt::Let {
            name: "y".to_string(),
            ty: None,
            value: Some(ast::Expr::BoolLiteral {
                value: true,
                span: make_span(),
            }),
            is_mut: true,
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_stmt(&stmt).unwrap();
        
        // Verify the variable is marked as mutable
        let symbol = codegen.symbol_table.lookup("y").unwrap();
        assert_eq!(symbol.type_id, crate::codegen::types::TypeId::BOOL);
        assert!(symbol.is_mut);
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_expression_statement() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create an expression statement: 1 + 2;
        let stmt = ast::Stmt::Expr(ast::Expr::Binary {
            op: ast::BinaryOp::Add,
            left: Box::new(ast::Expr::IntLiteral {
                value: 1,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            right: Box::new(ast::Expr::IntLiteral {
                value: 2,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        });
        
        // Should not error even though we discard the result
        codegen.gen_stmt(&stmt).unwrap();
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("add")); // The expression should still be evaluated
    }
    
    #[test]
    fn test_variable_usage() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // let x = 10
        let let_stmt = ast::Stmt::Let {
            name: "x".to_string(),
            ty: None,
            value: Some(ast::Expr::IntLiteral {
                value: 10,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            is_mut: false,
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_stmt(&let_stmt).unwrap();
        
        // Show QBE after variable declaration (only with SHOW_QBE=1)
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "After: let x = 10");
        
        // Now use x in an expression: x + 5
        let expr = ast::Expr::Binary {
            op: ast::BinaryOp::Add,
            left: Box::new(ast::Expr::Ident {
                name: "x".to_string(),
                span: make_span(),
            }),
            right: Box::new(ast::Expr::IntLiteral {
                value: 5,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, crate::codegen::types::TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        // Show final QBE (only with SHOW_QBE=1)
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Final QBE for: let x = 10; x + 5");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy")); // Variable assignment
        assert!(output.contains("add"));  // x + 5
    }
    
    #[test]
    fn test_mutable_assignment() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // mut x = 10
        let mut_stmt = ast::Stmt::Let {
            name: "x".to_string(),
            ty: None,
            value: Some(ast::Expr::IntLiteral {
                value: 10,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            is_mut: true,
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_stmt(&mut_stmt).unwrap();
        
        // Show QBE after mutable declaration
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "After: mut x = 10");
        
        // x = 20
        let assign_expr = ast::Expr::Assign {
            target: Box::new(ast::Expr::Ident {
                name: "x".to_string(),
                span: make_span(),
            }),
            value: Box::new(ast::Expr::IntLiteral {
                value: 20,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        // Use it as an expression statement
        let assign_stmt = ast::Stmt::Expr(assign_expr);
        codegen.gen_stmt(&assign_stmt).unwrap();
        
        // Show QBE after assignment
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "After: x = 20");
        
        // let y = x + 5 (should use the updated value)
        let let_y = ast::Stmt::Let {
            name: "y".to_string(),
            ty: None,
            value: Some(ast::Expr::Binary {
                op: ast::BinaryOp::Add,
                left: Box::new(ast::Expr::Ident {
                    name: "x".to_string(),
                    span: make_span(),
                }),
                right: Box::new(ast::Expr::IntLiteral {
                    value: 5,
                    suffix: IntSuffix::None,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            is_mut: false,
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_stmt(&let_y).unwrap();
        
        codegen.qbe.function_end().unwrap();
        
        // Show final QBE
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Final QBE for: mut x = 10; x = 20; let y = x + 5");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("copy")); // Multiple copy instructions
        assert!(output.contains("add"));  // x + 5
    }
    
    #[test]
    fn test_return_statement() {
        let mut codegen = CodeGen::new();
        
        // Set up a function context
        codegen.current_function = Some(crate::codegen::FunctionContext {
            name: "test".to_string(),
            return_type: Some(crate::codegen::types::TypeId::INT64),
        });
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a return statement: return 42
        let stmt = ast::Stmt::Return {
            value: Some(ast::Expr::IntLiteral {
                value: 42,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.gen_stmt(&stmt).unwrap();
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("ret %t.0")); // Should return the temporary
    }
    
    #[test]
    fn test_void_return() {
        let mut codegen = CodeGen::new();
        
        // Set up a void function context
        codegen.current_function = Some(crate::codegen::FunctionContext {
            name: "do_nothing".to_string(),
            return_type: Some(crate::codegen::types::TypeId::VOID),
        });
        
        codegen.qbe.function_start("do_nothing", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Create a return statement with no value
        let stmt = ast::Stmt::Return {
            value: None,
            span: make_span(),
        };
        
        codegen.gen_stmt(&stmt).unwrap();
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("ret\n")); // Should have bare ret
    }
    
    #[test]
    fn test_return_type_mismatch() {
        let mut codegen = CodeGen::new();
        
        // Set up a function expecting Int
        codegen.current_function = Some(crate::codegen::FunctionContext {
            name: "test".to_string(),
            return_type: Some(crate::codegen::types::TypeId::INT64),
        });
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to return a bool
        let stmt = ast::Stmt::Return {
            value: Some(ast::Expr::BoolLiteral {
                value: true,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let result = codegen.gen_stmt(&stmt);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Type mismatch in return"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_break_outside_loop() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to use break outside of a loop
        let stmt = ast::Stmt::Break {
            value: None,
            span: make_span(),
        };
        
        let result = codegen.gen_stmt(&stmt);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Break statement outside of loop"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_continue_outside_loop() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // Try to use continue outside of a loop
        let stmt = ast::Stmt::Continue {
            span: make_span(),
        };
        
        let result = codegen.gen_stmt(&stmt);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Continue statement outside of loop"));
        
        codegen.qbe.function_end().unwrap();
    }
}