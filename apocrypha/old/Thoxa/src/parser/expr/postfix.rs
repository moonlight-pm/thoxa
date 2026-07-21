//! Postfix expression parsing (function calls, field access)

use crate::ast::Expr;
use crate::error::{Error, Result};
use crate::lexer::TokenKind;
use crate::parser::Parser;

impl Parser {
    /// Parse postfix expressions (calls, field access, etc.)
    /// This handles the loop for all postfix operators on a primary expression
    pub(in crate::parser) fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        
        loop {
            match &self.current.kind {
                TokenKind::Less => {
                    // Could be generic type arguments for a function call
                    // Only if expr is an identifier and we can parse type args
                    if let Expr::Ident { .. } = &expr {
                        // Try to parse as generic type arguments
                        // We need to look ahead to see if this is actually type args
                        let saved_pos = self.lexer.position;
                        let saved_line = self.lexer.line;
                        let saved_col = self.lexer.column;
                        let saved_current = self.current.clone();
                        
                        // Advance past the < token
                        self.advance()?;
                        
                        // Try to parse type arguments (without expecting < again)
                        if let Ok(type_args) = self.try_parse_type_args_after_less() {
                            // Successfully parsed type args, now expect function call
                            if self.at(TokenKind::LeftParen) {
                                expr = self.parse_generic_call(expr, type_args)?;
                                continue;
                            }
                        }
                        
                        // Not generic type args, restore and break (let operator parsing handle <)
                        self.lexer.position = saved_pos;
                        self.lexer.line = saved_line;
                        self.lexer.column = saved_col;
                        self.current = saved_current;
                        break;
                    } else {
                        // Not an identifier, so < is an operator
                        break;
                    }
                }
                TokenKind::LeftParen => {
                    // Function call
                    expr = self.parse_call(expr)?;
                }
                TokenKind::LeftBrace => {
                    // Unit construction (only if expr is an identifier)
                    // But we need to be careful not to consume the { that starts match arms
                    // In a match expression like "match x { ... }", the x is followed by {
                    // but that { starts the match arms, not a unit construction
                    
                    // Check if expr is an identifier
                    let (is_ident, name_clone, span_copy) = if let Expr::Ident { name, span } = &expr {
                        (true, name.clone(), *span)
                    } else {
                        (false, String::new(), self.current_span())
                    };
                    
                    if is_ident {
                        // Check if the { might be for match arms by looking ahead
                        // A unit construction would have "field:" after the {
                        // Match arms would have a pattern (number, identifier, etc) followed by ->
                        let saved_pos = self.lexer.position;
                        let saved_line = self.lexer.line;
                        let saved_col = self.lexer.column;
                        let saved_current = self.current.clone();
                        
                        // Look ahead past the {
                        self.advance()?; // consume {
                        
                        // Check if this looks like a unit construction or match arms
                        let is_unit_construction = if self.at_eof() || self.at(TokenKind::RightBrace) {
                            // Empty braces - could be either, assume unit for now
                            true
                        } else if let TokenKind::Identifier(id) = &self.current.kind {
                            // Look for : after identifier (unit field) or -> (match arm)
                            // BUT we need to look further to distinguish:
                            // - Unit field: `name: expr,` or `name: expr }`
                            // - Typed pattern: `name: Type ->` or `name: Type |`
                            let _id_str = id.clone();
                            let next = self.peek()?;
                            
                            if matches!(next, TokenKind::Colon) {
                                // We have `identifier:` - need to look further
                                // Save state for deeper lookahead
                                let saved_pos2 = self.lexer.position;
                                let saved_line2 = self.lexer.line;
                                let saved_col2 = self.lexer.column;
                                let saved_current2 = self.current.clone();
                                
                                // Skip past identifier and colon
                                self.advance()?; // skip identifier
                                self.advance()?; // skip colon
                                
                                // Now check what comes after the colon
                                // If it's a type name followed by ->, it's a match pattern
                                let is_pattern = if let TokenKind::Identifier(_) = &self.current.kind {
                                    // Could be a type name, check what follows
                                    let after_type = self.peek()?;
                                    matches!(after_type, TokenKind::Arrow | TokenKind::Pipe | TokenKind::Comma)
                                } else {
                                    false
                                };
                                
                                // Restore position
                                self.lexer.position = saved_pos2;
                                self.lexer.line = saved_line2;
                                self.lexer.column = saved_col2;
                                self.current = saved_current2;
                                
                                !is_pattern // If it's a pattern, it's NOT unit construction
                            } else {
                                false // No colon, not a unit construction
                            }
                        } else {
                            // Starts with something else (like a number) - probably match arms
                            false
                        };
                        
                        // Restore position
                        self.lexer.position = saved_pos;
                        self.lexer.line = saved_line;
                        self.lexer.column = saved_col;
                        self.current = saved_current;
                        
                        if is_unit_construction {
                            expr = self.parse_unit_construction(name_clone, span_copy)?;
                        } else {
                            break;
                        }
                    } else {
                        break;  // Not a unit construction
                    }
                }
                TokenKind::Dot => {
                    // Field access
                    expr = self.parse_field_access(expr)?;
                }
                TokenKind::LeftBracket => {
                    // Array indexing
                    expr = self.parse_index(expr)?;
                }
                TokenKind::Question => {
                    // Try operator
                    expr = self.parse_try(expr)?;
                }
                _ => break,
            }
        }
        
        Ok(expr)
    }
    
    /// Parse a function call: expr(arg1, arg2, ...)
    /// Try to parse generic type arguments <T1, T2, ...>
    fn try_parse_type_args(&mut self) -> Result<Vec<crate::ast::Type>> {
        self.expect(TokenKind::Less)?;
        self.try_parse_type_args_after_less()
    }
    
    fn try_parse_type_args_after_less(&mut self) -> Result<Vec<crate::ast::Type>> {
        let mut type_args = Vec::new();
        
        while !self.at(TokenKind::Greater) && !self.at_eof() {
            type_args.push(self.parse_type()?);
            
            if !self.at(TokenKind::Greater) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::Greater)?;
        Ok(type_args)
    }
    
    /// Parse a generic function call with type arguments
    fn parse_generic_call(&mut self, func: Expr, type_args: Vec<crate::ast::Type>) -> Result<Expr> {
        let start_span = func.span();
        
        // Now parse the regular function call arguments
        self.expect(TokenKind::LeftParen)?;
        
        let mut args = Vec::new();
        
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            args.push(self.parse_expr()?);
            
            if !self.at(TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightParen)?;
        let span = self.span_from(start_span);
        
        Ok(Expr::Call {
            func: Box::new(func),
            type_args,
            args,
            span,
        })
    }
    
    fn parse_call(&mut self, func: Expr) -> Result<Expr> {
        let start_span = func.span();
        
        // Check if this is a Pass/Fail/Has/Not constructor
        if let Expr::Ident { name, .. } = &func {
            if name == "Pass" || name == "Fail" || name == "Has" || name == "Not" {
                // Parse as enum variant constructor
                self.expect(TokenKind::LeftParen)?;
                
                // Parse the single argument (Trial variants have exactly one value)
                let value = if self.at(TokenKind::RightParen) {
                    None
                } else {
                    Some(Box::new(self.parse_expr()?))
                };
                
                let _end_span = self.expect(TokenKind::RightParen)?;
                let span = start_span;  // Use the start span for now
                
                return Ok(Expr::EnumVariant {
                    enum_name: None,  // Built-in Trial
                    variant: name.clone(),
                    value,
                    span,
                });
            }
        }
        
        // Regular function call
        self.expect(TokenKind::LeftParen)?;
        
        let mut args = Vec::new();
        
        // Parse arguments
        while !self.at(TokenKind::RightParen) && !self.at_eof() {
            args.push(self.parse_expr()?);
            
            if !self.at(TokenKind::RightParen) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightParen)?;
        let span = self.span_from(start_span);
        
        Ok(Expr::Call {
            func: Box::new(func),
            type_args: Vec::new(),  // No type args for regular calls
            args,
            span,
        })
    }
    
    /// Parse field access: expr.field
    fn parse_field_access(&mut self, object: Expr) -> Result<Expr> {
        let start_span = object.span();
        self.expect(TokenKind::Dot)?;
        
        match &self.current.kind {
            TokenKind::Identifier(name) => {
                let field_name = name.clone();
                self.advance()?;
                
                let span = self.span_from(start_span);
                Ok(Expr::FieldAccess {
                    object: Box::new(object),
                    field: field_name,
                    span,
                })
            }
            TokenKind::IntLiteral(value, _) => {
                // Tuple access like tuple.0, tuple.1
                let index = *value as usize;
                self.advance()?;
                
                let span = self.span_from(start_span);
                Ok(Expr::TupleAccess {
                    tuple: Box::new(object),
                    index,
                    span,
                })
            }
            _ => {
                Err(Error::Parse("Expected field name or tuple index after '.'".to_string()))
            }
        }
    }
    
    /// Parse try operator: expr?
    fn parse_try(&mut self, expr: Expr) -> Result<Expr> {
        let start_span = expr.span();
        self.expect(TokenKind::Question)?;
        
        let span = self.span_from(start_span);
        
        Ok(Expr::Try {
            expr: Box::new(expr),
            span,
        })
    }
    
    /// Parse unit construction: Person { name: "Alice", age: 30 }
    fn parse_unit_construction(&mut self, name: String, name_span: crate::lexer::Span) -> Result<Expr> {
        let start_span = name_span;
        self.expect(TokenKind::LeftBrace)?;
        
        let mut fields = Vec::new();
        
        while !self.at(TokenKind::RightBrace) && !self.at_eof() {
            // Parse field name
            let field_name = match &self.current.kind {
                TokenKind::Identifier(n) => {
                    let name = n.clone();
                    self.advance()?;
                    name
                }
                _ => return Err(Error::Parse("Expected field name in type construction".to_string())),
            };
            
            self.expect(TokenKind::Colon)?;
            
            // Parse field value
            let field_value = self.parse_expr()?;
            
            fields.push((field_name, field_value));
            
            // Check for comma or end of fields
            if !self.at(TokenKind::RightBrace) {
                self.expect(TokenKind::Comma)?;
            }
        }
        
        self.expect(TokenKind::RightBrace)?;
        let span = self.span_from(start_span);
        
        Ok(Expr::UnitConstruction {
            name,
            fields,
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::Expr;
    use crate::parser::Parser;
    
    #[test]
    fn test_function_call_no_args() {
        let mut parser = Parser::new("foo()").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Call { func, args, .. } => {
                match func.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "foo"),
                    _ => panic!("Expected identifier"),
                }
                assert_eq!(args.len(), 0);
            }
            _ => panic!("Expected Call"),
        }
    }
    
    #[test]
    fn test_function_call_with_args() {
        let mut parser = Parser::new("add(1, 2, 3)").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Call { func, args, .. } => {
                match func.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "add"),
                    _ => panic!("Expected identifier"),
                }
                assert_eq!(args.len(), 3);
                // Check arguments
                match &args[0] {
                    Expr::IntLiteral { value: 1, .. } => {},
                    _ => panic!("Expected 1"),
                }
                match &args[1] {
                    Expr::IntLiteral { value: 2, .. } => {},
                    _ => panic!("Expected 2"),
                }
                match &args[2] {
                    Expr::IntLiteral { value: 3, .. } => {},
                    _ => panic!("Expected 3"),
                }
            }
            _ => panic!("Expected Call"),
        }
    }
    
    #[test]
    fn test_field_access() {
        let mut parser = Parser::new("obj.field").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::FieldAccess { object, field, .. } => {
                match object.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "obj"),
                    _ => panic!("Expected identifier"),
                }
                assert_eq!(field, "field");
            }
            _ => panic!("Expected FieldAccess"),
        }
    }
    
    #[test]
    fn test_chained_field_access() {
        let mut parser = Parser::new("a.b.c").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::FieldAccess { object, field, .. } => {
                assert_eq!(field, "c");
                match object.as_ref() {
                    Expr::FieldAccess { object, field, .. } => {
                        assert_eq!(field, "b");
                        match object.as_ref() {
                            Expr::Ident { name, .. } => assert_eq!(name, "a"),
                            _ => panic!("Expected identifier"),
                        }
                    }
                    _ => panic!("Expected nested FieldAccess"),
                }
            }
            _ => panic!("Expected FieldAccess"),
        }
    }
    
    #[test]
    fn test_method_call_chain() {
        let mut parser = Parser::new("obj.method().field").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::FieldAccess { object, field, .. } => {
                assert_eq!(field, "field");
                match object.as_ref() {
                    Expr::Call { func, args, .. } => {
                        assert_eq!(args.len(), 0);
                        match func.as_ref() {
                            Expr::FieldAccess { object, field, .. } => {
                                assert_eq!(field, "method");
                                match object.as_ref() {
                                    Expr::Ident { name, .. } => assert_eq!(name, "obj"),
                                    _ => panic!("Expected identifier"),
                                }
                            }
                            _ => panic!("Expected FieldAccess"),
                        }
                    }
                    _ => panic!("Expected Call"),
                }
            }
            _ => panic!("Expected FieldAccess"),
        }
    }
    
    #[test]
    fn test_nested_calls() {
        let mut parser = Parser::new("f(g(x))").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Call { func, args, .. } => {
                match func.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "f"),
                    _ => panic!("Expected identifier"),
                }
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Expr::Call { func, args, .. } => {
                        match func.as_ref() {
                            Expr::Ident { name, .. } => assert_eq!(name, "g"),
                            _ => panic!("Expected identifier"),
                        }
                        assert_eq!(args.len(), 1);
                        match &args[0] {
                            Expr::Ident { name, .. } => assert_eq!(name, "x"),
                            _ => panic!("Expected identifier"),
                        }
                    }
                    _ => panic!("Expected nested Call"),
                }
            }
            _ => panic!("Expected Call"),
        }
    }
    
    #[test]
    fn test_precedence_with_calls() {
        // Function calls should bind tighter than binary operators
        let mut parser = Parser::new("2 + f(3) * 4").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Binary { op: crate::ast::BinaryOp::Add, left, right, .. } => {
                match left.as_ref() {
                    Expr::IntLiteral { value: 2, .. } => {},
                    _ => panic!("Expected 2"),
                }
                match right.as_ref() {
                    Expr::Binary { op: crate::ast::BinaryOp::Mul, left, right, .. } => {
                        match left.as_ref() {
                            Expr::Call { func, args, .. } => {
                                match func.as_ref() {
                                    Expr::Ident { name, .. } => assert_eq!(name, "f"),
                                    _ => panic!("Expected f"),
                                }
                                assert_eq!(args.len(), 1);
                            }
                            _ => panic!("Expected Call"),
                        }
                        match right.as_ref() {
                            Expr::IntLiteral { value: 4, .. } => {},
                            _ => panic!("Expected 4"),
                        }
                    }
                    _ => panic!("Expected multiplication"),
                }
            }
            _ => panic!("Expected addition"),
        }
    }
    
    #[test]
    fn test_try_operator() {
        let mut parser = Parser::new("result?").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Try { expr, .. } => {
                match expr.as_ref() {
                    Expr::Ident { name, .. } => assert_eq!(name, "result"),
                    _ => panic!("Expected identifier"),
                }
            }
            _ => panic!("Expected Try expression"),
        }
    }
    
    #[test]
    fn test_chained_try() {
        let mut parser = Parser::new("getValue()?.field?").unwrap();
        let expr = parser.parse_expr().unwrap();
        
        match expr {
            Expr::Try { expr, .. } => {
                match expr.as_ref() {
                    Expr::FieldAccess { object, field, .. } => {
                        assert_eq!(field, "field");
                        match object.as_ref() {
                            Expr::Try { expr, .. } => {
                                match expr.as_ref() {
                                    Expr::Call { func, args, .. } => {
                                        assert_eq!(args.len(), 0);
                                        match func.as_ref() {
                                            Expr::Ident { name, .. } => assert_eq!(name, "getValue"),
                                            _ => panic!("Expected identifier"),
                                        }
                                    }
                                    _ => panic!("Expected Call"),
                                }
                            }
                            _ => panic!("Expected Try on call"),
                        }
                    }
                    _ => panic!("Expected FieldAccess"),
                }
            }
            _ => panic!("Expected Try expression"),
        }
    }
}