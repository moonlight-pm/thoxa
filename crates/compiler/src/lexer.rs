//! Lexer for a minimal Thoxa subset (R0+).

use crate::error::{Error, Result};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Fn,
    // Identifiers & literals
    Ident(String),
    Int(i64),
    String(String),
    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Arrow, // ->
    Colon,
    Comma,
    // End
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
            b'(' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::LParen,
                    span: Span::new(start, self.pos),
                })
            }
            b')' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::RParen,
                    span: Span::new(start, self.pos),
                })
            }
            b'{' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::LBrace,
                    span: Span::new(start, self.pos),
                })
            }
            b'}' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::RBrace,
                    span: Span::new(start, self.pos),
                })
            }
            b':' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::Colon,
                    span: Span::new(start, self.pos),
                })
            }
            b',' => {
                self.pos += 1;
                Ok(Token {
                    kind: TokenKind::Comma,
                    span: Span::new(start, self.pos),
                })
            }
            b'-' if self.peek(1) == Some(b'>') => {
                self.pos += 2;
                Ok(Token {
                    kind: TokenKind::Arrow,
                    span: Span::new(start, self.pos),
                })
            }
            b'"' => self.string(start),
            b if b.is_ascii_digit() => self.number(start),
            b if is_ident_start(b) => self.ident_or_keyword(start),
            _ => Err(Error::at(
                format!("unexpected character {:?}", self.src[self.pos..].chars().next()),
                Span::new(start, start + 1),
            )),
        }
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
            // Line comment: #
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
        self.pos += 1; // opening "
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
                    return Err(Error::at("unterminated string escape", Span::new(start, self.pos)));
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
            // UTF-8: take full char
            let ch = self.src[self.pos..].chars().next().unwrap();
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err(Error::at("unterminated string", Span::new(start, self.pos)))
    }

    fn number(&mut self, start: usize) -> Result<Token> {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Optional underscores already not handled; R0 is plain digits
        let text = &self.src[start..self.pos];
        let value: i64 = text
            .parse()
            .map_err(|_| Error::at(format!("invalid integer `{text}`"), Span::new(start, self.pos)))?;
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
}
