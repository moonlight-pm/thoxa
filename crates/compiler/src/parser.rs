//! Recursive-descent parser for the R0+ subset.

use crate::ast::*;
use crate::error::{Error, Result};
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse_module(&mut self) -> Result<Module> {
        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) {
            items.push(Item::Function(self.parse_function()?));
        }
        Ok(Module { items })
    }

    fn parse_function(&mut self) -> Result<Function> {
        self.expect_kind(&TokenKind::Fn)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::LParen)?;
        // R0: no parameters yet
        self.expect_kind(&TokenKind::RParen)?;
        let return_type = if self.at_arrow() {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(Function {
            name,
            name_span,
            params: Vec::new(),
            return_type,
            body,
        })
    }

    fn parse_type(&mut self) -> Result<TypeExpr> {
        let (name, span) = self.expect_ident()?;
        Ok(TypeExpr { name, span })
    }

    fn parse_block(&mut self) -> Result<Block> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::LBrace)?;
        let mut exprs = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            exprs.push(self.parse_expr()?);
        }
        self.expect_kind(&TokenKind::RBrace)?;
        let end = self.prev_span();
        // Last expression is the block value if present.
        let (stmts, tail) = if let Some(last) = exprs.pop() {
            (exprs, Some(Box::new(last)))
        } else {
            (exprs, None)
        };
        Ok(Block {
            stmts,
            tail,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        match &self.current().kind {
            TokenKind::Int(n) => {
                let n = *n;
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Int(n),
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(Expr {
                    kind: ExprKind::String(s),
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.bump();
                if self.at(TokenKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !self.at(TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.at(TokenKind::Comma) {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect_kind(&TokenKind::RParen)?;
                    Ok(Expr {
                        kind: ExprKind::Call { callee: name, args },
                        span: Span::new(start.start, self.prev_span().end),
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::Name(name),
                        span: Span::new(start.start, self.prev_span().end),
                    })
                }
            }
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Ok(Expr {
                    kind: ExprKind::Block(block),
                    span,
                })
            }
            _ => Err(Error::at(
                format!("expected expression, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn at(&self, kind: TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(&kind)
    }

    fn at_arrow(&self) -> bool {
        matches!(self.current().kind, TokenKind::Arrow)
    }

    fn bump(&mut self) {
        if self.pos < self.tokens.len().saturating_sub(1) {
            self.pos += 1;
        }
    }

    fn expect_kind(&mut self, kind: &TokenKind) -> Result<()> {
        if std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind) {
            self.bump();
            Ok(())
        } else {
            Err(Error::at(
                format!("expected {:?}, found {:?}", kind, self.current().kind),
                self.peek_span(),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<(String, Span)> {
        match &self.current().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                let span = self.current().span;
                self.bump();
                Ok((s, span))
            }
            _ => Err(Error::at(
                format!("expected identifier, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    fn peek_span(&self) -> Span {
        self.current().span
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            Span::dummy()
        } else {
            self.tokens[self.pos - 1].span
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    #[test]
    fn parse_hello() {
        let src = r#"
            fn main() -> Int64 {
                println("Hello, world!")
                0
            }
        "#;
        let tokens = Lexer::new(src).tokenize().unwrap();
        let module = Parser::new(tokens).parse_module().unwrap();
        assert_eq!(module.items.len(), 1);
    }
}
