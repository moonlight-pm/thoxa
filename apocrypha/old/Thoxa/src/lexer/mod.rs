use crate::error::{Error, Result};

mod string;
mod char;
mod number;
mod identifier;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    IntLiteral(i64, IntSuffix),
    FloatLiteral(f64, FloatSuffix),
    StringLiteral(String),
    CharLiteral(char),
    
    // Keywords
    Let,
    Mut,
    Fn,
    Type,
    If,
    Else,
    Loop,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    Match,
    Import,
    Export,
    From,
    Interface,
    As,
    
    // Literals
    True,
    False,
    Void,
    
    // Identifiers
    Identifier(String),
    
    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    And,
    Or,
    Not,
    Question,
    Arrow,
    Pipe,
    
    // Delimiters
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    LeftAngle,
    RightAngle,
    Semicolon,
    Comma,
    Dot,
    Colon,
    
    // Special
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntSuffix {
    None,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatSuffix {
    None,
    F32,
    F64,
}

pub struct Lexer {
    pub(super) input: Vec<char>,
    pub(super) position: usize,
    pub(super) line: usize,
    pub(super) column: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            position: 0,
            line: 1,
            column: 1,
        }
    }
    
    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace_and_comments();
        
        if self.is_at_end() {
            return Ok(self.make_token(TokenKind::Eof));
        }
        
        let start_pos = self.position;
        let start_line = self.line;
        let start_column = self.column;
        
        let ch = self.advance();
        
        let kind = match ch {
            // Single-character tokens
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            ':' => TokenKind::Colon,
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '?' => TokenKind::Question,
            
            // Multi-character tokens
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqualEqual
                } else {
                    TokenKind::Equal
                }
            }
            
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LessEqual
                } else {
                    // In a real implementation, we'd need context to distinguish
                    // between Less and LeftAngle for generics
                    TokenKind::Less
                }
            }
            
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GreaterEqual
                } else {
                    // In a real implementation, we'd need context to distinguish
                    // between Greater and RightAngle for generics
                    TokenKind::Greater
                }
            }
            
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::NotEqual
                } else {
                    return Err(Error::Parse("Unexpected character '!'".to_string()));
                }
            }
            
            '|' => TokenKind::Pipe,
            
            // String literals
            '"' => return self.lex_string(),
            
            // Char literals
            '\'' => return self.lex_char(),
            
            // Numbers
            '0'..='9' => return self.lex_number(ch),
            
            // Identifiers and keywords (no leading underscores allowed)
            'a'..='z' | 'A'..='Z' => return self.lex_identifier(ch),
            
            _ => return Err(Error::Parse(format!("Unexpected character: {}", ch))),
        };
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        let text = self.input[start_pos..self.position].iter().collect();
        
        Ok(Token { kind, span, text })
    }
    
    fn advance(&mut self) -> char {
        let ch = self.input[self.position];
        self.position += 1;
        
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        
        ch
    }
    
    fn peek(&self) -> Option<char> {
        if self.is_at_end() {
            None
        } else {
            Some(self.input[self.position])
        }
    }
    
    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }
    
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.advance();
                }
                Some('#') => {
                    // Skip # comment until end of line
                    while self.peek() != Some('\n') && !self.is_at_end() {
                        self.advance();
                    }
                    // If we stopped at a newline, consume it
                    if self.peek() == Some('\n') {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }
    
    fn make_token(&self, kind: TokenKind) -> Token {
        Token {
            kind,
            span: Span {
                start: self.position,
                end: self.position,
                line: self.line,
                column: self.column,
            },
            text: String::new(),
        }
    }
    
    // Helper methods
    pub(super) fn peek_ahead(&self, n: usize) -> Option<char> {
        let pos = self.position + n;
        if pos < self.input.len() {
            Some(self.input[pos])
        } else {
            None
        }
    }
    
    pub(super) fn match_str(&mut self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            if self.peek_ahead(i) != Some(ch) {
                return false;
            }
        }
        
        // Check that the next character is not alphanumeric (to avoid matching "i8" in "i8x")
        if let Some(next_ch) = self.peek_ahead(chars.len()) {
            if next_ch.is_alphanumeric() || next_ch == '_' {
                return false;
            }
        }
        
        // Consume the matched string
        for _ in 0..chars.len() {
            self.advance();
        }
        true
    }
    
    pub(super) fn match_char(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.advance();
            true
        } else {
            false
        }
    }
    
    pub(super) fn extend_span(&self, span: Span) -> Span {
        Span {
            start: span.start,
            end: self.position,
            line: span.line,
            column: span.column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_single_char_tokens() {
        let mut lexer = Lexer::new("(){}[];,.:+-*/%?");
        
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::LeftParen);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::RightParen);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::LeftBrace);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::RightBrace);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::LeftBracket);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::RightBracket);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Semicolon);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Comma);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Dot);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Colon);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Plus);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Minus);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Star);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Slash);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Percent);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Question);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Eof);
    }
    
    #[test]
    fn test_multi_char_operators() {
        let mut lexer = Lexer::new("== != <= >= ->");
        
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::EqualEqual);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::NotEqual);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::LessEqual);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::GreaterEqual);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Arrow);
    }
    
    #[test]
    fn test_comments() {
        let mut lexer = Lexer::new("42 # this is a comment\n  43 # another comment");
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, _) => assert_eq!(val, 42),
            _ => panic!("Expected int literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::IntLiteral(val, _) => assert_eq!(val, 43),
            _ => panic!("Expected int literal"),
        }
    }
    
    #[test]
    fn test_positions() {
        let mut lexer = Lexer::new("let x = 42");
        
        let token = lexer.next_token().unwrap();
        assert_eq!(token.kind, TokenKind::Let);
        assert_eq!(token.span.start, 0);
        assert_eq!(token.span.end, 3);
        assert_eq!(token.span.line, 1);
        assert_eq!(token.span.column, 1);
        
        let token = lexer.next_token().unwrap();
        assert_eq!(token.kind, TokenKind::Identifier("x".to_string()));
        assert_eq!(token.span.start, 4);
        assert_eq!(token.span.end, 5);
        
        let token = lexer.next_token().unwrap();
        assert_eq!(token.kind, TokenKind::Equal);
        
        let token = lexer.next_token().unwrap();
        match token.kind {
            TokenKind::IntLiteral(val, _) => assert_eq!(val, 42),
            _ => panic!("Expected int literal"),
        }
    }
}