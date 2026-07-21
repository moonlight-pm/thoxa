//! Statement parsing for the Thoxa language

use crate::ast::Stmt;
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse a statement
    pub(super) fn parse_stmt(&mut self) -> Result<Stmt> {
        match &self.current.kind {
            TokenKind::Let => self.parse_let_stmt(),
            TokenKind::Mut => self.parse_mut_stmt(),
            TokenKind::Fn => self.parse_nested_function(),
            TokenKind::Return => self.parse_return_stmt(),
            TokenKind::Break => self.parse_break_stmt(),
            TokenKind::Continue => self.parse_continue_stmt(),
            _ => {
                // Try to parse as expression statement or assignment
                self.parse_expr_or_assign_stmt()
            }
        }
    }
    
    /// Parse a let statement: `let x = 42;` or `let x: Int = 42;`
    fn parse_let_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Let)?;
        
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected identifier after 'let'".to_string())),
        };
        
        // Optional type annotation
        let ty = if self.at(TokenKind::Colon) {
            self.advance()?; // consume ':'
            Some(self.parse_type()?)
        } else {
            None
        };
        
        // Optional initializer
        let value = if self.at(TokenKind::Equal) {
            self.advance()?; // consume '='
            Some(self.parse_expr()?)
        } else {
            None
        };
        
        self.expect(TokenKind::Semicolon)?;
        
        Ok(Stmt::Let {
            name,
            ty,
            value,
            is_mut: false,
            is_export: false,
            span: self.span_from(start),
        })
    }
    
    /// Parse a mut statement: `mut x = 42;` or `mut x: Int = 42;`
    fn parse_mut_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Mut)?;
        
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected identifier after 'mut'".to_string())),
        };
        
        // Optional type annotation
        let ty = if self.at(TokenKind::Colon) {
            self.advance()?; // consume ':'
            Some(self.parse_type()?)
        } else {
            None
        };
        
        // Required initializer for mut
        self.expect(TokenKind::Equal)?;
        let value = Some(self.parse_expr()?);
        
        self.expect(TokenKind::Semicolon)?;
        
        Ok(Stmt::Let {
            name,
            ty,
            value,
            is_mut: true,
            is_export: false,
            span: self.span_from(start),
        })
    }
    
    /// Parse a return statement: `return;` or `return expr;`
    fn parse_return_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Return)?;
        
        let value = if self.at(TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        
        self.expect(TokenKind::Semicolon)?;
        
        Ok(Stmt::Return {
            value,
            span: self.span_from(start),
        })
    }
    
    /// Parse a break statement: `break;` or `break expr;`
    fn parse_break_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Break)?;
        
        let value = if self.at(TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        
        self.expect(TokenKind::Semicolon)?;
        
        Ok(Stmt::Break {
            value,
            span: self.span_from(start),
        })
    }
    
    /// Parse a continue statement: `continue;`
    fn parse_continue_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Continue)?;
        self.expect(TokenKind::Semicolon)?;
        
        Ok(Stmt::Continue {
            span: self.span_from(start),
        })
    }
    
    /// Parse an expression statement or assignment
    fn parse_expr_or_assign_stmt(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        let expr = self.parse_expr()?;
        
        // Check if this is an assignment
        if self.at(TokenKind::Equal) {
            self.advance()?; // consume '='
            let value = self.parse_expr()?;
            self.expect(TokenKind::Semicolon)?;
            
            // Validate that the target is a valid lvalue
            if !is_valid_assignment_target(&expr) {
                return Err(Error::Parse("Invalid assignment target".to_string()));
            }
            
            // Create assignment expression
            let assign_expr = crate::ast::Expr::Assign {
                target: Box::new(expr),
                value: Box::new(value),
                span: self.span_from(start),
            };
            
            Ok(Stmt::Expr(assign_expr))
        } else {
            // Regular expression statement
            self.expect(TokenKind::Semicolon)?;
            Ok(Stmt::Expr(expr))
        }
    }
    
    /// Parse a nested function definition
    fn parse_nested_function(&mut self) -> Result<Stmt> {
        let start = self.current_span();
        self.expect(TokenKind::Fn)?;
        
        // Parse function name
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected function name after 'fn'".to_string())),
        };
        
        // Note: Generic type parameters are not supported for nested functions yet
        // This is a simplification - we could add support later if needed
        
        // Parse parameters
        self.expect(TokenKind::LeftParen)?;
        let mut params = Vec::new();
        
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            // Parse parameter name
            let param_name = match &self.current.kind {
                TokenKind::Identifier(n) => {
                    let name = n.clone();
                    self.advance()?;
                    name
                }
                _ => return Err(Error::Parse("Expected parameter name".to_string())),
            };
            
            // Parse parameter type
            self.expect(TokenKind::Colon)?;
            let param_ty = self.parse_type()?;
            
            params.push(crate::ast::Param {
                name: param_name,
                ty: param_ty,
                is_mut: false,  // Parameters are immutable by default
                span: self.current_span(),
            });
            
            // Check for comma
            if !self.at(TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightParen)?;
        
        // Parse optional return type
        let return_type = if self.at(TokenKind::Arrow) {
            self.advance()?; // consume '->'
            Some(self.parse_type()?)
        } else {
            None
        };
        
        // Parse function body
        let body = self.parse_block()?;
        
        Ok(Stmt::Method {
            name,
            params,
            return_type,
            body,
            is_export: false, // Nested functions are never exported
            span: self.span_from(start),
        })
    }
    
}

/// Check if an expression is a valid assignment target (lvalue)
fn is_valid_assignment_target(expr: &crate::ast::Expr) -> bool {
    use crate::ast::Expr;
    matches!(
        expr,
        Expr::Ident { .. } | 
        Expr::FieldAccess { .. } | 
        Expr::Index { .. }
    )
}

#[cfg(test)]
mod tests {
    use crate::ast::Stmt;
    use crate::error::Error;
    use crate::parser::Parser;
    
    #[test]
    fn test_let_statement() {
        let mut parser = Parser::new("let x = 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Let { name, value, is_mut, ty, .. } => {
                assert_eq!(name, "x");
                assert!(!is_mut);
                assert!(ty.is_none());
                assert!(value.is_some());
            }
            _ => panic!("Expected Let statement"),
        }
    }
    
    #[test]
    fn test_let_with_type() {
        let mut parser = Parser::new("let x: Int = 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Let { name, value, is_mut, ty, .. } => {
                assert_eq!(name, "x");
                assert!(!is_mut);
                assert!(ty.is_some());
                assert!(value.is_some());
            }
            _ => panic!("Expected Let statement"),
        }
    }
    
    #[test]
    fn test_mut_statement() {
        let mut parser = Parser::new("mut x = 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Let { name, value, is_mut, ty, .. } => {
                assert_eq!(name, "x");
                assert!(is_mut);
                assert!(ty.is_none());
                assert!(value.is_some());
            }
            _ => panic!("Expected Let statement with is_mut=true"),
        }
    }
    
    #[test]
    fn test_return_statement() {
        let mut parser = Parser::new("return 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Return { value, .. } => {
                assert!(value.is_some());
            }
            _ => panic!("Expected Return statement"),
        }
    }
    
    #[test]
    fn test_return_void() {
        let mut parser = Parser::new("return;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Return { value, .. } => {
                assert!(value.is_none());
            }
            _ => panic!("Expected Return statement"),
        }
    }
    
    #[test]
    fn test_expression_statement() {
        let mut parser = Parser::new("foo();").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Expr(_) => {},
            _ => panic!("Expected expression statement"),
        }
    }
    
    #[test]
    fn test_break_statement() {
        let mut parser = Parser::new("break;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Break { value, .. } => {
                assert!(value.is_none());
            }
            _ => panic!("Expected Break statement"),
        }
    }
    
    #[test]
    fn test_break_with_value() {
        let mut parser = Parser::new("break 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Break { value, .. } => {
                assert!(value.is_some());
            }
            _ => panic!("Expected Break statement"),
        }
    }
    
    #[test]
    fn test_continue_statement() {
        let mut parser = Parser::new("continue;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Continue { .. } => {},
            _ => panic!("Expected Continue statement"),
        }
    }
    
    #[test]
    fn test_assignment_statement() {
        let mut parser = Parser::new("x = 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Expr(expr) => {
                match expr {
                    crate::ast::Expr::Assign { target, value, .. } => {
                        match target.as_ref() {
                            crate::ast::Expr::Ident { name, .. } => assert_eq!(name, "x"),
                            _ => panic!("Expected identifier target"),
                        }
                        match value.as_ref() {
                            crate::ast::Expr::IntLiteral { value: 42, .. } => {},
                            _ => panic!("Expected 42"),
                        }
                    }
                    _ => panic!("Expected Assign expression"),
                }
            }
            _ => panic!("Expected expression statement"),
        }
    }
    
    #[test]
    fn test_field_assignment() {
        let mut parser = Parser::new("obj.field = 42;").unwrap();
        let stmt = parser.parse_stmt().unwrap();
        
        match stmt {
            Stmt::Expr(expr) => {
                match expr {
                    crate::ast::Expr::Assign { target, .. } => {
                        match target.as_ref() {
                            crate::ast::Expr::FieldAccess { .. } => {},
                            _ => panic!("Expected field access target"),
                        }
                    }
                    _ => panic!("Expected Assign expression"),
                }
            }
            _ => panic!("Expected expression statement"),
        }
    }
    
    #[test]
    fn test_invalid_assignment_target() {
        let mut parser = Parser::new("42 = x;").unwrap();
        let result = parser.parse_stmt();
        
        assert!(result.is_err());
        match result {
            Err(Error::Parse(msg)) => assert!(msg.contains("Invalid assignment target")),
            _ => panic!("Expected parse error for invalid assignment target"),
        }
    }
}