//! Control flow parsing (if, loop)

use crate::ast::{Expr, Block};
use crate::error::Result;
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse an if expression
    pub(super) fn parse_if_expr(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::If)?;
        
        // Parse condition (no parentheses required)
        let condition = Box::new(self.parse_expr()?);
        
        // Parse then block
        let then_block = self.parse_block()?;
        
        // Parse optional else block
        let else_block = if self.at(TokenKind::Else) {
            self.advance()?; // consume 'else'
            
            if self.at(TokenKind::If) {
                // else if - parse as nested if expression
                let else_if = self.parse_if_expr()?;
                let span = else_if.span();
                // Wrap the else-if in a block
                Some(Block {
                    stmts: vec![],
                    expr: Some(Box::new(else_if)),
                    span,
                })
            } else {
                // else block
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        
        Ok(Expr::If {
            condition,
            then_block,
            else_block,
            span: self.span_from(start),
        })
    }
    
    /// Parse a loop expression
    pub(super) fn parse_loop_expr(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::Loop)?;
        
        let body = self.parse_block()?;
        
        Ok(Expr::Loop {
            body,
            span: self.span_from(start),
        })
    }
    
    /// Parse a while expression
    pub(super) fn parse_while_expr(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::While)?;
        
        // Parse condition (no parentheses required)
        let condition = Box::new(self.parse_expr()?);
        
        // Parse body block
        let body = self.parse_block()?;
        
        Ok(Expr::While {
            condition,
            body,
            span: self.span_from(start),
        })
    }
    
    /// Parse a for-in expression
    pub(super) fn parse_for_in_expr(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::For)?;
        
        // Parse the loop variable
        let variable = if let TokenKind::Identifier(name) = &self.current.kind {
            let var_name = name.clone();
            self.advance()?;
            var_name
        } else {
            return Err(crate::error::Error::Parse(
                format!("Expected identifier after 'for', got {:?}", self.current.kind)
            ));
        };
        
        // Expect 'in'
        self.expect(TokenKind::In)?;
        
        // Parse the iterable expression
        // We need to be careful here - we can't use parse_assignment() because
        // it includes postfix parsing, which would try to parse "x {" as 
        // a unit construction. We need to parse just a primary expression
        // without any postfix operations.
        let iterable = Box::new(self.parse_primary()?);
        
        // Parse body block
        let body = self.parse_block()?;
        
        Ok(Expr::ForIn {
            variable,
            iterable,
            body,
            span: self.span_from(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expr, Stmt};
    use crate::parser::Parser;
    use crate::lexer::TokenKind;
    
    #[test]
    fn test_if_expression() {
        let mut parser = Parser::new("if x > 0 { 42 }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::If { then_block, else_block, .. } => {
                assert!(then_block.expr.is_some());
                assert!(else_block.is_none());
            }
            _ => panic!("Expected If expression"),
        }
    }
    
    #[test]
    fn test_if_else_expression() {
        let mut parser = Parser::new("if x > 0 { 42 } else { 0 }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::If { then_block, else_block, .. } => {
                assert!(then_block.expr.is_some());
                assert!(else_block.is_some());
            }
            _ => panic!("Expected If expression"),
        }
    }
    
    #[test]
    fn test_if_else_if_chain() {
        let mut parser = Parser::new("if x > 0 { 1 } else if x < 0 { -1 } else { 0 }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::If { else_block, .. } => {
                let else_block = else_block.expect("Expected else block");
                assert!(else_block.expr.is_some());
                // The else block should contain a nested if expression
                match else_block.expr.as_ref().unwrap().as_ref() {
                    Expr::If { .. } => {},
                    _ => panic!("Expected nested If in else block"),
                }
            }
            _ => panic!("Expected If expression"),
        }
    }
    
    #[test]
    fn test_loop_expression() {
        let mut parser = Parser::new("loop { break 42; }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Loop { body, .. } => {
                assert_eq!(body.stmts.len(), 1);
                match &body.stmts[0] {
                    Stmt::Break { value, .. } => {
                        assert!(value.is_some());
                    }
                    _ => panic!("Expected Break statement"),
                }
            }
            _ => panic!("Expected Loop expression"),
        }
    }
    
    #[test]
    fn test_for_in_expression() {
        let mut parser = Parser::new("for n in nums { 0 }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::ForIn { variable, body, .. } => {
                assert_eq!(variable, "n");
                assert!(body.expr.is_some());
            }
            _ => panic!("Expected ForIn expression, got {:?}", expr),
        }
    }
    
    #[test]
    fn test_loop_with_continue() {
        let mut parser = Parser::new("loop { continue; }").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Loop { body, .. } => {
                assert_eq!(body.stmts.len(), 1);
                match &body.stmts[0] {
                    Stmt::Continue { .. } => {},
                    _ => panic!("Expected Continue statement"),
                }
            }
            _ => panic!("Expected Loop expression"),
        }
    }
}