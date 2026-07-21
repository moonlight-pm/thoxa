//! Array parsing (array literals and indexing)

use crate::ast::Expr;
use crate::error::Result;
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse an array literal: [1, 2, 3]
    pub(super) fn parse_array_literal(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::LeftBracket)?;
        
        let mut elements = Vec::new();
        
        while !self.at(TokenKind::RightBracket) && !self.at_eof() {
            elements.push(self.parse_expr()?);
            
            if !self.at(TokenKind::RightBracket) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightBracket)?;
        
        Ok(Expr::ArrayLiteral {
            elements,
            span: self.span_from(start),
        })
    }
    
    /// Parse index access: array[index]
    pub(super) fn parse_index(&mut self, array: Expr) -> Result<Expr> {
        let start = array.span();
        self.expect(TokenKind::LeftBracket)?;
        
        let index = Box::new(self.parse_expr()?);
        
        self.expect(TokenKind::RightBracket)?;
        
        Ok(Expr::Index {
            array: Box::new(array),
            index,
            span: self.span_from(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Expr;
    use crate::parser::Parser;
    
    #[test]
    fn test_empty_array() {
        let mut parser = Parser::new("[]").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::ArrayLiteral { elements, .. } => {
                assert_eq!(elements.len(), 0);
            }
            _ => panic!("Expected ArrayLiteral"),
        }
    }
    
    #[test]
    fn test_array_literal() {
        let mut parser = Parser::new("[1, 2, 3]").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::ArrayLiteral { elements, .. } => {
                assert_eq!(elements.len(), 3);
                
                for (i, elem) in elements.iter().enumerate() {
                    match elem {
                        Expr::IntLiteral { value, .. } => {
                            assert_eq!(*value, (i + 1) as i64);
                        }
                        _ => panic!("Expected IntLiteral"),
                    }
                }
            }
            _ => panic!("Expected ArrayLiteral"),
        }
    }
    
    #[test]
    fn test_nested_arrays() {
        let mut parser = Parser::new("[[1, 2], [3, 4]]").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::ArrayLiteral { elements, .. } => {
                assert_eq!(elements.len(), 2);
                
                for elem in &elements {
                    match elem {
                        Expr::ArrayLiteral { elements: inner, .. } => {
                            assert_eq!(inner.len(), 2);
                        }
                        _ => panic!("Expected nested ArrayLiteral"),
                    }
                }
            }
            _ => panic!("Expected ArrayLiteral"),
        }
    }
    
    #[test]
    fn test_array_indexing() {
        let mut parser = Parser::new("arr[0]").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Index { array, index, .. } => {
                match array.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "arr"),
                    _ => panic!("Expected Ident for array"),
                }
                match index.as_ref() {
                    Expr::IntLiteral { value: 0, .. } => {},
                    _ => panic!("Expected IntLiteral 0 for index"),
                }
            }
            _ => panic!("Expected Index expression"),
        }
    }
    
    #[test]
    fn test_chained_indexing() {
        let mut parser = Parser::new("matrix[i][j]").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Index { array, index, .. } => {
                // The outer index should have another Index as its array
                match array.as_ref() {
                    Expr::Index { array: inner_array, index: inner_index, .. } => {
                        match inner_array.as_ref() {
                            Expr::Ident { name, .. } => assert_eq!(name, "matrix"),
                            _ => panic!("Expected Ident for matrix"),
                        }
                        match inner_index.as_ref() {
                            Expr::Ident { name, .. } => assert_eq!(name, "i"),
                            _ => panic!("Expected Ident i for first index"),
                        }
                    }
                    _ => panic!("Expected nested Index expression"),
                }
                match index.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "j"),
                    _ => panic!("Expected Ident j for second index"),
                }
            }
            _ => panic!("Expected Index expression"),
        }
    }
}