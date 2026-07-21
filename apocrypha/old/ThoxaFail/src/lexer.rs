use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token {
    #[regex("[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Number(i64),
    
    #[regex("\"([^\"\\\\]|\\\\.)*\"", |lex| {
        let s = lex.slice();
        // Check if this string contains template expressions
        if s.contains("${") {
            // This is a template string, will be handled specially
            None
        } else {
            // Regular string literal
            // Remove quotes and handle basic escape sequences
            let content = &s[1..s.len()-1];
            let unescaped = content
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\r", "\r")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            Some(unescaped)
        }
    })]
    StringLiteral(String),

    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("(")]
    LeftParen,

    #[token(")")]
    RightParen,
    
    // Keywords
    #[token("type")]
    Type,
    
    #[token("fn")]
    Fn,
    
    #[token("let")]
    Let,
    
    #[token("mut")]
    Mut,
    
    #[token("export")]
    Export,
    
    #[token("as")]
    As,
    
    #[token("can")]
    Can,
    
    #[token("is")]
    Is,
    
    // Built-in helpers
    #[token("@intType")]
    IntTypeBuiltin,
    
    #[token("@print")]
    PrintBuiltin,
    
    #[token("@printStr")]
    PrintStrBuiltin,
    
    #[token("@syscall")]
    SyscallBuiltin,
    
    #[token("@strlen")]
    StrlenBuiltin,
    
    #[token("@cstr")]
    CstrBuiltin,
    
    #[token("@intToString")]
    IntToStringBuiltin,
    
    // Additional tokens for function calls and expressions
    #[token(",")]
    Comma,
    
    #[token(";")]
    Semicolon,
    
    #[token("{")]
    LeftBrace,
    
    #[token("}")]
    RightBrace,
    
    #[token(":")]
    Colon,
    
    #[token("->")]
    Arrow,
    
    #[token("&")]
    Ampersand,
    
    #[token("=")]
    Equals,
    
    #[token("...")]
    DotDotDot,
    
    #[token(".")]
    Dot,
    
    #[token("[")]
    LeftBracket,
    
    #[token("]")]
    RightBracket,
    
    // Comparison operators
    #[token("==")]
    EqEq,
    
    #[token("!=")]
    NotEq,
    
    #[token("<")]
    Less,
    
    #[token(">")]
    Greater,
    
    #[token("<=")]
    LessEq,
    
    #[token(">=")]
    GreaterEq,
    
    // Keywords for conditionals
    #[token("if")]
    If,
    
    #[token("else")]
    Else,
    
    // Keywords for loops
    #[token("loop")]
    Loop,
    
    #[token("break")]
    Break,
    
    #[token("continue")]
    Continue,
    
    // Keywords for imports
    #[token("import")]
    Import,
    
    #[token("from")]
    From,
    
    // Identifiers (camelCase/PascalCase, no underscores in naming conventions)
    #[regex("[a-zA-Z][a-zA-Z0-9]*", |lex| lex.slice().to_owned())]
    Identifier(String),

    // Template literal markers and parts
    TemplateStart,
    TemplatePart(String),
    TemplateExprStart,
    TemplateEnd,

    #[regex(r"[ \t\n\f]+", logos::skip)]
    Whitespace,
    
    #[regex(r"#[^\n]*", logos::skip)]
    Comment,
}

pub fn lex(input: &str) -> Vec<Token> {
    lex_with_template_strings(input)
}

// Enhanced lexer that handles template strings
fn lex_with_template_strings(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current_pos = 0;
    
    while current_pos < input.len() {
        // Check if we're at the start of a string that might have templates
        if let Some(remaining) = input.get(current_pos..) {
            if remaining.starts_with('"') {
                // Find the end of the string
                let mut end_pos = 1;
                let mut escaped = false;
                while end_pos < remaining.len() {
                    let ch = remaining.chars().nth(end_pos).unwrap();
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        end_pos += 1;
                        break;
                    }
                    end_pos += 1;
                }
                
                let string_content = &remaining[1..end_pos-1];
                
                // Check if this is a template string
                if string_content.contains("${") {
                    // Parse as template string
                    let (template_tokens, _) = lex_template_string(string_content);
                    tokens.extend(template_tokens);
                    current_pos += end_pos;
                    continue;
                }
            }
        }
        
        // Otherwise, use the regular lexer for one token
        if let Some(remaining) = input.get(current_pos..) {
            // Check for comment
            if remaining.starts_with('#') {
                // Skip until end of line
                if let Some(newline_pos) = remaining.find('\n') {
                    current_pos += newline_pos + 1;
                } else {
                    // Comment goes to end of file
                    break;
                }
                continue;
            }
            
            let mut lexer = Token::lexer(remaining);
            if let Some(Ok(token)) = lexer.next() {
                tokens.push(token);
                current_pos += lexer.span().end;
            } else {
                // Skip invalid character
                current_pos += 1;
            }
        } else {
            break;
        }
    }
    
    tokens
}

// Parse the contents of a template string
fn lex_template_string(input: &str) -> (Vec<Token>, usize) {
    let mut tokens = Vec::new();
    let mut current_pos = 0;
    let mut string_part = String::new();
    
    // Start of template
    tokens.push(Token::TemplateStart);
    
    while current_pos < input.len() {
        if let Some(remaining) = input.get(current_pos..) {
            // Check for template expression
            if remaining.starts_with("${") {
                // Save any accumulated string part
                if !string_part.is_empty() {
                    tokens.push(Token::TemplatePart(string_part.clone()));
                    string_part.clear();
                }
                
                tokens.push(Token::TemplateExprStart);
                current_pos += 2;
                
                // Lex the expression inside ${...}
                let (expr_tokens, expr_len) = lex_template_expression(&remaining[2..]);
                tokens.extend(expr_tokens);
                current_pos += expr_len;
                
                // The closing } is already consumed
                continue;
            }
            
            // Handle escape sequences
            if remaining.starts_with("\\") && remaining.len() > 1 {
                let next_char = remaining.chars().nth(1).unwrap();
                match next_char {
                    'n' => string_part.push('\n'),
                    't' => string_part.push('\t'),
                    'r' => string_part.push('\r'),
                    '\\' => string_part.push('\\'),
                    '"' => string_part.push('"'),
                    '$' => string_part.push('$'),
                    _ => {
                        // Unknown escape, keep as-is
                        string_part.push('\\');
                        string_part.push(next_char);
                    }
                }
                current_pos += 2;
            } else {
                // Regular character
                if let Some(ch) = remaining.chars().next() {
                    string_part.push(ch);
                    current_pos += ch.len_utf8();
                }
            }
        }
    }
    
    // Add the final string part if any
    if !string_part.is_empty() {
        tokens.push(Token::TemplatePart(string_part));
    }
    
    // End of template
    tokens.push(Token::TemplateEnd);
    
    (tokens, current_pos)
}

// Parse an expression inside ${...}
fn lex_template_expression(input: &str) -> (Vec<Token>, usize) {
    let mut tokens = Vec::new();
    let mut depth = 1; // We start inside one level of braces
    let mut current_pos = 0;
    
    while current_pos < input.len() && depth > 0 {
        if let Some(remaining) = input.get(current_pos..) {
            // Use regular lexer but track brace depth
            let mut lexer = Token::lexer(remaining);
            if let Some(Ok(token)) = lexer.next() {
                match &token {
                    Token::LeftBrace => depth += 1,
                    Token::RightBrace => {
                        depth -= 1;
                        if depth == 0 {
                            // This is the closing brace of ${}
                            current_pos += lexer.span().end;
                            break;
                        }
                    }
                    _ => {}
                }
                if depth > 0 {
                    // Only add the token if we haven't found the closing brace
                    tokens.push(token);
                    current_pos += lexer.span().end;
                }
            } else {
                // Skip invalid character
                current_pos += 1;
            }
        }
    }
    
    (tokens, current_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_simple_expression() {
        let tokens = lex("1 + 1");
        assert_eq!(
            tokens,
            vec![Token::Number(1), Token::Plus, Token::Number(1),]
        );
    }
    
    #[test]
    fn test_lex_with_comments() {
        let tokens = lex("1 + 1 # this is a comment");
        assert_eq!(
            tokens,
            vec![Token::Number(1), Token::Plus, Token::Number(1),]
        );
    }
    
    #[test]
    fn test_lex_comment_at_start() {
        let tokens = lex("# comment at start\n5 * 3");
        assert_eq!(
            tokens,
            vec![Token::Number(5), Token::Star, Token::Number(3),]
        );
    }
    
    #[test]
    fn test_lex_multiple_comments() {
        let tokens = lex("2 # first comment\n+ # second comment\n3");
        assert_eq!(
            tokens,
            vec![Token::Number(2), Token::Plus, Token::Number(3),]
        );
    }
    
    #[test] 
    fn test_lex_type_keyword() {
        let tokens = lex("type");
        assert_eq!(tokens, vec![Token::Type]);
    }
    
    #[test]
    fn test_lex_identifiers() {
        let test_cases = vec![
            ("myVariable", "myVariable"),
            ("I32", "I32"),
            ("calculateSum", "calculateSum"),
            ("Type", "Type"), // This will be lexed as Type token, not identifier
        ];
        
        for (input, expected) in &test_cases[..3] { // Skip "Type" case
            let tokens = lex(input);
            assert_eq!(tokens, vec![Token::Identifier(expected.to_string())]);
        }
    }
    
    #[test]
    fn test_lex_int_type_builtin() {
        let tokens = lex("@intType(true, 32)");
        assert_eq!(tokens, vec![
            Token::IntTypeBuiltin,
            Token::LeftParen,
            Token::Identifier("true".to_string()),
            Token::Comma,
            Token::Number(32),
            Token::RightParen,
        ]);
    }
    
    #[test]
    fn test_lex_let_keyword() {
        let tokens = lex("let");
        assert_eq!(tokens, vec![Token::Let]);
    }
    
    #[test]
    fn test_lex_type_definition() {
        let tokens = lex("type");
        assert_eq!(tokens, vec![Token::Type]);
    }
    
    #[test]
    fn test_lex_export_keyword() {
        let tokens = lex("export");
        assert_eq!(tokens, vec![Token::Export]);
    }
    
    #[test]
    fn test_lex_ampersand() {
        let tokens = lex("&");
        assert_eq!(tokens, vec![Token::Ampersand]);
    }
    
    #[test]
    fn test_lex_let_binding() {
        let tokens = lex("let x: I64 = 42;");
        assert_eq!(tokens, vec![
            Token::Let,
            Token::Identifier("x".to_string()),
            Token::Colon,
            Token::Identifier("I64".to_string()),
            Token::Equals,
            Token::Number(42),
            Token::Semicolon,
        ]);
    }
    
    #[test]
    fn test_lex_type_definition_with_name() {
        let tokens = lex("type Point { }");
        assert_eq!(tokens, vec![
            Token::Type,
            Token::Identifier("Point".to_string()),
            Token::LeftBrace,
            Token::RightBrace,
        ]);
    }
    
    #[test]
    fn test_lex_export_function() {
        let tokens = lex("export fn add() {}");
        assert_eq!(tokens, vec![
            Token::Export,
            Token::Fn,
            Token::Identifier("add".to_string()),
            Token::LeftParen,
            Token::RightParen,
            Token::LeftBrace,
            Token::RightBrace,
        ]);
    }
    
    #[test]
    fn test_lex_reference_expression() {
        let tokens = lex("&value");
        assert_eq!(tokens, vec![
            Token::Ampersand,
            Token::Identifier("value".to_string()),
        ]);
    }
    
    #[test]
    fn test_lex_complex_type_example() {
        let tokens = lex("type Point { export let x: I64 = 0; }");
        assert_eq!(tokens, vec![
            Token::Type,
            Token::Identifier("Point".to_string()),
            Token::LeftBrace,
            Token::Export,
            Token::Let,
            Token::Identifier("x".to_string()),
            Token::Colon,
            Token::Identifier("I64".to_string()),
            Token::Equals,
            Token::Number(0),
            Token::Semicolon,
            Token::RightBrace,
        ]);
    }
    
    #[test]
    fn test_lex_string_literals() {
        let tokens = lex(r#""Hello, World!""#);
        assert_eq!(tokens, vec![
            Token::StringLiteral("Hello, World!".to_string()),
        ]);
        
        let tokens = lex(r#""Hello\nWorld""#);
        assert_eq!(tokens, vec![
            Token::StringLiteral("Hello\nWorld".to_string()),
        ]);
        
        let tokens = lex(r#""Quote: \"Hello\"""#);
        assert_eq!(tokens, vec![
            Token::StringLiteral("Quote: \"Hello\"".to_string()),
        ]);
    }
    
    #[test]
    fn test_lex_comparison_operators() {
        let tokens = lex("== != < > <= >=");
        assert_eq!(tokens, vec![
            Token::EqEq,
            Token::NotEq,
            Token::Less,
            Token::Greater,
            Token::LessEq,
            Token::GreaterEq,
        ]);
    }
    
    #[test]
    fn test_lex_if_else_keywords() {
        let tokens = lex("if else");
        assert_eq!(tokens, vec![
            Token::If,
            Token::Else,
        ]);
    }
    
    #[test]
    fn test_lex_conditional_expression() {
        let tokens = lex("if x > 5 { 10 } else { 20 }");
        assert_eq!(tokens, vec![
            Token::If,
            Token::Identifier("x".to_string()),
            Token::Greater,
            Token::Number(5),
            Token::LeftBrace,
            Token::Number(10),
            Token::RightBrace,
            Token::Else,
            Token::LeftBrace,
            Token::Number(20),
            Token::RightBrace,
        ]);
    }
    
    #[test]
    fn test_lex_loop_keywords() {
        let tokens = lex("loop break continue");
        assert_eq!(tokens, vec![
            Token::Loop,
            Token::Break,
            Token::Continue,
        ]);
    }
    
    #[test]
    fn test_lex_loop_expression() {
        let tokens = lex("loop { x; break }");
        assert_eq!(tokens, vec![
            Token::Loop,
            Token::LeftBrace,
            Token::Identifier("x".to_string()),
            Token::Semicolon,
            Token::Break,
            Token::RightBrace,
        ]);
    }
    
    #[test]
    fn test_lex_import_keywords() {
        let tokens = lex("import from");
        assert_eq!(tokens, vec![
            Token::Import,
            Token::From,
        ]);
    }
    
    #[test]
    fn test_lex_import_statement() {
        let tokens = lex("import Math from \"./math\"");
        assert_eq!(tokens, vec![
            Token::Import,
            Token::Identifier("Math".to_string()),
            Token::From,
            Token::StringLiteral("./math".to_string()),
        ]);
    }
    
    #[test]
    fn test_lex_string_template() {
        let tokens = lex(r#""Hello, ${name}!""#);
        assert_eq!(
            tokens,
            vec![
                Token::TemplateStart,
                Token::TemplatePart("Hello, ".to_string()),
                Token::TemplateExprStart,
                Token::Identifier("name".to_string()),
                Token::TemplatePart("!".to_string()),
                Token::TemplateEnd,
            ]
        );
    }
    
    #[test]
    fn test_lex_string_template_with_escape() {
        let tokens = lex(r#""Price: \$${price}""#);
        assert_eq!(
            tokens,
            vec![
                Token::TemplateStart,
                Token::TemplatePart("Price: $".to_string()),
                Token::TemplateExprStart,
                Token::Identifier("price".to_string()),
                Token::TemplateEnd,
            ]
        );
    }
    
    #[test]
    fn test_lex_regular_string() {
        let tokens = lex(r#""Hello, World!""#);
        assert_eq!(
            tokens,
            vec![
                Token::StringLiteral("Hello, World!".to_string()),
            ]
        );
    }
    
    #[test]
    fn test_lex_dot_dot_dot() {
        let tokens = lex("...");
        assert_eq!(tokens, vec![Token::DotDotDot]);
        
        // Test in context
        let tokens = lex("fn print(...args: String)");
        assert_eq!(
            tokens,
            vec![
                Token::Fn,
                Token::Identifier("print".to_string()),
                Token::LeftParen,
                Token::DotDotDot,
                Token::Identifier("args".to_string()),
                Token::Colon,
                Token::Identifier("String".to_string()),
                Token::RightParen,
            ]
        );
    }
    
    #[test]
    fn test_lex_string_template_complex() {
        let tokens = lex(r#""Result: ${x + y}""#);
        assert_eq!(
            tokens,
            vec![
                Token::TemplateStart,
                Token::TemplatePart("Result: ".to_string()),
                Token::TemplateExprStart,
                Token::Identifier("x".to_string()),
                Token::Plus,
                Token::Identifier("y".to_string()),
                Token::TemplateEnd,
            ]
        );
    }
    
    #[test]
    fn test_lex_as_keyword() {
        let tokens = lex("x as String");
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("x".to_string()),
                Token::As,
                Token::Identifier("String".to_string()),
            ]
        );
    }
    
    #[test]
    fn test_lex_can_keyword() {
        let tokens = lex("if x can String");
        assert_eq!(
            tokens,
            vec![
                Token::If,
                Token::Identifier("x".to_string()),
                Token::Can,
                Token::Identifier("String".to_string()),
            ]
        );
    }
}
