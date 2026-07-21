//! Type checking for expressions without code generation

use crate::ast::Expr;
use crate::codegen::{CodeGen, types::TypeId};
use crate::error::{Error, Result};

impl CodeGen {
    /// Get the type of an expression without generating code
    pub fn get_expr_type(&self, expr: &Expr) -> Result<TypeId> {
        match expr {
            Expr::IntLiteral { .. } => Ok(TypeId::INT64),
            Expr::FloatLiteral { .. } => Ok(TypeId::FLOAT64),
            Expr::BoolLiteral { .. } => Ok(TypeId::BOOL),
            Expr::StringLiteral { .. } => Ok(TypeId::STRING),
            
            Expr::Ident { name, .. } => {
                // Look up variable in symbol table
                let symbol = self.symbol_table.lookup(name)
                    .ok_or_else(|| Error::Codegen(format!("Unknown variable: {}", name)))?;
                Ok(symbol.type_id)
            }
            
            Expr::Binary { left, op, right, .. } => {
                let left_ty = self.get_expr_type(left)?;
                let right_ty = self.get_expr_type(right)?;
                
                // Type check binary operation
                if left_ty != right_ty {
                    return Err(Error::Codegen(format!(
                        "Type mismatch in binary operation: {:?} vs {:?}",
                        left_ty, right_ty
                    )));
                }
                
                // Determine result type based on operator
                use crate::ast::BinaryOp;
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                        Ok(left_ty) // Arithmetic ops preserve type
                    }
                    BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | 
                    BinaryOp::Gt | BinaryOp::Ge => {
                        Ok(TypeId::BOOL) // Comparison ops return bool
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        Ok(TypeId::BOOL) // Logical ops return bool
                    }
                }
            }
            
            Expr::Unary { op, expr, .. } => {
                let operand_ty = self.get_expr_type(expr)?;
                
                use crate::ast::UnaryOp;
                match op {
                    UnaryOp::Neg => Ok(operand_ty), // Negation preserves type
                    UnaryOp::Not => Ok(TypeId::BOOL), // Not returns bool
                }
            }
            
            Expr::Call { func, .. } => {
                // For now, only support direct function calls
                if let Expr::Ident { name, .. } = &**func {
                    // Check if it's a known function
                    if let Some(func_info) = self.functions.get(name) {
                        Ok(func_info.return_type.unwrap_or(TypeId::VOID))
                    } else if let Some(_ast_func) = self.ast_functions.get(name) {
                        // Generic function - for now just return a placeholder
                        // We can't resolve the actual type without type arguments
                        // TODO: Enhance this when we have better type tracking
                        Ok(TypeId::INT64) // Default assumption
                    } else {
                        Err(Error::Codegen(format!("Unknown function: {}", name)))
                    }
                } else {
                    Err(Error::Codegen("Complex function calls not supported in type checking".to_string()))
                }
            }
            
            Expr::Block(block) => {
                // The type of a block is the type of its last expression
                if let Some(last_stmt) = block.stmts.last() {
                    use crate::ast::Stmt;
                    match last_stmt {
                        Stmt::Expr(expr) => self.get_expr_type(expr),
                        _ => Ok(TypeId::VOID),
                    }
                } else {
                    Ok(TypeId::VOID)
                }
            }
            
            Expr::If { condition: _, then_block, else_block, .. } => {
                // Type of if expression is the type of its branches
                // Convert block to expression for type checking
                let then_ty = self.get_expr_type(&Expr::Block(then_block.clone()))?;
                
                if let Some(else_block) = else_block {
                    let else_ty = self.get_expr_type(&Expr::Block(else_block.clone()))?;
                    if then_ty != else_ty {
                        return Err(Error::Codegen(format!(
                            "If branches have different types: {:?} vs {:?}",
                            then_ty, else_ty
                        )));
                    }
                }
                
                Ok(then_ty)
            }
            
            Expr::Assign { .. } => {
                // Assignment expressions return void
                Ok(TypeId::VOID)
            }
            
            
            Expr::ArrayLiteral { .. } => {
                // Array literals are pointers
                // TODO: Track element type
                Ok(TypeId::POINTER)
            }
            
            Expr::Index { .. } => {
                // Array indexing returns the element type
                // For now, we don't track element types precisely
                Ok(TypeId::INT64) // Assume Int for now
            }
            
            Expr::FieldAccess { object, field, .. } => {
                let obj_ty = self.get_expr_type(object)?;
                
                // Special case for array properties
                if obj_ty == TypeId::POINTER && field == "count" {
                    return Ok(TypeId::INT64);
                }
                
                // Look up field type in struct
                if let Some(type_info) = self.type_registry.get(obj_ty) {
                    if let Some(fields) = &type_info.fields {
                        for (field_name, field_ty, _) in fields {
                            if field_name == field {
                                return Ok(*field_ty);
                            }
                        }
                    }
                }
                
                Err(Error::Codegen(format!("Unknown field {} on type", field)))
            }
            
            _ => Err(Error::Codegen(format!("Type checking not implemented for {:?}", expr))),
        }
    }
}