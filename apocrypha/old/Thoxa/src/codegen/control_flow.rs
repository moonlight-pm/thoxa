//! Control flow code generation (if, loops, etc.)

use crate::ast::{Expr, Block};
use crate::codegen::{CodeGen, types::TypeId, builder};
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for a loop expression
    pub fn gen_loop(&mut self, body: &Block) -> Result<super::expr::ExprResult> {
        // Generate labels
        let loop_label = self.qbe.gen_label("loop");
        let end_label = self.qbe.gen_label("loop_end");
        
        // Allocate a slot for break value (even if not used)
        let break_slot = self.symbol_table.gen_temp();
        self.qbe.alloc(&break_slot, 8, 8);
        
        // Initialize with default value
        self.qbe.store(builder::QbeType::L, "0", &break_slot);
        
        // Save current loop context and set up new one
        let prev_loop = self.current_loop.take();
        self.current_loop = Some(super::LoopContext {
            continue_label: loop_label.clone(),
            break_label: end_label.clone(),
            break_value_slot: Some(break_slot.clone()),
            break_value_type: None, // Will be set by first break with value
        });
        
        // Jump to loop start
        self.qbe.label(&loop_label);
        
        // Generate loop body in new scope
        self.symbol_table.push_scope();
        let _body_result = self.gen_block(body)?;
        
        // Generate cleanup before looping back
        let cleanup_vars = self.symbol_table.pop_scope()?;
        for var in cleanup_vars {
            self.qbe.call_void("thoxa_decref", vec![(var.qbe_name, "l")]);
        }
        
        // Jump back to loop start
        self.qbe.jmp(&loop_label);
        
        // End label (for break statements)
        self.qbe.label(&end_label);
        
        // Get the break value type from the loop context
        let result_type = self.current_loop.as_ref()
            .and_then(|ctx| ctx.break_value_type)
            .unwrap_or(TypeId::VOID);
        
        // Restore previous loop context
        self.current_loop = prev_loop;
        
        // Load and return the break value
        let result_temp = self.symbol_table.gen_temp();
        if result_type != TypeId::VOID {
            // Load the boxed value
            let boxed_temp = self.symbol_table.gen_temp();
            self.qbe.load(&boxed_temp, builder::QbeType::L, &break_slot);
            
            // Unbox based on type
            match result_type {
                TypeId::INT | TypeId::INT64 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::L,
                        "thoxa_unbox_int64_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::INT32 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_int32_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::BOOL => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_bool_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                _ => {
                    // Already a pointer, no unboxing needed
                    self.qbe.copy(&result_temp, &boxed_temp);
                }
            }
        } else {
            // If no break with value was used, return void
            self.qbe.copy(&result_temp, "0");
        }
        
        Ok(super::expr::ExprResult {
            temp: result_temp,
            ty: result_type,
        })
    }
    
    /// Generate code for a while loop
    pub fn gen_while(&mut self, condition: &Expr, body: &Block) -> Result<super::expr::ExprResult> {
        // Generate labels
        let cond_label = self.qbe.gen_label("while_cond");
        let body_label = self.qbe.gen_label("while_body");
        let end_label = self.qbe.gen_label("while_end");
        
        // Allocate a slot for break value (even if not used)
        let break_slot = self.symbol_table.gen_temp();
        self.qbe.alloc(&break_slot, 8, 8);
        
        // Initialize with default value
        self.qbe.store(builder::QbeType::L, "0", &break_slot);
        
        // Save current loop context and set up new one
        let prev_loop = self.current_loop.take();
        self.current_loop = Some(super::LoopContext {
            continue_label: cond_label.clone(),  // Continue goes to condition check
            break_label: end_label.clone(),
            break_value_slot: Some(break_slot.clone()),
            break_value_type: None, // Will be set by first break with value
        });
        
        // Jump to condition check
        self.qbe.jmp(&cond_label);
        
        // Condition label
        self.qbe.label(&cond_label);
        
        // Evaluate condition
        let cond_result = self.gen_expr(condition)?;
        
        // Condition must be boolean
        if cond_result.ty != TypeId::BOOL {
            return Err(Error::Codegen(format!(
                "While condition must be boolean, got {:?}",
                cond_result.ty
            )));
        }
        
        // Branch based on condition
        self.qbe.jnz(&cond_result.temp, &body_label, &end_label);
        
        // Body label
        self.qbe.label(&body_label);
        
        // Generate loop body in new scope
        self.symbol_table.push_scope();
        let _body_result = self.gen_block(body)?;
        
        // Generate cleanup before looping back
        let cleanup_vars = self.symbol_table.pop_scope()?;
        for var in cleanup_vars {
            self.qbe.call_void("thoxa_decref", vec![(var.qbe_name, "l")]);
        }
        
        // Jump back to condition check
        self.qbe.jmp(&cond_label);
        
        // End label (for break statements or when condition is false)
        self.qbe.label(&end_label);
        
        // Get the break value type from the loop context
        let result_type = self.current_loop.as_ref()
            .and_then(|ctx| ctx.break_value_type)
            .unwrap_or(TypeId::VOID);
        
        // Restore previous loop context
        self.current_loop = prev_loop;
        
        // Load and return the break value
        let result_temp = self.symbol_table.gen_temp();
        if result_type != TypeId::VOID {
            // Load the boxed value
            let boxed_temp = self.symbol_table.gen_temp();
            self.qbe.load(&boxed_temp, builder::QbeType::L, &break_slot);
            
            // Unbox based on type
            match result_type {
                TypeId::INT | TypeId::INT64 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::L,
                        "thoxa_unbox_int64_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::INT32 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_int32_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::BOOL => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_bool_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                _ => {
                    // Already a pointer, no unboxing needed
                    self.qbe.copy(&result_temp, &boxed_temp);
                }
            }
        } else {
            // If no break with value was used, return void
            self.qbe.copy(&result_temp, "0");
        }
        
        Ok(super::expr::ExprResult {
            temp: result_temp,
            ty: result_type,
        })
    }
    
    /// Generate code for a for-in loop
    pub fn gen_for_in(&mut self, variable: &str, iterable: &Expr, body: &Block) -> Result<super::expr::ExprResult> {
        // Generate code for the iterable (should be an array)
        let iterable_result = self.gen_expr(iterable)?;
        
        // Get the element type from the array type
        let element_type = self.type_registry.get_array_element_type(iterable_result.ty)
            .unwrap_or(TypeId::POINTER);  // Default to pointer if not an array type
        
        // Start a new scope for the loop
        self.symbol_table.push_scope();
        
        // Store array in a temporary to ensure it stays alive
        let array_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&array_temp, &iterable_result.temp);
        
        // Initialize index counter
        let index_temp = self.symbol_table.gen_temp();
        self.qbe.copy(&index_temp, "0");
        
        // Get array length once
        let length_temp = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&length_temp),
            builder::QbeType::L,
            "thoxa_array_length",
            &[(&array_temp, builder::QbeType::L)]
        );
        
        // Generate labels
        let cond_label = self.qbe.gen_label("for_cond");
        let body_label = self.qbe.gen_label("for_body");
        let inc_label = self.qbe.gen_label("for_inc");
        let end_label = self.qbe.gen_label("for_end");
        
        // Allocate a slot for break value (even if not used)
        let break_slot = self.symbol_table.gen_temp();
        self.qbe.alloc(&break_slot, 8, 8);
        
        // Initialize with default value
        self.qbe.store(builder::QbeType::L, "0", &break_slot);
        
        // Save current loop context and set up new one
        let prev_loop = self.current_loop.take();
        self.current_loop = Some(super::LoopContext {
            continue_label: inc_label.clone(),  // Continue goes to increment
            break_label: end_label.clone(),
            break_value_slot: Some(break_slot.clone()),
            break_value_type: None, // Will be set by first break with value
        });
        
        // Jump to condition check
        self.qbe.jmp(&cond_label);
        
        // Condition: index < length
        self.qbe.label(&cond_label);
        let cond_temp = self.symbol_table.gen_temp();
        self.qbe.instruction(format!("{} =w csltl {}, {}", cond_temp, index_temp, length_temp));
        self.qbe.jnz(&cond_temp, &body_label, &end_label);
        
        // Body: Extract element and execute body
        self.qbe.label(&body_label);
        
        // Get element at current index
        let elem_ptr = self.symbol_table.gen_temp();
        self.qbe.call(
            Some(&elem_ptr),
            builder::QbeType::L,
            "thoxa_array_get",
            &[
                (&array_temp, builder::QbeType::L),
                (&index_temp, builder::QbeType::L),
            ]
        );
        
        // Load the value from the pointer
        let elem_temp = self.symbol_table.gen_temp();
        self.qbe.load(&elem_temp, builder::QbeType::L, &elem_ptr);
        
        // Bind the loop variable with the correct element type
        let symbol = self.symbol_table.insert(
            variable.to_string(),
            element_type,  // Use the actual element type from the array
            false  // Loop variables are immutable
        )?;
        self.qbe.copy(&symbol.qbe_name, &elem_temp);
        
        // Execute loop body
        let _body_result = self.gen_block(body)?;
        
        // Increment: index = index + 1
        self.qbe.label(&inc_label);
        let inc_temp = self.symbol_table.gen_temp();
        self.qbe.instruction(format!("{} =l add {}, 1", inc_temp, index_temp));
        self.qbe.copy(&index_temp, &inc_temp);
        
        // Jump back to condition
        self.qbe.jmp(&cond_label);
        
        // End label
        self.qbe.label(&end_label);
        
        // Clean up scope
        let cleanup_vars = self.symbol_table.pop_scope()?;
        for var in cleanup_vars {
            if var.needs_refcount {
                self.qbe.call_void("thoxa_decref", vec![(var.qbe_name, "l")]);
            }
        }
        
        // Get the break value type from the loop context
        let result_type = self.current_loop.as_ref()
            .and_then(|ctx| ctx.break_value_type)
            .unwrap_or(TypeId::VOID);
        
        // Restore previous loop context
        self.current_loop = prev_loop;
        
        // Load and return the break value
        let result_temp = self.symbol_table.gen_temp();
        if result_type != TypeId::VOID {
            // Load the boxed value
            let boxed_temp = self.symbol_table.gen_temp();
            self.qbe.load(&boxed_temp, builder::QbeType::L, &break_slot);
            
            // Unbox based on type
            match result_type {
                TypeId::INT | TypeId::INT64 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::L,
                        "thoxa_unbox_int64_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::INT32 => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_int32_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                TypeId::BOOL => {
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_unbox_bool_unified",
                        &[(&boxed_temp, builder::QbeType::L)]
                    );
                }
                _ => {
                    // Already a pointer, no unboxing needed
                    self.qbe.copy(&result_temp, &boxed_temp);
                }
            }
        } else {
            // If no break with value was used, return void
            self.qbe.copy(&result_temp, "0");
        }
        
        Ok(super::expr::ExprResult {
            temp: result_temp,
            ty: result_type,
        })
    }
    
    /// Generate code for an if expression
    pub fn gen_if(&mut self, condition: &Expr, then_block: &Block, else_block: Option<&Block>) -> Result<super::expr::ExprResult> {
        // Generate code for the condition
        let cond_result = self.gen_expr(condition)?;
        
        // Condition must be a boolean
        if cond_result.ty != TypeId::BOOL {
            return Err(Error::Codegen(format!(
                "If condition must be boolean, got {:?}",
                cond_result.ty
            )));
        }
        
        // Generate labels for the branches
        let then_label = self.qbe.gen_label("then");
        let else_label = self.qbe.gen_label("else");
        let end_label = self.qbe.gen_label("end");
        
        // Branch on the condition
        self.qbe.jnz(&cond_result.temp, &then_label, &else_label);
        
        // Generate then block
        self.qbe.label(&then_label);
        let then_result = self.gen_block(then_block)?;
        let then_type = then_result.ty;
        let then_temp = then_result.temp;
        let then_terminated = self.qbe.is_terminated();
        
        // Track where the then branch jumps from
        let then_predecessor = if !then_terminated {
            // Create a unique label for the jump point
            let then_end = self.qbe.gen_label("then_end");
            self.qbe.label(&then_end);
            self.qbe.jmp(&end_label);
            then_end
        } else {
            then_label.clone()
        };
        
        // Generate else block (or void if no else)
        self.qbe.label(&else_label);
        let (else_temp, else_type, else_terminated) = if let Some(else_block) = else_block {
            let else_result = self.gen_block(else_block)?;
            let terminated = self.qbe.is_terminated();
            (else_result.temp, else_result.ty, terminated)
        } else {
            // No else block - return void
            let void_temp = self.symbol_table.gen_temp();
            self.qbe.copy(&void_temp, "0");
            (void_temp, TypeId::VOID, false)
        };
        
        // Track where the else branch jumps from
        let else_predecessor = if !else_terminated {
            // Create a unique label for the jump point
            let else_end = self.qbe.gen_label("else_end");
            self.qbe.label(&else_end);
            self.qbe.jmp(&end_label);
            else_end
        } else {
            else_label.clone()
        };
        
        // If both branches terminated, we don't need the end label or phi node
        if then_terminated && else_terminated {
            // Both branches jump away, so the if expression never produces a value
            // Return a dummy result
            let dummy_temp = self.symbol_table.gen_temp();
            return Ok(super::expr::ExprResult {
                temp: dummy_temp,
                ty: TypeId::VOID,
            });
        }
        
        // End label - phi node to select result
        self.qbe.label(&end_label);
        
        // If one branch terminated, we can't use a phi node
        if then_terminated || else_terminated {
            // One branch jumps away - just use the value from the non-terminated branch
            if then_terminated {
                // Then branch terminated, use else value
                return Ok(super::expr::ExprResult {
                    temp: else_temp,
                    ty: else_type,
                });
            } else {
                // Else branch terminated, use then value
                return Ok(super::expr::ExprResult {
                    temp: then_temp,
                    ty: then_type,
                });
            }
        }
        
        // Both branches must have the same type
        if then_type != else_type {
            return Err(Error::Codegen(format!(
                "If branches have different types: then has {:?}, else has {:?}",
                then_type, else_type
            )));
        }
        
        // Use a phi node to select the result
        let result_temp = self.symbol_table.gen_temp();
        let qbe_type = self.get_qbe_type(then_type)?;
        self.qbe.phi(&result_temp, qbe_type, &[
            (&then_predecessor, &then_temp),
            (&else_predecessor, &else_temp),
        ]);
        
        Ok(super::expr::ExprResult {
            temp: result_temp,
            ty: then_type,
        })
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
    fn test_simple_if() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // if true { 42 } else { 0 }
        let if_expr = ast::Expr::If {
            condition: Box::new(ast::Expr::BoolLiteral {
                value: true,
                span: make_span(),
            }),
            then_block: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 42,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            },
            else_block: Some(ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 0,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&if_expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        // Show QBE output
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "If expression: if true { 42 } else { 0 }");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("jnz"));     // Conditional jump
        assert!(output.contains("@then"));   // Then label
        assert!(output.contains("@else"));   // Else label
        assert!(output.contains("@end"));    // End label
        assert!(output.contains("phi"));     // Phi node
    }
    
    #[test]
    fn test_if_no_else_type_error() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // if condition { 42 }  // No else with non-void then = error
        let condition = ast::Expr::BoolLiteral {
            value: false,
            span: make_span(),
        };
        
        let if_expr = ast::Expr::If {
            condition: Box::new(condition),
            then_block: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 42,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            },
            else_block: None,
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&if_expr);
        // Should error because then returns Int but else (missing) returns Void
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("different types"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_if_no_else_void() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // if condition { }  // Both branches void - OK
        let condition = ast::Expr::BoolLiteral {
            value: false,
            span: make_span(),
        };
        
        let if_expr = ast::Expr::If {
            condition: Box::new(condition),
            then_block: ast::Block {
                stmts: vec![],
                expr: None,  // No expression = void
                span: make_span(),
            },
            else_block: None,
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&if_expr).unwrap();
        // Should be void because both branches are void
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_if_type_mismatch() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // if true { 42 } else { true }  // Type mismatch!
        let if_expr = ast::Expr::If {
            condition: Box::new(ast::Expr::BoolLiteral {
                value: true,
                span: make_span(),
            }),
            then_block: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 42,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            },
            else_block: Some(ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::BoolLiteral {
                    value: true,
                    span: make_span(),
                })),
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&if_expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("different types"));
        
        codegen.qbe.function_end().unwrap();
    }
    
    #[test]
    fn test_nested_if() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // let x = true
        codegen.symbol_table.insert("x".to_string(), TypeId::BOOL, false).unwrap();
        let x_temp = codegen.symbol_table.gen_temp();
        codegen.qbe.copy(&x_temp, "1");
        codegen.qbe.copy("%x.1", &x_temp);
        
        // if x { if x { 1 } else { 2 } } else { 3 }
        let inner_if = ast::Expr::If {
            condition: Box::new(ast::Expr::Ident {
                name: "x".to_string(),
                span: make_span(),
            }),
            then_block: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 1,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            },
            else_block: Some(ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 2,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let outer_if = ast::Expr::If {
            condition: Box::new(ast::Expr::Ident {
                name: "x".to_string(),
                span: make_span(),
            }),
            then_block: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(inner_if)),
                span: make_span(),
            },
            else_block: Some(ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 3,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            }),
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&outer_if).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Nested if expression");
    }
    
    #[test]
    fn test_loop() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // loop { }
        let loop_expr = ast::Expr::Loop {
            body: ast::Block {
                stmts: vec![],
                expr: None,
                span: make_span(),
            },
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&loop_expr).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Loop expression: loop { }");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("@loop"));       // Loop start label
        assert!(output.contains("jmp @loop"));   // Jump back to start
        assert!(output.contains("@loop_end"));   // End label for breaks
    }
    
    #[test]
    fn test_loop_with_break() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // mut i = 0
        codegen.symbol_table.insert("i".to_string(), TypeId::INT64, true).unwrap();
        let i_temp = codegen.symbol_table.gen_temp();
        codegen.qbe.copy(&i_temp, "0");
        codegen.qbe.copy("%i.1", &i_temp);
        
        // loop { if i == 10 { break; } i = i + 1; }
        let loop_expr = ast::Expr::Loop {
            body: ast::Block {
                stmts: vec![
                    // if i == 10 { break; }
                    ast::Stmt::Expr(ast::Expr::If {
                        condition: Box::new(ast::Expr::Binary {
                            op: ast::BinaryOp::Eq,
                            left: Box::new(ast::Expr::Ident {
                                name: "i".to_string(),
                                span: make_span(),
                            }),
                            right: Box::new(ast::Expr::IntLiteral {
                                value: 10,
                                suffix: IntSuffix::None,
                                span: make_span(),
                            }),
                            span: make_span(),
                        }),
                        then_block: ast::Block {
                            stmts: vec![
                                ast::Stmt::Break {
                                    value: None,
                                    span: make_span(),
                                },
                            ],
                            expr: None,
                            span: make_span(),
                        },
                        else_block: None,
                        span: make_span(),
                    }),
                    // i = i + 1
                    ast::Stmt::Expr(ast::Expr::Assign {
                        target: Box::new(ast::Expr::Ident {
                            name: "i".to_string(),
                            span: make_span(),
                        }),
                        value: Box::new(ast::Expr::Binary {
                            op: ast::BinaryOp::Add,
                            left: Box::new(ast::Expr::Ident {
                                name: "i".to_string(),
                                span: make_span(),
                            }),
                            right: Box::new(ast::Expr::IntLiteral {
                                value: 1,
                                suffix: IntSuffix::None,
                                span: make_span(),
                            }),
                            span: make_span(),
                        }),
                        span: make_span(),
                    }),
                ],
                expr: None,
                span: make_span(),
            },
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&loop_expr).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Loop with break: loop { if i == 10 { break; } i = i + 1; }");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("@loop"));       // Loop start label
        assert!(output.contains("@loop_end"));   // End label for breaks
        assert!(output.contains("jmp @loop_end")); // Break jump
    }
    
    #[test]
    fn test_loop_with_continue() {
        let mut codegen = CodeGen::new();
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        // mut i = 0
        codegen.symbol_table.insert("i".to_string(), TypeId::INT64, true).unwrap();
        let i_temp = codegen.symbol_table.gen_temp();
        codegen.qbe.copy(&i_temp, "0");
        codegen.qbe.copy("%i.1", &i_temp);
        
        // loop { i = i + 1; if i % 2 == 0 { continue; } break; }
        let loop_expr = ast::Expr::Loop {
            body: ast::Block {
                stmts: vec![
                    // i = i + 1
                    ast::Stmt::Expr(ast::Expr::Assign {
                        target: Box::new(ast::Expr::Ident {
                            name: "i".to_string(),
                            span: make_span(),
                        }),
                        value: Box::new(ast::Expr::Binary {
                            op: ast::BinaryOp::Add,
                            left: Box::new(ast::Expr::Ident {
                                name: "i".to_string(),
                                span: make_span(),
                            }),
                            right: Box::new(ast::Expr::IntLiteral {
                                value: 1,
                                suffix: IntSuffix::None,
                                span: make_span(),
                            }),
                            span: make_span(),
                        }),
                        span: make_span(),
                    }),
                    // if i % 2 == 0 { continue; }
                    ast::Stmt::Expr(ast::Expr::If {
                        condition: Box::new(ast::Expr::Binary {
                            op: ast::BinaryOp::Eq,
                            left: Box::new(ast::Expr::Binary {
                                op: ast::BinaryOp::Rem,
                                left: Box::new(ast::Expr::Ident {
                                    name: "i".to_string(),
                                    span: make_span(),
                                }),
                                right: Box::new(ast::Expr::IntLiteral {
                                    value: 2,
                                    suffix: IntSuffix::None,
                                    span: make_span(),
                                }),
                                span: make_span(),
                            }),
                            right: Box::new(ast::Expr::IntLiteral {
                                value: 0,
                                suffix: IntSuffix::None,
                                span: make_span(),
                            }),
                            span: make_span(),
                        }),
                        then_block: ast::Block {
                            stmts: vec![
                                ast::Stmt::Continue {
                                    span: make_span(),
                                },
                            ],
                            expr: None,
                            span: make_span(),
                        },
                        else_block: None,
                        span: make_span(),
                    }),
                    // break
                    ast::Stmt::Break {
                        value: None,
                        span: make_span(),
                    },
                ],
                expr: None,
                span: make_span(),
            },
            span: make_span(),
        };
        
        let result = codegen.gen_expr(&loop_expr).unwrap();
        assert_eq!(result.ty, TypeId::VOID);
        
        codegen.qbe.function_end().unwrap();
        
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Loop with continue");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("@loop"));       // Loop start label
        assert!(output.contains("jmp @loop"));   // Continue jump
        assert!(output.contains("@loop_end"));   // End label for breaks
    }
}