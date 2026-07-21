use chumsky::prelude::*;
use chumsky::input::ValueInput;
use crate::ast::{Expr, BinaryOp};
#[cfg(test)]
use crate::ast::TypeMember;
use crate::lexer::Token;
use crate::symbols::SymbolTable;

use super::symbols::populate_symbol_table;
use super::types::parse_type_definition;
use super::expressions::parse_complex_expressions;

fn parser<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>>
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    let expr = recursive(|expr| {
        // Type definition parsing delegated to types module
        let module_def = parse_type_definition(expr.clone());
        
        // Complex expression parsing delegated to expressions module
        let complex_expressions = parse_complex_expressions(expr.clone());

        let number = select! {
            Token::Number(n) => Expr::Number(n),
        };
        
        let string_literal = select! {
            Token::StringLiteral(s) => Expr::StringLiteral(s),
        };
        
        // Template literal: "Hello, ${name}!"
        // The lexer emits: TemplateStart, [TemplatePart|TemplateExprStart|tokens]*, TemplateEnd
        let template_literal = just(Token::TemplateStart)
            .then(
                any().filter(|token| !matches!(token, Token::TemplateEnd))
                .repeated()
                .collect::<Vec<_>>()
            )
            .then(just(Token::TemplateEnd))
            .try_map(|((_, tokens), _), span| {
                let mut parts = Vec::new();
                let mut expressions = Vec::new();
                let mut i = 0;
                
                // Handle empty template literal
                if tokens.is_empty() {
                    return Ok(Expr::TemplateLiteral {
                        parts: vec![String::new()],
                        expressions: vec![],
                    });
                }
                
                while i < tokens.len() {
                    match &tokens[i] {
                        Token::TemplatePart(s) => {
                            parts.push(s.clone());
                            i += 1;
                        }
                        Token::TemplateExprStart => {
                            // If we don't have a part yet, add an empty one
                            if parts.len() == expressions.len() {
                                parts.push(String::new());
                            }
                            i += 1;
                            
                            // Collect tokens for the expression until we hit another TemplatePart or end
                            let expr_start = i;
                            while i < tokens.len() {
                                match &tokens[i] {
                                    Token::TemplatePart(_) => break,
                                    _ => i += 1,
                                }
                            }
                            
                            // Parse the expression recursively
                            let expr_tokens = tokens[expr_start..i].to_vec();
                            if expr_tokens.is_empty() {
                                return Err(Rich::custom(span, "Empty expression in template literal"));
                            }
                            
                            // We need to parse these tokens as an expression
                            // For now, let's just handle simple identifiers and numbers
                            match expr_tokens.get(0) {
                                Some(Token::Identifier(name)) if expr_tokens.len() == 1 => {
                                    expressions.push(Expr::Identifier(name.clone()));
                                }
                                Some(Token::Number(n)) if expr_tokens.len() == 1 => {
                                    expressions.push(Expr::Number(n.clone()));
                                }
                                _ => {
                                    // For complex expressions, we'd need to recursively call the parser
                                    // This is a limitation of the current approach
                                    return Err(Rich::custom(span, "Complex expressions in templates not yet supported"));
                                }
                            }
                        }
                        _ => {
                            return Err(Rich::custom(span, format!("Unexpected token in template literal: {:?}", tokens[i])));
                        }
                    }
                }
                
                // Ensure we have one more part than expressions
                while parts.len() <= expressions.len() {
                    parts.push(String::new());
                }
                
                Ok(Expr::TemplateLiteral { parts, expressions })
            });
        
        let identifier = select! {
            Token::Identifier(name) => Expr::Identifier(name),
        };
        
        // Reference expression: &value
        let reference = just(Token::Ampersand)
            .ignore_then(expr.clone())
            .map(|expr| Expr::Reference {
                expr: Box::new(expr),
            });

        // Array literal: [elem1, elem2, ...]
        let array_literal = just(Token::LeftBracket)
            .ignore_then(
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
            )
            .then_ignore(just(Token::RightBracket))
            .map(|elements| Expr::ArrayLiteral { elements });

        let atom = number.clone()
            .or(string_literal.clone())
            .or(template_literal.clone())
            .or(module_def.clone())  // Type definitions: type Name { ... }
            .or(complex_expressions.clone())  // All complex expressions moved to expressions module
            .or(reference.clone())
            .or(array_literal.clone())  // Add array literals
            .or(identifier.clone())  // Moved identifier after keywords
            .or(expr.clone().delimited_by(
                just(Token::LeftParen),
                just(Token::RightParen),
            ));

        // Postfix operations: field access and function calls (highest precedence)
        #[derive(Clone)]
        enum PostfixOp {
            FieldAccess(String),
            FunctionCall(Vec<Expr>),
            ArrayIndex(Expr),
        }
        
        let postfix_op = choice((
            // Field access: .field
            just(Token::Dot)
                .ignore_then(select! {
                    Token::Identifier(field) => field,
                })
                .map(PostfixOp::FieldAccess),
            // Function call: (arg1, arg2, ...)
            just(Token::LeftParen)
                .ignore_then(
                    expr.clone()
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>()
                )
                .then_ignore(just(Token::RightParen))
                .map(|args| PostfixOp::FunctionCall(args)),
            // Array index: [index]
            just(Token::LeftBracket)
                .ignore_then(expr.clone())
                .then_ignore(just(Token::RightBracket))
                .map(PostfixOp::ArrayIndex),
        ));
        
        let postfix_expr = atom.clone()
            .foldl(
                postfix_op.repeated(),
                |expr, op| match op {
                    PostfixOp::FieldAccess(field) => Expr::FieldAccess {
                        object: Box::new(expr),
                        field,
                    },
                    PostfixOp::FunctionCall(args) => Expr::FunctionCall {
                        function: Box::new(expr),
                        arguments: args,
                    },
                    PostfixOp::ArrayIndex(index) => Expr::ArrayIndex {
                        array: Box::new(expr),
                        index: Box::new(index),
                    },
                }
            );

        let factor = postfix_expr.clone()
            .foldl(
                just(Token::Star).or(just(Token::Slash)).then(postfix_expr).repeated(),
                |left, (op, right)| {
                    let op = match op {
                        Token::Star => BinaryOp::Mul,
                        Token::Slash => BinaryOp::Div,
                        _ => unreachable!(),
                    };
                    Expr::Binary {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    }
                }
            );

        let term = factor.clone()
            .foldl(
                just(Token::Plus).or(just(Token::Minus)).then(factor).repeated(),
                |left, (op, right)| {
                    let op = match op {
                        Token::Plus => BinaryOp::Add,
                        Token::Minus => BinaryOp::Sub,
                        _ => unreachable!(),
                    };
                    Expr::Binary {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    }
                }
            );
        
        // Comparison operators (lower precedence than arithmetic)
        let comparison = term.clone()
            .foldl(
                choice((
                    just(Token::EqEq),
                    just(Token::NotEq),
                    just(Token::Less),
                    just(Token::Greater),
                    just(Token::LessEq),
                    just(Token::GreaterEq),
                ))
                .then(term)
                .repeated(),
                |left, (op, right)| {
                    let op = match op {
                        Token::EqEq => BinaryOp::Eq,
                        Token::NotEq => BinaryOp::NotEq,
                        Token::Less => BinaryOp::Less,
                        Token::Greater => BinaryOp::Greater,
                        Token::LessEq => BinaryOp::LessEq,
                        Token::GreaterEq => BinaryOp::GreaterEq,
                        _ => unreachable!(),
                    };
                    Expr::Binary {
                        left: Box::new(left),
                        op,
                        right: Box::new(right),
                    }
                }
            );
        
        // Type conversion and checking operators (lower precedence than comparison)
        comparison.clone()
            .foldl(
                choice((
                    just(Token::As).then(comparison.clone()).map(|(_, target)| (1, target)),
                    just(Token::Can).then(comparison.clone()).map(|(_, target)| (2, target)),
                    just(Token::Is).then(comparison).map(|(_, target)| (3, target)),
                ))
                .repeated(),
                |expr, (op_type, target_type)| {
                    match op_type {
                        1 => Expr::Conversion {
                            expr: Box::new(expr),
                            target_type: Box::new(target_type),
                        },
                        2 => Expr::CanConvert {
                            expr: Box::new(expr),
                            target_type: Box::new(target_type),
                        },
                        3 => Expr::TypeGuard {
                            expr: Box::new(expr),
                            type_expr: Box::new(target_type),
                        },
                        _ => unreachable!(),
                    }
                }
            )
    });

    // Statement parser: handles both expressions and assignments
    let statement = choice((
        // Try to parse assignment first: identifier = expression
        select! {
            Token::Identifier(name) => name,
        }
        .then(just(Token::Equals))
        .then(expr.clone())
        .map(|((name, _), value)| Expr::Assignment {
            name,
            value: Box::new(value),
        }),
        // Otherwise parse as a regular expression
        expr.clone(),
    ));
    
    // Top-level: handle sequences separated by semicolons
    statement
        .separated_by(just(Token::Semicolon))
        .allow_trailing()
        .collect::<Vec<_>>()
        .then_ignore(end())
        .map(|exprs| {
            match exprs.len() {
                0 => unreachable!(), // Should not happen with current grammar
                1 => exprs.into_iter().next().unwrap(),
                _ => Expr::Sequence(exprs),
            }
        })
}

pub fn parse(tokens: Vec<Token>) -> Result<Expr, String> {
    match parser().parse(&tokens[..]).into_result() {
        Ok(ast) => Ok(ast),
        Err(errs) => Err(format!("Parse error: {:?}", errs)),
    }
}

/// Parse tokens and return both AST and populated symbol table
pub fn parse_with_symbols(tokens: Vec<Token>) -> Result<(Expr, SymbolTable), String> {
    match parser().parse(&tokens[..]).into_result() {
        Ok(ast) => {
            let mut symbol_table = SymbolTable::new();
            populate_symbol_table(&ast, &mut symbol_table)?;
            Ok((ast, symbol_table))
        }
        Err(errs) => Err(format!("Parse error: {:?}", errs)),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::symbols::ValueCategory;

    #[test]
    fn test_parse_simple_expression() {
        let tokens = lex("1 + 1");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Binary {
            left: Box::new(Expr::Number(1)),
            op: BinaryOp::Add,
            right: Box::new(Expr::Number(1)),
        });
    }
    
    // Removed test_parse_type_keyword - "type" by itself is not a valid expression
    
    #[test]
    fn test_parse_identifiers() {
        let test_cases = vec![
            "I32",
            "myVariable", 
            "calculateSum",
        ];
        
        for input in test_cases {
            let tokens = lex(input);
            let ast = parse(tokens).unwrap();
            assert_eq!(ast, Expr::Identifier(input.to_string()));
        }
    }
    
    #[test]
    fn test_parse_int_type_builtin() {
        let tokens = lex("@intType(true, 32)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::IntType { signed: true, bits: 32 });
        
        let tokens = lex("@intType(false, 64)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::IntType { signed: false, bits: 64 });
    }
    
    #[test]
    fn test_parse_sequence() {
        let tokens = lex("1; 2; 3");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Sequence(vec![
            Expr::Number(1),
            Expr::Number(2),
            Expr::Number(3),
        ]));
    }
    
    #[test]
    fn test_parse_sequence_with_trailing_semicolon() {
        let tokens = lex("1; 2; 3;");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Sequence(vec![
            Expr::Number(1),
            Expr::Number(2),
            Expr::Number(3),
        ]));
    }
    
    #[test]
    fn test_parse_block() {
        let tokens = lex("{ 1; 2; 3 }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Block(vec![
            Expr::Number(1),
            Expr::Number(2),
            Expr::Number(3),
        ]));
    }
    
    #[test]
    fn test_parse_empty_block() {
        let tokens = lex("{}");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Block(vec![Expr::Void]));
    }
    
    #[test]
    fn test_parse_nested_blocks() {
        let tokens = lex("{ 1; { 2; 3 }; 4 }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Block(vec![
            Expr::Number(1),
            Expr::Block(vec![
                Expr::Number(2),
                Expr::Number(3),
            ]),
            Expr::Number(4),
        ]));
    }
    
    
    #[test]
    fn test_parse_let_binding() {
        let tokens = lex("let x: I64 = 42");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::LetBinding {
            name: "x".to_string(),
            type_expr: Box::new(Expr::Identifier("I64".to_string())),
            value: Box::new(Expr::Number(42)),
            mutable: false,
        });
    }
    
    #[test]
    fn test_parse_field_access() {
        let tokens = lex("p.x");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FieldAccess {
            object: Box::new(Expr::Identifier("p".to_string())),
            field: "x".to_string(),
        });
    }
    
    #[test]
    fn test_parse_chained_field_access() {
        let tokens = lex("obj.field1.field2");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FieldAccess {
            object: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier("obj".to_string())),
                field: "field1".to_string(),
            }),
            field: "field2".to_string(),
        });
    }
    
    #[test]
    fn test_parse_reference() {
        let tokens = lex("&value");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Reference {
            expr: Box::new(Expr::Identifier("value".to_string())),
        });
    }
    
    #[test]
    fn test_parse_reference_field_access() {
        let tokens = lex("&obj.field");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Reference {
            expr: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier("obj".to_string())),
                field: "field".to_string(),
            }),
        });
    }
    
    #[test]
    fn test_parse_function_call_no_args() {
        let tokens = lex("foo()");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FunctionCall {
            function: Box::new(Expr::Identifier("foo".to_string())),
            arguments: vec![],
        });
    }
    
    #[test]
    fn test_parse_function_call_one_arg() {
        let tokens = lex("add(42)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FunctionCall {
            function: Box::new(Expr::Identifier("add".to_string())),
            arguments: vec![Expr::Number(42)],
        });
    }
    
    #[test]
    fn test_parse_function_call_multiple_args() {
        let tokens = lex("sum(1, 2, 3)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FunctionCall {
            function: Box::new(Expr::Identifier("sum".to_string())),
            arguments: vec![Expr::Number(1), Expr::Number(2), Expr::Number(3)],
        });
    }
    
    #[test]
    fn test_parse_nested_function_calls() {
        let tokens = lex("add(mul(2, 3), 4)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FunctionCall {
            function: Box::new(Expr::Identifier("add".to_string())),
            arguments: vec![
                Expr::FunctionCall {
                    function: Box::new(Expr::Identifier("mul".to_string())),
                    arguments: vec![Expr::Number(2), Expr::Number(3)],
                },
                Expr::Number(4),
            ],
        });
    }
    
    #[test]
    fn test_parse_method_call() {
        let tokens = lex("obj.method(1, 2)");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::FunctionCall {
            function: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::Identifier("obj".to_string())),
                field: "method".to_string(),
            }),
            arguments: vec![Expr::Number(1), Expr::Number(2)],
        });
    }
    
    #[test]
    fn test_parse_simple_type() {
        let tokens = lex("type Point { let x: I64 = 0; }");
        let ast = parse(tokens).unwrap();
        if let Expr::TypeDef { name, members } = ast {
            assert_eq!(name, "Point");
            assert_eq!(members.len(), 1);
            if let TypeMember::LetBinding { name, exported, .. } = &members[0] {
                assert_eq!(name, "x");
                assert_eq!(*exported, false);
            } else {
                panic!("Expected LetBinding member");
            }
        } else {
            panic!("Expected TypeDef");
        }
    }
    
    #[test]
    fn test_parse_type_with_export() {
        let tokens = lex("type Point { export let x: I64 = 10; }");
        let ast = parse(tokens).unwrap();
        if let Expr::TypeDef { name, members } = ast {
            assert_eq!(name, "Point");
            assert_eq!(members.len(), 1);
            if let TypeMember::LetBinding { name, exported, .. } = &members[0] {
                assert_eq!(name, "x");
                assert_eq!(*exported, true);
            } else {
                panic!("Expected LetBinding member");
            }
        } else {
            panic!("Expected TypeDef");
        }
    }
    
    #[test]
    fn test_parse_type_with_function() {
        let tokens = lex("type Math { export fn add(a: I64, b: I64) -> I64 { a + b } }");
        let ast = parse(tokens).unwrap();
        if let Expr::TypeDef { name, members } = ast {
            assert_eq!(name, "Math");
            assert_eq!(members.len(), 1);
            if let TypeMember::FunctionDef { name, exported, params, .. } = &members[0] {
                assert_eq!(name, "add");
                assert_eq!(*exported, true);
                assert_eq!(params.len(), 2);
            } else {
                panic!("Expected FunctionDef member");
            }
        } else {
            panic!("Expected TypeDef");
        }
    }
    
    #[test]
    fn test_parse_with_symbols_let_binding() {
        let tokens = lex("let x: I64 = 42");
        let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();
        
        // Check AST
        if let Expr::LetBinding { name, .. } = ast {
            assert_eq!(name, "x");
        } else {
            panic!("Expected LetBinding");
        }
        
        // Check symbol table
        let binding = symbol_table.get_let_binding("x").unwrap();
        assert_eq!(binding.0, Expr::Identifier("I64".to_string()));
        assert_eq!(binding.1, Expr::Number(42));
    }
    
    #[test]
    fn test_parse_with_symbols_type() {
        let tokens = lex("type Point { export let x: I64 = 10; let y: I64 = 20; }");
        let (ast, symbol_table) = parse_with_symbols(tokens).unwrap();
        
        // Check AST
        if let Expr::TypeDef { name, members } = ast {
            assert_eq!(name, "Point");
            assert_eq!(members.len(), 2);
        } else {
            panic!("Expected TypeDef");
        }
        
        // Check symbol table
        let module = symbol_table.get_type("Point").unwrap();
        assert_eq!(module.name, "Point");
        assert_eq!(module.members.len(), 2);
        assert!(module.exported_members.contains("x"));
        assert!(!module.exported_members.contains("y"));
        
        // Test access control
        assert_eq!(symbol_table.is_member_exported("Point", "x").unwrap(), true);
        assert_eq!(symbol_table.is_member_exported("Point", "y").unwrap(), false);
    }
    
    #[test]
    fn test_parse_with_symbols_nested_expressions() {
        let tokens = lex("let result: I64 = { let inner: I64 = 5; inner + 1; }");
        let (_ast, symbol_table) = parse_with_symbols(tokens).unwrap();
        
        // Check that both let bindings are registered
        assert!(symbol_table.get_let_binding("result").is_some());
        assert!(symbol_table.get_let_binding("inner").is_some());
        
        // Verify the values and categories
        let inner_binding = symbol_table.get_let_binding("inner").unwrap();
        assert_eq!(inner_binding.1, Expr::Number(5));
        assert_eq!(inner_binding.2, ValueCategory::Stack);
    }
    
    #[test]
    fn test_reference_validation_in_parser() {
        // Test that referencing stack values produces parse errors
        let tokens = lex("let p: Point = 42; &p");
        let result = parse_with_symbols(tokens);
        
        // Should fail due to reference validation
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.contains("Cannot reference stack value 'p'"));
    }
    
    #[test]
    fn test_field_reference_validation_in_parser() {
        // Test that referencing fields of stack values produces parse errors
        let tokens = lex("let obj: Point = 42; &obj.field");
        let result = parse_with_symbols(tokens);
        
        // Should fail due to reference validation
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.contains("Cannot reference field 'field' of stack value 'obj'"));
    }
    
    #[test]
    fn test_valid_literal_references() {
        // Test that referencing literals is allowed
        let tokens = lex("&42");
        let result = parse_with_symbols(tokens);
        
        // Should succeed - literals can be referenced
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_parse_comparison_operators() {
        let test_cases = vec![
            ("1 == 2", BinaryOp::Eq),
            ("1 != 2", BinaryOp::NotEq),
            ("1 < 2", BinaryOp::Less),
            ("1 > 2", BinaryOp::Greater),
            ("1 <= 2", BinaryOp::LessEq),
            ("1 >= 2", BinaryOp::GreaterEq),
        ];
        
        for (input, expected_op) in test_cases {
            let tokens = lex(input);
            let ast = parse(tokens).unwrap();
            if let Expr::Binary { left, op, right } = ast {
                assert_eq!(*left, Expr::Number(1));
                assert_eq!(op, expected_op);
                assert_eq!(*right, Expr::Number(2));
            } else {
                panic!("Expected Binary expression for {}", input);
            }
        }
    }
    
    #[test]
    fn test_parse_loop() {
        let tokens = lex("loop { 42 }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![Expr::Number(42)])),
        });
    }
    
    #[test]
    fn test_parse_loop_with_break() {
        let tokens = lex("loop { x; break }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Identifier("x".to_string()),
                Expr::Break { value: None },
            ])),
        });
    }
    
    #[test]
    fn test_parse_break_without_value() {
        let tokens = lex("loop { break }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Break { value: None }
            ])),
        });
    }
    
    #[test]
    fn test_parse_nested_loops() {
        let tokens = lex("loop { loop { break }; break }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Loop {
                    body: Box::new(Expr::Block(vec![
                        Expr::Break { value: None }
                    ])),
                },
                Expr::Break { value: None },
            ])),
        });
    }
    
    #[test]
    fn test_parse_continue() {
        let tokens = lex("loop { continue }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Continue,
            ])),
        });
    }
    
    #[test]
    fn test_parse_loop_with_continue_and_break() {
        let tokens = lex("loop { @print(1); continue; @print(2); break }");
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::Loop {
            body: Box::new(Expr::Block(vec![
                Expr::Print { value: Box::new(Expr::Number(1)) },
                Expr::Continue,
                Expr::Print { value: Box::new(Expr::Number(2)) },
                Expr::Break { value: None },
            ])),
        });
    }
    
    #[test]
    fn test_parse_if_expression() {
        let tokens = lex("if x > 5 { 10 } else { 20 }");
        let ast = parse(tokens).unwrap();
        if let Expr::If { condition, then_expr, else_expr } = ast {
            // Check condition
            if let Expr::Binary { left, op, right } = condition.as_ref() {
                assert!(matches!(left.as_ref(), Expr::Identifier(s) if s == "x"));
                assert_eq!(op, &BinaryOp::Greater);
                assert_eq!(right.as_ref(), &Expr::Number(5));
            } else {
                panic!("Expected Binary condition");
            }
            // Check then branch
            assert_eq!(then_expr.as_ref(), &Expr::Block(vec![Expr::Number(10)]));
            // Check else branch
            assert!(else_expr.is_some());
            assert_eq!(else_expr.as_ref().unwrap().as_ref(), &Expr::Block(vec![Expr::Number(20)]));
        } else {
            panic!("Expected If expression");
        }
    }
    
    #[test]
    fn test_parse_if_without_else() {
        let tokens = lex("if x < 0 { 42 }");
        let ast = parse(tokens).unwrap();
        if let Expr::If { condition, then_expr, else_expr } = ast {
            // Check condition
            if let Expr::Binary { left, op, right } = condition.as_ref() {
                assert!(matches!(left.as_ref(), Expr::Identifier(s) if s == "x"));
                assert_eq!(op, &BinaryOp::Less);
                assert_eq!(right.as_ref(), &Expr::Number(0));
            } else {
                panic!("Expected Binary condition");
            }
            // Check then branch
            assert_eq!(then_expr.as_ref(), &Expr::Block(vec![Expr::Number(42)]));
            // Check no else branch
            assert!(else_expr.is_none());
        } else {
            panic!("Expected If expression");
        }
    }
    
    #[test]
    fn test_parse_complex_if_condition() {
        let tokens = lex("if x >= 10 + 5 { 100 } else { 0 }");
        let ast = parse(tokens).unwrap();
        if let Expr::If { condition, then_expr, else_expr } = ast {
            // Check condition: x >= (10 + 5)
            if let Expr::Binary { left, op, right } = condition.as_ref() {
                assert!(matches!(left.as_ref(), Expr::Identifier(s) if s == "x"));
                assert_eq!(op, &BinaryOp::GreaterEq);
                // Right side should be 10 + 5
                if let Expr::Binary { left: rl, op: rop, right: rr } = right.as_ref() {
                    assert_eq!(rl.as_ref(), &Expr::Number(10));
                    assert_eq!(rop, &BinaryOp::Add);
                    assert_eq!(rr.as_ref(), &Expr::Number(5));
                } else {
                    panic!("Expected Binary expression on right side");
                }
            } else {
                panic!("Expected Binary condition");
            }
            // Check branches
            assert_eq!(then_expr.as_ref(), &Expr::Block(vec![Expr::Number(100)]));
            assert_eq!(else_expr.as_ref().unwrap().as_ref(), &Expr::Block(vec![Expr::Number(0)]));
        } else {
            panic!("Expected If expression");
        }
    }
    
    #[test]
    fn test_parse_string_template() {
        let tokens = lex(r#""Hello, ${name}!""#);
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::TemplateLiteral {
            parts: vec!["Hello, ".to_string(), "!".to_string()],
            expressions: vec![Expr::Identifier("name".to_string())],
        });
    }
    
    #[test]
    fn test_parse_string_template_with_number() {
        let tokens = lex(r#""The answer is ${42}""#);
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::TemplateLiteral {
            parts: vec!["The answer is ".to_string(), String::new()],
            expressions: vec![Expr::Number(42)],
        });
    }
    
    #[test]
    fn test_parse_regular_string() {
        let tokens = lex(r#""Hello, World!""#);
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::StringLiteral("Hello, World!".to_string()));
    }
    
    #[test]
    fn test_parse_string_template_multiple_expressions() {
        let tokens = lex(r#""${x} + ${y} = ${z}""#);
        let ast = parse(tokens).unwrap();
        assert_eq!(ast, Expr::TemplateLiteral {
            parts: vec![String::new(), " + ".to_string(), " = ".to_string(), String::new()],
            expressions: vec![
                Expr::Identifier("x".to_string()),
                Expr::Identifier("y".to_string()),
                Expr::Identifier("z".to_string()),
            ],
        });
    }
    
}