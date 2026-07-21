//! Expression parsing for the Thoxa language
//!
//! This module is organized into submodules:
//! - mod.rs: Main expression parsing entry point and block expressions
//! - operators.rs: Binary and unary operator parsing with precedence
//! - postfix.rs: Postfix operators (function calls, field access)
//! - primary.rs: Primary expressions (literals, identifiers, parentheses)

mod operators;
mod postfix;
mod primary;
mod interpolation;

use crate::ast::{Expr, Block};
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use super::Parser;

impl Parser {
    /// Parse an expression
    pub(super) fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_assignment()
    }
    
    /// Parse assignment expression (lowest precedence)
    pub(super) fn parse_assignment(&mut self) -> Result<Expr> {
        let expr = self.parse_or()?;
        
        // Check if this is an assignment
        if self.at(TokenKind::Equal) {
            let start_span = expr.span();
            self.advance()?; // consume '='
            let value = self.parse_assignment()?; // Right-associative
            
            // Validate that the target is a valid lvalue
            if !is_valid_assignment_target(&expr) {
                return Err(Error::Parse("Invalid assignment target".to_string()));
            }
            
            Ok(Expr::Assign {
                target: Box::new(expr),
                value: Box::new(value),
                span: self.span_from(start_span),
            })
        } else {
            Ok(expr)
        }
    }
    
    /// Parse a block expression
    fn parse_block_expr(&mut self) -> Result<Expr> {
        let block = self.parse_block()?;
        Ok(Expr::Block(block))
    }
    
    /// Parse a block (used by block expressions and control flow)
    pub(super) fn parse_block(&mut self) -> Result<Block> {
        let start = self.current_span();
        self.expect(TokenKind::LeftBrace)?;
        
        let mut stmts = Vec::new();
        let mut expr = None;
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            // Check if this looks like a statement
            if matches!(
                &self.current.kind,
                TokenKind::Let | TokenKind::Mut | TokenKind::Return | 
                TokenKind::Break | TokenKind::Continue | TokenKind::Fn
            ) {
                match self.parse_stmt() {
                    Ok(stmt) => stmts.push(stmt),
                    Err(e) => {
                        self.error(e);
                        self.synchronize();
                    }
                }
            } else {
                // Try to parse as expression
                match self.parse_expr() {
                    Ok(e) => {
                        // Check if this is the final expression (no semicolon)
                        if self.at(TokenKind::Semicolon) {
                            self.advance()?; // consume semicolon
                            stmts.push(crate::ast::Stmt::Expr(e));
                        } else if self.at(TokenKind::RightBrace) {
                            // Final expression without semicolon
                            expr = Some(Box::new(e));
                        } else {
                            // Expression without semicolon in middle of block
                            return Err(Error::Parse(
                                "Expected ';' or '}' after expression".to_string()
                            ));
                        }
                    }
                    Err(e) => {
                        self.error(e);
                        self.synchronize();
                    }
                }
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        
        Ok(Block {
            stmts,
            expr,
            span: self.span_from(start),
        })
    }
}

/// Check if an expression is a valid assignment target (lvalue)
fn is_valid_assignment_target(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident { .. } | Expr::FieldAccess { .. } | Expr::Index { .. }
    )
}

#[cfg(test)]
mod tests {
    use crate::ast::Expr;
    use crate::parser::Parser;
    
    #[test]
    fn test_block_expr() {
        let mut parser = Parser::new("{ 42 }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Block(block) => {
                assert_eq!(block.stmts.len(), 0);
                assert!(block.expr.is_some());
            }
            _ => panic!("Expected Block"),
        }
    }
    
    #[test]
    fn test_block_with_statements() {
        let mut parser = Parser::new("{ let x = 10; let y = 20; x + y }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Block(block) => {
                assert_eq!(block.stmts.len(), 2);
                assert!(block.expr.is_some());
            }
            _ => panic!("Expected Block"),
        }
    }
    
    #[test]
    fn test_block_with_return() {
        let mut parser = Parser::new("{ return 42; }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Block(block) => {
                assert_eq!(block.stmts.len(), 1);
                assert!(block.expr.is_none());
                match &block.stmts[0] {
                    crate::ast::Stmt::Return { .. } => {},
                    _ => panic!("Expected Return statement"),
                }
            }
            _ => panic!("Expected Block"),
        }
    }
}