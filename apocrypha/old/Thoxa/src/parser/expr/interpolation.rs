//! String interpolation parsing

use crate::ast::{Expr, StringSegment};
use crate::error::{Error, Result};
use crate::lexer::Span;
use crate::parser::Parser;

impl Parser {
    /// Parse a string literal, checking for interpolation
    pub(in crate::parser) fn parse_string_literal(&mut self, value: String, span: Span) -> Result<Expr> {
        // Check if the string contains interpolation markers
        if !value.contains('{') {
            // Simple string literal
            return Ok(Expr::StringLiteral { value, span });
        }
        
        // Parse interpolated string
        let segments = self.parse_string_segments(&value, span)?;
        
        Ok(Expr::InterpolatedString { segments, span })
    }
    
    /// Parse string segments with interpolation
    fn parse_string_segments(&mut self, input: &str, span: Span) -> Result<Vec<StringSegment>> {
        let mut segments = Vec::new();
        let mut chars = input.chars().peekable();
        let mut current_text = String::new();
        
        while let Some(ch) = chars.next() {
            match ch {
                '{' => {
                    // Check for escaped brace
                    if chars.peek() == Some(&'{') {
                        chars.next(); // Consume second {
                        current_text.push('{');
                    } else {
                        // End of text segment, start of interpolation
                        if !current_text.is_empty() {
                            segments.push(StringSegment::Text(current_text.clone()));
                            current_text.clear();
                        }
                        
                        // Extract the expression text until '}'
                        let mut expr_text = String::new();
                        let mut brace_count = 1;
                        
                        while let Some(ch) = chars.next() {
                            if ch == '{' {
                                brace_count += 1;
                                expr_text.push(ch);
                            } else if ch == '}' {
                                brace_count -= 1;
                                if brace_count == 0 {
                                    break;
                                }
                                expr_text.push(ch);
                            } else {
                                expr_text.push(ch);
                            }
                        }
                        
                        if brace_count != 0 {
                            return Err(Error::Parse(format!(
                                "Unclosed interpolation in string at {:?}",
                                span
                            )));
                        }
                        
                        // Parse the expression
                        let expr = self.parse_interpolation_expr(&expr_text, span)?;
                        segments.push(StringSegment::Interpolation(Box::new(expr)));
                    }
                }
                '}' => {
                    // Check for escaped brace
                    if chars.peek() == Some(&'}') {
                        chars.next(); // Consume second }
                        current_text.push('}');
                    } else {
                        return Err(Error::Parse(format!(
                            "Unexpected '}}' in string at {:?}",
                            span
                        )));
                    }
                }
                '\\' => {
                    // Handle escape sequences
                    if let Some(next) = chars.next() {
                        match next {
                            'n' => current_text.push('\n'),
                            'r' => current_text.push('\r'),
                            't' => current_text.push('\t'),
                            '\\' => current_text.push('\\'),
                            '"' => current_text.push('"'),
                            _ => {
                                current_text.push('\\');
                                current_text.push(next);
                            }
                        }
                    } else {
                        current_text.push('\\');
                    }
                }
                _ => {
                    current_text.push(ch);
                }
            }
        }
        
        // Add any remaining text
        if !current_text.is_empty() {
            segments.push(StringSegment::Text(current_text));
        }
        
        // If no segments, add an empty text segment
        if segments.is_empty() {
            segments.push(StringSegment::Text(String::new()));
        }
        
        Ok(segments)
    }
    
    /// Parse an expression within string interpolation
    fn parse_interpolation_expr(&mut self, expr_text: &str, span: Span) -> Result<Expr> {
        let trimmed = expr_text.trim();
        
        if trimmed.is_empty() {
            return Err(Error::Parse(format!(
                "Empty interpolation expression at {:?}",
                span
            )));
        }
        
        // Create a new parser for the interpolation expression
        // This allows full expression parsing within {}
        let mut sub_parser = Parser::new(trimmed)?;
        
        // Parse the expression
        let expr = sub_parser.parse_expr()?;
        
        // Ensure we consumed all input
        if !sub_parser.at_eof() {
            return Err(Error::Parse(format!(
                "Unexpected token after interpolation expression at {:?}",
                span
            )));
        }
        
        Ok(expr)
    }
}