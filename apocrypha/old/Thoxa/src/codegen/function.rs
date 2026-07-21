//! Function code generation

use crate::ast::{Function, Block, Item, Stmt};
use crate::codegen::{CodeGen, types::TypeId, builder};
use crate::error::{Error, Result};

impl CodeGen {
    /// Generate code for a function definition
    pub fn gen_function(&mut self, func: &Function) -> Result<()> {
        // Resolve return type
        let return_type = if let Some(ref ty) = func.return_type {
            Some(self.resolve_type(ty)?)
        } else {
            None
        };
        
        // Process parameters
        let mut param_types = Vec::new();
        let mut qbe_params = Vec::new();
        
        for param in &func.params {
            let param_type = self.resolve_type(&param.ty)?;
            param_types.push(param_type);
            
            let qbe_type = self.get_qbe_param_type(param_type)?;
            qbe_params.push((param.name.as_str(), qbe_type));
        }
        
        // Register the function
        self.functions.insert(func.name.clone(), super::FunctionInfo {
            name: func.name.clone(),
            param_types: param_types.clone(),
            return_type,
            external_name: None,
        });
        
        // Set up function context
        self.current_function = Some(super::FunctionContext {
            name: func.name.clone(),
            return_type,
        });
        
        // Enter a new scope for the function
        self.symbol_table.enter_scope();
        
        // Add parameters to the symbol table
        for (param, param_type) in func.params.iter().zip(param_types.iter()) {
            let needs_refcount = self.type_registry.needs_refcount(*param_type);
            self.symbol_table.insert_param(param.name.clone(), *param_type, param.is_mut, needs_refcount)?;
        }
        
        // Process nested functions first (they need to be available before the body is generated)
        // But only scan them - don't generate code yet  
        for stmt in &func.body.stmts {
            if let crate::ast::Stmt::Method { name, params, return_type, .. } = stmt {
                // Register the nested function
                let nested_name = format!("{}__{}", func.name, name);
                
                // Resolve parameter types and return type
                let mut param_types = Vec::new();
                for param in params {
                    let param_type = self.resolve_type(&param.ty)?;
                    param_types.push(param_type);
                }
                
                let ret_type = if let Some(ty) = return_type {
                    Some(self.resolve_type(ty)?)
                } else {
                    None
                };
                
                // Register in function table
                self.functions.insert(nested_name.clone(), super::FunctionInfo {
                    name: nested_name,
                    param_types,
                    return_type: ret_type,
                    external_name: None,
                });
            }
        }
        
        // Generate QBE function header
        // Special case for main: QBE expects 'main' not '$main'
        let is_main = func.name == "main";
        let ret_qbe_type = if let Some(rt) = return_type {
            self.get_qbe_type(rt)?
        } else {
            // Void functions should still return something in QBE
            // Use L for main (which returns an exit code), W for others
            if is_main {
                builder::QbeType::L
            } else {
                builder::QbeType::W
            }
        };
        
        // Convert params to references for the builder
        let param_refs: Vec<(&str, builder::QbeType)> = qbe_params.iter()
            .map(|(name, ty)| (*name, *ty))
            .collect();
            
        self.qbe.function_start(&func.name, &param_refs, ret_qbe_type, func.is_export || is_main)?;
        self.qbe.label("start");
        
        // If this is main, call the type initialization
        if is_main {
            self.qbe.call_void("thoxa_init_runtime", vec![]);
        }
        
        // Generate code for the function body
        let result = self.gen_block(&func.body)?;
        
        // Handle the return value
        // Check if the last statement was a return (which would have already emitted ret)
        let last_was_return = func.body.stmts.last()
            .map(|stmt| matches!(stmt, Stmt::Return { .. }))
            .unwrap_or(false);
            
        if !last_was_return {
            // Only emit return if we didn't already have an explicit return statement
            if let Some(return_type) = &self.current_function.as_ref().unwrap().return_type {
                // Function has a return type, use the block's result
                // Special case: Allow base Trial type to match any specific Trial instance
                let types_match = result.ty == *return_type || 
                    (result.ty == TypeId::TRIAL && self.is_trial_instance(*return_type));
                
                if !types_match {
                    return Err(Error::Codegen(format!(
                        "Type mismatch: function '{}' expects return type {:?}, got {:?}",
                        func.name, return_type, result.ty
                    )));
                }
                self.qbe.ret(Some(&result.temp));
            } else {
                // No return type specified, assume void
                self.qbe.ret(None);
            }
        }
        
        self.qbe.function_end()?;
        
        // Exit the function scope and generate cleanup code
        let _cleanup_vars = self.symbol_table.exit_scope();
        // Note: Function-level cleanup happens before return, handled in return statement generation
        
        // Now generate code for nested functions (after the parent function)
        // Keep current_function set while generating nested functions
        for stmt in &func.body.stmts {
            if let crate::ast::Stmt::Method { name, params, return_type, body, is_export, .. } = stmt {
                let nested_name = format!("{}__{}", func.name, name);
                
                // Create a Function AST node for the nested function
                let nested_func = crate::ast::Function {
                    name: nested_name,
                    type_params: vec![],
                    params: params.clone(),
                    return_type: return_type.clone(),
                    body: body.clone(),
                    is_export: *is_export,
                    span: crate::lexer::Span { start: 0, end: 0, line: 1, column: 1 },
                };
                
                // Save and restore context to avoid nested functions overwriting it
                let saved_context = self.current_function.clone();
                self.gen_function(&nested_func)?;
                self.current_function = saved_context;
            }
        }
        
        // Now clear the function context
        self.current_function = None;
        
        Ok(())
    }
    
    /// Generate code for a block expression
    pub fn gen_block(&mut self, block: &Block) -> Result<super::expr::ExprResult> {
        // Enter a new scope for the block
        self.symbol_table.enter_scope();
        
        // Generate code for each statement
        for stmt in &block.stmts {
            self.gen_stmt(stmt)?;
            
            // If the block has been terminated (by break, continue, or return),
            // we shouldn't generate any more code
            if self.qbe.is_terminated() {
                // Exit the block scope and generate cleanup
                let _cleanup_vars = self.symbol_table.exit_scope();
                // Note: cleanup can't be generated after terminator, it should have been done before
                
                // Return a dummy result that won't be used
                let dummy_temp = self.symbol_table.gen_temp();
                return Ok(super::expr::ExprResult {
                    temp: dummy_temp,
                    ty: TypeId::VOID,
                });
            }
        }
        
        // Generate code for the final expression (if any)
        let result = if let Some(ref expr) = block.expr {
            self.gen_expr(expr)?
        } else {
            // No final expression, return void
            let void_temp = self.symbol_table.gen_temp();
            self.qbe.copy(&void_temp, "0");
            super::expr::ExprResult {
                temp: void_temp,
                ty: TypeId::VOID,
            }
        };
        
        // Exit the block scope and generate cleanup code
        let _cleanup_vars = self.symbol_table.exit_scope();
        
        // Filter to only user-defined variables (not temporaries)
        let user_vars: Vec<_> = _cleanup_vars.iter()
            .filter(|var| !var.qbe_name.starts_with("%t."))
            .collect();
        
        // Only generate cleanup if there are variables to clean up
        if !user_vars.is_empty() {
            for var in user_vars {
                // The runtime handles null gracefully
                self.qbe.call_void("thoxa_release", vec![(var.qbe_name.clone(), "l")]);
            }
        }
        
        Ok(result)
    }
    
    
    /// Generate code for module items
    pub fn gen_item(&mut self, item: &Item) -> Result<()> {
        match item {
            Item::Function(func) => {
                // Store AST function for potential generic instantiation
                self.ast_functions.insert(func.name.clone(), func.clone());
                
                // Skip generic functions for now (they need instantiation)
                if !func.type_params.is_empty() {
                    return Ok(());
                }
                self.gen_function(func)
            }
            Item::Unit(unit_def) => {
                // Skip generic types for now (they need instantiation)
                if !unit_def.type_params.is_empty() {
                    return Ok(());
                }
                // Register the unit type in the type registry
                self.register_unit_type(unit_def)?;
                Ok(())  // Units don't generate code themselves
            }
            Item::Union(union_def) => {
                // Skip generic unions for now (they need instantiation)
                if !union_def.type_params.is_empty() {
                    return Ok(());
                }
                // Unions were already registered in the first pass
                // They don't generate code themselves, just type information
                Ok(())
            }
            _ => Err(Error::Codegen("Unsupported item type".to_string())),
        }
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
    fn test_simple_main_function() {
        let mut codegen = CodeGen::new();
        
        // Create a simple main function: fn main() -> Int { 42 }
        let func = ast::Function {
            name: "main".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Some(ast::Type::Named {
                name: "Int".to_string(),
                args: vec![],
                span: make_span(),
            }),
            body: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::IntLiteral {
                    value: 42,
                    suffix: IntSuffix::None,
                    span: make_span(),
                })),
                span: make_span(),
            },
            is_export: true,
            span: make_span(),
        };
        
        codegen.gen_function(&func).unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("export function l $main()"));
        assert!(output.contains("ret %t.0"));
        assert!(output.contains("copy 42"));
    }
    
    #[test]
    fn test_function_with_statements() {
        let mut codegen = CodeGen::new();
        
        // fn compute() -> Int { let x = 10; let y = 20; x + y }
        let func = ast::Function {
            name: "compute".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Some(ast::Type::Named {
                name: "Int".to_string(),
                args: vec![],
                span: make_span(),
            }),
            body: ast::Block {
                stmts: vec![
                    ast::Stmt::Let {
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
                    },
                    ast::Stmt::Let {
                        name: "y".to_string(),
                        ty: None,
                        value: Some(ast::Expr::IntLiteral {
                            value: 20,
                            suffix: IntSuffix::None,
                            span: make_span(),
                        }),
                        is_mut: false,
                        is_export: false,
                        span: make_span(),
                    },
                ],
                expr: Some(Box::new(ast::Expr::Binary {
                    op: ast::BinaryOp::Add,
                    left: Box::new(ast::Expr::Ident {
                        name: "x".to_string(),
                        span: make_span(),
                    }),
                    right: Box::new(ast::Expr::Ident {
                        name: "y".to_string(),
                        span: make_span(),
                    }),
                    span: make_span(),
                })),
                span: make_span(),
            },
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_function(&func).unwrap();
        
        // Show QBE output
        #[cfg(test)]
        crate::codegen::test_utils::show_qbe(&codegen, "Function: compute() -> Int { let x = 10; let y = 20; x + y }");
        
        let output = codegen.qbe.finish();
        assert!(output.contains("function l $compute()"));
        assert!(output.contains("add"));
        assert!(output.contains("ret"));
    }
    
    #[test]
    fn test_function_with_parameters() {
        let mut codegen = CodeGen::new();
        
        // Create a function: fn add(x: Int, y: Int) -> Int { x + y }
        let func = ast::Function {
            name: "add".to_string(),
            type_params: vec![],
            params: vec![
                ast::Param {
                    name: "x".to_string(),
                    ty: ast::Type::Named {
                        name: "Int".to_string(),
                        args: vec![],
                        span: make_span(),
                    },
                    is_mut: false,
                    span: make_span(),
                },
                ast::Param {
                    name: "y".to_string(),
                    ty: ast::Type::Named {
                        name: "Int".to_string(),
                        args: vec![],
                        span: make_span(),
                    },
                    is_mut: false,
                    span: make_span(),
                },
            ],
            return_type: Some(ast::Type::Named {
                name: "Int".to_string(),
                args: vec![],
                span: make_span(),
            }),
            body: ast::Block {
                stmts: vec![],
                expr: Some(Box::new(ast::Expr::Binary {
                    op: ast::BinaryOp::Add,
                    left: Box::new(ast::Expr::Ident {
                        name: "x".to_string(),
                        span: make_span(),
                    }),
                    right: Box::new(ast::Expr::Ident {
                        name: "y".to_string(),
                        span: make_span(),
                    }),
                    span: make_span(),
                })),
                span: make_span(),
            },
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_function(&func).unwrap();
        
        let output = codegen.qbe.finish();
        println!("QBE output:\n{}", output);
        
        // Check that the function has the correct signature
        assert!(output.contains("function l $add(l %x, l %y)"));
        // Check that parameters are used directly
        assert!(output.contains("add %x, %y"));
    }
    
    #[test]
    fn test_block_scoping() {
        let mut codegen = CodeGen::new();
        
        // Test that variables in blocks are scoped correctly
        // fn test() -> Int { let x = 1; { let x = 2; }; x }
        let func = ast::Function {
            name: "test".to_string(),
            type_params: vec![],
            params: vec![],
            return_type: Some(ast::Type::Named {
                name: "Int".to_string(),
                args: vec![],
                span: make_span(),
            }),
            body: ast::Block {
                stmts: vec![
                    // let x = 1
                    ast::Stmt::Let {
                        name: "x".to_string(),
                        ty: None,
                        value: Some(ast::Expr::IntLiteral {
                            value: 1,
                            suffix: IntSuffix::None,
                            span: make_span(),
                        }),
                        is_mut: false,
                        is_export: false,
                        span: make_span(),
                    },
                    // { let x = 2; }
                    ast::Stmt::Expr(ast::Expr::Block(ast::Block {
                        stmts: vec![
                            ast::Stmt::Let {
                                name: "x".to_string(),
                                ty: None,
                                value: Some(ast::Expr::IntLiteral {
                                    value: 2,
                                    suffix: IntSuffix::None,
                                    span: make_span(),
                                }),
                                is_mut: false,
                                is_export: false,
                                span: make_span(),
                            },
                        ],
                        expr: None,
                        span: make_span(),
                    })),
                ],
                // Return outer x
                expr: Some(Box::new(ast::Expr::Ident {
                    name: "x".to_string(),
                    span: make_span(),
                })),
                span: make_span(),
            },
            is_export: false,
            span: make_span(),
        };
        
        codegen.gen_function(&func).unwrap();
        
        // Show QBE output
        #[cfg(test)]
        crate::codegen::test_utils::debug_qbe(&codegen, "Block scoping test");
        
        let output = codegen.qbe.finish();
        println!("Output: {}", output);
        // Should have two different x variables
        assert!(output.contains("%x."));
        // The exact numbers depend on what other temporaries were allocated
    }
}