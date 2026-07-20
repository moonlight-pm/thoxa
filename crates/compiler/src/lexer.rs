//! Lexer for the current Thoxa subset (R0–R9).

use crate::error::{Error, Result};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    Let,
    Mut,
    If,
    Else,
    Match,
    Type,
    Export,
    Extern,
    Import,
    From,
    As,
    True,
    False,
    And,
    Or,
    Not,
    While,
    Loop,
    Break,
    Continue,
    For,
    In,
    Return,
    // Identifiers & literals
    Ident(String),
    Int(i64),
    String(String),
    // Operators / punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LAngle,
    RAngle,
    Arrow,
    Colon,
    Comma,
    Dot,
    /// Exclusive range `..`
    DotDot,
    /// Inclusive range `..=`
    DotDotEq,
    Eq,
    EqEq,
    Ne,
    Le,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    /// Bitwise and `&`
    Amp,
    /// Bitwise or `|`
    Pipe,
    /// Bitwise xor `^`
    Caret,
    /// Bitwise not `~`
    Tilde,
    /// Shift left `<<`
    Shl,
    /// Shift right `>>`
    Shr,
    Question,
    Bang,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token> {
        self.skip_trivia();
        let start = self.pos;
        if self.pos >= self.bytes.len() {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span::new(start, start),
            });
        }

        let b = self.bytes[self.pos];
        match b {
            b'(' => self.simple(TokenKind::LParen, start, 1),
            b')' => self.simple(TokenKind::RParen, start, 1),
            b'{' => self.simple(TokenKind::LBrace, start, 1),
            b'}' => self.simple(TokenKind::RBrace, start, 1),
            b'[' => self.simple(TokenKind::LBracket, start, 1),
            b']' => self.simple(TokenKind::RBracket, start, 1),
            b':' => self.simple(TokenKind::Colon, start, 1),
            b',' => self.simple(TokenKind::Comma, start, 1),
            b'.' if self.peek(1) == Some(b'.') && self.peek(2) == Some(b'=') => {
                self.simple(TokenKind::DotDotEq, start, 3)
            }
            b'.' if self.peek(1) == Some(b'.') => self.simple(TokenKind::DotDot, start, 2),
            b'.' => self.simple(TokenKind::Dot, start, 1),
            b'+' => self.simple(TokenKind::Plus, start, 1),
            b'*' => self.simple(TokenKind::Star, start, 1),
            b'%' => self.simple(TokenKind::Percent, start, 1),
            b'/' => self.simple(TokenKind::Slash, start, 1),
            b'?' => self.simple(TokenKind::Question, start, 1),
            b'&' => self.simple(TokenKind::Amp, start, 1),
            b'|' => self.simple(TokenKind::Pipe, start, 1),
            b'^' => self.simple(TokenKind::Caret, start, 1),
            b'~' => self.simple(TokenKind::Tilde, start, 1),
            b'-' if self.peek(1) == Some(b'>') => self.simple(TokenKind::Arrow, start, 2),
            b'-' => self.simple(TokenKind::Minus, start, 1),
            b'=' if self.peek(1) == Some(b'=') => self.simple(TokenKind::EqEq, start, 2),
            b'=' => self.simple(TokenKind::Eq, start, 1),
            b'!' if self.peek(1) == Some(b'=') => self.simple(TokenKind::Ne, start, 2),
            b'!' => self.simple(TokenKind::Bang, start, 1),
            b'<' if self.peek(1) == Some(b'<') => self.simple(TokenKind::Shl, start, 2),
            b'<' if self.peek(1) == Some(b'=') => self.simple(TokenKind::Le, start, 2),
            b'<' => self.simple(TokenKind::LAngle, start, 1),
            b'>' if self.peek(1) == Some(b'>') => self.simple(TokenKind::Shr, start, 2),
            b'>' if self.peek(1) == Some(b'=') => self.simple(TokenKind::Ge, start, 2),
            b'>' => self.simple(TokenKind::RAngle, start, 1),
            b'"' => self.string(start),
            b if b.is_ascii_digit() => self.number(start),
            // Standalone `_` → Ident("_") (ordinary name). Match patterns special-case it
            // as discard (D66); `_foo` is still not an identifier (alpha-only start).
            b'_' if !self.peek(1).map(is_ident_continue).unwrap_or(false) => {
                self.simple(TokenKind::Ident("_".into()), start, 1)
            }
            b if is_ident_start(b) => self.ident_or_keyword(start),
            _ => Err(Error::at(
                format!(
                    "unexpected character {:?}",
                    self.src[self.pos..].chars().next()
                ),
                Span::new(start, start + 1),
            )),
        }
    }

    fn simple(&mut self, kind: TokenKind, start: usize, len: usize) -> Result<Token> {
        self.pos += len;
        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
        })
    }

    fn skip_trivia(&mut self) {
        loop {
            if self.pos >= self.bytes.len() {
                return;
            }
            let b = self.bytes[self.pos];
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'#' {
                self.pos += 1;
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            return;
        }
    }

    fn string(&mut self, start: usize) -> Result<Token> {
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Ok(Token {
                    kind: TokenKind::String(out),
                    span: Span::new(start, self.pos),
                });
            }
            if b == b'\\' {
                self.pos += 1;
                if self.pos >= self.bytes.len() {
                    return Err(Error::at(
                        "unterminated string escape",
                        Span::new(start, self.pos),
                    ));
                }
                let esc = self.bytes[self.pos];
                out.push(match esc {
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'\\' => '\\',
                    b'"' => '"',
                    b'0' => '\0',
                    other => {
                        return Err(Error::at(
                            format!("unknown escape \\{}", other as char),
                            Span::new(self.pos, self.pos + 1),
                        ));
                    }
                });
                self.pos += 1;
                continue;
            }
            let ch = self.src[self.pos..].chars().next().unwrap();
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err(Error::at("unterminated string", Span::new(start, self.pos)))
    }

    fn number(&mut self, start: usize) -> Result<Token> {
        // Hex: 0x… / 0X…
        if self.bytes[start] == b'0'
            && self
                .peek(1)
                .map(|c| c == b'x' || c == b'X')
                .unwrap_or(false)
        {
            self.pos += 2; // skip 0x
            let hex_start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_hexdigit() {
                self.pos += 1;
            }
            if self.pos == hex_start {
                return Err(Error::at(
                    "expected hex digits after 0x",
                    Span::new(start, self.pos),
                ));
            }
            let text = &self.src[hex_start..self.pos];
            let value = i64::from_str_radix(text, 16).map_err(|_| {
                Error::at(
                    format!("invalid hex integer `0x{text}`"),
                    Span::new(start, self.pos),
                )
            })?;
            return Ok(Token {
                kind: TokenKind::Int(value),
                span: Span::new(start, self.pos),
            });
        }

        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let text = &self.src[start..self.pos];
        let value: i64 = text.parse().map_err(|_| {
            Error::at(
                format!("invalid integer `{text}`"),
                Span::new(start, self.pos),
            )
        })?;
        Ok(Token {
            kind: TokenKind::Int(value),
            span: Span::new(start, self.pos),
        })
    }

    fn ident_or_keyword(&mut self, start: usize) -> Result<Token> {
        while self.pos < self.bytes.len() && is_ident_continue(self.bytes[self.pos]) {
            self.pos += 1;
        }
        let text = &self.src[start..self.pos];
        let kind = match text {
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "match" => TokenKind::Match,
            "type" => TokenKind::Type,
            "export" => TokenKind::Export,
            "extern" => TokenKind::Extern,
            "import" => TokenKind::Import,
            "from" => TokenKind::From,
            "as" => TokenKind::As,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "while" => TokenKind::While,
            "loop" => TokenKind::Loop,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            _ => TokenKind::Ident(text.to_string()),
        };
        Ok(Token {
            kind,
            span: Span::new(start, self.pos),
        })
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_tokens() {
        let src = r#"fn main() -> Int64 { println("Hello, world!") 0 }"#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Fn));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "main"));
    }

    #[test]
    fn operators() {
        let tokens = Lexer::new("a + b == 3 and not false").tokenize().unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Plus));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::EqEq));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::And));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Not));
    }

    #[test]
    fn trial_tokens() {
        let tokens = Lexer::new("fn half(x: Int64) -> Trial<Int64, Int64> { half(x)? }")
            .tokenize()
            .unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Question));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::LAngle));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::RAngle));
    }

    #[test]
    fn type_tokens() {
        let tokens = Lexer::new("type Point(x: Int64) { export fn len() -> Int64 { x } } p.x")
            .tokenize()
            .unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Type));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Export));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Dot));
    }

    #[test]
    fn loop_and_range_tokens() {
        let tokens = Lexer::new("while true { break } for i in 0..=10 { continue } loop { } 1..2")
            .tokenize()
            .unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::While));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Break));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::For));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::In));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::DotDotEq));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Continue));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Loop));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::DotDot));
    }

    #[test]
    fn return_token() {
        let tokens = Lexer::new("fn f() -> Int64 { return 1 }")
            .tokenize()
            .unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Return));
    }

    #[test]
    fn standalone_underscore_ident() {
        let tokens = Lexer::new("Pass(_)").tokenize().unwrap();
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "_")),
            "standalone `_` should lex as Ident(\"_\")"
        );
        // `_foo` is not an identifier (underscore is not is_ident_start).
        assert!(Lexer::new("_foo").tokenize().is_err());
        // Bare underscore alone.
        let one = Lexer::new("_").tokenize().unwrap();
        assert!(matches!(one[0].kind, TokenKind::Ident(ref s) if s == "_"));
    }
}
