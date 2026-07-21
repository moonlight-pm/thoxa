//! Type parsing (generics, function types)

use crate::ast::Type;
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse a type
    pub(super) fn parse_type(&mut self) -> Result<Type> {
        let start = self.current_span();
        
        // Check for array type [T]
        if self.at(TokenKind::LeftBracket) {
            self.advance()?; // consume '['
            let elem = Box::new(self.parse_type()?);
            self.expect(TokenKind::RightBracket)?;
            return Ok(Type::Array {
                elem,
                span: self.span_from(start),
            });
        }
        
        // Check for function type
        if self.at(TokenKind::Fn) {
            return self.parse_function_type();
        }
        
        // Check for tuple type or function type without Fn keyword
        if self.at(TokenKind::LeftParen) {
            // Peek ahead to see if this is a function type (has arrow) or tuple type
            let checkpoint = self.checkpoint();
            self.advance()?; // consume '('
            
            // Empty parens could be function type () -> T
            if self.at(TokenKind::RightParen) {
                self.advance()?; // consume ')'
                if self.at(TokenKind::Arrow) {
                    // It's a function type with no parameters
                    self.restore(checkpoint);
                    return self.parse_bare_function_type();
                }
                self.restore(checkpoint);
                return Err(Error::Parse("Empty tuple types are not supported".to_string()));
            }
            
            // Try to determine if it's a function or tuple by looking ahead
            let mut paren_depth = 0;
            let mut is_function = false;
            
            while !self.at_eof() {
                if self.at(TokenKind::LeftParen) {
                    paren_depth += 1;
                } else if self.at(TokenKind::RightParen) {
                    if paren_depth == 0 {
                        self.advance()?;
                        // Check if there's an arrow after the closing paren
                        if self.at(TokenKind::Arrow) {
                            is_function = true;
                        }
                        break;
                    }
                    paren_depth -= 1;
                }
                self.advance()?;
            }
            
            self.restore(checkpoint);
            
            if is_function {
                return self.parse_bare_function_type();
            } else {
                return self.parse_tuple_type();
            }
        }
        
        // Parse type name
        let name = match &self.current.kind {
            TokenKind::Identifier(n) => {
                let name = n.clone();
                self.advance()?;
                name
            }
            TokenKind::Void => {
                self.advance()?;
                "Void".to_string()
            }
            _ => return Err(Error::Parse("Expected type name".to_string())),
        };
        
        // Parse optional generic arguments
        let args = if self.at(TokenKind::Less) {
            self.advance()?; // consume '<'
            let mut args = Vec::new();
            
            while !self.at(TokenKind::Greater) && !self.at_eof() {
                args.push(self.parse_type()?);
                
                if !self.at(TokenKind::Greater) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::Greater)?;
            args
        } else {
            vec![]
        };
        
        Ok(Type::Named {
            name,
            args,
            span: self.span_from(start),
        })
    }
    
    /// Parse a tuple type: (T1, T2, T3)
    fn parse_tuple_type(&mut self) -> Result<Type> {
        let start = self.current_span();
        self.expect(TokenKind::LeftParen)?;
        
        let mut elems = Vec::new();
        
        // Parse first element (required)
        elems.push(self.parse_type()?);
        
        // Must have at least one comma to be a tuple
        if !self.at(TokenKind::Comma) {
            // Single element without comma - could be parenthesized type
            self.expect(TokenKind::RightParen)?;
            // For now, treat single element with parens as just the type itself
            return Ok(elems.into_iter().next().unwrap());
        }
        
        // Parse remaining elements
        while self.at(TokenKind::Comma) {
            self.advance()?; // consume ','
            
            // Allow trailing comma
            if self.at(TokenKind::RightParen) {
                break;
            }
            
            elems.push(self.parse_type()?);
        }
        
        self.expect(TokenKind::RightParen)?;
        
        Ok(Type::Tuple {
            elems,
            span: self.span_from(start),
        })
    }
    
    /// Parse a function type: Fn(T1, T2) -> R
    fn parse_function_type(&mut self) -> Result<Type> {
        let start = self.current_span();
        self.expect(TokenKind::Fn)?;
        self.expect(TokenKind::LeftParen)?;
        
        let mut params = Vec::new();
        
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            params.push(self.parse_type()?);
            
            if !self.at(TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightParen)?;
        
        // Parse return type
        self.expect(TokenKind::Arrow)?;
        let ret = Box::new(self.parse_type()?);
        
        Ok(Type::Function {
            params,
            ret,
            span: self.span_from(start),
        })
    }
    
    /// Parse a function type without Fn keyword: (T1, T2) -> R
    fn parse_bare_function_type(&mut self) -> Result<Type> {
        let start = self.current_span();
        self.expect(TokenKind::LeftParen)?;
        
        let mut params = Vec::new();
        
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            params.push(self.parse_type()?);
            
            if !self.at(TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightParen)?;
        
        // Parse return type
        self.expect(TokenKind::Arrow)?;
        let ret = Box::new(self.parse_type()?);
        
        Ok(Type::Function {
            params,
            ret,
            span: self.span_from(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Type;
    use crate::parser::Parser;
    
    #[test]
    fn test_simple_type() {
        let mut parser = Parser::new("Int").unwrap();
        let ty = parser.parse_type().unwrap();
        
        match ty {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "Int");
                assert!(args.is_empty());
            }
            _ => panic!("Expected Named type"),
        }
    }
    
    #[test]
    fn test_generic_type() {
        let mut parser = Parser::new("Array<Int>").unwrap();
        let ty = parser.parse_type().unwrap();
        
        match ty {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "Array");
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Type::Named { name, .. } => assert_eq!(name, "Int"),
                    _ => panic!("Expected Named type argument"),
                }
            }
            _ => panic!("Expected Named type"),
        }
    }
    
    #[test]
    fn test_nested_generics() {
        let mut parser = Parser::new("Map<String, Array<Int>>").unwrap();
        let ty = parser.parse_type().unwrap();
        
        match ty {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "Map");
                assert_eq!(args.len(), 2);
                match &args[0] {
                    Type::Named { name, .. } => assert_eq!(name, "String"),
                    _ => panic!("Expected String type"),
                }
                match &args[1] {
                    Type::Named { name, args, .. } => {
                        assert_eq!(name, "Array");
                        assert_eq!(args.len(), 1);
                    }
                    _ => panic!("Expected Array type"),
                }
            }
            _ => panic!("Expected Named type"),
        }
    }
    
    #[test]
    fn test_function_type() {
        let mut parser = Parser::new("fn(Int, String) -> Bool").unwrap();
        let ty = parser.parse_type().unwrap();
        
        match ty {
            Type::Function { params, ret, .. } => {
                assert_eq!(params.len(), 2);
                match &params[0] {
                    Type::Named { name, .. } => assert_eq!(name, "Int"),
                    _ => panic!("Expected Int type"),
                }
                match &params[1] {
                    Type::Named { name, .. } => assert_eq!(name, "String"),
                    _ => panic!("Expected String type"),
                }
                match ret.as_ref() {
                    Type::Named { name, .. } => assert_eq!(name, "Bool"),
                    _ => panic!("Expected Bool return type"),
                }
            }
            _ => panic!("Expected Function type"),
        }
    }
    
    #[test]
    fn test_higher_order_function_type() {
        let mut parser = Parser::new("fn(Int) -> fn() -> Int").unwrap();
        let ty = parser.parse_type().unwrap();
        
        match ty {
            Type::Function { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                match ret.as_ref() {
                    Type::Function { params, ret, .. } => {
                        assert!(params.is_empty());
                        match ret.as_ref() {
                            Type::Named { name, .. } => assert_eq!(name, "Int"),
                            _ => panic!("Expected Int return type"),
                        }
                    }
                    _ => panic!("Expected Function return type"),
                }
            }
            _ => panic!("Expected Function type"),
        }
    }
}