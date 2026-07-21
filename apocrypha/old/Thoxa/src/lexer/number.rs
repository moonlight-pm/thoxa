use crate::error::{Error, Result};
use super::{Lexer, Token, TokenKind, Span, IntSuffix, FloatSuffix};

impl Lexer {
    pub(super) fn lex_number(&mut self, first_digit: char) -> Result<Token> {
        let start_pos = self.position - 1;
        let start_line = self.line;
        let start_column = self.column - 1;
        
        // Check for hex or binary literals
        if first_digit == '0' {
            match self.peek() {
                Some('x') | Some('X') => {
                    self.advance();
                    return self.lex_hex_number(start_pos, start_line, start_column);
                }
                Some('b') | Some('B') => {
                    self.advance();
                    return self.lex_binary_number(start_pos, start_line, start_column);
                }
                _ => {}
            }
        }
        
        // Collect integer part
        let mut number_str = String::new();
        number_str.push(first_digit);
        
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '_' {
                if ch != '_' {
                    number_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }
        
        // Check for float
        let mut is_float = false;
        if self.peek() == Some('.') && self.peek_ahead(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            is_float = true;
            number_str.push('.');
            self.advance();
            
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    if ch != '_' {
                        number_str.push(ch);
                    }
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        // Check for exponent
        if let Some('e') | Some('E') = self.peek() {
            is_float = true;
            number_str.push('e');
            self.advance();
            
            if let Some('+') | Some('-') = self.peek() {
                number_str.push(self.advance());
            }
            
            let has_exp_digits = self.peek().map(|c| c.is_ascii_digit()).unwrap_or(false);
            if !has_exp_digits {
                return Err(Error::Parse("Invalid number: expected digits after exponent".to_string()));
            }
            
            while let Some(ch) = self.peek() {
                if ch.is_ascii_digit() || ch == '_' {
                    if ch != '_' {
                        number_str.push(ch);
                    }
                    self.advance();
                } else {
                    break;
                }
            }
        }
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        let _text: String = self.input[start_pos..self.position].iter().collect();
        
        // Parse suffix or create token
        if is_float {
            let suffix = self.parse_float_suffix();
            let value = number_str.parse::<f64>()
                .map_err(|_| Error::Parse(format!("Invalid float literal: {}", number_str)))?;
            
            Ok(Token {
                kind: TokenKind::FloatLiteral(value, suffix),
                span: self.extend_span(span),
                text: self.input[start_pos..self.position].iter().collect(),
            })
        } else {
            let suffix = self.parse_int_suffix();
            let value = number_str.parse::<i64>()
                .map_err(|_| Error::Parse(format!("Invalid integer literal: {}", number_str)))?;
            
            Ok(Token {
                kind: TokenKind::IntLiteral(value, suffix),
                span: self.extend_span(span),
                text: self.input[start_pos..self.position].iter().collect(),
            })
        }
    }
    
    pub(super) fn lex_hex_number(&mut self, start_pos: usize, start_line: usize, start_column: usize) -> Result<Token> {
        let mut number_str = String::new();
        
        let has_digits = self.peek().map(|c| c.is_ascii_hexdigit()).unwrap_or(false);
        if !has_digits {
            return Err(Error::Parse("Invalid hex literal: expected hex digits after 0x".to_string()));
        }
        
        while let Some(ch) = self.peek() {
            if ch.is_ascii_hexdigit() || ch == '_' {
                if ch != '_' {
                    number_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }
        
        let value = i64::from_str_radix(&number_str, 16)
            .map_err(|_| Error::Parse(format!("Invalid hex literal: 0x{}", number_str)))?;
        
        let suffix = self.parse_int_suffix();
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        Ok(Token {
            kind: TokenKind::IntLiteral(value, suffix),
            span,
            text: self.input[start_pos..self.position].iter().collect(),
        })
    }
    
    pub(super) fn lex_binary_number(&mut self, start_pos: usize, start_line: usize, start_column: usize) -> Result<Token> {
        let mut number_str = String::new();
        
        let has_digits = self.peek().map(|c| c == '0' || c == '1').unwrap_or(false);
        if !has_digits {
            return Err(Error::Parse("Invalid binary literal: expected binary digits after 0b".to_string()));
        }
        
        while let Some(ch) = self.peek() {
            if ch == '0' || ch == '1' || ch == '_' {
                if ch != '_' {
                    number_str.push(ch);
                }
                self.advance();
            } else {
                break;
            }
        }
        
        let value = i64::from_str_radix(&number_str, 2)
            .map_err(|_| Error::Parse(format!("Invalid binary literal: 0b{}", number_str)))?;
        
        let suffix = self.parse_int_suffix();
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        Ok(Token {
            kind: TokenKind::IntLiteral(value, suffix),
            span,
            text: self.input[start_pos..self.position].iter().collect(),
        })
    }
    
    pub(super) fn parse_int_suffix(&mut self) -> IntSuffix {
        let pos = self.position;
        
        // Try to match suffix
        if self.match_str("i8") {
            IntSuffix::I8
        } else if self.match_str("i16") {
            IntSuffix::I16
        } else if self.match_str("i32") {
            IntSuffix::I32
        } else if self.match_str("i64") {
            IntSuffix::I64
        } else if self.match_str("u8") {
            IntSuffix::U8
        } else if self.match_str("u16") {
            IntSuffix::U16
        } else if self.match_str("u32") {
            IntSuffix::U32
        } else if self.match_str("u64") {
            IntSuffix::U64
        } else if self.match_char('u') {
            IntSuffix::U64  // Default unsigned
        } else {
            self.position = pos;  // Reset if no match
            IntSuffix::None
        }
    }
    
    pub(super) fn parse_float_suffix(&mut self) -> FloatSuffix {
        if self.match_char('f') {
            FloatSuffix::F32
        } else if self.match_char('d') {
            FloatSuffix::F64
        } else {
            FloatSuffix::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_integers() {
        let mut lexer = Lexer::new("42 100_000 0xFF 0b1010 123i32 456u64");
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 42);
                assert_eq!(suffix, IntSuffix::None);
            }
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 100000);
                assert_eq!(suffix, IntSuffix::None);
            }
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 255);
                assert_eq!(suffix, IntSuffix::None);
            }
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 10);
                assert_eq!(suffix, IntSuffix::None);
            }
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 123);
                assert_eq!(suffix, IntSuffix::I32);
            }
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, suffix) => {
                assert_eq!(val, 456);
                assert_eq!(suffix, IntSuffix::U64);
            }
            _ => panic!("Expected int literal"),
        }
    }
    
    #[test]
    fn test_floats() {
        let mut lexer = Lexer::new("3.14 10.0 1e10 2.5e-3 3.14f 2.71d");
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 3.14);
                assert_eq!(suffix, FloatSuffix::None);
            }
            _ => panic!("Expected float literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 10.0);
                assert_eq!(suffix, FloatSuffix::None);
            }
            _ => panic!("Expected float literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 1e10);
                assert_eq!(suffix, FloatSuffix::None);
            }
            _ => panic!("Expected float literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 2.5e-3);
                assert_eq!(suffix, FloatSuffix::None);
            }
            _ => panic!("Expected float literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 3.14);
                assert_eq!(suffix, FloatSuffix::F32);
            }
            _ => panic!("Expected float literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::FloatLiteral(val, suffix) => {
                assert_eq!(val, 2.71);
                assert_eq!(suffix, FloatSuffix::F64);
            }
            _ => panic!("Expected float literal"),
        }
    }
}