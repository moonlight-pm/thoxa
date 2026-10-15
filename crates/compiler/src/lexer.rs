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
    Where,
    // Identifiers & literals
    Ident(String),
    Int(i64),
    /// Decimal / exponent / `f64`-suffixed float (always `Float64`).
    Float(f64),
    String(String),
    /// Unexpected character kept as a token so JSX raw text can contain `'`,
    /// `·`, etc. The parser errors on these outside tag children.
    Junk(String),
    // Operators / punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LAngle,
    RAngle,
    /// Thin arrow `->` (return types, etc.).
    Arrow,
    /// Nowait `-->` (begin a call; do not wait).
    Nowait,
    /// Fat arrow `=>` (single-expression match arm body).
    FatArrow,
    Colon,
    Comma,
    Dot,
    /// Exclusive range `..`
    DotDot,
    /// Spread `...` (JSX `{...attrs}` / `{...props}`).
    DotDotDot,
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
        let mut errors: Vec<Error> = Vec::new();
        loop {
            match self.next_token() {
                Ok(tok) => {
                    let is_eof = tok.kind == TokenKind::Eof;
                    tokens.push(tok);
                    if is_eof {
                        break;
                    }
                }
                Err(e) => {
                    // Skip one unexpected character and keep scanning (LSP multi-diag).
                    errors.push(e);
                    if self.pos < self.bytes.len() {
                        // Advance one UTF-8 char when possible.
                        let rest = &self.src[self.pos..];
                        if let Some(ch) = rest.chars().next() {
                            self.pos += ch.len_utf8();
                        } else {
                            self.pos += 1;
                        }
                    } else {
                        break;
                    }
                    // Cap runaway garbage so we don't loop forever on bad input.
                    if errors.len() >= 32 {
                        break;
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(Error::multi(errors));
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
            b'.' if self.peek(1) == Some(b'.') && self.peek(2) == Some(b'.') => {
                self.simple(TokenKind::DotDotDot, start, 3)
            }
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
            b'-' if self.peek(1) == Some(b'-') && self.peek(2) == Some(b'>') => {
                self.simple(TokenKind::Nowait, start, 3)
            }
            b'-' if self.peek(1) == Some(b'>') => self.simple(TokenKind::Arrow, start, 2),
            b'-' => self.simple(TokenKind::Minus, start, 1),
            b'=' if self.peek(1) == Some(b'>') => self.simple(TokenKind::FatArrow, start, 2),
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
            // `\#` is a literal `#` (JSX text). Bare `#` remains a line comment.
            b'\\' if self.peek(1) == Some(b'#') => {
                self.pos += 2;
                Ok(Token {
                    kind: TokenKind::Junk("#".into()),
                    span: Span::new(start, self.pos),
                })
            }
            b if b.is_ascii_digit() => self.number(start),
            // Standalone `_` → Ident("_") (ordinary name). Match patterns special-case it
            // as discard (D66); `_foo` is still not an identifier (alpha-only start).
            b'_' if !self.peek(1).map(is_ident_continue).unwrap_or(false) => {
                self.simple(TokenKind::Ident("_".into()), start, 1)
            }
            b if is_ident_start(b) => self.ident_or_keyword(start),
            _ => {
                // One UTF-8 char as Junk — JSX text may include it; elsewhere parse errors.
                let rest = &self.src[self.pos..];
                let ch = rest.chars().next().unwrap_or('\u{FFFD}');
                let len = ch.len_utf8();
                self.pos += len;
                Ok(Token {
                    kind: TokenKind::Junk(ch.to_string()),
                    span: Span::new(start, self.pos),
                })
            }
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

    /// `"…"` may span lines. Newlines are kept (no dedent). Same for `sql"…"`.
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
                // `\xHH` — two hex digits → one byte (needed for VT100 / kilo).
                if esc == b'x' {
                    self.pos += 1;
                    let hi = self.bytes.get(self.pos).copied();
                    let lo = self.bytes.get(self.pos + 1).copied();
                    match (hi, lo) {
                        (Some(h), Some(l)) if h.is_ascii_hexdigit() && l.is_ascii_hexdigit() => {
                            let byte = (hex_nibble(h) << 4) | hex_nibble(l);
                            if byte > 0x7f {
                                // Strings are UTF-8; one high byte alone is not a character.
                                return Err(Error::at(
                                    "\\xHH above \\x7F is not valid in a String (strings are UTF-8); write the character itself, or build Bytes",
                                    Span::new(self.pos.saturating_sub(2), self.pos + 2),
                                ));
                            }
                            out.push(byte as char);
                            self.pos += 2;
                        }
                        _ => {
                            return Err(Error::at(
                                "invalid \\xHH escape (need two hex digits)",
                                Span::new(self.pos.saturating_sub(1), self.pos + 1),
                            ));
                        }
                    }
                    continue;
                }
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

        // Float if we see `.` + digit (so `1..5` stays a range) or an exponent.
        let mut is_float = false;
        if self.peek(0) == Some(b'.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            is_float = true;
        }
        if matches!(self.peek(0), Some(b'e' | b'E')) {
            let exp_ok = self.peek(1).is_some_and(|c| c.is_ascii_digit())
                || (matches!(self.peek(1), Some(b'+' | b'-'))
                    && self.peek(2).is_some_and(|c| c.is_ascii_digit()));
            if exp_ok {
                self.pos += 1;
                if matches!(self.peek(0), Some(b'+' | b'-')) {
                    self.pos += 1;
                }
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                is_float = true;
            }
        }

        // Optional width suffix. `f64` marks Float64; `f32` is not shipped.
        if self.at_word("f64") {
            self.pos += 3;
            is_float = true;
        } else if self.at_word("f32") {
            return Err(Error::at(
                "Float32 is not shipped; use Float64 (literal `1.0` or `1.0f64`)",
                Span::new(start, self.pos + 3),
            ));
        }

        let text = &self.src[start..self.pos];
        if is_float {
            let num = text.strip_suffix("f64").unwrap_or(text);
            let value: f64 = num.parse().map_err(|_| {
                Error::at(
                    format!("invalid float `{text}`"),
                    Span::new(start, self.pos),
                )
            })?;
            if !value.is_finite() {
                return Err(Error::at(
                    format!("float `{text}` is out of range for Float64"),
                    Span::new(start, self.pos),
                ));
            }
            Ok(Token {
                kind: TokenKind::Float(value),
                span: Span::new(start, self.pos),
            })
        } else {
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
    }

    fn at_word(&self, word: &str) -> bool {
        let rest = &self.src[self.pos..];
        rest.starts_with(word)
            && rest
                .as_bytes()
                .get(word.len())
                .map(|b| !is_ident_continue(*b))
                .unwrap_or(true)
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
            "where" => TokenKind::Where,
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

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_tokens() {
        let src = r#"fn main() -> Int64 { 0 }"#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Fn));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "main"));
    }

    #[test]
    fn nowait_is_not_arrow() {
        let tokens = Lexer::new("--> foo() -> Int64").tokenize().unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Nowait));
        assert!(matches!(tokens[1].kind, TokenKind::Ident(ref s) if s == "foo"));
        assert!(matches!(
            tokens.iter().find(|t| matches!(t.kind, TokenKind::Arrow)),
            Some(_)
        ));
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
    fn fat_arrow_token() {
        let tokens = Lexer::new("Pass => 0").tokenize().unwrap();
        assert!(
            tokens.iter().any(|t| t.kind == TokenKind::FatArrow),
            "expected FatArrow token for `=>`"
        );
        assert!(
            !tokens.iter().any(|t| t.kind == TokenKind::Eq),
            "`=>` must not split into Eq + RAngle"
        );
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
        let bad = Lexer::new("_foo").tokenize().unwrap();
        assert!(
            matches!(&bad[0].kind, TokenKind::Junk(s) if s == "_"),
            "{bad:?}"
        );
        // Bare underscore alone.
        let one = Lexer::new("_").tokenize().unwrap();
        assert!(matches!(one[0].kind, TokenKind::Ident(ref s) if s == "_"));
    }

    #[test]
    fn float_literals() {
        let tokens = Lexer::new("1.5 1e2 1.5e-1 3.14f64 1f64").tokenize().unwrap();
        let floats: Vec<f64> = tokens
            .iter()
            .filter_map(|t| match t.kind {
                TokenKind::Float(v) => Some(v),
                _ => None,
            })
            .collect();
        assert_eq!(floats.len(), 5, "{tokens:?}");
        assert!((floats[0] - 1.5).abs() < 1e-15);
        assert!((floats[1] - 100.0).abs() < 1e-15);
        assert!((floats[2] - 0.15).abs() < 1e-15);
        assert!((floats[3] - 3.14).abs() < 1e-15);
        assert!((floats[4] - 1.0).abs() < 1e-15);
    }

    #[test]
    fn range_is_not_a_float() {
        let tokens = Lexer::new("1..5 0..=10").tokenize().unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::DotDot));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::DotDotEq));
        assert!(!tokens.iter().any(|t| matches!(t.kind, TokenKind::Float(_))));
        assert!(matches!(&tokens[0].kind, TokenKind::Int(1)));
        assert!(matches!(&tokens[2].kind, TokenKind::Int(5)));
    }

    #[test]
    fn multiline_string_is_one_token() {
        let tokens = Lexer::new("\"a\nb\"").tokenize().unwrap();
        assert!(
            matches!(&tokens[0].kind, TokenKind::String(s) if s == "a\nb"),
            "{tokens:?}"
        );
    }

    #[test]
    fn float32_suffix_is_rejected() {
        let err = Lexer::new("1.0f32").tokenize().unwrap_err();
        assert!(
            err.message.contains("Float32"),
            "{}",
            err.message
        );
    }

    #[test]
    fn escaped_hash_is_not_a_comment() {
        let tokens = Lexer::new("<h1>Ticket \\#{n}</h1>").tokenize().unwrap();
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Junk(s) if s == "#")),
            "expected Junk(\"#\") for `\\#`: {tokens:?}"
        );
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "n")),
            "`\\#` must not eat `{{n}}`: {tokens:?}"
        );
        assert!(
            tokens
                .iter()
                .any(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "h1")),
            "`\\#` must not eat `</h1>`: {tokens:?}"
        );
    }

    #[test]
    fn hash_still_starts_a_line_comment() {
        let tokens = Lexer::new("a # hidden\nb").tokenize().unwrap();
        let kinds: Vec<&TokenKind> = tokens
            .iter()
            .map(|t| &t.kind)
            .filter(|k| !matches!(k, TokenKind::Eof))
            .collect();
        assert!(
            matches!(kinds.as_slice(), [TokenKind::Ident(a), TokenKind::Ident(b)] if a == "a" && b == "b"),
            "{kinds:?}"
        );
    }
}
