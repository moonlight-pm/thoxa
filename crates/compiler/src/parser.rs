//! Recursive-descent + precedence parser (R0–R1).

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
        self.expect_kind(&TokenKind::RParen)?;
        let return_type = if self.at(TokenKind::Arrow) {
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
        let mut elems: Vec<BlockElem> = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Let) {
                elems.push(BlockElem::Let(self.parse_let()?));
            } else {
                elems.push(BlockElem::Expr(self.parse_expr()?));
            }
        }
        self.expect_kind(&TokenKind::RBrace)?;
        let end = self.prev_span();

        let (items, tail) = match elems.pop() {
            Some(BlockElem::Expr(e)) => {
                let items = elems
                    .into_iter()
                    .map(|el| match el {
                        BlockElem::Let(l) => BlockItem::Let {
                            name: l.name,
                            name_span: l.name_span,
                            init: l.init,
                        },
                        BlockElem::Expr(e) => BlockItem::Expr(e),
                    })
                    .collect();
                (items, Some(Box::new(e)))
            }
            Some(BlockElem::Let(l)) => {
                let mut items: Vec<BlockItem> = elems
                    .into_iter()
                    .map(|el| match el {
                        BlockElem::Let(l) => BlockItem::Let {
                            name: l.name,
                            name_span: l.name_span,
                            init: l.init,
                        },
                        BlockElem::Expr(e) => BlockItem::Expr(e),
                    })
                    .collect();
                items.push(BlockItem::Let {
                    name: l.name,
                    name_span: l.name_span,
                    init: l.init,
                });
                (items, None)
            }
            None => (Vec::new(), None),
        };

        Ok(Block {
            items,
            tail,
            span: Span::new(start.start, end.end),
        })
    }

    fn parse_let(&mut self) -> Result<LetBind> {
        self.expect_kind(&TokenKind::Let)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::Eq)?;
        let init = self.parse_expr()?;
        Ok(LetBind {
            name,
            name_span,
            init,
        })
    }

    /// Precedence climbing entry.
    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.at(TokenKind::Or) {
            let start = left.span.start;
            self.bump();
            let right = self.parse_and()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_cmp()?;
        while self.at(TokenKind::And) {
            let start = left.span.start;
            self.bump();
            let right = self.parse_cmp()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.current().kind {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::Ne => BinaryOp::Ne,
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::Le => BinaryOp::Le,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::Ge => BinaryOp::Ge,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            let right = self.parse_add()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            let right = self.parse_mul()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            let right = self.parse_unary()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        if self.at(TokenKind::Minus) {
            self.bump();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                span: Span::new(start.start, end),
            });
        }
        if self.at(TokenKind::Not) {
            self.bump();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                span: Span::new(start.start, end),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
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
            TokenKind::True => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(false),
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
            TokenKind::If => self.parse_if(),
            TokenKind::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Ok(Expr {
                    kind: ExprKind::Block(block),
                    span,
                })
            }
            TokenKind::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect_kind(&TokenKind::RParen)?;
                Ok(e)
            }
            _ => Err(Error::at(
                format!("expected expression, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    fn parse_if(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        let else_branch = if self.at(TokenKind::Else) {
            self.bump();
            // else if …
            if self.at(TokenKind::If) {
                let nested = self.parse_if()?;
                let span = nested.span;
                Some(Block {
                    items: Vec::new(),
                    tail: Some(Box::new(nested)),
                    span,
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        let end = else_branch
            .as_ref()
            .map(|b| b.span.end)
            .unwrap_or(then_branch.span.end);
        Ok(Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_branch,
                else_branch,
            },
            span: Span::new(start.start, end),
        })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn at(&self, kind: TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(&kind)
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

struct LetBind {
    name: String,
    name_span: Span,
    init: Expr,
}

enum BlockElem {
    Let(LetBind),
    Expr(Expr),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Module {
        let tokens = Lexer::new(src).tokenize().unwrap();
        Parser::new(tokens).parse_module().unwrap()
    }

    #[test]
    fn parse_hello() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                println("Hello, world!")
                0
            }
        "#,
        );
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_let_and_arith() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let a = 7 + 3
                println(a)
                0
            }
        "#,
        );
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_if_else() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let x = if 1 < 2 { 10 } else { 20 }
                x
            }
        "#,
        );
        assert_eq!(module.items.len(), 1);
    }
}
