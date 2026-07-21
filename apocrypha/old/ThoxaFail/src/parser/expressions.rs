use chumsky::prelude::*;
use chumsky::input::ValueInput;
use crate::ast::{Expr, ImportSpec};
use crate::lexer::Token;

/// Parse complex expressions that start with specific keywords or patterns
/// Returns a parser that handles:
/// 1. Let/mut binding parsing (starts with "let" or "mut")
/// 2. Built-in function parsing (@intType, @print, @printStr, @syscall, @strlen, @cstr, @intToString)
/// 3. Control flow parsing (if, loop, break, continue)
/// 4. Import statement parsing
/// 5. Block expression parsing
pub fn parse_complex_expressions<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    choice((
        parse_let_binding(expr.clone()),
        parse_mut_binding(expr.clone()),
        parse_builtin_functions(expr.clone()),
        parse_control_flow(expr.clone()),
        parse_import_statement(),
        parse_block_expression(expr.clone()),
    ))
}

/// Parse let binding: let name: Type = value; or let name: Type;
fn parse_let_binding<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::Let)
        .ignore_then(select! {
            Token::Identifier(name) => name,
        })
        .then_ignore(just(Token::Colon))
        .then(expr.clone())
        .then(
            just(Token::Equals)
                .ignore_then(expr.clone())
                .or_not()
        )
        .map(|((name, type_expr), value)| {
            let value = value.unwrap_or(Expr::Void);
            Expr::LetBinding {
                name,
                type_expr: Box::new(type_expr),
                value: Box::new(value),
                mutable: false,
            }
        })
}

/// Parse mut binding: mut name: Type = value;
fn parse_mut_binding<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::Mut)
        .ignore_then(select! {
            Token::Identifier(name) => name,
        })
        .then_ignore(just(Token::Colon))
        .then(expr.clone())
        .then_ignore(just(Token::Equals))
        .then(expr.clone())
        .map(|((name, type_expr), value)| {
            Expr::LetBinding {
                name,
                type_expr: Box::new(type_expr),
                value: Box::new(value),
                mutable: true,
            }
        })
}

/// Parse built-in functions (@intType, @print, @printStr, @syscall, @strlen, @cstr, @intToString)
fn parse_builtin_functions<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    choice((
        parse_int_type_builtin(),
        parse_print_builtin(expr.clone()),
        parse_print_str_builtin(expr.clone()),
        parse_syscall_builtin(expr.clone()),
        parse_strlen_builtin(expr.clone()),
        parse_cstr_builtin(expr.clone()),
        parse_int_to_string_builtin(expr.clone()),
    ))
}

/// Parse @intType(signed: bool, bits: number)
fn parse_int_type_builtin<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::IntTypeBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(
            select! {
                Token::Identifier(name) if name == "true" => true,
                Token::Identifier(name) if name == "false" => false,
            }
        )
        .then_ignore(just(Token::Comma))
        .then(
            select! {
                Token::Number(n) if n >= 1 && n <= 128 => n as u8,
            }
        )
        .then_ignore(just(Token::RightParen))
        .map(|(signed, bits)| Expr::IntType { signed, bits })
}

/// Parse @print(expr)
fn parse_print_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::PrintBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then_ignore(just(Token::RightParen))
        .map(|value| Expr::Print {
            value: Box::new(value),
        })
}

/// Parse @printStr(expr)
fn parse_print_str_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::PrintStrBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then_ignore(just(Token::RightParen))
        .map(|value| Expr::PrintStr {
            value: Box::new(value),
        })
}

/// Parse @syscall(number, arg1, arg2, ...)
fn parse_syscall_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::SyscallBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then(
            just(Token::Comma)
                .ignore_then(expr.clone())
                .repeated()
                .collect::<Vec<_>>()
        )
        .then_ignore(just(Token::RightParen))
        .map(|(number, arguments)| Expr::Syscall {
            number: Box::new(number),
            arguments,
        })
}

/// Parse @strlen(expr)
fn parse_strlen_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::StrlenBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then_ignore(just(Token::RightParen))
        .map(|value| Expr::Strlen {
            value: Box::new(value),
        })
}

/// Parse @cstr(expr)
fn parse_cstr_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::CstrBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then_ignore(just(Token::RightParen))
        .map(|value| Expr::Cstr {
            value: Box::new(value),
        })
}

/// Parse @intToString(expr)
fn parse_int_to_string_builtin<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::IntToStringBuiltin)
        .ignore_then(just(Token::LeftParen))
        .ignore_then(expr.clone())
        .then_ignore(just(Token::RightParen))
        .map(|value| Expr::IntToString {
            value: Box::new(value),
        })
}

/// Parse control flow constructs (if, loop, break, continue)
fn parse_control_flow<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    choice((
        parse_if_expression(expr.clone()),
        parse_loop_expression(expr.clone()),
        parse_break_expression(),
        parse_continue_expression(),
    ))
}

/// Parse if expression: if condition { then_expr } else { else_expr }
fn parse_if_expression<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    let block = parse_block_expression(expr.clone());
    
    just(Token::If)
        .ignore_then(expr.clone())
        .then(block.clone())
        .then(
            just(Token::Else)
                .ignore_then(block.clone())
                .or_not()
        )
        .map(|((condition, then_expr), else_expr)| Expr::If {
            condition: Box::new(condition),
            then_expr: Box::new(then_expr),
            else_expr: else_expr.map(Box::new),
        })
}

/// Parse loop expression: loop { body }
fn parse_loop_expression<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    let block = parse_block_expression(expr.clone());
    
    just(Token::Loop)
        .ignore_then(block.clone())
        .map(|body| Expr::Loop {
            body: Box::new(body),
        })
}

/// Parse break expression: break [value]
fn parse_break_expression<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::Break)
        .map(|_| Expr::Break { value: None })
}

/// Parse continue expression: continue
fn parse_continue_expression<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    just(Token::Continue)
        .map(|_| Expr::Continue)
}

/// Parse import statements
fn parse_import_statement<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    // Destructured imports: { name1, name2 }
    let destructured = select! {
        Token::Identifier(name) => name,
    }
    .separated_by(just(Token::Comma))
    .allow_trailing()
    .collect::<Vec<_>>()
    .delimited_by(just(Token::LeftBrace), just(Token::RightBrace))
    .map(ImportSpec::Destructured);
    
    // Type and instance: Type, instance
    let type_and_instance = select! {
        Token::Identifier(type_name) => type_name,
    }
    .then_ignore(just(Token::Comma))
    .then(select! {
        Token::Identifier(instance_name) => instance_name,
    })
    .map(|(type_name, instance_name)| ImportSpec::TypeAndInstance {
        type_name,
        instance_name,
    });
    
    // Default import: Name
    let default_import = select! {
        Token::Identifier(name) => name,
    }
    .map(ImportSpec::Default);
    
    // Parse import spec
    let import_spec = choice((
        destructured,
        type_and_instance,
        default_import,
    ));
    
    // Full import statement
    just(Token::Import)
        .ignore_then(import_spec)
        .then_ignore(just(Token::From))
        .then(select! {
            Token::StringLiteral(path) => path,
        })
        .map(|(imports, path)| Expr::Import { imports, path })
}

/// Parse block expression: { stmt; stmt; stmt }
pub fn parse_block_expression<'tokens, I>(
    expr: impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    // We need to define a mini statement parser here for blocks
    let block_stmt = choice((
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
    
    block_stmt
        .separated_by(just(Token::Semicolon))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LeftBrace), just(Token::RightBrace))
        .map(|exprs| {
            if exprs.is_empty() {
                // Empty block evaluates to Void
                Expr::Block(vec![Expr::Void])
            } else {
                Expr::Block(exprs)
            }
        })
}