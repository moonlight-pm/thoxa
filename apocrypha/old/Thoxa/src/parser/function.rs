//! Function parsing

use crate::ast::{Item, Function, Param};
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse a function definition
    pub(super) fn parse_fn(&mut self, is_export: bool) -> Result<Item> {
        let start = self.current_span();
        self.expect(TokenKind::Fn)?;
        
        // Parse function name
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected function name".to_string())),
        };
        
        // Parse optional generic type parameters
        let type_params = if self.at(TokenKind::Less) {
            self.advance()?; // consume '<'
            let mut params = Vec::new();
            
            while !self.at(TokenKind::Greater) && !self.at_eof() {
                match &self.current.kind {
                    TokenKind::Identifier(param) => {
                        params.push(param.clone());
                        self.advance()?;
                    }
                    _ => return Err(Error::Parse("Expected type parameter name".to_string())),
                }
                
                if !self.at(TokenKind::Greater) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::Greater)?;
            params
        } else {
            Vec::new()
        };
        
        // Parse parameters
        self.expect(TokenKind::LeftParen)?;
        let mut params = Vec::new();
        
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            params.push(self.parse_param()?);
            
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
        
        // Parse body
        let body = self.parse_block()?;
        
        Ok(Item::Function(Function {
            name,
            type_params,
            params,
            return_type,
            body,
            is_export,
            span: self.span_from(start),
        }))
    }
    
    /// Parse a function parameter
    pub(super) fn parse_param(&mut self) -> Result<Param> {
        let start = self.current_span();
        
        // Parse parameter name
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            _ => return Err(Error::Parse("Expected parameter name".to_string())),
        };
        
        // Parse type annotation
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_type()?;
        
        Ok(Param {
            name,
            ty,
            is_mut: false,  // Parameters are always immutable
            span: self.span_from(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Item, Function};
    use crate::parser::Parser;
    
    #[test]
    fn test_simple_function() {
        let mut parser = Parser::new("fn main() { }").unwrap();
        let item = parser.parse_fn(false).unwrap();
        
        match item {
            Item::Function(Function { name, params, return_type, body, is_export, .. }) => {
                assert_eq!(name, "main");
                assert!(params.is_empty());
                assert!(return_type.is_none());
                assert!(body.stmts.is_empty());
                assert!(!is_export);
            }
            _ => panic!("Expected function"),
        }
    }
    
    #[test]
    fn test_function_with_params() {
        let mut parser = Parser::new("fn add(x: Int, y: Int) -> Int { x + y }").unwrap();
        let item = parser.parse_fn(false).unwrap();
        
        match item {
            Item::Function(Function { name, params, return_type, body, .. }) => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name, "x");
                assert_eq!(params[1].name, "y");
                assert!(!params[0].is_mut);  // Parameters are immutable
                assert!(!params[1].is_mut);  // Parameters are immutable
                assert!(return_type.is_some());
                assert!(body.expr.is_some());
            }
            _ => panic!("Expected function"),
        }
    }
    
    #[test]
    fn test_export_function() {
        let mut parser = Parser::new("fn greet() { }").unwrap();
        let item = parser.parse_fn(true).unwrap();
        
        match item {
            Item::Function(Function { is_export, .. }) => {
                assert!(is_export);
            }
            _ => panic!("Expected function"),
        }
    }
}