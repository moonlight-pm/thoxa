use crate::error::{Error, Result};
use super::{Lexer, Token, TokenKind, Span};

impl Lexer {
    pub(super) fn lex_char(&mut self) -> Result<Token> {
        let start_pos = self.position - 1;
        let start_line = self.line;
        let start_column = self.column - 1;
        
        let ch = match self.peek() {
            None => return Err(Error::Parse("Unterminated character literal".to_string())),
            Some('\'') => return Err(Error::Parse("Empty character literal".to_string())),
            Some('\\') => {
                self.advance();
                match self.peek() {
                    Some('n') => {
                        self.advance();
                        '\n'
                    }
                    Some('r') => {
                        self.advance();
                        '\r'
                    }
                    Some('t') => {
                        self.advance();
                        '\t'
                    }
                    Some('\\') => {
                        self.advance();
                        '\\'
                    }
                    Some('\'') => {
                        self.advance();
                        '\''
                    }
                    Some(ch) => {
                        self.advance();
                        return Err(Error::Parse(format!("Invalid escape sequence: \\{}", ch)));
                    }
                    None => return Err(Error::Parse("Unterminated character escape".to_string())),
                }
            }
            Some(ch) => {
                self.advance();
                ch
            }
        };
        
        if self.peek() != Some('\'') {
            return Err(Error::Parse("Unterminated character literal".to_string()));
        }
        self.advance();
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        let text = self.input[start_pos..self.position].iter().collect();
        
        Ok(Token {
            kind: TokenKind::CharLiteral(ch),
            span,
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chars() {
        let mut lexer = Lexer::new(r"'a' '\n' '\''");
        
        match lexer.next_token().unwrap().kind {
            TokenKind::CharLiteral(c) => assert_eq!(c, 'a'),
            _ => panic!("Expected char literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::CharLiteral(c) => assert_eq!(c, '\n'),
            _ => panic!("Expected char literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::CharLiteral(c) => assert_eq!(c, '\''),
            _ => panic!("Expected char literal"),
        }
    }
}