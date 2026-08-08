//! Recursive-descent + precedence parser (R0–R8).

use crate::ast::*;
use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::span::Span;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Synthetic `>` closers left after splitting a `>>` (Shr) in type args (D113).
    extra_rangles: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            extra_rangles: 0,
        }
    }

    pub fn parse_module(&mut self) -> Result<Module> {
        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) {
            let exported = if self.at(TokenKind::Export) {
                self.bump();
                true
            } else {
                false
            };

            if self.at(TokenKind::Import) {
                if exported {
                    return Err(Error::at(
                        "`export import` is not supported",
                        self.peek_span(),
                    ));
                }
                items.push(Item::Import(self.parse_import()?));
            } else if self.at(TokenKind::Type) {
                items.push(Item::Type(self.parse_type_def(exported)?));
            } else if self.at(TokenKind::Extern) {
                items.push(Item::Extern(self.parse_extern(exported)?));
            } else if self.at(TokenKind::Fn) {
                items.push(Item::Function(self.parse_function(exported)?));
            } else if self.at(TokenKind::Let) || self.at(TokenKind::Mut) {
                items.push(Item::Global(self.parse_global(exported)?));
            } else {
                // Session-script top-level statement (assign or expression).
                // Ordinary programs with `fn main` reject these in session::prepare
                // if mixed incorrectly; pure programs never produce them if the
                // author only uses decls — free stmts mark a session body.
                if exported {
                    return Err(Error::at(
                        "`export` only applies to declarations, not statements",
                        self.peek_span(),
                    ));
                }
                items.push(Item::SessionStmt(self.parse_session_stmt()?));
            }
        }
        Ok(Module { items })
    }

    /// Top-level session body item: assignment or expression statement.
    /// (`let`/`mut` at module level remain `Global` — visible from `step`.)
    fn parse_session_stmt(&mut self) -> Result<BlockItem> {
        if self.at_assign_stmt() {
            let a = self.parse_assign()?;
            Ok(BlockItem::Assign {
                target: a.target,
                value: a.value,
            })
        } else {
            Ok(BlockItem::Expr(self.parse_expr()?))
        }
    }

    /// `import { a, b as c } from "…"` or `import ns from "…"`
    fn parse_import(&mut self) -> Result<Import> {
        self.expect_kind(&TokenKind::Import)?;
        let kind = if self.at(TokenKind::LBrace) {
            self.bump();
            let mut names = Vec::new();
            if !self.at(TokenKind::RBrace) {
                loop {
                    let (name, name_span) = self.expect_ident()?;
                    let alias = if self.at(TokenKind::As) {
                        self.bump();
                        let (a, aspan) = self.expect_ident()?;
                        Some((a, aspan))
                    } else {
                        None
                    };
                    names.push(ImportName {
                        name,
                        name_span,
                        alias,
                    });
                    if self.at(TokenKind::Comma) {
                        self.bump();
                        continue;
                    }
                    break;
                }
            }
            self.expect_kind(&TokenKind::RBrace)?;
            ImportKind::Selective(names)
        } else {
            let (binding, binding_span) = self.expect_ident()?;
            // Namespace binding must be camelCase (value instance).
            if binding
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                return Err(Error::at(
                    "a module import binds an instance; use a camelCase name",
                    binding_span,
                ));
            }
            ImportKind::Namespace {
                binding,
                binding_span,
            }
        };
        self.expect_kind(&TokenKind::From)?;
        let (path, path_span) = self.expect_string()?;
        Ok(Import {
            kind,
            path,
            path_span,
        })
    }

    /// Top-level `let` / `mut name = expr`.
    fn parse_global(&mut self, exported: bool) -> Result<Global> {
        let mutable = if self.at(TokenKind::Mut) {
            self.bump();
            true
        } else {
            self.expect_kind(&TokenKind::Let)?;
            false
        };
        if exported && mutable {
            return Err(Error::at(
                "top-level `mut` cannot be exported (module state is private)",
                self.peek_span(),
            ));
        }
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::Eq)?;
        let init = self.parse_expr()?;
        Ok(Global {
            exported,
            mutable,
            name,
            name_span,
            init,
        })
    }

    /// `extern fn name(params) [-> T] from "lib" ["symbol"]`
    fn parse_extern(&mut self, exported: bool) -> Result<ExternFn> {
        self.expect_kind(&TokenKind::Extern)?;
        self.expect_kind(&TokenKind::Fn)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let p = self.parse_param()?;
                if !p.positional {
                    return Err(Error::at(
                        "extern fn parameters must be marked positional with `!`",
                        p.name_span,
                    ));
                }
                params.push(p);
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RParen)?;
        let return_type = if self.at(TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_kind(&TokenKind::From)?;
        let (library, library_span) = self.expect_string()?;
        let c_symbol = if matches!(self.current().kind, TokenKind::String(_)) {
            Some(self.expect_string()?.0)
        } else {
            None
        };
        Ok(ExternFn {
            exported,
            name,
            name_span,
            params,
            return_type,
            library,
            library_span,
            c_symbol,
        })
    }

    fn expect_string(&mut self) -> Result<(String, Span)> {
        match &self.current().kind {
            TokenKind::String(s) => {
                let s = s.clone();
                let span = self.current().span;
                self.bump();
                Ok((s, span))
            }
            _ => Err(Error::at(
                format!("expected string literal, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    fn parse_type_def(&mut self, exported: bool) -> Result<TypeDef> {
        self.expect_kind(&TokenKind::Type)?;
        let (name, name_span) = self.expect_ident()?;

        // Optional type parameters: `type Box<T>` / `type Trial<P, F>` (D118).
        let type_params = if self.at(TokenKind::LAngle) {
            self.parse_type_params()?
        } else {
            Vec::new()
        };

        // Union alias: `type Name = A | B | …` or `type Opt<T> = …` (D118).
        if self.at(TokenKind::Eq) {
            self.bump();
            let members = self.parse_union_members()?;
            if members.len() < 2 {
                return Err(Error::at(
                    "union type requires at least two members (`A | B`)",
                    name_span,
                ));
            }
            return Ok(TypeDef {
                exported,
                name,
                name_span,
                type_params,
                params: Vec::new(),
                fields: Vec::new(),
                methods: Vec::new(),
                union_members: Some(members),
            });
        }

        // R3: `type Point(x: Int64)` or `type Point(x: Int64) { methods }`
        // R7: `type Regs { mut a: UInt32 … }` (no constructor paren list)
        // Nullary: `type Up` (D118).
        // Generic product: `type Box<T>(value: T)`.
        let mut params = Vec::new();
        if self.at(TokenKind::LParen) {
            self.bump();
            if !self.at(TokenKind::RParen) {
                loop {
                    params.push(self.parse_param()?);
                    if self.at(TokenKind::Comma) {
                        self.bump();
                        continue;
                    }
                    break;
                }
            }
            self.expect_kind(&TokenKind::RParen)?;
        }

        // Body is optional: `type Point(x: Int64, y: Int64)` needs no `{}`
        // when there are no fields or methods. Empty `{}` still accepted.
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        if self.at(TokenKind::LBrace) {
            self.bump();
            while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                if self.at(TokenKind::Fn) || self.at(TokenKind::Export) {
                    methods.push(self.parse_method()?);
                } else if self.at(TokenKind::Mut) || self.at(TokenKind::Let) {
                    fields.push(self.parse_field_decl()?);
                } else {
                    return Err(Error::at(
                        "expected field (`mut`/`let name: Type`) or method in type body",
                        self.peek_span(),
                    ));
                }
            }
            self.expect_kind(&TokenKind::RBrace)?;
        }

        if !params.is_empty() && !fields.is_empty() {
            return Err(Error::at(
                "type cannot have both constructor parameters and body fields",
                name_span,
            ));
        }
        if !type_params.is_empty() && !fields.is_empty() {
            return Err(Error::at(
                "generic types cannot use C-layout body fields yet",
                name_span,
            ));
        }
        if !type_params.is_empty() && !methods.is_empty() {
            return Err(Error::at(
                "generic types cannot declare methods yet",
                name_span,
            ));
        }

        Ok(TypeDef {
            exported,
            name,
            name_span,
            type_params,
            params,
            fields,
            methods,
            union_members: None,
        })
    }

    /// `<T, U>` type parameter list on a type definition.
    fn parse_type_params(&mut self) -> Result<Vec<String>> {
        self.expect_kind(&TokenKind::LAngle)?;
        let mut params = Vec::new();
        if !self.at_rangle() {
            loop {
                let (p, pspan) = self.expect_ident()?;
                if params.iter().any(|x| x == &p) {
                    return Err(Error::at(
                        format!("duplicate type parameter `{p}`"),
                        pspan,
                    ));
                }
                params.push(p);
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_rangle()?;
        if params.is_empty() {
            return Err(Error::at(
                "type parameter list cannot be empty",
                self.peek_span(),
            ));
        }
        Ok(params)
    }

    /// `A | B | C` type members (each may carry trailing `!` / `?` marks).
    fn parse_union_members(&mut self) -> Result<Vec<TypeExpr>> {
        let mut members = Vec::new();
        loop {
            members.push(self.parse_type()?);
            if self.at(TokenKind::Pipe) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(members)
    }

    /// `[mut|let] name: Type` inside a type body (R7 C-layout fields).
    fn parse_field_decl(&mut self) -> Result<FieldDecl> {
        let mutable = if self.at(TokenKind::Mut) {
            self.bump();
            true
        } else if self.at(TokenKind::Let) {
            self.bump();
            false
        } else {
            return Err(Error::at("expected `mut` or `let` field", self.peek_span()));
        };
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        Ok(FieldDecl {
            mutable,
            name,
            name_span,
            ty,
        })
    }

    fn parse_method(&mut self) -> Result<Method> {
        let exported = if self.at(TokenKind::Export) {
            self.bump();
            true
        } else {
            false
        };
        self.expect_kind(&TokenKind::Fn)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                params.push(self.parse_param()?);
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RParen)?;
        let return_type = if self.at(TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(Method {
            exported,
            name,
            name_span,
            params,
            return_type,
            body,
        })
    }

    fn parse_function(&mut self, exported: bool) -> Result<Function> {
        self.expect_kind(&TokenKind::Fn)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::LParen)?;
        let mut params = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                params.push(self.parse_param()?);
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RParen)?;
        let return_type = if self.at(TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(Function {
            exported,
            name,
            name_span,
            params,
            return_type,
            body,
        })
    }

    fn parse_param(&mut self) -> Result<Param> {
        let (name, name_span) = self.expect_ident()?;
        let positional = if self.at(TokenKind::Bang) {
            self.bump();
            true
        } else {
            false
        };
        self.expect_kind(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        let default = if self.at(TokenKind::Eq) {
            if positional {
                return Err(Error::at(
                    "positional parameters (`!`) cannot have defaults",
                    name_span,
                ));
            }
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Param {
            name,
            name_span,
            positional,
            ty,
            default,
        })
    }

    fn parse_type(&mut self) -> Result<TypeExpr> {
        // Literal types (D118): `0`, `true`, `"x"` in type position (union members).
        if let TokenKind::Int(n) = self.current().kind {
            let n = n;
            let span = self.current().span;
            self.bump();
            let mark = self.parse_type_mark();
            let end = mark.map(|_| self.prev_span().end).unwrap_or(span.end);
            return Ok(TypeExpr {
                name: String::new(),
                args: Vec::new(),
                span: Span::new(span.start, end),
                lit: Some(TypeLit::Int(n)),
                mark,
            });
        }
        if self.at(TokenKind::True) || self.at(TokenKind::False) {
            let value = self.at(TokenKind::True);
            let span = self.current().span;
            self.bump();
            let mark = self.parse_type_mark();
            let end = mark.map(|_| self.prev_span().end).unwrap_or(span.end);
            return Ok(TypeExpr {
                name: String::new(),
                args: Vec::new(),
                span: Span::new(span.start, end),
                lit: Some(TypeLit::Bool(value)),
                mark,
            });
        }
        if let TokenKind::String(ref s) = self.current().kind.clone() {
            let s = s.clone();
            let span = self.current().span;
            self.bump();
            let mark = self.parse_type_mark();
            let end = mark.map(|_| self.prev_span().end).unwrap_or(span.end);
            return Ok(TypeExpr {
                name: String::new(),
                args: Vec::new(),
                span: Span::new(span.start, end),
                lit: Some(TypeLit::String(s)),
                mark,
            });
        }

        let (name, start_span) = self.expect_ident()?;
        let mut args = Vec::new();
        let mut end = if self.at(TokenKind::LAngle) {
            self.bump();
            if !self.at_rangle() {
                loop {
                    args.push(self.parse_type_arg()?);
                    if self.at(TokenKind::Comma) {
                        self.bump();
                        continue;
                    }
                    break;
                }
            }
            self.expect_rangle()?;
            // After a synthetic `>` from `>>`, prev token may still be Shr — fine for spans.
            self.prev_span().end
        } else {
            start_span.end
        };
        let mark = self.parse_type_mark();
        if mark.is_some() {
            end = self.prev_span().end;
        }
        Ok(TypeExpr {
            name,
            args,
            span: Span::new(start_span.start, end),
            lit: None,
            mark,
        })
    }

    /// Optional trailing `!` (extract) or `?` (residual) on a type (D118).
    fn parse_type_mark(&mut self) -> Option<TypeMark> {
        if self.at(TokenKind::Bang) {
            self.bump();
            Some(TypeMark::Extract)
        } else if self.at(TokenKind::Question) {
            self.bump();
            Some(TypeMark::Residual)
        } else {
            None
        }
    }

    /// `>` or the first half of `>>` (lexer emits Shr for nested generics like `May<Array<T>>`).
    fn at_rangle(&self) -> bool {
        self.extra_rangles > 0 || self.at(TokenKind::RAngle) || self.at(TokenKind::Shr)
    }

    fn expect_rangle(&mut self) -> Result<()> {
        if self.extra_rangles > 0 {
            self.extra_rangles -= 1;
            return Ok(());
        }
        if self.at(TokenKind::RAngle) {
            self.bump();
            Ok(())
        } else if self.at(TokenKind::Shr) {
            // Consume one `>` from `>>`; leave one synthetic closer for the outer type.
            self.bump();
            self.extra_rangles += 1;
            Ok(())
        } else {
            Err(Error::at(
                format!("expected `>`, found {:?}", self.current().kind),
                self.peek_span(),
            ))
        }
    }

    fn parse_type_arg(&mut self) -> Result<TypeArg> {
        if let TokenKind::Int(n) = self.current().kind {
            let n = n;
            let span = self.current().span;
            self.bump();
            return Ok(TypeArg::Int { value: n, span });
        }
        Ok(TypeArg::Type(self.parse_type()?))
    }

    fn parse_block(&mut self) -> Result<Block> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::LBrace)?;
        let mut elems: Vec<BlockElem> = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Let) {
                elems.push(BlockElem::Let(self.parse_binding(false)?));
            } else if self.at(TokenKind::Mut) {
                elems.push(BlockElem::Let(self.parse_binding(true)?));
            } else if self.at_assign_stmt() {
                elems.push(BlockElem::Assign(self.parse_assign()?));
            } else {
                elems.push(BlockElem::Expr(self.parse_expr()?));
            }
        }
        self.expect_kind(&TokenKind::RBrace)?;
        let end = self.prev_span();

        let (items, tail) = match elems.pop() {
            Some(BlockElem::Expr(e)) => {
                let items = elems.into_iter().map(block_elem_to_item).collect();
                (items, Some(Box::new(e)))
            }
            Some(other) => {
                let mut items: Vec<BlockItem> = elems.into_iter().map(block_elem_to_item).collect();
                items.push(block_elem_to_item(other));
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

    /// Statement starts with an assignable place:
    /// `name =`, `name[i] =`, `name.field =`, or `name.field[i] =`.
    fn at_assign_stmt(&self) -> bool {
        if !matches!(self.current().kind, TokenKind::Ident(_)) {
            return false;
        }
        let mut i = self.pos + 1;
        // Optional `.field`
        if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Dot) {
            i += 1;
            if !matches!(
                self.tokens.get(i).map(|t| &t.kind),
                Some(TokenKind::Ident(_))
            ) {
                return false;
            }
            i += 1;
            // Optional `[index]`
            if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::LBracket) {
                if !Self::skip_bracket_group(&self.tokens, &mut i) {
                    return false;
                }
            }
        } else if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::LBracket) {
            // `name[index] = …` dynamic Array element assign
            if !Self::skip_bracket_group(&self.tokens, &mut i) {
                return false;
            }
        }
        self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Eq)
    }

    /// Advance `i` past a balanced `[…]` group. `i` starts at `[`.
    fn skip_bracket_group(tokens: &[Token], i: &mut usize) -> bool {
        let mut depth = 1usize;
        *i += 1;
        while *i < tokens.len() && depth > 0 {
            match &tokens[*i].kind {
                TokenKind::LBracket => depth += 1,
                TokenKind::RBracket => depth -= 1,
                TokenKind::Eof => return false,
                _ => {}
            }
            *i += 1;
        }
        depth == 0
    }

    fn parse_binding(&mut self, mutable: bool) -> Result<LetBind> {
        if mutable {
            self.expect_kind(&TokenKind::Mut)?;
        } else {
            self.expect_kind(&TokenKind::Let)?;
        }
        let (name, name_span) = self.expect_ident()?;
        let ty = if self.at(TokenKind::Colon) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_kind(&TokenKind::Eq)?;
        let init = self.parse_expr()?;
        Ok(LetBind {
            name,
            name_span,
            mutable,
            ty,
            init,
        })
    }

    fn parse_assign(&mut self) -> Result<AssignStmt> {
        let (base, base_span) = self.expect_ident()?;
        let target = if self.at(TokenKind::Dot) {
            self.bump();
            let (field, field_span) = self.expect_ident()?;
            if self.at(TokenKind::LBracket) {
                self.bump();
                let index = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBracket)?;
                AssignTarget::Index {
                    base,
                    base_span,
                    field,
                    field_span,
                    index,
                }
            } else {
                AssignTarget::Field {
                    base,
                    base_span,
                    field,
                    field_span,
                }
            }
        } else if self.at(TokenKind::LBracket) {
            self.bump();
            let index = self.parse_expr()?;
            self.expect_kind(&TokenKind::RBracket)?;
            AssignTarget::DynIndex {
                name: base,
                name_span: base_span,
                index,
            }
        } else {
            AssignTarget::Name {
                name: base,
                name_span: base_span,
            }
        };
        self.expect_kind(&TokenKind::Eq)?;
        let value = self.parse_expr()?;
        Ok(AssignStmt { target, value })
    }

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
        // Bitwise binds tighter than comparison (SPEC §7.5).
        let mut left = self.parse_bitor()?;
        loop {
            let op = match self.current().kind {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::Ne => BinaryOp::Ne,
                TokenKind::LAngle => BinaryOp::Lt,
                TokenKind::Le => BinaryOp::Le,
                TokenKind::RAngle => BinaryOp::Gt,
                TokenKind::Ge => BinaryOp::Ge,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            let right = self.parse_bitor()?;
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

    fn parse_bitor(&mut self) -> Result<Expr> {
        let mut left = self.parse_bitxor()?;
        while self.at(TokenKind::Pipe) {
            let start = left.span.start;
            self.bump();
            let right = self.parse_bitxor()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitOr,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<Expr> {
        let mut left = self.parse_bitand()?;
        while self.at(TokenKind::Caret) {
            let start = left.span.start;
            self.bump();
            let right = self.parse_bitand()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitXor,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<Expr> {
        let mut left = self.parse_shift()?;
        while self.at(TokenKind::Amp) {
            let start = left.span.start;
            self.bump();
            let right = self.parse_shift()?;
            let end = right.span.end;
            left = Expr {
                kind: ExprKind::Binary {
                    op: BinaryOp::BitAnd,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span: Span::new(start, end),
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr> {
        let mut left = self.parse_add()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Shl => BinaryOp::Shl,
                TokenKind::Shr => BinaryOp::Shr,
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
        if self.at(TokenKind::Tilde) {
            self.bump();
            let expr = self.parse_unary()?;
            let end = expr.span.end;
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                },
                span: Span::new(start.start, end),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.at(TokenKind::Question) {
                let start = expr.span.start;
                self.bump();
                let end = self.prev_span().end;
                expr = Expr {
                    kind: ExprKind::Try(Box::new(expr)),
                    span: Span::new(start, end),
                };
                continue;
            }
            if self.at(TokenKind::Dot) {
                let start = expr.span.start;
                self.bump();
                let (name, name_span) = self.expect_ident()?;
                if self.at(TokenKind::LParen) {
                    self.bump();
                    let args = self.parse_arg_list()?;
                    self.expect_kind(&TokenKind::RParen)?;
                    let end = self.prev_span().end;
                    expr = Expr {
                        kind: ExprKind::MethodCall {
                            receiver: Box::new(expr),
                            method: name,
                            method_span: name_span,
                            args,
                        },
                        span: Span::new(start, end),
                    };
                } else {
                    let end = name_span.end;
                    expr = Expr {
                        kind: ExprKind::Field {
                            base: Box::new(expr),
                            name,
                            name_span,
                        },
                        span: Span::new(start, end),
                    };
                }
                continue;
            }
            if self.at(TokenKind::LBracket) {
                let start = expr.span.start;
                self.bump();
                let index = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBracket)?;
                let end = self.prev_span().end;
                expr = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                    span: Span::new(start, end),
                };
                continue;
            }
            break;
        }
        Ok(expr)
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
                let span = Span::new(start.start, self.current().span.end);
                self.bump();
                parse_string_literal(s, span)
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                self.bump();
                if self.at(TokenKind::LParen) {
                    self.bump();
                    let args = self.parse_arg_list()?;
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
            TokenKind::Match => self.parse_match(),
            TokenKind::While => self.parse_while(),
            TokenKind::Loop => self.parse_loop(),
            TokenKind::For => self.parse_for(),
            TokenKind::Break => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Break,
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::Continue => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Continue,
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::Return => {
                self.bump();
                // Optional value: parse when the next token can start an expression
                // (so bare `return` before `}` is allowed for Void).
                let value = if self.at_expr_start() {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };
                Ok(Expr {
                    kind: ExprKind::Return { value },
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
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
            TokenKind::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                if !self.at(TokenKind::RBracket) {
                    loop {
                        elems.push(self.parse_expr()?);
                        if self.at(TokenKind::Comma) {
                            self.bump();
                            // trailing comma ok
                            if self.at(TokenKind::RBracket) {
                                break;
                            }
                            continue;
                        }
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RBracket)?;
                Ok(Expr {
                    kind: ExprKind::ArrayLit(elems),
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            _ => Err(Error::at(
                format!("expected expression, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    /// Parse argument list until `)` (caller already consumed `(`).
    fn parse_arg_list(&mut self) -> Result<Vec<Arg>> {
        let mut args = Vec::new();
        if self.at(TokenKind::RParen) {
            return Ok(args);
        }
        loop {
            args.push(self.parse_arg()?);
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(args)
    }

    /// Named `name: expr` if `Ident` followed by `:`, else positional expression.
    fn parse_arg(&mut self) -> Result<Arg> {
        if matches!(self.current().kind, TokenKind::Ident(_))
            && self.peek_kind_is(1, TokenKind::Colon)
        {
            let (name, name_span) = self.expect_ident()?;
            self.expect_kind(&TokenKind::Colon)?;
            let value = self.parse_expr()?;
            Ok(Arg::Named {
                name,
                name_span,
                value,
            })
        } else {
            Ok(Arg::Pos(self.parse_expr()?))
        }
    }

    fn parse_if(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        let else_branch = if self.at(TokenKind::Else) {
            self.bump();
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

    fn parse_match(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::Match)?;
        let scrutinee = self.parse_expr()?;
        self.expect_kind(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            arms.push(self.parse_match_arm()?);
        }
        self.expect_kind(&TokenKind::RBrace)?;
        let end = self.prev_span().end;
        Ok(Expr {
            kind: ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span: Span::new(start.start, end),
        })
    }

    fn parse_while(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span.end;
        Ok(Expr {
            kind: ExprKind::While {
                cond: Box::new(cond),
                body,
            },
            span: Span::new(start.start, end),
        })
    }

    fn parse_loop(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::Loop)?;
        let body = self.parse_block()?;
        let end = body.span.end;
        Ok(Expr {
            kind: ExprKind::Loop { body },
            span: Span::new(start.start, end),
        })
    }

    /// `for name in start..end` / `start..=end` or `for name in collection { body }`
    fn parse_for(&mut self) -> Result<Expr> {
        let start_span = self.peek_span();
        self.expect_kind(&TokenKind::For)?;
        let (name, name_span) = self.expect_ident()?;
        self.expect_kind(&TokenKind::In)?;
        let range_start = self.parse_expr()?;
        let (end_expr, inclusive) = if self.at(TokenKind::DotDotEq) {
            self.bump();
            (Some(Box::new(self.parse_expr()?)), true)
        } else if self.at(TokenKind::DotDot) {
            self.bump();
            (Some(Box::new(self.parse_expr()?)), false)
        } else {
            // Collection iteration: `for x in xs`
            (None, false)
        };
        let body = self.parse_block()?;
        let end = body.span.end;
        Ok(Expr {
            kind: ExprKind::For {
                name,
                name_span,
                start: Box::new(range_start),
                end: end_expr,
                inclusive,
                body,
            },
            span: Span::new(start_span.start, end),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm> {
        let pattern = self.parse_pattern()?;
        // Optional `if guard` before the arm body (D111).
        let guard = if self.at(TokenKind::If) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(MatchArm {
            pattern,
            guard,
            body,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        let first = self.parse_pattern_atom()?;
        if !self.at(TokenKind::Pipe) {
            return Ok(first);
        }
        if matches!(first, Pattern::Else { .. }) {
            return Err(Error::at(
                "`else` cannot appear in an or-pattern",
                pattern_span(&first),
            ));
        }
        let start = pattern_span(&first).start;
        let mut alts = vec![first];
        while self.at(TokenKind::Pipe) {
            self.bump();
            let next = self.parse_pattern_atom()?;
            if matches!(next, Pattern::Else { .. }) {
                return Err(Error::at(
                    "`else` cannot appear in an or-pattern",
                    pattern_span(&next),
                ));
            }
            alts.push(next);
        }
        let end = pattern_span(alts.last().unwrap()).end;
        Ok(Pattern::Or {
            alts,
            span: Span::new(start, end),
        })
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern> {
        let span = self.peek_span();
        match &self.current().kind {
            TokenKind::Else => {
                self.bump();
                Ok(Pattern::Else { span })
            }
            TokenKind::String(s) => {
                let value = s.clone();
                self.bump();
                Ok(Pattern::StringLit {
                    value,
                    span: self.prev_span(),
                })
            }
            TokenKind::Int(n) => {
                let value = *n;
                self.bump();
                Ok(Pattern::IntLit {
                    value,
                    span: self.prev_span(),
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(Pattern::BoolLit {
                    value: true,
                    span: self.prev_span(),
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Pattern::BoolLit {
                    value: false,
                    span: self.prev_span(),
                })
            }
            TokenKind::Ident(_) => {
                let (name, name_span) = self.expect_ident()?;
                // Nullary `Not` (May): no parentheses. Payload variants require `(bind)`.
                // Bare name with following `{` is a value-match bind pattern (D110),
                // not a nullary variant — match arms always parse pattern then block.
                if !self.at(TokenKind::LParen) {
                    // `Not` remains a nullary variant for May match.
                    if name == "Not" || name == "Pass" || name == "Fail" || name == "Has" {
                        return Ok(Pattern::Variant {
                            name,
                            binds: Vec::new(),
                            bind_span: name_span,
                            span: name_span,
                        });
                    }
                    // Value-match bare bind (D110). `_` is discard catch-all without binding.
                    if name == "_" {
                        return Ok(Pattern::Else { span: name_span });
                    }
                    return Ok(Pattern::Bind {
                        name,
                        span: name_span,
                    });
                }
                self.bump();
                let mut binds = Vec::new();
                let mut bind_span = name_span;
                if !self.at(TokenKind::RParen) {
                    loop {
                        let (bind_raw, bspan) = self.expect_ident()?;
                        if binds.is_empty() {
                            bind_span = bspan;
                        }
                        // Pattern `_` is discard only here (D66).
                        binds.push(if bind_raw == "_" {
                            None
                        } else {
                            Some(bind_raw)
                        });
                        if self.at(TokenKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                self.expect_kind(&TokenKind::RParen)?;
                let end = self.prev_span().end;
                Ok(Pattern::Variant {
                    name,
                    binds,
                    bind_span,
                    span: Span::new(name_span.start, end),
                })
            }
            other => Err(Error::at(
                format!("expected match pattern, got {other:?}"),
                span,
            )),
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len().saturating_sub(1))]
    }

    fn at(&self, kind: TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(&kind)
    }

    /// Whether the current token can begin an expression (for optional `return` value).
    fn at_expr_start(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Int(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::String(_)
                | TokenKind::Ident(_)
                | TokenKind::If
                | TokenKind::Match
                | TokenKind::While
                | TokenKind::Loop
                | TokenKind::For
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Return
                | TokenKind::LBrace
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::Minus
                | TokenKind::Not
                | TokenKind::Tilde
        )
    }

    fn peek_kind_is(&self, offset: usize, kind: TokenKind) -> bool {
        let i = self.pos + offset;
        if i >= self.tokens.len() {
            return false;
        }
        std::mem::discriminant(&self.tokens[i].kind) == std::mem::discriminant(&kind)
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

fn pattern_span(p: &Pattern) -> Span {
    match p {
        Pattern::Variant { span, .. }
        | Pattern::StringLit { span, .. }
        | Pattern::IntLit { span, .. }
        | Pattern::BoolLit { span, .. }
        | Pattern::Or { span, .. }
        | Pattern::Bind { span, .. }
        | Pattern::Else { span } => *span,
    }
}

struct LetBind {
    name: String,
    name_span: Span,
    mutable: bool,
    ty: Option<TypeExpr>,
    init: Expr,
}

struct AssignStmt {
    target: AssignTarget,
    value: Expr,
}

enum BlockElem {
    Let(LetBind),
    Assign(AssignStmt),
    Expr(Expr),
}

fn block_elem_to_item(el: BlockElem) -> BlockItem {
    match el {
        BlockElem::Let(l) => BlockItem::Let {
            name: l.name,
            name_span: l.name_span,
            mutable: l.mutable,
            ty: l.ty,
            init: l.init,
        },
        BlockElem::Assign(a) => BlockItem::Assign {
            target: a.target,
            value: a.value,
        },
        BlockElem::Expr(e) => BlockItem::Expr(e),
    }
}

/// Expand a string token into plain `String` or `Interpolate` (handles `{{` / `}}`).
fn parse_string_literal(content: String, span: Span) -> Result<Expr> {
    let mut parts: Vec<InterpPart> = Vec::new();
    let mut lit = String::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' => {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                if !lit.is_empty() {
                    parts.push(InterpPart::Lit(std::mem::take(&mut lit)));
                }
                i += 1;
                let expr_start = i;
                let mut depth = 1usize;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(Error::at("unclosed interpolation `{`", span));
                }
                let expr_src: String = chars[expr_start..i].iter().collect();
                i += 1; // consume closing `}`
                let expr = parse_embedded_expr(&expr_src, span)?;
                parts.push(InterpPart::Expr(expr));
            }
            '}' => {
                if i + 1 < chars.len() && chars[i + 1] == '}' {
                    lit.push('}');
                    i += 2;
                    continue;
                }
                return Err(Error::at(
                    "unmatched `}` in string (use `}}` for a literal brace)",
                    span,
                ));
            }
            ch => {
                lit.push(ch);
                i += 1;
            }
        }
    }
    if parts.is_empty() {
        return Ok(Expr {
            kind: ExprKind::String(lit),
            span,
        });
    }
    if !lit.is_empty() {
        parts.push(InterpPart::Lit(lit));
    }
    Ok(Expr {
        kind: ExprKind::Interpolate(parts),
        span,
    })
}

fn parse_embedded_expr(src: &str, parent_span: Span) -> Result<Expr> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(Error::at("empty interpolation expression", parent_span));
    }
    let tokens = Lexer::new(trimmed).tokenize().map_err(|e| {
        Error::at(
            format!("invalid interpolation expression: {}", e.message),
            parent_span,
        )
    })?;
    let mut p = Parser::new(tokens);
    let expr = p.parse_expr()?;
    if !p.at(TokenKind::Eof) {
        return Err(Error::at(
            "trailing tokens in interpolation expression",
            parent_span,
        ));
    }
    Ok(expr)
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

    #[test]
    fn parse_loops() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                mut n = 0
                while n < 3 {
                    n = n + 1
                    if n == 2 { continue }
                    if n == 3 { break }
                }
                loop { break }
                for i in 0..=2 {
                    println(i)
                }
                for j in 0..2 {
                    println(j)
                }
                0
            }
        "#,
        );
        assert_eq!(module.items.len(), 1);
    }

    #[test]
    fn parse_trials() {
        let module = parse(
            r#"
            fn half(x: Int64) -> Trial<Int64, Int64> {
                if x % 2 == 0 { Pass(x / 2) } else { Fail(x) }
            }
            fn quarter(x: Int64) -> Trial<Int64, Int64> {
                let a = half(x)?
                Pass(a)
            }
            fn main() -> Int64 {
                match quarter(20) {
                    Pass(v) { println(v) }
                    Fail(e) { println(e) }
                }
                0
            }
        "#,
        );
        assert_eq!(module.items.len(), 3);
        match &module.items[0] {
            Item::Function(f) => {
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.return_type.as_ref().unwrap().name, "Trial");
                assert_eq!(f.return_type.as_ref().unwrap().args.len(), 2);
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_nested_type_args_double_rangle() {
        // Lexer glues `>>` as Shr; type parser must split for nested generics.
        let module = parse(
            r#"
            fn g() -> May<Array<Int64>> {
                Has([1, 2])
            }
            fn h() -> Trial<May<Int64>, String> {
                Pass(Has(1))
            }
            "#,
        );
        assert_eq!(module.items.len(), 2);
        match &module.items[0] {
            Item::Function(f) => {
                let rt = f.return_type.as_ref().unwrap();
                assert_eq!(rt.name, "May");
                assert_eq!(rt.args.len(), 1);
                match &rt.args[0] {
                    TypeArg::Type(inner) => {
                        assert_eq!(inner.name, "Array");
                        assert_eq!(inner.args.len(), 1);
                    }
                    _ => panic!("expected type arg"),
                }
            }
            _ => panic!("expected function"),
        }
    }

    #[test]
    fn parse_type_and_methods() {
        let module = parse(
            r#"
            type Point(x: Int64, y: Int64) {
                export fn lengthSquared() -> Int64 {
                    x * x + y * y
                }
                export fn movedBy(dx: Int64, dy: Int64) -> Point {
                    Point(x: x + dx, y: y + dy)
                }
            }
            fn main() -> Int64 {
                let p = Point(x: 3, y: 4)
                println(p.lengthSquared())
                0
            }
        "#,
        );
        assert_eq!(module.items.len(), 2);
        match &module.items[0] {
            Item::Type(t) => {
                assert_eq!(t.name, "Point");
                assert_eq!(t.params.len(), 2);
                assert!(t.fields.is_empty());
                assert_eq!(t.methods.len(), 2);
                assert!(t.methods[0].exported);
            }
            _ => panic!("expected type"),
        }
    }

    #[test]
    fn parse_type_without_body() {
        let module = parse(
            r#"
            type Header(name: String, value: String)
            type Marker
            type EmptyBody(n: Int64) {}
            fn main() -> Int64 { 0 }
        "#,
        );
        assert_eq!(module.items.len(), 4);
        match &module.items[0] {
            Item::Type(t) => {
                assert_eq!(t.name, "Header");
                assert_eq!(t.params.len(), 2);
                assert!(t.fields.is_empty());
                assert!(t.methods.is_empty());
            }
            _ => panic!("expected Header type"),
        }
        match &module.items[1] {
            Item::Type(t) => {
                assert_eq!(t.name, "Marker");
                assert!(t.params.is_empty());
                assert!(t.fields.is_empty());
                assert!(t.methods.is_empty());
            }
            _ => panic!("expected Marker type"),
        }
        match &module.items[2] {
            Item::Type(t) => {
                assert_eq!(t.name, "EmptyBody");
                assert_eq!(t.params.len(), 1);
                assert!(t.fields.is_empty());
                assert!(t.methods.is_empty());
            }
            _ => panic!("expected EmptyBody type"),
        }
    }

    #[test]
    fn parse_cstruct_fields() {
        let module = parse(
            r#"
            type Regs {
                mut a: UInt32
                mut cc: Array<Byte, 4>
            }
            fn main() -> Int64 {
                mut r = Regs()
                r.a = 0xFF
                r.cc[0] = 65
                0
            }
        "#,
        );
        match &module.items[0] {
            Item::Type(t) => {
                assert_eq!(t.name, "Regs");
                assert!(t.params.is_empty());
                assert_eq!(t.fields.len(), 2);
                assert!(t.fields[0].mutable);
                assert_eq!(t.fields[1].ty.name, "Array");
                match &t.fields[1].ty.args[1] {
                    TypeArg::Int { value: 4, .. } => {}
                    _ => panic!("expected Array length 4"),
                }
            }
            _ => panic!("expected type"),
        }
    }

    #[test]
    fn parse_hex_and_bitwise() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let x = 0x01020304 & 0xFFFF
                x
            }
        "#,
        );
        assert_eq!(module.items.len(), 1);
    }
}
