//! Primary expression parsing (literals, identifiers, parentheses)

use crate::ast::Expr;
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse a primary expression (literals, identifiers, parentheses)
    pub(in crate::parser) fn parse_primary(&mut self) -> Result<Expr> {
        let start = self.current_span();
        
        match &self.current.kind {
            // Integer literals
            TokenKind::IntLiteral(value, suffix) => {
                let value = *value;
                let suffix = *suffix;
                self.advance()?;
                
                Ok(Expr::IntLiteral {
                    value,
                    suffix,
                    span: start,
                })
            }
            
            // Float literals
            TokenKind::FloatLiteral(value, suffix) => {
                let value = *value;
                let suffix = *suffix;
                self.advance()?;
                
                Ok(Expr::FloatLiteral {
                    value,
                    suffix,
                    span: start,
                })
            }
            
            // String literals (may contain interpolation)
            TokenKind::StringLiteral(s) => {
                let value = s.clone();
                self.advance()?;
                
                // Check for interpolation
                self.parse_string_literal(value, start)
            }
            
            // Character literals
            TokenKind::CharLiteral(ch) => {
                let value = *ch;
                self.advance()?;
                
                Ok(Expr::CharLiteral {
                    value,
                    span: start,
                })
            }
            
            // Boolean literals
            TokenKind::True => {
                self.advance()?;
                Ok(Expr::BoolLiteral {
                    value: true,
                    span: start,
                })
            }
            
            TokenKind::False => {
                self.advance()?;
                Ok(Expr::BoolLiteral {
                    value: false,
                    span: start,
                })
            }
            
            // Void literal
            TokenKind::Void => {
                self.advance()?;
                Ok(Expr::VoidLiteral { span: start })
            }
            
            // Identifiers
            TokenKind::Identifier(name) => {
                let name = name.clone();
                self.advance()?;
                
                Ok(Expr::Ident {
                    name,
                    span: start,
                })
            }
            
            // Parenthesized expressions or tuples
            TokenKind::LeftParen => {
                self.advance()?; // consume '('
                
                // Check for empty tuple (not supported for now)
                if self.at(TokenKind::RightParen) {
                    return Err(Error::Parse("Empty tuples are not supported".to_string()));
                }
                
                // Parse first expression
                let first = self.parse_expr()?;
                
                // Check if this is a tuple
                if self.at(TokenKind::Comma) {
                    // It's a tuple!
                    let mut elements = vec![first];
                    
                    while self.at(TokenKind::Comma) {
                        self.advance()?; // consume ','
                        
                        // Allow trailing comma
                        if self.at(TokenKind::RightParen) {
                            break;
                        }
                        
                        elements.push(self.parse_expr()?);
                    }
                    
                    self.expect(TokenKind::RightParen)?;
                    
                    Ok(Expr::Tuple {
                        elements,
                        span: self.span_from(start),
                    })
                } else {
                    // Just a parenthesized expression
                    self.expect(TokenKind::RightParen)?;
                    Ok(first)
                }
            }
            
            // Block expressions
            TokenKind::LeftBrace => {
                self.parse_block_expr()
            }
            
            // Control flow
            TokenKind::If => {
                self.parse_if_expr()
            }
            
            TokenKind::Loop => {
                self.parse_loop_expr()
            }
            
            TokenKind::While => {
                self.parse_while_expr()
            }
            
            TokenKind::For => {
                self.parse_for_in_expr()
            }
            
            TokenKind::Match => {
                self.parse_match_expr()
            }
            
            // Array literals
            TokenKind::LeftBracket => {
                self.parse_array_literal()
            }
            
            _ => Err(Error::Parse(format!(
                "Expected expression, found {:?}",
                self.current.kind
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Expr;
    use crate::parser::Parser;
    use crate::lexer::{IntSuffix, FloatSuffix};
    
    #[test]
    fn test_int_literals() {
        let mut parser = Parser::new("42").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::IntLiteral { value, suffix, .. } => {
                assert_eq!(value, 42);
                assert_eq!(suffix, IntSuffix::None);
            }
            _ => panic!("Expected IntLiteral"),
        }
    }
    
    #[test]
    fn test_int_with_suffix() {
        let mut parser = Parser::new("42u32").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::IntLiteral { value, suffix, .. } => {
                assert_eq!(value, 42);
                assert_eq!(suffix, IntSuffix::U32);
            }
            _ => panic!("Expected IntLiteral"),
        }
    }
    
    #[test]
    fn test_float_literal() {
        let mut parser = Parser::new("3.14").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::FloatLiteral { value, suffix, .. } => {
                assert_eq!(value, 3.14);
                assert_eq!(suffix, FloatSuffix::None);
            }
            _ => panic!("Expected FloatLiteral"),
        }
    }
    
    #[test]
    fn test_string_literal() {
        let mut parser = Parser::new(r#""hello world""#).unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::StringLiteral { value, .. } => {
                assert_eq!(value, "hello world");
            }
            _ => panic!("Expected StringLiteral"),
        }
    }
    
    #[test]
    fn test_char_literal() {
        let mut parser = Parser::new("'a'").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::CharLiteral { value, .. } => {
                assert_eq!(value, 'a');
            }
            _ => panic!("Expected CharLiteral"),
        }
    }
    
    #[test]
    fn test_bool_literals() {
        let mut parser = Parser::new("true").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::BoolLiteral { value, .. } => {
                assert_eq!(value, true);
            }
            _ => panic!("Expected BoolLiteral"),
        }
        
        let mut parser = Parser::new("false").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::BoolLiteral { value, .. } => {
                assert_eq!(value, false);
            }
            _ => panic!("Expected BoolLiteral"),
        }
    }
    
    #[test]
    fn test_void_literal() {
        let mut parser = Parser::new("Void").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::VoidLiteral { .. } => {},
            _ => panic!("Expected VoidLiteral"),
        }
    }
    
    #[test]
    fn test_identifier() {
        let mut parser = Parser::new("myVariable").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Ident { name, .. } => {
                assert_eq!(name, "myVariable");
            }
            _ => panic!("Expected Ident"),
        }
    }
    
    #[test]
    fn test_parenthesized() {
        let mut parser = Parser::new("(42)").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::IntLiteral { value, .. } => {
                assert_eq!(value, 42);
            }
            _ => panic!("Expected IntLiteral"),
        }
    }
}