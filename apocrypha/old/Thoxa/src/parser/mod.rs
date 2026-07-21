//! The Thoxa parser module
//! 
//! This module is organized into submodules for different parsing concerns:
//! - mod.rs: Core parser infrastructure and coordination
//! - module.rs: Module-level parsing (imports, top-level items)
//! - expr.rs: Expression parsing (literals, operators, etc.)
//! - stmt.rs: Statement parsing (let, return, etc.)
//! - item.rs: Top-level item parsing (functions, types, etc.)
//! - pattern.rs: Pattern matching parsing

mod module;
mod expr;
mod stmt;
mod flow;
mod function;
mod types;
mod pattern;
mod arrays;

use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token, TokenKind, Span};
use std::mem;

/// The Thoxa parser - converts tokens into AST nodes
pub struct Parser {
    /// The lexer providing tokens
    pub(super) lexer: Lexer,
    
    /// Current token being examined
    pub(super) current: Token,
    
    /// Errors collected during parsing
    pub(super) errors: Vec<ParseError>,
    
    /// Whether we're in panic mode (recovering from an error)
    pub(super) panic_mode: bool,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

/// Checkpoint for parser backtracking
#[derive(Debug, Clone)]
pub struct ParserCheckpoint {
    position: usize,
    line: usize,
    column: usize,
    current: Token,
}

impl Parser {
    /// Create a new parser for the given input
    pub fn new(input: &str) -> Result<Self> {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token()?;
        
        Ok(Parser {
            lexer,
            current,
            errors: Vec::new(),
            panic_mode: false,
        })
    }
    
    
    // Token management
    
    /// Check if current token is of given kind
    pub(super) fn at(&self, kind: TokenKind) -> bool {
        mem::discriminant(&self.current.kind) == mem::discriminant(&kind)
    }
    
    /// Check if at end of file
    pub(super) fn at_eof(&self) -> bool {
        matches!(self.current.kind, TokenKind::Eof)
    }
    
    /// Get current token's span
    pub(super) fn current_span(&self) -> Span {
        self.current.span
    }
    
    /// Create span from start to current position
    pub(super) fn span_from(&self, start: Span) -> Span {
        Span {
            start: start.start,
            end: self.current.span.end,
            line: start.line,
            column: start.column,
        }
    }
    
    /// Advance to next token
    pub(super) fn advance(&mut self) -> Result<()> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }
    
    /// Consume token of expected kind or error
    pub(super) fn expect(&mut self, kind: TokenKind) -> Result<Token> {
        if !self.at(kind.clone()) {
            return Err(Error::Parse(format!(
                "Expected {:?}, found {:?}",
                kind, self.current.kind
            )));
        }
        
        let token = self.current.clone();
        self.advance()?;
        Ok(token)
    }
    
    /// Peek at next token without consuming
    pub(super) fn peek(&mut self) -> Result<TokenKind> {
        let saved_pos = self.lexer.position;
        let saved_line = self.lexer.line;
        let saved_col = self.lexer.column;
        
        let next = self.lexer.next_token()?;
        let kind = next.kind;
        
        // Restore lexer position
        self.lexer.position = saved_pos;
        self.lexer.line = saved_line;
        self.lexer.column = saved_col;
        
        Ok(kind)
    }
    
    // Error handling
    
    /// Record an error and enter panic mode
    pub(super) fn error(&mut self, err: Error) {
        if !self.panic_mode {
            self.panic_mode = true;
            
            let message = match err {
                Error::Parse(msg) => msg,
                _ => format!("{:?}", err),
            };
            
            self.errors.push(ParseError {
                message,
                span: self.current_span(),
            });
        }
    }
    
    /// Create a checkpoint to allow backtracking
    pub(super) fn checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            position: self.lexer.position,
            line: self.lexer.line,
            column: self.lexer.column,
            current: self.current.clone(),
        }
    }
    
    /// Restore to a previous checkpoint
    pub(super) fn restore(&mut self, checkpoint: ParserCheckpoint) {
        self.lexer.position = checkpoint.position;
        self.lexer.line = checkpoint.line;
        self.lexer.column = checkpoint.column;
        self.current = checkpoint.current;
    }
    
    /// Synchronize parser state after error
    pub(super) fn synchronize(&mut self) {
        self.panic_mode = false;
        
        // Always advance at least once to avoid infinite loops
        if !self.at_eof() {
            let _ = self.advance();
        }
        
        while !self.at_eof() {
            // Stop at statement boundaries
            if matches!(
                self.current.kind,
                TokenKind::Semicolon | TokenKind::RightBrace
            ) {
                let _ = self.advance();
                return;
            }
            
            // Stop at keywords that start valid top-level items
            if matches!(
                self.current.kind,
                TokenKind::Fn | TokenKind::Type | TokenKind::Interface | 
                TokenKind::Export | TokenKind::Import
            ) {
                return;
            }
            
            // Skip statement keywords that aren't valid at top level
            // These will cause an error when we try to parse them as items
            
            let _ = self.advance();
        }
    }
    
    /// Create error from collected errors
    pub(super) fn make_error(&self) -> Error {
        let messages: Vec<String> = self.errors
            .iter()
            .map(|e| format!("{}: {}", e.span.line, e.message))
            .collect();
            
        Error::Parse(messages.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parser_creation() {
        let parser = Parser::new("42").unwrap();
        assert!(!parser.at_eof());
    }
    
    #[test]
    fn test_empty_module() {
        let mut parser = Parser::new("").unwrap();
        assert!(parser.at_eof());
        
        let module = parser.parse_module().unwrap();
        assert_eq!(module.imports.len(), 0);
        assert_eq!(module.items.len(), 0);
    }
}