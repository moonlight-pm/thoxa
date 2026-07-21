use crate::error::{Error, Result};
use super::{Lexer, Token, TokenKind, Span};

impl Lexer {
    pub(super) fn lex_string(&mut self) -> Result<Token> {
        let start_pos = self.position - 1;  // -1 for opening quote
        let start_line = self.line;
        let start_column = self.column - 1;
        let mut lines = Vec::new();
        let mut current_line = String::new();
        let mut has_newline = false;
        
        // Skip initial newline if present (common pattern for multi-line strings)
        if self.peek() == Some('\n') {
            self.advance();
            has_newline = true;
            // Don't add an empty line - just mark that we have a multiline string
        }
        
        loop {
            match self.peek() {
                None => return Err(Error::Parse("Unterminated string".to_string())),
                Some('"') => {
                    // End of string  
                    self.advance();
                    break;
                }
                Some('\n') => {
                    has_newline = true;
                    lines.push(current_line.clone());
                    current_line.clear();
                    self.advance();
                }
                Some('\\') => {
                    self.advance();
                    match self.peek() {
                        Some('n') => {
                            self.advance();
                            current_line.push('\n');
                        }
                        Some('r') => {
                            self.advance();
                            current_line.push('\r');
                        }
                        Some('t') => {
                            self.advance();
                            current_line.push('\t');
                        }
                        Some('\\') => {
                            self.advance();
                            current_line.push('\\');
                        }
                        Some('"') => {
                            self.advance();
                            current_line.push('"');
                        }
                        Some(ch) => {
                            self.advance();
                            return Err(Error::Parse(format!("Invalid escape sequence: \\{}", ch)));
                        }
                        None => return Err(Error::Parse("Unterminated string escape".to_string())),
                    }
                }
                Some(ch) => {
                    current_line.push(ch);
                    self.advance();
                }
            }
        }
        
        // Determine if this is a multi-line string (has actual newlines)
        let value = if has_newline {
            // Add the last line if not empty
            if !current_line.is_empty() {
                lines.push(current_line);
            }
            // Multi-line string - apply dedenting
            Self::dedent_string(lines)
        } else {
            // Single-line string - use as is
            current_line
        };
        
        let span = Span {
            start: start_pos,
            end: self.position,
            line: start_line,
            column: start_column,
        };
        
        let text = self.input[start_pos..self.position].iter().collect();
        
        Ok(Token {
            kind: TokenKind::StringLiteral(value),
            span,
            text,
        })
    }
    
    /// Dedent a multi-line string by removing common leading whitespace
    fn dedent_string(lines: Vec<String>) -> String {
        if lines.is_empty() {
            return String::new();
        }
        
        // Find minimum indentation (excluding empty lines)
        let min_indent = lines.iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        
        // Remove common indentation from all lines
        let dedented_lines: Vec<String> = lines.iter()
            .map(|line| {
                if line.len() >= min_indent {
                    line[min_indent..].to_string()
                } else {
                    // Empty or whitespace-only line
                    String::new()
                }
            })
            .collect();
        
        // Join lines and trim trailing whitespace
        let result = dedented_lines.join("\n");
        
        // Trim trailing whitespace and newlines
        result.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_strings() {
        let mut lexer = Lexer::new(r#""hello" "world\n" "quote: \"test\"""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected string literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => assert_eq!(s, "world\n"),
            _ => panic!("Expected string literal"),
        }
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => assert_eq!(s, "quote: \"test\""),
            _ => panic!("Expected string literal"),
        }
    }
    
    #[test]
    fn test_multiline_string_basic() {
        let mut lexer = Lexer::new(r#""
            Hello
            World
        ""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => {
                assert_eq!(s, "Hello\nWorld");
            }
            _ => panic!("Expected string literal"),
        }
    }
    
    #[test]
    fn test_multiline_string_with_indentation() {
        let mut lexer = Lexer::new(r#""
            Line 1
                Line 2 with extra indent
            Line 3
        ""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => {
                assert_eq!(s, "Line 1\n    Line 2 with extra indent\nLine 3");
            }
            _ => panic!("Expected string literal"),
        }
    }
    
    #[test]
    fn test_multiline_string_empty_lines() {
        let mut lexer = Lexer::new(r#""
            First line
            
            Third line
        ""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => {
                assert_eq!(s, "First line\n\nThird line");
            }
            _ => panic!("Expected string literal"),
        }
    }
    
    #[test]
    fn test_multiline_string_no_dedent() {
        let mut lexer = Lexer::new(r#""
Line at start
    Indented line
        More indented
""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => {
                assert_eq!(s, "Line at start\n    Indented line\n        More indented");
            }
            _ => panic!("Expected string literal"),
        }
    }
    
    #[test]
    fn test_single_line_string_unchanged() {
        let mut lexer = Lexer::new(r#""This is a single line string""#);
        
        match lexer.next_token().unwrap().kind {
            TokenKind::StringLiteral(s) => {
                assert_eq!(s, "This is a single line string");
            }
            _ => panic!("Expected string literal"),
        }
    }
}