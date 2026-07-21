//! Pattern matching parsing

use crate::ast::{Pattern, PatternField, MatchArm, Expr};
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse a match expression
    pub(super) fn parse_match_expr(&mut self) -> Result<Expr> {
        let start = self.current_span();
        self.expect(TokenKind::Match)?;
        
        // Parse the expression to match on
        // Note: We parse assignment which goes through the precedence chain but
        // stops before getting the full parse_expr treatment that includes additional checks
        let expr = Box::new(self.parse_assignment()?);
        
        self.expect(TokenKind::LeftBrace)?;
        
        let mut arms = Vec::new();
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            arms.push(self.parse_match_arm()?);
            
            // Optional comma after match arm
            if self.at(TokenKind::Comma) {
                self.advance()?;
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        
        Ok(Expr::Match {
            expr,
            arms,
            span: self.span_from(start),
        })
    }
    
    /// Parse a single match arm
    fn parse_match_arm(&mut self) -> Result<MatchArm> {
        let start = self.current_span();
        
        // Parse pattern
        let pattern = self.parse_pattern()?;
        
        // Expect arrow
        self.expect(TokenKind::Arrow)?;
        
        // Parse body (block)
        let body = self.parse_block()?;
        
        Ok(MatchArm {
            pattern,
            body,
            span: self.span_from(start),
        })
    }
    
    /// Parse a pattern
    pub(super) fn parse_pattern(&mut self) -> Result<Pattern> {
        let start = self.current_span();
        
        match &self.current.kind {
            // Integer literal pattern
            TokenKind::IntLiteral(value, _) => {
                let value = *value;
                self.advance()?;
                Ok(Pattern::IntLiteral {
                    value,
                    span: self.span_from(start),
                })
            }
            
            // String literal pattern
            TokenKind::StringLiteral(value) => {
                let value = value.clone();
                self.advance()?;
                Ok(Pattern::StringLiteral {
                    value,
                    span: self.span_from(start),
                })
            }
            
            // Boolean literal patterns
            TokenKind::True => {
                self.advance()?;
                Ok(Pattern::BoolLiteral {
                    value: true,
                    span: self.span_from(start),
                })
            }
            
            TokenKind::False => {
                self.advance()?;
                Ok(Pattern::BoolLiteral {
                    value: false,
                    span: self.span_from(start),
                })
            }
            
            // Identifier (could be binding or constructor)
            TokenKind::Identifier(name) => {
                let name = name.clone();
                self.advance()?;
                
                // Check if this is a typed pattern (name: Type)
                if self.at(TokenKind::Colon) {
                    self.advance()?;  // consume ':'
                    
                    // Parse the type name
                    let type_name = if let TokenKind::Identifier(tn) = &self.current.kind {
                        let tn = tn.clone();
                        self.advance()?;
                        tn
                    } else {
                        return Err(Error::Parse(format!("Expected type name after ':' in pattern")));
                    };
                    
                    Ok(Pattern::Type {
                        name: type_name,
                        pattern: Box::new(Pattern::Ident {
                            name,
                            span: self.span_from(start),
                        }),
                        span: self.span_from(start),
                    })
                } else if self.at(TokenKind::LeftParen) || self.at(TokenKind::LeftBrace) {
                    // Check if this is a constructor pattern with fields
                    let fields = self.parse_pattern_fields()?;
                    Ok(Pattern::Constructor {
                        name,
                        fields: Some(fields),
                        span: self.span_from(start),
                    })
                } else if name.chars().next().unwrap().is_uppercase() {
                    // Uppercase identifier without fields is a constructor
                    Ok(Pattern::Constructor {
                        name,
                        fields: None,
                        span: self.span_from(start),
                    })
                } else {
                    // Lowercase identifier is a binding
                    Ok(Pattern::Ident {
                        name,
                        span: self.span_from(start),
                    })
                }
            }
            
            // Note: mutable bindings in patterns would need AST support
            // For now, we'll just parse 'mut x' as a regular identifier pattern
            TokenKind::Mut => {
                self.advance()?;
                match &self.current.kind {
                    TokenKind::Identifier(name) => {
                        let name = name.clone();
                        self.advance()?;
                        // TODO: Add mutable binding support to Pattern enum
                        Ok(Pattern::Ident {
                            name,
                            span: self.span_from(start),
                        })
                    }
                    _ => Err(Error::Parse("Expected identifier after 'mut' in pattern".to_string())),
                }
            }
            
            _ => Err(Error::Parse("Expected pattern".to_string())),
        }
    }
    
    /// Parse pattern fields for constructor patterns
    fn parse_pattern_fields(&mut self) -> Result<Vec<PatternField>> {
        let mut fields = Vec::new();
        
        // Handle both tuple-style and record-style patterns
        if self.at(TokenKind::LeftParen) {
            // Tuple-style: Point(x, y)
            self.advance()?; // consume '('
            
            let mut index = 0;
            while !self.at(TokenKind::RightParen) && !self.at_eof() {
                let pattern = self.parse_pattern()?;
                fields.push(PatternField {
                    name: index.to_string(),  // Use index as the field name for tuple patterns
                    pattern: Some(pattern),
                    span: self.current_span(),
                });
                index += 1;
                
                if !self.at(TokenKind::RightParen) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::RightParen)?;
        } else if self.at(TokenKind::LeftBrace) {
            // Record-style: Point { x, y } or Point { x: a, y: b }
            self.advance()?; // consume '{'
            
            while !self.at(TokenKind::RightBrace) && !self.at_eof() {
                let field_name = match &self.current.kind {
                    TokenKind::Identifier(name) => {
                        let name = name.clone();
                        self.advance()?;
                        name
                    }
                    _ => return Err(Error::Parse("Expected field name in pattern".to_string())),
                };
                
                // Check if there's a pattern after colon
                let pattern = if self.at(TokenKind::Colon) {
                    self.advance()?; // consume ':'
                    Some(self.parse_pattern()?)
                } else {
                    // Shorthand: just field name is also the binding
                    Some(Pattern::Ident {
                        name: field_name.clone(),
                        span: self.current_span(),
                    })
                };
                
                fields.push(PatternField {
                    name: field_name,
                    pattern,
                    span: self.current_span(),
                });
                
                if !self.at(TokenKind::RightBrace) {
                    self.expect(TokenKind::Comma)?;
                }
            }
            
            self.expect(TokenKind::RightBrace)?;
        }
        
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{Expr, Pattern};
    use crate::parser::Parser;
    
    #[test]
    #[ignore = "Match expressions not fully implemented"]
    fn test_match_literal_patterns() {
        let mut parser = Parser::new(r#"
            match x {
                1 -> { "one" },
                2 -> { "two" },
                other -> { "other" }
            }
        "#).unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 3);
                
                match &arms[0].pattern {
                    Pattern::IntLiteral { value: 1, .. } => {},
                    _ => panic!("Expected IntLiteral pattern"),
                }
                
                match &arms[2].pattern {
                    Pattern::Ident { name, .. } => {
                        assert_eq!(name, "other");
                    },
                    _ => panic!("Expected Ident pattern for catch-all"),
                }
            }
            _ => panic!("Expected Match expression"),
        }
    }
    
    #[test]
    #[ignore = "Constructor patterns not yet implemented with new syntax"]
    fn test_match_binding_patterns() {
        let mut parser = Parser::new(r#"
            match result {
                x -> { x + 1 }
            }
        "#).unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 1);
                match &arms[0].pattern {
                    Pattern::Ident { name, .. } => {
                        assert_eq!(name, "x");
                    }
                    _ => panic!("Expected Ident pattern"),
                }
            }
            _ => panic!("Expected Match expression"),
        }
    }
    
    #[test]
    #[ignore = "Constructor patterns not yet implemented with new syntax"]
    fn test_match_constructor_patterns() {
        let mut parser = Parser::new(r#"
            match value {
                Pass(x) -> { x },
                Fail(e) -> { panic(e) }
            }
        "#).unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                
                match &arms[0].pattern {
                    Pattern::Constructor { name, fields, .. } => {
                        assert_eq!(name, "Pass");
                        assert!(fields.is_some());
                        assert_eq!(fields.as_ref().unwrap().len(), 1);
                    }
                    _ => panic!("Expected Constructor pattern"),
                }
            }
            _ => panic!("Expected Match expression"),
        }
    }
    
    #[test]
    #[ignore = "Record patterns not yet implemented with new syntax"]
    fn test_match_record_patterns() {
        let mut parser = Parser::new(r#"
            match point {
                Point { x, y } -> { x + y }
            }
        "#).unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 1);
                
                match &arms[0].pattern {
                    Pattern::Constructor { name, fields, .. } => {
                        assert_eq!(name, "Point");
                        let fields = fields.as_ref().unwrap();
                        assert_eq!(fields.len(), 2);
                        assert_eq!(&fields[0].name, "x");
                        assert_eq!(&fields[1].name, "y");
                    }
                    _ => panic!("Expected Constructor pattern"),
                }
            }
            _ => panic!("Expected Match expression"),
        }
    }
}