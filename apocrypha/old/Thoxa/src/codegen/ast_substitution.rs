use crate::ast::{self, Expr, Stmt, Block, Pattern};
use crate::codegen::types::TypeId;
use std::collections::HashMap;
use crate::error::{Error, Result};
use super::types::TypeRegistry;

/// Substitutes type parameters with concrete types throughout an AST
pub struct AstTypeSubstitutor<'a> {
    type_param_map: &'a HashMap<String, TypeId>,
    type_registry: &'a TypeRegistry,
}

impl<'a> AstTypeSubstitutor<'a> {
    pub fn new(type_param_map: &'a HashMap<String, TypeId>, type_registry: &'a TypeRegistry) -> Self {
        Self { type_param_map, type_registry }
    }

    /// Substitute types in a function body (Block)
    pub fn substitute_function_body(&self, body: &mut Block) -> Result<()> {
        self.substitute_block(body)
    }

    /// Substitute types in a block
    fn substitute_block(&self, block: &mut Block) -> Result<()> {
        for stmt in block.stmts.iter_mut() {
            self.substitute_stmt(stmt)?;
        }
        if let Some(ref mut expr) = block.expr {
            self.substitute_expr(expr)?;
        }
        Ok(())
    }

    /// Substitute types in a statement
    fn substitute_stmt(&self, stmt: &mut Stmt) -> Result<()> {
        match stmt {
            Stmt::Let { ty, value, .. } => {
                if let Some(type_annotation) = ty {
                    *type_annotation = self.substitute_type(type_annotation)?;
                }
                if let Some(init) = value {
                    self.substitute_expr(init)?;
                }
            }
            Stmt::Method { params, return_type, body, .. } => {
                for param in params.iter_mut() {
                    param.ty = self.substitute_type(&param.ty)?;
                }
                if let Some(ret_ty) = return_type {
                    *ret_ty = self.substitute_type(ret_ty)?;
                }
                self.substitute_block(body)?;
            }
            Stmt::Expr(expr) => {
                self.substitute_expr(expr)?;
            }
            Stmt::Return { value, .. } => {
                if let Some(ret_expr) = value {
                    self.substitute_expr(ret_expr)?;
                }
            }
            Stmt::Break { value, .. } => {
                if let Some(break_expr) = value {
                    self.substitute_expr(break_expr)?;
                }
            }
            Stmt::Continue { .. } => {
                // No substitution needed
            }
        }
        Ok(())
    }

    /// Substitute types in an expression
    fn substitute_expr(&self, expr: &mut Expr) -> Result<()> {
        match expr {
            // Literals don't need substitution
            Expr::IntLiteral { .. } |
            Expr::FloatLiteral { .. } |
            Expr::StringLiteral { .. } |
            Expr::CharLiteral { .. } |
            Expr::BoolLiteral { .. } |
            Expr::VoidLiteral { .. } => {
                // No substitution needed
            }
            
            Expr::InterpolatedString { segments, .. } => {
                for segment in segments.iter_mut() {
                    if let ast::StringSegment::Interpolation(interp_expr) = segment {
                        self.substitute_expr(interp_expr)?;
                    }
                }
            }
            
            Expr::Ident { .. } => {
                // Identifiers don't need type substitution
            }
            
            Expr::Binary { left, right, .. } => {
                self.substitute_expr(left)?;
                self.substitute_expr(right)?;
            }
            
            Expr::Unary { expr, .. } => {
                self.substitute_expr(expr)?;
            }
            
            Expr::Call { args, type_args, .. } => {
                for arg in args.iter_mut() {
                    self.substitute_expr(arg)?;
                }
                // Substitute type arguments
                for targ in type_args.iter_mut() {
                    *targ = self.substitute_type(targ)?;
                }
            }
            
            Expr::Index { array, index, .. } => {
                self.substitute_expr(array)?;
                self.substitute_expr(index)?;
            }
            
            Expr::FieldAccess { object, .. } => {
                self.substitute_expr(object)?;
            }
            
            Expr::If { condition, then_block, else_block, .. } => {
                self.substitute_expr(condition)?;
                self.substitute_block(then_block)?;
                if let Some(else_blk) = else_block {
                    self.substitute_block(else_blk)?;
                }
            }
            
            Expr::While { condition, body, .. } => {
                self.substitute_expr(condition)?;
                self.substitute_block(body)?;
            }
            
            Expr::ForIn { variable: _, iterable, body, .. } => {
                self.substitute_expr(iterable)?;
                self.substitute_block(body)?;
            }
            
            Expr::Loop { body, .. } => {
                self.substitute_block(body)?;
            }
            
            Expr::Match { expr, arms, .. } => {
                self.substitute_expr(expr)?;
                for arm in arms.iter_mut() {
                    self.substitute_pattern(&mut arm.pattern)?;
                    self.substitute_block(&mut arm.body)?;
                }
            }
            
            Expr::ArrayLiteral { elements, .. } => {
                for elem in elements.iter_mut() {
                    self.substitute_expr(elem)?;
                }
            }
            
            Expr::Assign { target, value, .. } => {
                self.substitute_expr(target)?;
                self.substitute_expr(value)?;
            }
            
            Expr::EnumVariant { value, .. } => {
                // This handles Has/Not/Pass/Fail
                if let Some(val) = value {
                    self.substitute_expr(val)?;
                }
            }
            
            Expr::UnitConstruction { .. } => {
                // No substitution needed for unit construction
            }
            
            Expr::As { expr, ty, .. } => {
                self.substitute_expr(expr)?;
                *ty = self.substitute_type(ty)?;
            }
            
            Expr::Block(block) => {
                self.substitute_block(block)?;
            }
            
            Expr::Try { expr, .. } => {
                self.substitute_expr(expr)?;
            }
            
            Expr::Tuple { elements, .. } => {
                for elem in elements.iter_mut() {
                    self.substitute_expr(elem)?;
                }
            }
            
            Expr::TupleAccess { tuple, .. } => {
                self.substitute_expr(tuple)?;
            }
        }
        Ok(())
    }

    /// Substitute types in a pattern
    fn substitute_pattern(&self, pattern: &mut Pattern) -> Result<()> {
        match pattern {
            Pattern::Constructor { fields, .. } => {
                if let Some(pattern_fields) = fields {
                    for field in pattern_fields.iter_mut() {
                        if let Some(field_pattern) = &mut field.pattern {
                            self.substitute_pattern(field_pattern)?;
                        }
                    }
                }
            }
            Pattern::Type { pattern: inner_pattern, .. } => {
                self.substitute_pattern(inner_pattern)?;
            }
            // Literal and simple patterns don't need substitution
            Pattern::IntLiteral { .. } |
            Pattern::StringLiteral { .. } |
            Pattern::BoolLiteral { .. } |
            Pattern::Ident { .. } | 
            Pattern::Wildcard { .. } => {
                // No types to substitute
            }
        }
        Ok(())
    }

    /// Substitute types in an AST type
    fn substitute_type(&self, ty: &ast::Type) -> Result<ast::Type> {
        match ty {
            ast::Type::Named { name, args, span } => {
                // Check if this is a type parameter that needs substitution
                if let Some(&type_id) = self.type_param_map.get(name) {
                    // Convert TypeId back to AST type using the type registry
                    let type_name = self.type_registry.get_type_name(type_id)
                        .ok_or_else(|| Error::Codegen(format!("Unknown type ID: {:?}", type_id)))?;
                    
                    // Check if it's a generic instance like May<Int64>
                    if type_name.contains('<') {
                        // Parse the generic type
                        let (base_name, args_str) = type_name.split_once('<')
                            .ok_or_else(|| Error::Codegen(format!("Invalid generic type: {}", type_name)))?;
                        
                        let args_str = args_str.trim_end_matches('>');
                        let arg_names: Vec<String> = args_str.split(',')
                            .map(|s| s.trim().to_string())
                            .collect();
                        
                        // Convert argument names to AST types
                        let mut new_args = Vec::new();
                        for arg_name in arg_names {
                            new_args.push(ast::Type::Named {
                                name: arg_name,
                                args: vec![],
                                span: span.clone(),
                            });
                        }
                        
                        Ok(ast::Type::Named {
                            name: base_name.to_string(),
                            args: new_args,
                            span: span.clone(),
                        })
                    } else {
                        // Simple type
                        Ok(ast::Type::Named {
                            name: type_name,
                            args: vec![],
                            span: span.clone(),
                        })
                    }
                } else {
                    // Not a type parameter, but may have type arguments that need substitution
                    let mut new_args = Vec::new();
                    for arg in args {
                        new_args.push(self.substitute_type(arg)?);
                    }
                    Ok(ast::Type::Named {
                        name: name.clone(),
                        args: new_args,
                        span: span.clone(),
                    })
                }
            }
            ast::Type::Array { elem, span } => {
                Ok(ast::Type::Array {
                    elem: Box::new(self.substitute_type(elem)?),
                    span: span.clone(),
                })
            }
            ast::Type::Function { params, ret, span } => {
                let mut new_params = Vec::new();
                for param in params {
                    new_params.push(self.substitute_type(param)?);
                }
                Ok(ast::Type::Function {
                    params: new_params,
                    ret: Box::new(self.substitute_type(ret)?),
                    span: span.clone(),
                })
            }
            ast::Type::Tuple { elems, span } => {
                let mut new_elems = Vec::new();
                for elem in elems {
                    new_elems.push(self.substitute_type(elem)?);
                }
                Ok(ast::Type::Tuple {
                    elems: new_elems,
                    span: span.clone(),
                })
            }
            ast::Type::Union { variants, span } => {
                let mut new_variants = Vec::new();
                for variant in variants {
                    new_variants.push(self.substitute_type(variant)?);
                }
                Ok(ast::Type::Union {
                    variants: new_variants,
                    span: span.clone(),
                })
            }
        }
    }
}