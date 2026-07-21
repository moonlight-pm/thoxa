//! Binary and unary operations code generation

use crate::ast::{Expr, BinaryOp, UnaryOp};
use crate::codegen::{CodeGen, types::TypeId, builder};
use crate::error::{Error, Result};
use super::ExprResult;

impl CodeGen {
    /// Generate code for a binary operation
    pub(super) fn gen_binary_op(&mut self, op: &BinaryOp, left: &Expr, right: &Expr) -> Result<ExprResult> {
        // Special handling for short-circuit operators
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.gen_logical_op(op, left, right);
        }
        
        // Generate code for left and right operands
        let left_result = self.gen_expr(left)?;
        let right_result = self.gen_expr(right)?;
        
        // Type checking - for now, only support operations on matching types
        if left_result.ty != right_result.ty {
            return Err(Error::Codegen(format!(
                "Type mismatch in binary operation: {:?} vs {:?}",
                left_result.ty, right_result.ty
            )));
        }
        
        // Special handling for string operations
        if left_result.ty == TypeId::STRING {
            match op {
                BinaryOp::Eq => {
                    // String equality
                    let result_temp = self.symbol_table.gen_temp();
                    self.qbe.call(
                        Some(&result_temp),
                        builder::QbeType::W,
                        "thoxa_string_equals",
                        &[(&left_result.temp, builder::QbeType::L), (&right_result.temp, builder::QbeType::L)]
                    );
                    return Ok(ExprResult {
                        temp: result_temp,
                        ty: TypeId::BOOL,
                    });
                }
                BinaryOp::Ne => {
                    // String inequality
                    let equals_temp = self.symbol_table.gen_temp();
                    self.qbe.call(
                        Some(&equals_temp),
                        builder::QbeType::W,
                        "thoxa_string_equals",
                        &[(&left_result.temp, builder::QbeType::L), (&right_result.temp, builder::QbeType::L)]
                    );
                    // Negate the result
                    let result_temp = self.symbol_table.gen_temp();
                    self.qbe.instruction(format!("{} =w xor {}, 1", result_temp, equals_temp));
                    return Ok(ExprResult {
                        temp: result_temp,
                        ty: TypeId::BOOL,
                    });
                }
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                    // String comparison
                    let cmp_temp = self.symbol_table.gen_temp();
                    self.qbe.call(
                        Some(&cmp_temp),
                        builder::QbeType::W,
                        "thoxa_string_compare",
                        &[(&left_result.temp, builder::QbeType::L), (&right_result.temp, builder::QbeType::L)]
                    );
                    
                    // Compare result with 0
                    let result_temp = self.symbol_table.gen_temp();
                    let cmp_op = match op {
                        BinaryOp::Lt => "slt",
                        BinaryOp::Le => "sle",
                        BinaryOp::Gt => "sgt",
                        BinaryOp::Ge => "sge",
                        _ => unreachable!(),
                    };
                    self.qbe.cmp(&result_temp, cmp_op, builder::QbeType::W, &cmp_temp, "0");
                    return Ok(ExprResult {
                        temp: result_temp,
                        ty: TypeId::BOOL,
                    });
                }
                BinaryOp::Add => {
                    // String concatenation via + is not supported
                    // Use string interpolation instead: "Hello, {name}!"
                    return Err(Error::Codegen(
                        "String concatenation with + is not supported. Use string interpolation instead: \"Hello, {name}!\"".to_string()
                    ));
                }
                _ => {
                    return Err(Error::Codegen(format!(
                        "Operation {:?} not supported for strings",
                        op
                    )));
                }
            }
        }
        
        // Get the QBE type for the operation
        let qbe_type = self.get_qbe_type(left_result.ty)?;
        
        // Generate result temporary
        let result_temp = self.symbol_table.gen_temp();
        
        // Generate the appropriate QBE instruction
        match op {
            // Arithmetic operations
            BinaryOp::Add => self.qbe.add(&result_temp, qbe_type, &left_result.temp, &right_result.temp),
            BinaryOp::Sub => self.qbe.sub(&result_temp, qbe_type, &left_result.temp, &right_result.temp),
            BinaryOp::Mul => self.qbe.mul(&result_temp, qbe_type, &left_result.temp, &right_result.temp),
            BinaryOp::Div => self.qbe.div(&result_temp, qbe_type, &left_result.temp, &right_result.temp),
            BinaryOp::Rem => self.qbe.rem(&result_temp, qbe_type, &left_result.temp, &right_result.temp),
            
            // Comparison operations - result is always bool (w)
            BinaryOp::Eq => {
                self.qbe.cmp(&result_temp, "eq", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            BinaryOp::Ne => {
                self.qbe.cmp(&result_temp, "ne", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            BinaryOp::Lt => {
                self.qbe.cmp(&result_temp, "lt", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            BinaryOp::Le => {
                self.qbe.cmp(&result_temp, "le", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            BinaryOp::Gt => {
                self.qbe.cmp(&result_temp, "gt", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            BinaryOp::Ge => {
                self.qbe.cmp(&result_temp, "ge", qbe_type, &left_result.temp, &right_result.temp);
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
            
            // These are handled by gen_logical_op
            BinaryOp::And | BinaryOp::Or => {
                unreachable!("Logical operators should be handled by gen_logical_op");
            }
        }
        
        Ok(ExprResult {
            temp: result_temp,
            ty: left_result.ty,
        })
    }
    
    /// Generate code for a unary operation
    pub(super) fn gen_unary_op(&mut self, op: &UnaryOp, expr: &Expr) -> Result<ExprResult> {
        let expr_result = self.gen_expr(expr)?;
        let qbe_type = self.get_qbe_type(expr_result.ty)?;
        let result_temp = self.symbol_table.gen_temp();
        
        match op {
            UnaryOp::Neg => {
                // Negation: 0 - value
                self.qbe.sub(&result_temp, qbe_type, "0", &expr_result.temp);
            }
            UnaryOp::Not => {
                // Boolean not: compare with 0
                self.qbe.cmp(&result_temp, "eq", qbe_type, &expr_result.temp, "0");
                return Ok(ExprResult {
                    temp: result_temp,
                    ty: TypeId::BOOL,
                });
            }
        }
        
        Ok(ExprResult {
            temp: result_temp,
            ty: expr_result.ty,
        })
    }
    
    /// Generate code for logical operations with short-circuit evaluation  
    fn gen_logical_op(&mut self, op: &BinaryOp, left: &Expr, right: &Expr) -> Result<ExprResult> {
        // For now, just evaluate both sides without short-circuiting
        // TODO: Implement proper short-circuit evaluation with control flow
        
        // Evaluate both operands
        let left_result = self.gen_expr(left)?;
        let right_result = self.gen_expr(right)?;
        
        // Check that both operands are boolean
        if left_result.ty != TypeId::BOOL || right_result.ty != TypeId::BOOL {
            return Err(Error::Codegen(format!(
                "Logical operator requires boolean operands, got {:?} and {:?}",
                left_result.ty, right_result.ty
            )));
        }
        
        let result_temp = self.symbol_table.gen_temp();
        
        match op {
            BinaryOp::And => {
                // AND: result = left && right (both must be true)
                // In QBE, we multiply the boolean values (1 * 1 = 1, anything else = 0)
                self.qbe.mul(&result_temp, crate::codegen::builder::QbeType::W, 
                           &left_result.temp, &right_result.temp);
            }
            BinaryOp::Or => {
                // OR: result = left || right (at least one must be true)
                // We can use: result = left + right - (left * right)
                // Or simpler: result = (left + right) != 0
                let sum_temp = self.symbol_table.gen_temp();
                self.qbe.add(&sum_temp, crate::codegen::builder::QbeType::W,
                           &left_result.temp, &right_result.temp);
                // Check if sum is non-zero
                self.qbe.cmp(&result_temp, "ne", crate::codegen::builder::QbeType::W, &sum_temp, "0");
            }
            _ => unreachable!("Only And and Or should reach here"),
        }
        
        Ok(ExprResult {
            temp: result_temp,
            ty: TypeId::BOOL,
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
    fn test_binary_add() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(ast::Expr::IntLiteral {
                value: 10,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            right: Box::new(ast::Expr::IntLiteral {
                value: 20,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("add"));
    }
    
    #[test]
    fn test_binary_multiply() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(ast::Expr::IntLiteral {
                value: 3,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            right: Box::new(ast::Expr::IntLiteral {
                value: 7,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("mul"));
    }
    
    #[test]
    fn test_comparison() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::Binary {
            op: BinaryOp::Lt,
            left: Box::new(ast::Expr::IntLiteral {
                value: 5,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            right: Box::new(ast::Expr::IntLiteral {
                value: 10,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::BOOL); // Comparison returns bool
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("csltl")); // compare signed less than long
    }
    
    #[test]
    fn test_unary_neg() {
        let mut codegen = CodeGen::new();
        let expr = ast::Expr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(ast::Expr::IntLiteral {
                value: 42,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("sub")); // 0 - value
    }
    
    #[test]
    fn test_complex_arithmetic() {
        let mut codegen = CodeGen::new();
        // Test: 1 + 2 * 3
        let expr = ast::Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(ast::Expr::IntLiteral {
                value: 1,
                suffix: IntSuffix::None,
                span: make_span(),
            }),
            right: Box::new(ast::Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(ast::Expr::IntLiteral {
                    value: 2,
                    suffix: IntSuffix::None,
                    span: make_span(),
                }),
                right: Box::new(ast::Expr::IntLiteral {
                    value: 3,
                    suffix: IntSuffix::None,
                    span: make_span(),
                }),
                span: make_span(),
            }),
            span: make_span(),
        };
        
        codegen.qbe.function_start("test", &[], crate::codegen::builder::QbeType::L, false).unwrap();
        codegen.qbe.label("start");
        
        let result = codegen.gen_expr(&expr).unwrap();
        assert_eq!(result.ty, TypeId::INT64);
        
        codegen.qbe.function_end().unwrap();
        
        let output = codegen.qbe.finish();
        assert!(output.contains("mul")); // multiplication happens first
        assert!(output.contains("add")); // then addition
    }
}