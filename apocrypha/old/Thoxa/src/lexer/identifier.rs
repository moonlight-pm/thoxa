use crate::error::{Error, Result};
use super::{Lexer, Token, TokenKind, Span};

impl Lexer {
    pub(super) fn lex_identifier(&mut self, first_char: char) -> Result<Token> {
        let start_pos = self.position - 1;
        let start_line = self.line;
        let start_column = self.column - 1;
        
        let mut ident = String::new();
        ident.push(first_char);
        
        // Only allow letters and digits in identifiers (no underscores)
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        
        // Validate identifier follows camelCase or PascalCase
        if !is_valid_identifier(&ident) {
            return Err(Error::Parse(format!(
                "Invalid identifier '{}'. Identifiers must use camelCase or PascalCase (no underscores)",
                ident
            )));
        }
        
        let kind = match ident.as_str() {
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "fn" => TokenKind::Fn,
            "type" => TokenKind::Type,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "loop" => TokenKind::Loop,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "return" => TokenKind::Return,
            "match" => TokenKind::Match,
            "import" => TokenKind::Import,
            "export" => TokenKind::Export,
            "from" => TokenKind::From,
            "interface" => TokenKind::Interface,
            "as" => TokenKind::As,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "Void" => TokenKind::Void,
            _ => TokenKind::Identifier(ident.clone()),
        };
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        Ok(Token {
            kind,
            span,
            text: ident,
        })
    }
}

// Validate that an identifier follows camelCase or PascalCase conventions
pub(super) fn is_valid_identifier(ident: &str) -> bool {
    // Keywords are always valid
    match ident {
        "let" | "mut" | "fn" | "type" | "if" | "else" | "loop" | "for" | "in" | "while" 
        | "break" | "continue" | "return" | "match" | "import" | "export" | "from" 
        | "interface" | "unit" | "as" | "and" | "or" | "not" | "true" | "false" | "Void" => return true,
        _ => {}
    }
    
    // Empty string is invalid
    if ident.is_empty() {
        return false;
    }
    
    let chars: Vec<char> = ident.chars().collect();
    
    // First character must be a letter
    if !chars[0].is_alphabetic() {
        return false;
    }
    
    // No underscores allowed
    if ident.contains('_') {
        return false;
    }
    
    // Must start with lowercase (camelCase) or uppercase (PascalCase)
    // All characters must be alphanumeric
    ident.chars().all(|c| c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_keywords() {
        let mut lexer = Lexer::new("let mut fn type if else and or not true false Void");
        
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Let);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Mut);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Fn);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Type);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::If);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Else);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::And);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Or);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Not);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::True);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::False);
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Void);
    }
    
    #[test]
    fn test_identifiers() {
        let mut lexer = Lexer::new("hello world123 camelCase PascalCase Value Pass Fail");
        
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("hello".to_string()));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("world123".to_string()));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("camelCase".to_string()));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("PascalCase".to_string()));
        // Built-in constructors are identifiers
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("Value".to_string()));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("Pass".to_string()));
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("Fail".to_string()));
    }
    
    #[test]
    fn test_invalid_identifiers() {
        // Test underscores are rejected
        let mut lexer = Lexer::new("_underscore");
        assert!(lexer.next_token().is_err());
        
        // snake_case will be lexed as "snake" followed by an error on '_'
        let mut lexer = Lexer::new("snake_case");
        assert_eq!(lexer.next_token().unwrap().kind, TokenKind::Identifier("snake".to_string()));
        assert!(lexer.next_token().is_err()); // Should fail on underscore
    }
}