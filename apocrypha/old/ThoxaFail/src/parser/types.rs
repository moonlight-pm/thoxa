use chumsky::prelude::*;
use chumsky::input::ValueInput;
use crate::ast::{Expr, Parameter, TypeMember};
use crate::lexer::Token;

/// Parse a type definition (type Name { members... })
/// Returns Some(Expr::TypeDef) if a type definition was parsed, None otherwise
pub fn parse_type_definition<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone,
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    // Parse a module member (with optional export)
    let module_member = {
        let exported = just(Token::Export).or_not().map(|opt| opt.is_some());
        
        // Module let binding: [export] let name: Type = value;
        let module_let = exported.clone()
            .then_ignore(just(Token::Let))
            .then(select! {
                Token::Identifier(name) => name,
            })
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .then_ignore(just(Token::Equals))
            .then(expr.clone())
            .map(|(((exported, name), type_expr), value)| {
                TypeMember::LetBinding {
                    name,
                    type_expr,
                    value,
                    exported,
                }
            });
        
        // Module function: [export] fn name(params) -> RetType { body }
        let module_fn = {
            // Define function body statement parser (similar to block_stmt but can be used here)
            let func_body_stmt = choice((
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
            
            let parameter = choice((
                // Variadic parameter: ...name: Type
                just(Token::DotDotDot)
                    .then(select! {
                        Token::Identifier(name) => name,
                    })
                    .then_ignore(just(Token::Colon))
                    .then(expr.clone())
                    .map(|((_, name), type_expr)| Parameter { 
                        name, 
                        type_expr,
                        variadic: true
                    }),
                // Regular parameter: name: Type
                select! {
                    Token::Identifier(name) => name,
                }
                .then_ignore(just(Token::Colon))
                .then(expr.clone())
                .map(|(name, type_expr)| Parameter { 
                    name, 
                    type_expr,
                    variadic: false
                })
            ));
            
            exported.clone()
                .then_ignore(just(Token::Fn))
                .then(select! {
                    Token::Identifier(name) => name,
                })
                .then(
                    parameter
                        .separated_by(just(Token::Comma))
                        .allow_trailing()
                        .collect::<Vec<_>>()
                        .delimited_by(just(Token::LeftParen), just(Token::RightParen))
                )
                .then_ignore(just(Token::Arrow))
                .then(expr.clone())
                .then(
                    func_body_stmt
                        .separated_by(just(Token::Semicolon))
                        .allow_trailing()
                        .collect::<Vec<_>>()
                        .delimited_by(just(Token::LeftBrace), just(Token::RightBrace))
                        .map(|exprs| {
                            if exprs.is_empty() {
                                Expr::Block(vec![])
                            } else if exprs.len() == 1 {
                                exprs.into_iter().next().unwrap()
                            } else {
                                Expr::Sequence(exprs)
                            }
                        })
                )
                .map(|((((exported, name), params), return_type), body)| {
                    TypeMember::FunctionDef {
                        name,
                        params,
                        return_type,
                        body,
                        exported,
                    }
                })
        };
        
        // Conversion block: as TypeName { body }
        let conversion = just(Token::As)
            .ignore_then(expr.clone())
            .then(
                expr.clone()
                    .delimited_by(just(Token::LeftBrace), just(Token::RightBrace))
            )
            .map(|(target_type, body)| {
                TypeMember::Conversion {
                    target_type,
                    body,
                }
            });
        
        module_let.or(module_fn).or(conversion)
    };
    
    // Type definition: type Name { members... }
    just(Token::Type)
        .ignore_then(select! {
            Token::Identifier(name) => name,
        })
        .then(
            module_member
                .separated_by(just(Token::Semicolon))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LeftBrace), just(Token::RightBrace))
        )
        .map(|(name, members)| {
            Expr::TypeDef { name, members }
        })
}