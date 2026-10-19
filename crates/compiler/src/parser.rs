//! Recursive-descent + precedence parser (R0–R8).

use crate::ast::*;
use crate::diagnostic;
use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::span::Span;

/// Keywords that only introduce a declaration or a clause: `type`, `extend`,
/// `trait`, `conform`, `import … from`, `export`, `extern`, `x as T`,
/// `for x in`, `T where …`.
/// Wherever a name goes they are names: argument labels, parameters, fields,
/// bindings, and references to them (D198).
fn soft_keyword(kind: &TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Type => Some("type"),
        TokenKind::Extend => Some("extend"),
        TokenKind::Trait => Some("trait"),
        TokenKind::Conform => Some("conform"),
        TokenKind::Import => Some("import"),
        TokenKind::From => Some("from"),
        TokenKind::Export => Some("export"),
        TokenKind::Extern => Some("extern"),
        TokenKind::As => Some("as"),
        TokenKind::In => Some("in"),
        TokenKind::Where => Some("where"),
        _ => None,
    }
}

/// An identifier, or a soft keyword in a name's place.
fn is_name_token(kind: &TokenKind) -> bool {
    matches!(kind, TokenKind::Ident(_)) || soft_keyword(kind).is_some()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Synthetic `>` closers left after splitting a `>>` (Shr) in type args (D113).
    extra_rangles: usize,
    /// Recoverable errors inside blocks that still produced a partial AST.
    soft_errors: Vec<Error>,
    /// Original source (byte offsets match token spans). Needed so JSX text
    /// children can be sliced out; empty means raw text is not available.
    source: String,
    /// Expression nesting depth (sub-expressions, binary-operator chains,
    /// prefix operators). Capped so hostile input gets a diagnostic instead
    /// of overflowing the stack here or in the recursive passes after parsing.
    depth: usize,
}

/// Deepest expression nesting the parser accepts (see `Parser::depth`).
const MAX_EXPR_NESTING: usize = 1000;

/// User-facing message for a statement outside any function.
pub const TOP_LEVEL_STMT_MSG: &str =
    "a statement cannot be at the top level \
     (only import, type, trait, conform, extend, extern, fn, let, and mut go here)";

/// HTTP method idents recognized as route clauses inside a type body only
/// (**D150**). Not reserved keywords elsewhere.
fn is_route_http_method(s: &str) -> bool {
    matches!(
        s,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    )
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            extra_rangles: 0,
            soft_errors: Vec::new(),
            source: String::new(),
            depth: 0,
        }
    }

    /// Attach the source buffer the tokens were lexed from (JSX text children).
    pub fn with_source(mut self, src: impl Into<String>) -> Self {
        self.source = src.into();
        self
    }

    pub fn parse_module(&mut self) -> Result<Module> {
        let mut items = Vec::new();
        let mut errors: Vec<Error> = Vec::new();
        while !self.at(TokenKind::Eof) {
            match self.parse_top_item() {
                Ok(Some(item)) => items.push(item),
                Ok(None) => {}
                Err(e) => {
                    // Soft errors from a partial function body may already be recorded;
                    // flatten hard errors too.
                    errors.extend(e.into_diagnostics());
                    if !self.recover_to_next_item() {
                        break;
                    }
                    if errors.len() >= 32 {
                        break;
                    }
                }
            }
        }
        errors.append(&mut self.soft_errors);
        let module = Module { items };
        if !errors.is_empty() {
            return Err(Error::multi(errors).with_recovered_module(module));
        }
        Ok(module)
    }

    /// One top-level item. A statement is not one: it is parsed whole and
    /// reported (soft), so the items after it still parse; `None` then.
    fn parse_top_item(&mut self) -> Result<Option<Item>> {
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
            Ok(Some(Item::Import(self.parse_import()?)))
        } else if self.at(TokenKind::Type) {
            Ok(Some(Item::Type(self.parse_type_def(exported)?)))
        } else if self.at(TokenKind::Extend) {
            if exported {
                return Err(Error::at(
                    "an `extend` block is not exported; mark its methods `export`",
                    self.peek_span(),
                ));
            }
            Ok(Some(Item::Extend(self.parse_extension()?)))
        } else if self.at(TokenKind::Trait) {
            Ok(Some(Item::Trait(self.parse_trait(exported)?)))
        } else if self.at(TokenKind::Conform) {
            if exported {
                return Err(Error::at(
                    "a conformance is not exported; it holds wherever the type and the trait are visible",
                    self.peek_span(),
                ));
            }
            Ok(Some(Item::Conform(self.parse_conformance()?)))
        } else if self.at(TokenKind::Extern) {
            Ok(Some(Item::Extern(self.parse_extern(exported)?)))
        } else if self.at(TokenKind::Fn) {
            Ok(Some(Item::Function(self.parse_function(exported)?)))
        } else if self.at(TokenKind::Let) || self.at(TokenKind::Mut) {
            Ok(Some(Item::Global(self.parse_global(exported)?)))
        } else {
            if exported {
                return Err(Error::at(
                    "`export` only applies to declarations, not statements",
                    self.peek_span(),
                ));
            }
            let stmt = self.parse_top_level_stmt()?;
            self.soft_errors.push(
                Error::coded_at(
                    diagnostic::CODE_TOP_LEVEL_STMT,
                    TOP_LEVEL_STMT_MSG,
                    block_item_span(&stmt),
                )
                .with_help("move the statement into `fn main`"),
            );
            Ok(None)
        }
    }

    /// After a top-level parse error, skip to the next likely item start.
    /// Returns false if EOF is reached without a recovery point.
    fn recover_to_next_item(&mut self) -> bool {
        // Always consume at least one token so we cannot spin on the same error.
        if !self.at(TokenKind::Eof) {
            self.bump();
        }
        let mut depth: i32 = 0;
        while !self.at(TokenKind::Eof) {
            match &self.current().kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                TokenKind::Fn
                | TokenKind::Type
                | TokenKind::Extend
                | TokenKind::Trait
                | TokenKind::Conform
                | TokenKind::Extern
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Let
                | TokenKind::Mut
                    if depth == 0 =>
                {
                    return true;
                }
                _ => {
                    self.bump();
                }
            }
        }
        false
    }

    /// A statement where an item belongs: an assignment or an expression.
    fn parse_top_level_stmt(&mut self) -> Result<BlockItem> {
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

    /// `import { a, b as c } from "…"`, `import ns from "…"`, or
    /// `import ns, { a, b as c } from "…"`.
    fn parse_import(&mut self) -> Result<Import> {
        self.expect_kind(&TokenKind::Import)?;
        let kind = if self.at(TokenKind::LBrace) {
            ImportKind::Selective(self.parse_import_brace_list()?)
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
            if self.at(TokenKind::Comma) {
                self.bump();
                if !self.at(TokenKind::LBrace) {
                    return Err(Error::at(
                        "expected `{` after namespace binding in combined import \
                         (`import ns, { name, … } from \"…\"`)",
                        self.peek_span(),
                    ));
                }
                let names = self.parse_import_brace_list()?;
                ImportKind::Combined {
                    binding,
                    binding_span,
                    names,
                }
            } else {
                ImportKind::Namespace {
                    binding,
                    binding_span,
                }
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

    /// `{ name [as alias], … }` — brace-delimited selective import names.
    fn parse_import_brace_list(&mut self) -> Result<Vec<ImportName>> {
        self.expect_kind(&TokenKind::LBrace)?;
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
                    // Trailing comma before `}` is fine.
                    if self.at(TokenKind::RBrace) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RBrace)?;
        Ok(names)
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
        let ty = if self.at(TokenKind::Colon) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect_kind(&TokenKind::Eq)?;
        let init = self.parse_expr()?;
        Ok(Global {
            exported,
            mutable,
            name,
            name_span,
            ty,
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
                if p.mutable {
                    return Err(Error::at("an `extern fn` parameter cannot be `mut`", p.name_span));
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

        // Optional type parameters: `type Box<T>` / `type Trial<P, F>` (D118),
        // with bounds: `type SortedSet<T: Ordered>` (D228).
        let (type_params, param_bounds): (Vec<String>, Vec<Vec<(String, Span)>>) =
            if self.at(TokenKind::LAngle) {
                self.parse_bounded_type_params()?
                    .into_iter()
                    .map(|param| (param.name, param.traits))
                    .unzip()
            } else {
                (Vec::new(), Vec::new())
            };

        // Union / singleton alias: `type Name = A | B | …`, `type Home = "home"` (D118/D131).
        if self.at(TokenKind::Eq) {
            self.bump();
            let members = self.parse_union_members()?;
            if members.is_empty() {
                return Err(Error::at(
                    "type alias requires at least one member",
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
                routes: Vec::new(),
                conversions: Vec::new(),
                union_members: Some(members),
                traits: Vec::new(),
                conditional: Vec::new(),
                derived: Vec::new(),
                param_bounds: param_bounds.clone(),
                bounds: Vec::new(),
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
                    let param = self.parse_param()?;
                    if param.mutable {
                        return Err(Error::at(
                            "a constructor parameter cannot be `mut`",
                            param.name_span,
                        ));
                    }
                    params.push(param);
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
        let mut routes = Vec::new();
        let mut conversions = Vec::new();
        if self.at(TokenKind::LBrace) {
            self.bump();
            let mut type_body_errors: Vec<Error> = Vec::new();
            while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let start_pos = self.pos;
                let item_res: Result<()> = if self.at(TokenKind::From)
                    || (self.at(TokenKind::Export) && self.peek_kind_is(1, TokenKind::From))
                {
                    self.parse_conversion().map(|c| {
                        conversions.push(c);
                    })
                } else if self.at_accessor() || self.at_exported_accessor() {
                    self.parse_accessor().map(|m| {
                        methods.push(m);
                    })
                } else if self.at(TokenKind::Fn) || self.at(TokenKind::Export) {
                    self.parse_method().map(|m| {
                        methods.push(m);
                    })
                } else if self.at(TokenKind::Mut) || self.at(TokenKind::Let) {
                    self.parse_field_decl().map(|f| {
                        fields.push(f);
                    })
                } else if self.at_route_method() {
                    self.parse_route_clause().map(|r| {
                        routes.push(r);
                    })
                } else {
                    Err(Error::at(
                        "expected field (`mut`/`let name: Type`), method, accessor \
                         (`get name: Type { … }`), or route (`GET \"/…\" { … }`) in type body",
                        self.peek_span(),
                    ))
                };
                match item_res {
                    Ok(()) => {}
                    Err(e) => {
                        type_body_errors.push(e);
                        if self.pos == start_pos
                            && !self.at(TokenKind::Eof)
                            && !self.at(TokenKind::RBrace)
                        {
                            self.bump();
                        }
                        // Seek next method/field/route or closing brace.
                        while !self.at(TokenKind::Eof) && !self.at(TokenKind::RBrace) {
                            if self.at(TokenKind::Fn)
                                || self.at(TokenKind::Export)
                                || self.at(TokenKind::From)
                                || self.at(TokenKind::Mut)
                                || self.at(TokenKind::Let)
                                || self.at_route_method()
                                || self.at_accessor()
                            {
                                break;
                            }
                            self.bump();
                        }
                        if type_body_errors.len() >= 32 {
                            break;
                        }
                    }
                }
            }
            if self.at(TokenKind::RBrace) {
                self.bump();
            } else {
                type_body_errors.push(Error::at("expected `}`", self.peek_span()));
            }
            if !type_body_errors.is_empty() {
                return Err(Error::multi(type_body_errors));
            }
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
        // Methods on generic products are monomorphized with the type (D118 polish).

        Ok(TypeDef {
            exported,
            name,
            name_span,
            type_params,
            params,
            fields,
            methods,
            routes,
            conversions,
            union_members: None,
            traits: Vec::new(),
            conditional: Vec::new(),
            derived: Vec::new(),
            param_bounds: param_bounds.clone(),
            bounds: Vec::new(),
        })
    }

    /// `[export] from(source: S) -> T { … }` in a type body (D191 §3).
    fn parse_conversion(&mut self) -> Result<Conversion> {
        let exported = if self.at(TokenKind::Export) {
            self.bump();
            true
        } else {
            false
        };
        let span = self.peek_span();
        self.expect_kind(&TokenKind::From)?;
        self.expect_kind(&TokenKind::LParen)?;
        if self.at(TokenKind::RParen) {
            return Err(Error::at(
                "a `from` clause takes one parameter, the value it converts from",
                self.peek_span(),
            ));
        }
        let param = self.parse_param()?;
        if param.mutable {
            return Err(Error::at("a `from` clause's parameter cannot be `mut`", param.name_span));
        }
        if self.at(TokenKind::Comma) {
            return Err(Error::at(
                "a `from` clause takes one parameter, the value it converts from",
                self.peek_span(),
            ));
        }
        self.expect_kind(&TokenKind::RParen)?;
        if !self.at(TokenKind::Arrow) {
            return Err(Error::at(
                "a `from` clause needs a return type: the type itself, or `Trial<Type, E>`",
                self.peek_span(),
            ));
        }
        self.bump();
        let return_type = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(Conversion {
            exported,
            span,
            param,
            return_type,
            body,
        })
    }

    /// Fixed set of HTTP method identifiers recognized only in type bodies.
    fn at_route_method(&self) -> bool {
        match &self.current().kind {
            TokenKind::Ident(s) => is_route_http_method(s),
            _ => false,
        }
    }

    /// `GET "/path" { … }` / `PUT "/p/:id" (id: Int64) -> T { … }`
    fn parse_route_clause(&mut self) -> Result<RouteClause> {
        let (method, method_span) = match &self.current().kind {
            TokenKind::Ident(s) if is_route_http_method(s) => {
                let m = s.clone();
                let sp = self.current().span;
                self.bump();
                (m, sp)
            }
            _ => {
                return Err(Error::at(
                    "expected HTTP method (GET, POST, PUT, …)",
                    self.peek_span(),
                ));
            }
        };
        let (pattern, pattern_span) = self.expect_string()?;
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
        let return_type = if self.at(TokenKind::Arrow) {
            self.bump();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        Ok(RouteClause {
            method,
            method_span,
            pattern,
            pattern_span,
            params,
            return_type,
            body,
        })
    }

    /// `<T, U>` type parameter list on a type definition.
    /// `A | B | C` type members (atoms only — each may carry trailing `!` / `?` marks).
    fn parse_union_members(&mut self) -> Result<Vec<TypeExpr>> {
        let mut members = Vec::new();
        loop {
            members.push(self.parse_type_with_where()?);
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

    /// `extend Type as receiver { [export] fn … }` (D196).
    fn parse_extension(&mut self) -> Result<Extension> {
        let span = self.peek_span();
        self.expect_kind(&TokenKind::Extend)?;
        // `extend Array<T: Equatable>` names type parameters, with bounds or
        // as one capital letter (`extend Array<T>`); `extend Array<String>`
        // names a type.
        let parameter = match self.tokens.get(self.pos + 2).map(|token| &token.kind) {
            Some(TokenKind::Ident(name)) => {
                self.peek_kind_is(3, TokenKind::Colon)
                    || (name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase()))
            }
            _ => false,
        };
        let generic = parameter && self.peek_kind_is(1, TokenKind::LAngle);
        let (ty, params) = if generic {
            self.parse_conformance_target()?
        } else {
            (self.parse_type()?, Vec::new())
        };
        if self.at(TokenKind::As) {
            return Err(Error::at(
                "an `extend` block's methods call their receiver `self`; drop `as …`",
                self.peek_span(),
            ));
        }
        self.expect_kind(&TokenKind::LBrace)?;
        let mut methods = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at_accessor() || self.at_exported_accessor() {
                methods.push(self.parse_accessor()?);
                continue;
            }
            if !(self.at(TokenKind::Fn) || self.at(TokenKind::Export)) {
                return Err(Error::at(
                    "an `extend` block holds methods (`fn` / `export fn`) and getters",
                    self.peek_span(),
                ));
            }
            methods.push(self.parse_method()?);
        }
        self.expect_kind(&TokenKind::RBrace)?;
        Ok(Extension {
            span,
            ty,
            params,
            methods,
        })
    }

    /// `[export] trait Name [: Parent & Parent] { fn … }` (D228). A member
    /// without a body is required; one with a body is provided.
    fn parse_trait(&mut self, exported: bool) -> Result<TraitDef> {
        self.expect_kind(&TokenKind::Trait)?;
        let (name, name_span) = self.expect_ident()?;
        let mut parents = Vec::new();
        if self.at(TokenKind::Colon) {
            self.bump();
            loop {
                parents.push(self.expect_ident()?);
                if self.at(TokenKind::Amp) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::LBrace)?;
        let mut members = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Export) {
                return Err(Error::at(
                    "a trait's members are public; drop `export`",
                    self.peek_span(),
                ));
            }
            let (member_name, member_span, params, return_type) = if self.at_accessor() {
                let (method, _) = self.parse_accessor_head()?;
                (method.name, method.name_span, method.params, method.return_type)
            } else if self.at(TokenKind::Fn) {
                self.bump();
                let (name, span) = self.expect_ident()?;
                let params = self.parse_param_list()?;
                let return_type = if self.at(TokenKind::Arrow) {
                    self.bump();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                (name, span, params, return_type)
            } else {
                return Err(Error::at(
                    "a trait holds methods (`fn name(…)`) and accessors (`get name: Type`, `set name(value: Type)`), with a body or without",
                    self.peek_span(),
                ));
            };
            let body = if self.at(TokenKind::LBrace) {
                Some(self.parse_block()?)
            } else {
                None
            };
            members.push(TraitMember {
                name: member_name,
                name_span: member_span,
                params,
                return_type,
                body,
            });
        }
        self.expect_kind(&TokenKind::RBrace)?;
        Ok(TraitDef {
            exported,
            name,
            name_span,
            parents,
            members,
        })
    }

    /// `conform Type to Trait, Trait [{ fn … }]` (D228).
    fn parse_conformance(&mut self) -> Result<Conformance> {
        let span = self.peek_span();
        self.expect_kind(&TokenKind::Conform)?;
        let (ty, params) = self.parse_conformance_target()?;
        match &self.current().kind {
            TokenKind::Ident(word) if word == "to" => self.bump(),
            _ => {
                return Err(Error::at(
                    "expected `to` and the traits: `conform Type to Trait`",
                    self.peek_span(),
                ))
            }
        }
        let mut traits = vec![self.expect_ident()?];
        while self.at(TokenKind::Comma) {
            self.bump();
            traits.push(self.expect_ident()?);
        }
        let mut methods = Vec::new();
        if self.at(TokenKind::LBrace) {
            self.bump();
            while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                if self.at(TokenKind::Export) {
                    return Err(Error::at(
                        "a trait's members are public; drop `export`",
                        self.peek_span(),
                    ));
                }
                if self.at_accessor() {
                    methods.push(self.parse_accessor()?);
                    continue;
                }
                if !self.at(TokenKind::Fn) {
                    return Err(Error::at(
                        "a conformance holds the traits' methods (`fn name(…) { … }`) and accessors (`get name: Type { … }`)",
                        self.peek_span(),
                    ));
                }
                let mut method = self.parse_method()?;
                method.exported = true;
                methods.push(method);
            }
            self.expect_kind(&TokenKind::RBrace)?;
        }
        Ok(Conformance {
            span,
            ty,
            params,
            traits,
            methods,
        })
    }

    /// `Type` or `Type<T: Bound & Bound, U>` after `conform` (D228).
    fn parse_conformance_target(&mut self) -> Result<(TypeExpr, Vec<TypeParamBound>)> {
        let start = self.peek_span();
        let (name, _) = self.expect_ident()?;
        let mut params = Vec::new();
        if self.at(TokenKind::LAngle) {
            self.bump();
            loop {
                let (param, param_span) = self.expect_ident()?;
                let traits = self.parse_bounds()?;
                params.push(TypeParamBound {
                    name: param,
                    span: param_span,
                    traits,
                });
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            self.expect_kind(&TokenKind::RAngle)?;
        }
        let end = self.tokens[self.pos.saturating_sub(1)].span;
        let args = params
            .iter()
            .map(|param| {
                TypeArg::Type(TypeExpr {
                    name: param.name.clone(),
                    args: Vec::new(),
                    span: param.span,
                    lit: None,
                    mark: None,
                    union_members: None,
                    fn_type: None,
                    where_clause: None,
                })
            })
            .collect();
        let ty = TypeExpr {
            name,
            args,
            span: Span::new(start.start, end.end),
            lit: None,
            mark: None,
            union_members: None,
            fn_type: None,
            where_clause: None,
        };
        Ok((ty, params))
    }

    /// `<T: A & B, U>` after a function's name, or nothing (D228).
    fn parse_bounded_type_params(&mut self) -> Result<Vec<TypeParamBound>> {
        let mut params = Vec::new();
        if !self.at(TokenKind::LAngle) {
            return Ok(params);
        }
        self.bump();
        loop {
            let (name, span) = self.expect_ident()?;
            if params.iter().any(|param: &TypeParamBound| param.name == name) {
                return Err(Error::at(format!("duplicate type parameter `{name}`"), span));
            }
            let traits = self.parse_bounds()?;
            params.push(TypeParamBound { name, span, traits });
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokenKind::RAngle)?;
        Ok(params)
    }

    /// `: A & B` after a type parameter, or nothing (D228).
    fn parse_bounds(&mut self) -> Result<Vec<(String, Span)>> {
        let mut traits = Vec::new();
        if self.at(TokenKind::Colon) {
            self.bump();
            loop {
                traits.push(self.expect_ident()?);
                if self.at(TokenKind::Amp) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        Ok(traits)
    }

    /// `get name` / `set name` at a member's start (D228): `get` and `set` are
    /// words only there.
    fn at_accessor(&self) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(word) if word == "get" || word == "set")
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| is_name_token(&token.kind))
    }

    fn at_exported_accessor(&self) -> bool {
        self.at(TokenKind::Export)
            && matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.kind),
                Some(TokenKind::Ident(word)) if word == "get" || word == "set"
            )
    }

    /// `get name: T` or `set name(value: T)`, without the body: the method it
    /// is (`get_name` / `set_name`, public) and the property's name.
    fn parse_accessor_head(&mut self) -> Result<(Method, String)> {
        let getter = matches!(&self.current().kind, TokenKind::Ident(word) if word == "get");
        self.bump();
        let (property, property_span) = self.expect_ident()?;
        let (name, params, return_type) = if getter {
            self.expect_kind(&TokenKind::Colon)?;
            (getter_name(&property), Vec::new(), Some(self.parse_type()?))
        } else {
            let mut params = self.parse_param_list()?;
            if params.len() != 1 {
                return Err(Error::at(
                    "a setter takes one parameter, the new value: `set name(value: Type)`",
                    property_span,
                ));
            }
            if self.at(TokenKind::Arrow) {
                return Err(Error::at("a setter returns nothing", self.peek_span()));
            }
            // Called by position: `x.name = value` has no label to give.
            params[0].positional = true;
            (setter_name(&property), params, None)
        };
        let method = Method {
            exported: true,
            name,
            name_span: property_span,
            params,
            return_type,
            body: Block {
                items: Vec::new(),
                tail: None,
                span: property_span,
            },
            type_params: Vec::new(),
            provided: None,
        };
        Ok((method, property))
    }

    /// `get name: T { … }` / `set name(value: T) { … }` in a type, a
    /// conformance, or an `extend` block (D228). Accessors are public.
    fn parse_accessor(&mut self) -> Result<Method> {
        if self.at(TokenKind::Export) {
            return Err(Error::at(
                "accessors are public; drop `export`",
                self.peek_span(),
            ));
        }
        let (mut method, _) = self.parse_accessor_head()?;
        method.body = self.parse_block()?;
        Ok(method)
    }

    /// `(param, …)` after a name.
    fn parse_param_list(&mut self) -> Result<Vec<Param>> {
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
        Ok(params)
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
        let type_params = self.parse_bounded_type_params()?;
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
            type_params,
            provided: None,
        })
    }

    fn parse_function(&mut self, exported: bool) -> Result<Function> {
        self.expect_kind(&TokenKind::Fn)?;
        let (name, name_span) = self.expect_ident()?;
        let type_params = self.parse_bounded_type_params()?;
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
            type_params,
            generic: Vec::new(),
        })
    }

    fn parse_param(&mut self) -> Result<Param> {
        // `mut name: T` (D199): the caller's binding, written in place.
        let mutable = if self.at(TokenKind::Mut) {
            self.bump();
            true
        } else {
            false
        };
        let (name, name_span) = self.expect_ident()?;
        let positional = if self.at(TokenKind::Bang) {
            self.bump();
            true
        } else {
            false
        };
        self.expect_kind(&TokenKind::Colon)?;
        let ty = self.parse_type()?;
        // Defaults allowed on `!` formals too (filled at call site; C extern still
        // receives every argument after defaults expand).
        let default = if self.at(TokenKind::Eq) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        if mutable && default.is_some() {
            return Err(Error::at("a `mut` parameter cannot have a default", name_span));
        }
        Ok(Param {
            name,
            name_span,
            positional,
            mutable,
            ty,
            default,
        })
    }

    /// Type in any position: atom or inline member-set `A | B` (D131).
    fn parse_type(&mut self) -> Result<TypeExpr> {
        let first = self.parse_type_with_where()?;
        if !self.at(TokenKind::Pipe) {
            return Ok(first);
        }
        let start = first.span.start;
        let mut members = vec![first];
        while self.at(TokenKind::Pipe) {
            self.bump();
            members.push(self.parse_type_with_where()?);
        }
        let end = members.last().map(|m| m.span.end).unwrap_or(start);
        Ok(TypeExpr {
            name: String::new(),
            args: Vec::new(),
            span: Span::new(start, end),
            lit: None,
            mark: None,
            union_members: Some(members),
            fn_type: None,
            where_clause: None,
        })
    }

    fn parse_type_with_where(&mut self) -> Result<TypeExpr> {
        let mut ty = self.parse_type_atom()?;
        if self.at(TokenKind::Where) {
            let start = ty.span.start;
            self.bump();
            let expr = self.parse_where_or()?;
            let end = self.prev_span().end;
            ty.where_clause = Some(WhereClause {
                expr,
                span: Span::new(start, end),
            });
            ty.span = Span::new(start, end);
        }
        Ok(ty)
    }

    /// Single type atom: named, generic app, or literal (`0`, `true`, `"x"`).
    fn parse_type_atom(&mut self) -> Result<TypeExpr> {
        // Literal types (D118/D131): `0`, `true`, `"x"` in any type position.
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
                union_members: None,
                fn_type: None,
                where_clause: None,
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
                union_members: None,
                fn_type: None,
                where_clause: None,
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
                union_members: None,
                fn_type: None,
                where_clause: None,
            });
        }

        if self.at(TokenKind::LParen) {
            if self.is_fn_type_lparen() {
                return self.parse_fn_type();
            }
            let start = self.peek_span();
            self.bump();
            let mut inner = self.parse_type()?;
            self.expect_kind(&TokenKind::RParen)?;
            let mark = self.parse_type_mark();
            let end = mark.map(|_| self.prev_span().end).unwrap_or(self.prev_span().end);
            inner.mark = mark.or(inner.mark);
            inner.span = Span::new(start.start, end);
            return Ok(inner);
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
            union_members: None,
            fn_type: None,
            where_clause: None,
        })
    }

    /// `(a: T, b: U) -> R` / `() -> Void` (D132).
    fn parse_fn_type(&mut self) -> Result<TypeExpr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::LParen)?;
        let mut params: Vec<(String, TypeExpr)> = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                let (name, name_span) = self.expect_ident()?;
                if !self.at(TokenKind::Colon) {
                    return Err(Error::at(
                        "function type parameters are `name: Type`",
                        name_span,
                    ));
                }
                self.bump();
                let ty = self.parse_type()?;
                params.push((name, ty));
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RParen)?;
        if !self.at(TokenKind::Arrow) {
            return Err(Error::at(
                "expected `->` after function type parameters",
                self.peek_span(),
            ));
        }
        self.bump();
        let ret = self.parse_type()?;
        let span = Span::new(start.start, ret.span.end);
        Ok(TypeExpr {
            name: String::new(),
            args: Vec::new(),
            span,
            lit: None,
            mark: None,
            union_members: None,
            fn_type: Some(Box::new(crate::ast::FnType { params, ret })),
            where_clause: None,
        })
    }

    fn is_fn_type_lparen(&self) -> bool {
        let i = self.pos + 1;
        if i >= self.tokens.len() {
            return false;
        }
        match &self.tokens[i].kind {
            TokenKind::RParen => true,
            kind if is_name_token(kind) => {
                let j = i + 1;
                j < self.tokens.len() && matches!(self.tokens[j].kind, TokenKind::Colon)
            }
            _ => false,
        }
    }

    fn parse_where_or(&mut self) -> Result<WhereExpr> {
        let mut left = self.parse_where_and()?;
        while self.at(TokenKind::Pipe) {
            self.bump();
            let right = self.parse_where_and()?;
            left = WhereExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_where_and(&mut self) -> Result<WhereExpr> {
        let mut left = self.parse_where_primary()?;
        while self.at(TokenKind::Amp) {
            self.bump();
            let right = self.parse_where_primary()?;
            left = WhereExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_where_primary(&mut self) -> Result<WhereExpr> {
        if self.at(TokenKind::LParen) {
            self.bump();
            let inner = self.parse_where_or()?;
            self.expect_kind(&TokenKind::RParen)?;
            return Ok(inner);
        }
        let start = self.peek_span();
        let pred = self.parse_where_pred()?;
        let message = if let TokenKind::String(s) = &self.current().kind {
            let s = s.clone();
            self.bump();
            Some(s)
        } else {
            None
        };
        let end = self.prev_span().end;
        Ok(WhereExpr::Atom(WhereAtom {
            pred,
            message,
            span: Span::new(start.start, end),
        }))
    }

    fn parse_where_pred(&mut self) -> Result<WherePred> {
        if self.at(TokenKind::Ge) {
            self.bump();
            return Ok(WherePred::Ge(self.parse_where_lit()?));
        }
        if self.at(TokenKind::Le) {
            self.bump();
            return Ok(WherePred::Le(self.parse_where_lit()?));
        }
        if self.at(TokenKind::RAngle) {
            self.bump();
            return Ok(WherePred::Gt(self.parse_where_lit()?));
        }
        if self.at(TokenKind::LAngle) {
            self.bump();
            return Ok(WherePred::Lt(self.parse_where_lit()?));
        }
        if self.at(TokenKind::EqEq) {
            self.bump();
            return Ok(WherePred::Eq(self.parse_where_lit()?));
        }
        if self.at(TokenKind::Ne) {
            self.bump();
            return Ok(WherePred::Ne(self.parse_where_lit()?));
        }
        // `..64` stops before 64, `..=64` includes it (D222).
        if self.at(TokenKind::DotDot) || self.at(TokenKind::DotDotEq) {
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            let hi = Some(self.parse_where_lit()?);
            return Ok(WherePred::Range {
                lo: None,
                hi,
                inclusive,
            });
        }
        // `1..64` / `1..=64` / `"espresso"` / `0`
        let lo = self.parse_where_lit()?;
        if self.at(TokenKind::DotDot) || self.at(TokenKind::DotDotEq) {
            let inclusive = self.at(TokenKind::DotDotEq);
            self.bump();
            let hi = if self.at_where_lit() {
                Some(self.parse_where_lit()?)
            } else {
                None
            };
            return Ok(WherePred::Range {
                lo: Some(lo),
                hi,
                inclusive,
            });
        }
        Ok(WherePred::Lit(lo))
    }

    fn at_where_lit(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::String(_)
                | TokenKind::Minus
        )
    }

    fn parse_where_lit(&mut self) -> Result<WhereLit> {
        if self.at(TokenKind::Minus) {
            self.bump();
            match &self.current().kind {
                TokenKind::Int(n) => {
                    let n = *n;
                    self.bump();
                    return Ok(WhereLit::Int(-n));
                }
                TokenKind::Float(n) => {
                    let n = *n;
                    self.bump();
                    return Ok(WhereLit::Float(-n));
                }
                _ => {
                    return Err(Error::at(
                        "expected number after `-` in where clause",
                        self.peek_span(),
                    ));
                }
            }
        }
        match &self.current().kind {
            TokenKind::Int(n) => {
                let n = *n;
                self.bump();
                Ok(WhereLit::Int(n))
            }
            TokenKind::Float(n) => {
                let n = *n;
                self.bump();
                Ok(WhereLit::Float(n))
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(WhereLit::String(s))
            }
            _ => Err(Error::at(
                "expected a literal in where clause (`1`, `1.5`, `\"x\"`)",
                self.peek_span(),
            )),
        }
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

    /// A `-` that begins a line and touches its operand (`-1`, `-x`) starts a
    /// new expression instead of subtracting from the line above, so a
    /// block's `-1` tail after an `if` is a value (D162). `- x` with a space
    /// still continues the expression.
    fn minus_starts_new_expression(&self) -> bool {
        let minus = self.current();
        minus.line_start
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|next| next.span.start == minus.span.end)
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
        let mut errors: Vec<Error> = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let start_pos = self.pos;
            match self.parse_block_elem() {
                Ok(e) => elems.push(e),
                Err(e) => {
                    errors.push(e);
                    if self.pos == start_pos
                        && !self.at(TokenKind::Eof)
                        && !self.at(TokenKind::RBrace)
                    {
                        self.bump();
                    }
                    let ok = self.seek_block_recovery();
                    if !ok {
                        break;
                    }
                    if errors.len() >= 32 {
                        break;
                    }
                    if self.pos == start_pos {
                        break;
                    }
                }
            }
        }
        if self.at(TokenKind::RBrace) {
            self.bump();
        } else {
            errors.push(Error::at("expected `}`", self.peek_span()));
        }
        // Soft-recover: keep successfully parsed statements so later stages can
        // report undefined names, etc., even when one line failed (e.g. `as`).
        if !errors.is_empty() {
            self.soft_errors.extend(errors);
        }
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

    fn parse_block_elem(&mut self) -> Result<BlockElem> {
        if self.at(TokenKind::Let) {
            Ok(BlockElem::Let(self.parse_binding(false)?))
        } else if self.at(TokenKind::Mut) {
            Ok(BlockElem::Let(self.parse_binding(true)?))
        } else if self.at_assign_stmt() {
            Ok(BlockElem::Assign(self.parse_assign()?))
        } else {
            Ok(BlockElem::Expr(self.parse_expr()?))
        }
    }

    /// Skip to the next statement-like start or `}` (mid-block multi-diag).
    fn seek_block_recovery(&mut self) -> bool {
        let mut depth: i32 = 0;
        while !self.at(TokenKind::Eof) {
            if depth == 0 && self.at_block_recovery_point() {
                return true;
            }
            match &self.current().kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    if depth == 0 && matches!(self.current().kind, TokenKind::RBrace) {
                        return true;
                    }
                    depth = (depth - 1).max(0);
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
        false
    }

    fn at_block_recovery_point(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::RBrace
                | TokenKind::Let
                | TokenKind::Mut
                | TokenKind::If
                | TokenKind::Match
                | TokenKind::While
                | TokenKind::Loop
                | TokenKind::For
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Ident(_)
                | TokenKind::Nowait
        )
    }

    /// Statement starts with an assignable place:
    /// `name =`, `name[i] =`, `name.field =`, or `name.field[i] =`.
    fn at_assign_stmt(&self) -> bool {
        self.assign_stmt_at(self.pos)
    }

    /// An assignment statement starts at token `start`.
    fn assign_stmt_at(&self, start: usize) -> bool {
        if !self.tokens.get(start).is_some_and(|t| is_name_token(&t.kind)) {
            return false;
        }
        let mut i = start + 1;
        // Optional `.field`
        if self.tokens.get(i).map(|t| &t.kind) == Some(&TokenKind::Dot) {
            i += 1;
            if !self.tokens.get(i).is_some_and(|t| is_name_token(&t.kind)) {
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
        // Restore on every path (including errors and soft recovery).
        let saved = self.depth;
        let result = match self.enter_nesting() {
            Ok(()) => self.parse_or(),
            Err(error) => Err(error),
        };
        self.depth = saved;
        result
    }

    /// One more level of expression nesting; error past `MAX_EXPR_NESTING`.
    /// `parse_expr` resets the count when its expression ends.
    fn enter_nesting(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_EXPR_NESTING {
            return Err(Error::at(
                format!("expression is nested too deeply (more than {MAX_EXPR_NESTING} levels)"),
                self.peek_span(),
            ));
        }
        Ok(())
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.at(TokenKind::Or) {
            let start = left.span.start;
            self.bump();
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
            // `<name` / `<>` with no space is a tag, not `<`. A following tag
            // after whitespace or a newline ends this expression; `a<b` (glued
            // on both sides) is a hard error so it cannot silently become `a`.
            if self.at(TokenKind::LAngle) {
                if self.at_jsx_start() {
                    if self.angle_glued_to_prev() {
                        return Err(self.lt_space_error());
                    }
                    break;
                }
                if self.angle_glued_to_next() {
                    return Err(self.lt_space_error());
                }
            }
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
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
                TokenKind::Minus if self.minus_starts_new_expression() => break,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            self.enter_nesting()?;
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
        let mut left = self.parse_cast()?;
        loop {
            let op = match self.current().kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            let start = left.span.start;
            self.bump();
            self.enter_nesting()?;
            let right = self.parse_cast()?;
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

    /// `expr as Type` / `expr as? Type` (D191): binds tighter than `* / %` and
    /// looser than unary operators and postfix `?` `.` `[]` calls.
    fn parse_cast(&mut self) -> Result<Expr> {
        let mut expr = self.parse_unary()?;
        while self.at(TokenKind::As) {
            let start = expr.span.start;
            let as_span = self.peek_span();
            self.bump();
            let propagate = self.at(TokenKind::Question);
            if propagate {
                if self.peek_span().start != as_span.end {
                    return Err(Error::at(
                        "write `as?` with no space between `as` and `?`",
                        self.peek_span(),
                    ));
                }
                self.bump();
            }
            self.enter_nesting()?;
            let ty = self.parse_cast_type()?;
            let end = ty.span.end;
            expr = Expr {
                kind: ExprKind::Cast {
                    expr: Box::new(expr),
                    ty,
                    propagate,
                },
                span: Span::new(start, end),
            };
        }
        Ok(expr)
    }

    /// The type after `as`: a name with optional type arguments glued to it
    /// (`Array<Order>`). A literal union or function type needs a named alias,
    /// and a following `?` or `<` belongs to the expression.
    fn parse_cast_type(&mut self) -> Result<TypeExpr> {
        let TokenKind::Ident(_) = self.current().kind else {
            return Err(Error::at(
                "expected a type name after `as` (give a literal union or function type a name with `type`)",
                self.peek_span(),
            ));
        };
        let (name, start_span) = self.expect_ident()?;
        let mut args = Vec::new();
        let mut end = start_span.end;
        if self.at(TokenKind::LAngle) && self.peek_span().start == start_span.end {
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
            end = self.prev_span().end;
        }
        Ok(TypeExpr {
            name,
            args,
            span: Span::new(start_span.start, end),
            lit: None,
            mark: None,
            union_members: None,
            fn_type: None,
            where_clause: None,
        })
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        if self.at(TokenKind::Minus) {
            self.bump();
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
            self.enter_nesting()?;
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
                let (name, name_span) = self.expect_member_name()?;
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
            if self.at(TokenKind::LParen) {
                let start = expr.span.start;
                self.bump();
                let args = self.parse_arg_list()?;
                self.expect_kind(&TokenKind::RParen)?;
                let end = self.prev_span().end;
                expr = Expr {
                    kind: ExprKind::IndirectCall {
                        callee: Box::new(expr),
                        args,
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
        // Where an expression starts, a soft keyword is a name (D198).
        if let Some(word) = soft_keyword(&self.current().kind) {
            self.tokens[self.pos].kind = TokenKind::Ident(word.to_string());
        }
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
            TokenKind::Float(n) => {
                let n = *n;
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Float(n),
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
                parse_string_literal(s, span, self.raw_string_body(span))
            }
            TokenKind::Nowait => {
                self.bump(); // `-->`
                if self.at(TokenKind::LBrace) {
                    return Err(Error::at(
                        "`--> { … }` is not implemented yet; use `--> name(args)`",
                        self.peek_span(),
                    ));
                }
                let (callee, callee_span) = match &self.current().kind {
                    TokenKind::Ident(n) => (n.clone(), self.current().span),
                    _ => {
                        return Err(Error::at(
                            "`-->` requires a named call (`--> name(args)`)",
                            self.peek_span(),
                        ));
                    }
                };
                self.bump();
                self.expect_kind(&TokenKind::LParen)?;
                let args = self.parse_arg_list()?;
                self.expect_kind(&TokenKind::RParen)?;
                Ok(Expr {
                    kind: ExprKind::Start {
                        callee,
                        callee_span,
                        args,
                    },
                    span: Span::new(start.start, self.prev_span().end),
                })
            }
            TokenKind::Ident(name) => {
                let name = name.clone();
                let name_span = self.current().span;
                self.bump();
                if self.at(TokenKind::String(String::new()))
                    && self.current().span.start == name_span.end
                {
                    return self.parse_tagged_interp(name, name_span);
                }
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
                if self.at_map_lit() {
                    self.parse_map_lit()
                } else {
                    let block = self.parse_block()?;
                    let span = block.span;
                    Ok(Expr {
                        kind: ExprKind::Block(block),
                        span,
                    })
                }
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
            TokenKind::LAngle if self.at_jsx_start() => self.parse_jsx_element(),
            TokenKind::LAngle => Err(self.lt_space_error()),
            TokenKind::Junk(s) => Err(Error::at(
                format!("unexpected character {s:?}"),
                self.peek_span(),
            )),
            _ => Err(Error::at(
                format!("expected expression, found {:?}", self.current().kind),
                self.peek_span(),
            )),
        }
    }

    /// `{ key: value` in expression position — map literal, not a block.
    /// A key may also be a call, `{ Point(x: 1, y: 2): value` (D179).
    fn at_map_lit(&self) -> bool {
        if !self.at(TokenKind::LBrace) {
            return false;
        }
        if self.at_map_key_token(1) && self.peek_kind_is(2, TokenKind::Colon) {
            return true;
        }
        // `{ ...ticket, ok: true }` (D182 §4); no block starts with `...`.
        if self.peek_kind_is(1, TokenKind::DotDotDot) {
            return true;
        }
        self.call_key_end(1)
            .is_some_and(|after| self.peek_kind_is(after, TokenKind::Colon))
    }

    /// When `Name(…)` starts at `offset`: the offset just past its `)`.
    fn call_key_end(&self, offset: usize) -> Option<usize> {
        let at = |delta: usize| self.tokens.get(self.pos + offset + delta).map(|t| &t.kind);
        if !matches!(at(0), Some(TokenKind::Ident(_))) || at(1) != Some(&TokenKind::LParen) {
            return None;
        }
        let mut depth = 0usize;
        let mut delta = 1;
        loop {
            match at(delta)? {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(offset + delta + 1);
                    }
                }
                TokenKind::Eof => return None,
                _ => {}
            }
            delta += 1;
        }
    }

    fn at_map_key_token(&self, offset: usize) -> bool {
        let i = self.pos + offset;
        if i >= self.tokens.len() {
            return false;
        }
        is_name_token(&self.tokens[i].kind)
            || matches!(
                self.tokens[i].kind,
                TokenKind::String(_)
                    | TokenKind::Int(_)
                    | TokenKind::Float(_)
                    | TokenKind::True
                    | TokenKind::False
            )
    }

    fn parse_map_lit(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::LBrace)?;
        let mut entries = Vec::new();
        if !self.at(TokenKind::RBrace) {
            loop {
                if self.at(TokenKind::DotDotDot) {
                    self.bump();
                    entries.push(MapEntry::Spread(self.parse_expr()?));
                } else {
                    let key = self.parse_map_key()?;
                    self.expect_kind(&TokenKind::Colon)?;
                    let val = self.parse_expr()?;
                    entries.push(MapEntry::Pair(key, val));
                }
                if self.at(TokenKind::Comma) {
                    self.bump();
                    if self.at(TokenKind::RBrace) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        self.expect_kind(&TokenKind::RBrace)?;
        Ok(Expr {
            kind: ExprKind::MapLit(entries),
            span: Span::new(start.start, self.prev_span().end),
        })
    }

    fn parse_map_key(&mut self) -> Result<Expr> {
        let span = self.peek_span();
        if self.call_key_end(0).is_some() {
            return self.parse_expr();
        }
        if let Some(word) = soft_keyword(&self.current().kind) {
            self.bump();
            return Ok(Expr {
                kind: ExprKind::String(word.to_string()),
                span,
            });
        }
        match &self.current().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok(Expr {
                    kind: ExprKind::String(s),
                    span,
                })
            }
            TokenKind::String(s) => {
                let s = s.clone();
                self.bump();
                Ok(Expr {
                    kind: ExprKind::String(s),
                    span,
                })
            }
            TokenKind::Int(n) => {
                let n = *n;
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Int(n),
                    span,
                })
            }
            TokenKind::Float(value) => {
                let value = *value;
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Float(value),
                    span,
                })
            }
            TokenKind::True => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(true),
                    span,
                })
            }
            TokenKind::False => {
                self.bump();
                Ok(Expr {
                    kind: ExprKind::Bool(false),
                    span,
                })
            }
            _ => Err(Error::at(
                "map key must be an identifier, string, integer, or bool literal, or a call like `Point(x: 1, y: 2)`",
                span,
            )),
        }
    }

    /// `<Ident` or `<>` with **no** whitespace after `<` — tag, not comparison.
    fn at_jsx_start(&self) -> bool {
        self.at(TokenKind::LAngle)
            && self.angle_glued_to_next()
            && (self.peek_kind_is(1, TokenKind::Ident(String::new()))
                || self.peek_kind_is(1, TokenKind::RAngle))
    }

    fn angle_glued_to_next(&self) -> bool {
        self.tokens_adjacent(self.pos, self.pos + 1)
    }

    fn angle_glued_to_prev(&self) -> bool {
        self.pos > 0 && self.tokens_adjacent(self.pos - 1, self.pos)
    }

    fn tokens_adjacent(&self, a: usize, b: usize) -> bool {
        b < self.tokens.len() && self.tokens[a].span.end == self.tokens[b].span.start
    }

    fn lt_space_error(&self) -> Error {
        Error::at(
            "comparison `<` needs spaces (`a < b`); a tag is `<name` with no space",
            self.peek_span(),
        )
        .with_help("write `a < b` to compare, or put `<tag` on its own line")
    }

    /// `if a <b {` leaves a tag where `{` belongs — same rule as infix `<`.
    fn reject_tag_where_block(&self) -> Result<()> {
        if self.at_jsx_start() {
            return Err(self.lt_space_error());
        }
        Ok(())
    }

    /// `<tag attrs>children</tag>` / `<tag attrs />` / `<>children</>` →
    /// `html.el(...)` / `Type(...).view(children:)` / `html.child([...])`.
    fn parse_jsx_element(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::LAngle)?;
        if self.at(TokenKind::RAngle) {
            let frag_span = Span::new(start.start, self.current().span.end);
            self.bump();
            let kids = self.parse_jsx_children(None)?;
            let span = Span::new(start.start, self.prev_span().end);
            return Ok(desugar_jsx_fragment(kids, span, frag_span));
        }
        let (tag, tag_span) = self.expect_ident()?;
        let attrs = self.parse_jsx_attrs()?;
        let self_closing = if self.at(TokenKind::Slash) {
            self.bump();
            self.expect_kind(&TokenKind::RAngle)?;
            true
        } else {
            self.expect_kind(&TokenKind::RAngle)?;
            false
        };
        let kids = if self_closing {
            Vec::new()
        } else {
            self.parse_jsx_children(Some(tag.as_str()))?
        };
        let span = Span::new(start.start, self.prev_span().end);
        Ok(desugar_jsx(tag, tag_span, attrs, kids, span))
    }

    fn parse_jsx_attrs(&mut self) -> Result<Vec<JsxAttr>> {
        let mut attrs = Vec::new();
        while !self.at(TokenKind::Slash) && !self.at(TokenKind::RAngle) && !self.at(TokenKind::Eof)
        {
            attrs.push(self.parse_jsx_attr()?);
        }
        Ok(attrs)
    }

    fn parse_jsx_attr(&mut self) -> Result<JsxAttr> {
        if self.at(TokenKind::LBrace) {
            let start = self.peek_span();
            self.bump();
            if self.at(TokenKind::DotDotDot) {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBrace)?;
                return Ok(JsxAttr::Spread {
                    expr,
                    span: Span::new(start.start, self.prev_span().end),
                });
            }
            return Err(Error::at(
                "expected `{...expr}` spread or `name=...` attribute",
                start,
            ));
        }
        let (name, name_span) = self.parse_jsx_attr_name()?;
        if !self.at(TokenKind::Eq) {
            // Boolean attribute: `disabled` → empty value (render as `disabled`).
            return Ok(JsxAttr::Pair {
                name,
                name_span,
                value: jsx_string_lit(String::new(), name_span),
            });
        }
        self.expect_kind(&TokenKind::Eq)?;
        let value = if self.at(TokenKind::LBrace) {
            self.bump();
            let e = self.parse_expr()?;
            self.expect_kind(&TokenKind::RBrace)?;
            e
        } else if let TokenKind::String(s) = &self.current().kind {
            let s = s.clone();
            let span = self.current().span;
            self.bump();
            parse_string_literal(s, span, self.raw_string_body(span))?
        } else {
            return Err(Error::at(
                "expected attribute value (`\"…\"` or `{expr}`)",
                self.peek_span(),
            ));
        };
        Ok(JsxAttr::Pair {
            name,
            name_span,
            value,
        })
    }

    /// `class`, `type`, `for`, `data-foo`, `aria-label`.
    fn parse_jsx_attr_name(&mut self) -> Result<(String, Span)> {
        let (mut name, start) = self.expect_jsx_name()?;
        let mut end = start.end;
        while self.at(TokenKind::Minus)
            && (self.peek_kind_is(1, TokenKind::Ident(String::new()))
                || self.peek_is_jsx_name_word(1))
        {
            self.bump();
            let (more, more_span) = self.expect_jsx_name()?;
            name.push('-');
            name.push_str(&more);
            end = more_span.end;
        }
        Ok((name, Span::new(start.start, end)))
    }

    fn peek_is_jsx_name_word(&self, offset: usize) -> bool {
        let i = self.pos + offset;
        if i >= self.tokens.len() {
            return false;
        }
        jsx_name_from_token(&self.tokens[i].kind).is_some()
    }

    fn expect_jsx_name(&mut self) -> Result<(String, Span)> {
        if let Some(name) = jsx_name_from_token(&self.current().kind) {
            let span = self.current().span;
            self.bump();
            return Ok((name, span));
        }
        self.expect_ident()
    }

    fn parse_jsx_children(&mut self, end_tag: Option<&str>) -> Result<Vec<Expr>> {
        let mut kids = Vec::new();
        loop {
            if !self.source.is_empty() {
                let from = self.prev_span().end;
                let (text, next_at) = scan_jsx_text(&self.source, from);
                self.seek_to_byte(next_at);
                if !text.is_empty() {
                    kids.push(jsx_text_wrap(jsx_string_lit(text, self.peek_span())));
                }
            }
            if self.at(TokenKind::LAngle) && self.peek_kind_is(1, TokenKind::Slash) {
                self.bump();
                self.bump();
                match end_tag {
                    None => {
                        self.expect_kind(&TokenKind::RAngle)?;
                    }
                    Some(end_tag) => {
                        let (name, name_span) = self.expect_ident()?;
                        if name != end_tag {
                            return Err(Error::at(
                                format!("closing tag `</{name}>` does not match `<{end_tag}>`"),
                                name_span,
                            ));
                        }
                        self.expect_kind(&TokenKind::RAngle)?;
                    }
                }
                break;
            }
            if self.at_jsx_start() {
                kids.push(self.parse_jsx_element()?);
                continue;
            }
            if self.at(TokenKind::LBrace) {
                self.bump();
                if self.at(TokenKind::For) {
                    let for_expr = self.parse_for()?;
                    self.expect_kind(&TokenKind::RBrace)?;
                    kids.push(jsx_child_wrap(desugar_jsx_for(for_expr)?));
                    continue;
                }
                let e = self.parse_expr()?;
                self.expect_kind(&TokenKind::RBrace)?;
                kids.push(jsx_child_wrap(e));
                continue;
            }
            let close = match end_tag {
                None => "</>".to_string(),
                Some(t) => format!("</{t}>"),
            };
            return Err(Error::at(
                format!("expected child or `{close}`"),
                self.peek_span(),
            ));
        }
        Ok(kids)
    }

    /// Jump to the first token whose span starts at or after `offset`.
    fn seek_to_byte(&mut self, offset: usize) {
        while self.pos < self.tokens.len() {
            let tok = &self.tokens[self.pos];
            if tok.kind == TokenKind::Eof || tok.span.start >= offset {
                break;
            }
            self.pos += 1;
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
        if is_name_token(&self.current().kind) && self.peek_kind_is(1, TokenKind::Colon) {
            let (name, name_span) = self.expect_ident()?;
            self.expect_kind(&TokenKind::Colon)?;
            let value = self.parse_arg_value()?;
            Ok(Arg::Named {
                name,
                name_span,
                value,
            })
        } else {
            Ok(Arg::Pos(self.parse_arg_value()?))
        }
    }

    /// An argument's value: an expression, or `mut name` for a `mut`
    /// parameter (D199).
    fn parse_arg_value(&mut self) -> Result<Expr> {
        if !self.at(TokenKind::Mut) {
            return self.parse_expr();
        }
        let start = self.peek_span();
        self.bump();
        let (name, name_span) = self.expect_ident()?;
        Ok(Expr {
            kind: ExprKind::MutArg(name),
            span: Span::new(start.start, name_span.end),
        })
    }

    fn parse_if(&mut self) -> Result<Expr> {
        let start = self.peek_span();
        self.expect_kind(&TokenKind::If)?;
        let cond = self.parse_expr()?;
        self.reject_tag_where_block()?;
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
        self.reject_tag_where_block()?;
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
        self.reject_tag_where_block()?;
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
    /// or `for k, v in map { body }`.
    fn parse_for(&mut self) -> Result<Expr> {
        let start_span = self.peek_span();
        self.expect_kind(&TokenKind::For)?;
        let (name, name_span) = self.expect_ident()?;
        let value = if self.at(TokenKind::Comma) {
            self.bump();
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect_kind(&TokenKind::In)?;
        let range_start = self.parse_expr()?;
        let (end_expr, inclusive) = if self.at(TokenKind::DotDotEq) {
            self.bump();
            (Some(Box::new(self.parse_expr()?)), true)
        } else if self.at(TokenKind::DotDot) {
            self.bump();
            (Some(Box::new(self.parse_expr()?)), false)
        } else {
            // Collection iteration: `for x in xs` / `for k, v in m`
            (None, false)
        };
        if value.is_some() && end_expr.is_some() {
            return Err(Error::at(
                "`for k, v` is only for Map iteration (not ranges)",
                start_span,
            ));
        }
        self.reject_tag_where_block()?;
        let body = self.parse_block()?;
        let end = body.span.end;
        Ok(Expr {
            kind: ExprKind::For {
                name,
                name_span,
                value,
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
        // Optional `if guard` before the arm body (D111 / D130).
        let guard = if self.at(TokenKind::If) {
            self.bump();
            Some(self.parse_expr()?)
        } else {
            None
        };
        // `Pat => expr` is sugar for a one-expression block (D130), and
        // `Pat => name = value` for a one-assignment block (D201). Multi-stmt
        // arms use `{ }`.
        let body = if self.at(TokenKind::FatArrow) && self.assign_stmt_at(self.pos + 1) {
            let start = self.peek_span();
            self.bump();
            let assign = self.parse_assign()?;
            Block {
                items: vec![BlockItem::Assign {
                    target: assign.target,
                    value: assign.value,
                }],
                tail: None,
                span: Span::new(start.start, self.prev_span().end),
            }
        } else if self.at(TokenKind::FatArrow) {
            self.bump();
            let expr = self.parse_expr()?;
            let span = expr.span;
            Block {
                items: Vec::new(),
                tail: Some(Box::new(expr)),
                span,
            }
        } else if self.at(TokenKind::LBrace) {
            self.parse_block()?
        } else {
            return Err(Error::at(
                "expected `=>` or `{` after match pattern",
                self.peek_span(),
            ));
        };
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
            // `-1 =>`: a negative literal pattern.
            TokenKind::Minus
                if matches!(
                    self.tokens.get(self.pos + 1).map(|next| &next.kind),
                    Some(TokenKind::Int(_))
                ) =>
            {
                self.bump();
                let TokenKind::Int(n) = self.current().kind else {
                    unreachable!("checked above");
                };
                self.bump();
                Ok(Pattern::IntLit {
                    value: n.wrapping_neg(),
                    span: Span::new(span.start, self.prev_span().end),
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
                // Bare `Not` / `Pass` / `Fail` / `Has`: empty binds = tag-only (D130).
                // Payload binds still use `Name(x)` / `Name(_)`. Arm body is `=>` or `{ }`.
                if !self.at(TokenKind::LParen) {
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
        is_name_token(&self.current().kind)
            || matches!(
            self.current().kind,
            TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::String(_)
                | TokenKind::Ident(_)
                | TokenKind::Nowait
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
                | TokenKind::LAngle
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

    fn expect_member_name(&mut self) -> Result<(String, Span)> {
        self.expect_ident()
    }

    /// A name: an identifier, or a soft keyword in a name's place (D198).
    fn expect_ident(&mut self) -> Result<(String, Span)> {
        let span = self.current().span;
        match &self.current().kind {
            TokenKind::Ident(s) => {
                let s = s.clone();
                self.bump();
                Ok((s, span))
            }
            kind if soft_keyword(kind).is_some() => {
                let word = soft_keyword(kind).unwrap_or_default().to_string();
                self.bump();
                Ok((word, span))
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

    /// The source text between a string token's quotes, when the parser has
    /// the source. Interpolation holes use it to find their file offsets.
    fn raw_string_body(&self, span: Span) -> Option<String> {
        let text = self.source.get(span.start..span.end)?;
        let body = text.strip_prefix('"')?.strip_suffix('"')?;
        Some(body.to_string())
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            Span::dummy()
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    /// `sql"…"` immediately after the tag ident (no space). v0: only `sql`.
    fn parse_tagged_interp(&mut self, tag: String, tag_span: Span) -> Result<Expr> {
        if tag != "sql" {
            return Err(Error::at(
                format!("unknown string interpolator `{tag}` (only sql\"…\" is shipped)"),
                tag_span,
            ));
        }
        let TokenKind::String(content) = self.current().kind.clone() else {
            return Err(Error::at("expected string after interpolator tag", self.peek_span()));
        };
        let str_span = self.current().span;
        self.bump();
        let inner = parse_string_literal(content, str_span, self.raw_string_body(str_span))?;
        let parts = match inner.kind {
            ExprKind::String(s) => vec![InterpPart::Lit(s)],
            ExprKind::Interpolate(parts) => parts,
            _ => {
                return Err(Error::at(
                    "internal error: tagged interpolator string",
                    str_span,
                ));
            }
        };
        Ok(Expr {
            kind: ExprKind::TaggedInterp { tag, parts },
            span: Span::new(tag_span.start, str_span.end),
        })
    }
}

/// Slice raw JSX text from `from` until the next `<` or `{`. Trimmed; empty
/// means whitespace-only. `next_at` is the byte offset of that `<`/`{` (or EOF).
fn scan_jsx_text(source: &str, from: usize) -> (String, usize) {
    let bytes = source.as_bytes();
    if from >= bytes.len() {
        return (String::new(), from);
    }
    let mut i = from;
    let mut raw = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'<' || b == b'{' {
            break;
        }
        // `\#` → literal `#`. Bare `#` is a comment (already stripped from tokens).
        if b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'#' {
            raw.push('#');
            i += 2;
            continue;
        }
        let ch = source[i..].chars().next().unwrap();
        raw.push(ch);
        i += ch.len_utf8();
    }
    // Drop whitespace-only gaps (between tags). Keep spaces next to `{e}`.
    let text = if raw.trim().is_empty() {
        String::new()
    } else {
        raw
    };
    (text, i)
}

/// PascalCase → component type; otherwise HTML tag via `el`.
fn is_jsx_component(tag: &str) -> bool {
    tag.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn jsx_string_lit(s: String, span: Span) -> Expr {
    Expr {
        kind: ExprKind::String(s),
        span,
    }
}

fn jsx_array_lit(elems: Vec<Expr>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::ArrayLit(elems),
        span,
    }
}

fn jsx_named(name: &str, name_span: Span, value: Expr) -> Arg {
    Arg::Named {
        name: name.to_string(),
        name_span,
        value,
    }
}

/// Builtin `child(e)` — MIR picks text / identity / fragment from the type.
fn jsx_child_wrap(expr: Expr) -> Expr {
    let span = expr.span;
    Expr {
        kind: ExprKind::Call {
            callee: "__jsx_child".into(),
            args: vec![Arg::Pos(expr)],
        },
        span,
    }
}

/// HTML attr `{e}` — MIR stringifies Int64/Float64/Bool the same way children do.
fn jsx_attr_wrap(expr: Expr) -> Expr {
    let span = expr.span;
    Expr {
        kind: ExprKind::Call {
            callee: "__jsx_attr".into(),
            args: vec![Arg::Pos(expr)],
        },
        span,
    }
}

fn desugar_jsx_fragment(kids: Vec<Expr>, span: Span, _name_span: Span) -> Expr {
    jsx_child_wrap(jsx_array_lit(kids, span))
}

fn jsx_text_wrap(expr: Expr) -> Expr {
    jsx_child_wrap(expr)
}

enum JsxAttr {
    Pair {
        name: String,
        name_span: Span,
        value: Expr,
    },
    Spread {
        expr: Expr,
        span: Span,
    },
}

fn desugar_jsx(
    tag: String,
    tag_span: Span,
    attrs: Vec<JsxAttr>,
    kids: Vec<Expr>,
    span: Span,
) -> Expr {
    if is_jsx_component(&tag) {
        let prop_args = attrs
            .into_iter()
            .map(|a| match a {
                JsxAttr::Pair {
                    name,
                    name_span,
                    value,
                } => jsx_named(&name, name_span, value),
                JsxAttr::Spread { expr, span } => Arg::Spread { value: expr, span },
            })
            .collect();
        let ctor = Expr {
            kind: ExprKind::Call {
                callee: tag,
                args: prop_args,
            },
            span,
        };
        Expr {
            kind: ExprKind::MethodCall {
                receiver: Box::new(ctor),
                // A `Component`'s `view` (D228); no source name has an `_`.
                method: "__view".into(),
                method_span: tag_span,
                args: vec![jsx_named("children", tag_span, jsx_array_lit(kids, span))],
            },
            span,
        }
    } else {
        let attrs_expr = desugar_html_attrs(attrs, span);
        Expr {
            kind: ExprKind::Call {
                callee: "__jsx_el".into(),
                args: vec![
                    jsx_named("tag", tag_span, jsx_string_lit(tag, tag_span)),
                    jsx_named("attributes", tag_span, attrs_expr),
                    jsx_named("children", tag_span, jsx_array_lit(kids, span)),
                ],
            },
            span,
        }
    }
}

fn jsx_type_name(name: &str, span: Span) -> TypeExpr {
    TypeExpr {
        name: name.into(),
        args: Vec::new(),
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

fn jsx_array_node_ty(span: Span) -> TypeExpr {
    TypeExpr {
        name: "Array".into(),
        args: vec![TypeArg::Type(jsx_type_name("Node", span))],
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

/// `{for x in xs { child }}` → collect `Array<Node>` (does not change statement `for`).
fn desugar_jsx_for(for_expr: Expr) -> Result<Expr> {
    let span = for_expr.span;
    let collect = format!("__jsx_for_{}", span.start);
    let ExprKind::For { body, .. } = &for_expr.kind else {
        return Err(Error::at("internal: expected `for` in JSX embed", span));
    };
    let Some(tail) = &body.tail else {
        return Err(Error::at(
            "JSX `{for}` body must yield a child (last expression)",
            span,
        ));
    };
    let append = Expr {
        kind: ExprKind::MethodCall {
            receiver: Box::new(jsx_name_expr(&collect, span)),
            method: "append".into(),
            method_span: span,
            args: vec![Arg::Pos(jsx_child_wrap(*tail.clone()))],
        },
        span,
    };
    let mut new_body = body.clone();
    new_body.items.push(BlockItem::Expr(append));
    new_body.tail = None;
    let mut new_for = for_expr;
    match &mut new_for.kind {
        ExprKind::For { body, .. } => *body = new_body,
        _ => unreachable!(),
    }
    Ok(Expr {
        kind: ExprKind::Block(Block {
            items: vec![
                BlockItem::Let {
                    name: collect.clone(),
                    name_span: span,
                    mutable: true,
                    ty: Some(jsx_array_node_ty(span)),
                    init: Expr {
                        kind: ExprKind::ArrayLit(Vec::new()),
                        span,
                    },
                },
                BlockItem::Expr(new_for),
            ],
            tail: Some(Box::new(jsx_name_expr(&collect, span))),
            span,
        }),
        span,
    })
}

fn jsx_array_attr_ty(span: Span) -> TypeExpr {
    TypeExpr {
        name: "Array".into(),
        args: vec![TypeArg::Type(jsx_type_name("Attr", span))],
        span,
        lit: None,
        mark: None,
        union_members: None,
        fn_type: None,
        where_clause: None,
    }
}

fn jsx_attr_ctor(name: String, name_span: Span, value: Expr) -> Expr {
    Expr {
        kind: ExprKind::Call {
            callee: "Attr".into(),
            args: vec![
                jsx_named("name", name_span, jsx_string_lit(name, name_span)),
                jsx_named("value", name_span, jsx_attr_wrap(value)),
            ],
        },
        span: name_span,
    }
}

fn jsx_name_expr(name: &str, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Name(name.into()),
        span,
    }
}

fn desugar_html_attrs(attrs: Vec<JsxAttr>, span: Span) -> Expr {
    let has_spread = attrs.iter().any(|a| matches!(a, JsxAttr::Spread { .. }));
    if !has_spread {
        let elems = attrs
            .into_iter()
            .map(|a| match a {
                JsxAttr::Pair {
                    name,
                    name_span,
                    value,
                } => jsx_attr_ctor(name, name_span, value),
                JsxAttr::Spread { .. } => unreachable!(),
            })
            .collect();
        return jsx_array_lit(elems, span);
    }
    let mut items = Vec::new();
    items.push(BlockItem::Let {
        name: "__jsx_as".into(),
        name_span: span,
        mutable: true,
        ty: Some(jsx_array_attr_ty(span)),
        init: jsx_array_lit(Vec::new(), span),
    });
    for a in attrs {
        match a {
            JsxAttr::Pair {
                name,
                name_span,
                value,
            } => {
                items.push(BlockItem::Expr(Expr {
                    kind: ExprKind::MethodCall {
                        receiver: Box::new(jsx_name_expr("__jsx_as", name_span)),
                        method: "append".into(),
                        method_span: name_span,
                        args: vec![Arg::Pos(jsx_attr_ctor(name, name_span, value))],
                    },
                    span: name_span,
                }));
            }
            JsxAttr::Spread { expr, span: sp } => {
                items.push(BlockItem::Expr(Expr {
                    kind: ExprKind::For {
                        name: "__jsx_a".into(),
                        name_span: sp,
                        value: None,
                        start: Box::new(expr),
                        end: None,
                        inclusive: false,
                        body: Block {
                            items: Vec::new(),
                            tail: Some(Box::new(Expr {
                                kind: ExprKind::MethodCall {
                                    receiver: Box::new(jsx_name_expr("__jsx_as", sp)),
                                    method: "append".into(),
                                    method_span: sp,
                                    args: vec![Arg::Pos(jsx_name_expr("__jsx_a", sp))],
                                },
                                span: sp,
                            })),
                            span: sp,
                        },
                    },
                    span: sp,
                }));
            }
        }
    }
    Expr {
        kind: ExprKind::Block(Block {
            items,
            tail: Some(Box::new(jsx_name_expr("__jsx_as", span))),
            span,
        }),
        span,
    }
}

/// Attribute names may be ordinary idents or a handful of keywords (`type`, `for`).
fn jsx_name_from_token(kind: &TokenKind) -> Option<String> {
    match kind {
        TokenKind::Ident(s) => Some(s.clone()),
        TokenKind::Type => Some("type".into()),
        TokenKind::For => Some("for".into()),
        TokenKind::In => Some("in".into()),
        TokenKind::As => Some("as".into()),
        TokenKind::From => Some("from".into()),
        TokenKind::If => Some("if".into()),
        TokenKind::Else => Some("else".into()),
        TokenKind::Match => Some("match".into()),
        TokenKind::Fn => Some("fn".into()),
        TokenKind::Let => Some("let".into()),
        TokenKind::Mut => Some("mut".into()),
        TokenKind::Export => Some("export".into()),
        TokenKind::Import => Some("import".into()),
        TokenKind::Return => Some("return".into()),
        TokenKind::Loop => Some("loop".into()),
        TokenKind::While => Some("while".into()),
        TokenKind::Break => Some("break".into()),
        TokenKind::Continue => Some("continue".into()),
        TokenKind::True => Some("true".into()),
        TokenKind::False => Some("false".into()),
        TokenKind::And => Some("and".into()),
        TokenKind::Or => Some("or".into()),
        TokenKind::Not => Some("not".into()),
        TokenKind::Extern => Some("extern".into()),
        TokenKind::Where => Some("where".into()),
        TokenKind::Trait => Some("trait".into()),
        TokenKind::Conform => Some("conform".into()),
        _ => None,
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

fn block_item_span(bi: &BlockItem) -> Span {
    match bi {
        BlockItem::Let {
            name_span, init, ..
        } => Span::new(name_span.start, init.span.end),
        BlockItem::Assign { target, value } => {
            let start = match target {
                AssignTarget::Name { name_span, .. } => name_span.start,
                AssignTarget::Field { base_span, .. } => base_span.start,
                AssignTarget::Index { base_span, .. } => base_span.start,
                AssignTarget::DynIndex { name_span, .. } => name_span.start,
            };
            Span::new(start, value.span.end)
        }
        BlockItem::Expr(e) => e.span,
    }
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

/// Byte offsets, within a string body, of each interpolation hole's first
/// character (just after its `{`). `{{` is a literal brace.
fn interpolation_hole_starts(body: &str) -> Vec<usize> {
    let bytes = body.as_bytes();
    let mut starts = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'{' if bytes.get(index + 1) == Some(&b'{') => index += 2,
            b'}' if bytes.get(index + 1) == Some(&b'}') => index += 2,
            b'{' => {
                starts.push(index + 1);
                index = hole_end(bytes, index + 1).map_or(bytes.len(), |end| end + 1);
            }
            _ => index += 1,
        }
    }
    starts
}

/// The index of the `}` that closes a hole whose text starts at `from`.
/// Strings in the hole are skipped whole, holes and all (D176); an escape
/// (`\"`, the older way to quote inside a hole) is one character.
fn hole_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut index = from;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'"' => {
                index = string_end(bytes, index)?;
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// The index just past the string literal whose `"` is at `open`.
fn string_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut index = open + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'"' => return Some(index + 1),
            b'{' if bytes.get(index + 1) == Some(&b'{') => index += 1,
            b'{' => index = hole_end(bytes, index + 1)?,
            _ => {}
        }
        index += 1;
    }
    None
}

/// Expand a string token into plain `String` or `Interpolate` (handles `{{` / `}}`).
/// `raw` is the source between the quotes, when known: a hole's file offset
/// is taken from it, since escapes make the decoded `content` shorter.
fn parse_string_literal(content: String, span: Span, raw: Option<String>) -> Result<Expr> {
    let raw_hole_starts = raw.as_deref().map(interpolation_hole_starts);
    let mut hole_count = 0usize;
    let mut parts: Vec<InterpPart> = Vec::new();
    let mut lit = String::new();
    let chars: Vec<char> = content.chars().collect();
    // Byte offset of each char, to find a hole's end in `content`'s bytes.
    let byte_of: Vec<usize> = content.char_indices().map(|(at, _)| at).collect();
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
                let close = byte_of
                    .get(expr_start)
                    .and_then(|&from| hole_end(content.as_bytes(), from));
                let Some(close) = close else {
                    return Err(Error::at("unclosed interpolation `{`", span).with_help(
                        "a hole runs to its matching `}`, and a `\"` inside it starts a string of its own",
                    ));
                };
                i = byte_of.partition_point(|&at| at < close);
                let expr_src: String = chars[expr_start..i].iter().collect();
                i += 1; // consume closing `}`
                let decoded_offset: usize = chars[..expr_start].iter().map(|ch| ch.len_utf8()).sum();
                let body_offset = raw_hole_starts
                    .as_ref()
                    .and_then(|starts| starts.get(hole_count).copied())
                    .unwrap_or(decoded_offset);
                hole_count += 1;
                // `+ 1` steps over the opening quote.
                let expr = parse_embedded_expr(&expr_src, span, span.start + 1 + body_offset)?;
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

/// Parse one interpolation hole. `offset` is the file offset of `src`'s first
/// byte; token spans are shifted by it so diagnostics point into the hole.
fn parse_embedded_expr(src: &str, parent_span: Span, offset: usize) -> Result<Expr> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Err(Error::at("empty interpolation expression", parent_span));
    }
    let offset = offset + (src.len() - src.trim_start().len());
    let shift = |span: Span| Span::new(span.start + offset, span.end + offset);
    let mut tokens = Lexer::new(trimmed).tokenize().map_err(|e| {
        Error::at(
            format!("invalid interpolation expression: {}", e.message),
            e.span.map_or(parent_span, shift),
        )
    })?;
    for token in &mut tokens {
        token.span = shift(token.span);
    }
    let mut p = Parser::new(tokens);
    let expr = p.parse_expr()?;
    if !p.at(TokenKind::Eof) {
        return Err(Error::at(
            "trailing tokens in interpolation expression",
            p.peek_span(),
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
        Parser::new(tokens)
            .with_source(src.to_string())
            .parse_module()
            .unwrap()
    }

    fn parse_err(src: &str) -> String {
        let tokens = Lexer::new(src).tokenize().unwrap();
        let err = Parser::new(tokens)
            .with_source(src.to_string())
            .parse_module()
            .unwrap_err();
        format!("{err}")
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
    fn parse_combined_import() {
        let module = parse(
            r#"
            import logging, { info, warn as w } from "std/logging"
            fn main() -> Int64 { 0 }
        "#,
        );
        match &module.items[0] {
            Item::Import(imp) => {
                assert_eq!(imp.path, "std/logging");
                match &imp.kind {
                    ImportKind::Combined { binding, names, .. } => {
                        assert_eq!(binding, "logging");
                        assert_eq!(names.len(), 2);
                        assert_eq!(names[0].name, "info");
                        assert!(names[0].alias.is_none());
                        assert_eq!(names[1].name, "warn");
                        assert_eq!(names[1].alias.as_ref().map(|(a, _)| a.as_str()), Some("w"));
                    }
                    other => panic!("expected Combined, got {other:?}"),
                }
            }
            _ => panic!("expected import"),
        }
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
    fn parse_function_type() {
        let module = parse(
            r#"
            fn apply(f: (x: Int64) -> Int64, v: Int64) -> Int64 {
                f(v)
            }
            "#,
        );
        match &module.items[0] {
            Item::Function(f) => {
                let ft = f.params[0].ty.fn_type.as_ref().expect("fn type");
                assert_eq!(ft.params.len(), 1);
                assert_eq!(ft.params[0].0, "x");
                assert_eq!(ft.params[0].1.name, "Int64");
                assert_eq!(ft.ret.name, "Int64");
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
    fn parse_where_on_alias_and_field() {
        let module = parse(
            r#"
            type Qty = Int64 where >= 1 "must be at least 1"
            type Port = Int64 where (1..1023) | (8080..8090)
            type Order(slug: String where 1..64, qty: Qty)
            "#,
        );
        let Item::Type(qty) = &module.items[0] else {
            panic!("qty");
        };
        let members = qty.union_members.as_ref().unwrap();
        assert_eq!(members[0].name, "Int64");
        let w = members[0].where_clause.as_ref().unwrap();
        match &w.expr {
            WhereExpr::Atom(a) => {
                assert!(matches!(a.pred, WherePred::Ge(WhereLit::Int(1))));
                assert_eq!(a.message.as_deref(), Some("must be at least 1"));
            }
            _ => panic!("expected atom"),
        }
        let Item::Type(port) = &module.items[1] else {
            panic!("port");
        };
        let pw = port.union_members.as_ref().unwrap()[0]
            .where_clause
            .as_ref()
            .unwrap();
        assert!(matches!(pw.expr, WhereExpr::Or(_, _)));
        let Item::Type(order) = &module.items[2] else {
            panic!("order");
        };
        assert!(order.params[0].ty.where_clause.is_some());
        assert_eq!(order.params[1].ty.name, "Qty");
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
                assert!(t.routes.is_empty());
            }
            _ => panic!("expected type"),
        }
    }

    #[test]
    fn parse_route_clauses_in_type_body() {
        // Path patterns may use `:userId`; formals are camelCase (no `_` in idents).
        let module = parse(
            r#"
            type MyRouter {
                GET "/home" {
                    "Home"
                }

                PUT "/user/:userId" (userId: Int64) -> String {
                    "user {userId}"
                }

                export fn ping() -> Int64 {
                    1
                }
            }
            fn main() -> Int64 { 0 }
        "#,
        );
        match &module.items[0] {
            Item::Type(t) => {
                assert_eq!(t.name, "MyRouter");
                assert_eq!(t.routes.len(), 2);
                assert_eq!(t.methods.len(), 1);
                assert_eq!(t.routes[0].method, "GET");
                assert_eq!(t.routes[0].pattern, "/home");
                assert!(t.routes[0].params.is_empty());
                assert!(t.routes[0].return_type.is_none());
                assert_eq!(t.routes[1].method, "PUT");
                assert_eq!(t.routes[1].pattern, "/user/:userId");
                assert_eq!(t.routes[1].params.len(), 1);
                assert_eq!(t.routes[1].params[0].name, "userId");
                assert_eq!(
                    t.routes[1].return_type.as_ref().map(|ty| ty.name.as_str()),
                    Some("String")
                );
            }
            _ => panic!("expected type"),
        }
    }

    #[test]
    fn parse_route_clause_rejects_unknown_method_word() {
        // Lowercase / non-HTTP idents are not route methods; bare ident is an error.
        let err = parse_err(
            r#"
            type R {
                get "/home" { 0 }
            }
        "#,
        );
        assert!(
            err.contains("expected field") || err.contains("route"),
            "unexpected error: {err}"
        );
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

    #[test]
    fn parse_nowait_call() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                --> slow("A")
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        match &f.body.items[0] {
            BlockItem::Expr(e) => match &e.kind {
                ExprKind::Start { callee, args, .. } => {
                    assert_eq!(callee, "slow");
                    assert_eq!(args.len(), 1);
                }
                other => panic!("expected -->, got {other:?}"),
            },
            other => panic!("expected expr stmt, got {other:?}"),
        }
    }

    #[test]
    fn parse_start_as_name_still_works() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let start = 1
                start
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        match &f.body.items[0] {
            BlockItem::Let { name, .. } => assert_eq!(name, "start"),
            other => panic!("expected let start, got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_html_element() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let n = <div class="hero">Hello</div>
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(callee, "__jsx_el");
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected el(...), got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_component() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let n = <Page title="Home"><h1>Hi</h1></Page>
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::MethodCall { method, args, .. } => {
                assert_eq!(method, "__view");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Page(...).view(...), got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_self_closing_and_embed() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let n = <input type="text" value={who} />
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::Call { callee, args } => {
                assert_eq!(callee, "__jsx_el");
                assert_eq!(args.len(), 3);
            }
            other => panic!("expected el(...), got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_mismatched_close() {
        let err = parse_err(
            r#"
            fn main() -> Int64 {
                let n = <div>Hi</span>
                0
            }
        "#,
        );
        assert!(err.contains("does not match"), "unexpected error: {err}");
    }

    #[test]
    fn parse_jsx_for_collect() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let n = <ul>{for x in xs { <li>{x}</li> }}</ul>
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::Call { callee, .. } => assert_eq!(callee, "__jsx_el"),
            other => panic!("expected __jsx_el, got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_fragment_and_boolean_attr() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let n = <><input disabled /></>
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::Call { callee, .. } => assert_eq!(callee, "__jsx_child"),
            other => panic!("expected __jsx_child([...]), got {other:?}"),
        }
    }

    #[test]
    fn parse_jsx_after_expr_is_tag_not_comparison() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                1
                <p>ok</p>
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        assert_eq!(f.body.items.len(), 2, "1 and <p> should be separate items");
        match &f.body.items[1] {
            BlockItem::Expr(e) => match &e.kind {
                ExprKind::Call { callee, .. } => assert_eq!(callee, "__jsx_el"),
                other => panic!("expected tag after 1, got {other:?}"),
            },
            other => panic!("expected expr item, got {other:?}"),
        }
    }

    #[test]
    fn parse_spaced_lt_is_still_comparison() {
        let module = parse(
            r#"
            fn main() -> Int64 {
                let b = 1 < 2
                0
            }
        "#,
        );
        let Item::Function(f) = &module.items[0] else {
            panic!("expected fn");
        };
        let BlockItem::Let { init, .. } = &f.body.items[0] else {
            panic!("expected let");
        };
        match &init.kind {
            ExprKind::Binary { op, .. } => assert_eq!(*op, BinaryOp::Lt),
            other => panic!("expected `<` comparison, got {other:?}"),
        }
    }

    #[test]
    fn parse_glued_lt_ident_needs_spaces() {
        let err = parse_err(
            r#"
            fn main() -> Int64 {
                let b = a<b
                0
            }
        "#,
        );
        assert!(err.contains("needs spaces"), "unexpected error: {err}");
    }

    #[test]
    fn parse_glued_lt_int_needs_spaces() {
        let err = parse_err(
            r#"
            fn main() -> Int64 {
                let b = n<3
                0
            }
        "#,
        );
        assert!(err.contains("needs spaces"), "unexpected error: {err}");
    }

    #[test]
    fn parse_spaced_tag_is_not_jsx() {
        let err = parse_err(
            r#"
            fn main() -> Int64 {
                let n = < div>x</div>
                0
            }
        "#,
        );
        assert!(
            err.contains("needs spaces") || err.contains("no space"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_if_lt_tag_needs_spaces() {
        let err = parse_err(
            r#"
            fn main() -> Int64 {
                if a <b {
                    0
                }
                0
            }
        "#,
        );
        assert!(err.contains("needs spaces"), "unexpected error: {err}");
    }
}
