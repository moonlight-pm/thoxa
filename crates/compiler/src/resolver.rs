//! Shared lexical name resolver.
//!
//! Produces a position-queryable table of definitions ([`Binding`]) and
//! occurrences ([`Reference`]). Resolution is **type-independent** so both MIR
//! lower (future) and the LSP can consume one scope implementation.
//!
//! Member access (`.field` / `.method`) uses ctx `Member`. When the receiver has
//! a simple type hint (param/let annotation or type-ctor init), the reference is
//! linked to the field/method binding for go-to-definition / hover / refs.
//!
//! Phase 2 PR5: resolver + unit tests. MIR still uses its own env for lower;
//! LSP navigation (PR6) queries this table. Lower migration is deferred until
//! types-by-binding can be plumbed without changing diagnostics.

use crate::ast::{
    self, Arg, AssignTarget, Block, BlockItem, Expr, ExprKind, ImportKind, Item, MatchArm, Module,
    Pattern, TypeArg, TypeExpr,
};
use crate::error::Result;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::span::{self, Span};
use std::collections::HashMap;

/// Kind of a name binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Fn,
    Extern,
    Type,
    Field,
    Method,
    Param,
    Let,
    Mut,
    ForVar,
    Pattern,
    Import,
    Builtin,
}

impl BindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BindingKind::Fn => "fn",
            BindingKind::Extern => "extern",
            BindingKind::Type => "type",
            BindingKind::Field => "field",
            BindingKind::Method => "method",
            BindingKind::Param => "param",
            BindingKind::Let => "let",
            BindingKind::Mut => "mut",
            BindingKind::ForVar => "forvar",
            BindingKind::Pattern => "pattern",
            BindingKind::Import => "import",
            BindingKind::Builtin => "builtin",
        }
    }
}

/// How a name occurrence is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefCtx {
    Value,
    Type,
    Call,
    /// Pending member; may be filled when receiver type is known.
    Member,
    Label,
    Pattern,
}

/// Origin of an import binding (for cross-module navigation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOrigin {
    /// Specifier string from the import (e.g. `"./Helper"`, `"~/modlib/Geometry"`).
    pub spec: String,
    /// For selective imports: the export name in the source module (before `as`).
    /// `None` for namespace imports (`import ns from "…"`).
    pub original: Option<String>,
    /// True for namespace form `import ns from "…"`.
    pub is_namespace: bool,
}

/// Structural type for typed-member navigation (head + type-arg heads).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NavTy {
    /// Head type name (`Box` in `Box<Point>`).
    pub head: String,
    /// Type-argument head names in order (`["Point"]` for `Box<Point>`).
    pub args: Vec<String>,
}

/// One definition site (or external/builtin with no in-file span).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: u32,
    pub name: String,
    pub kind: BindingKind,
    /// Name span at the definition; `None` for builtins / extern-only.
    pub def_span: Option<Span>,
    pub exported: bool,
    /// Enclosing type name for field/method bindings.
    pub owner: Option<String>,
    /// Populated for [`BindingKind::Import`] bindings.
    pub import_origin: Option<ImportOrigin>,
    /// Value type head when known (annotation, ctor, fn return, …) for member nav.
    pub ty_hint: Option<String>,
    /// Type-argument heads for generic nav (`Box<Point>` → `["Point"]`).
    pub ty_args: Vec<String>,
}

/// One name occurrence in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub span: Span,
    pub binding_id: Option<u32>,
    pub ctx: RefCtx,
}

/// Result of resolving a module.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub bindings: HashMap<u32, Binding>,
    pub references: Vec<Reference>,
    /// Module-level binding ids in declaration order (outline).
    pub module_binding_ids: Vec<u32>,
    /// `(type_name, member_name)` → binding id.
    pub member_index: HashMap<(String, String), u32>,
}

impl Resolution {
    /// Narrowest reference whose span covers the 0-based LSP position.
    pub fn ref_at(&self, src: &str, line: u32, character: u32) -> Option<&Reference> {
        let mut best: Option<&Reference> = None;
        let mut best_len = usize::MAX;
        for r in &self.references {
            if !span::span_covers(src, r.span, line, character) {
                continue;
            }
            let len = r.span.end.saturating_sub(r.span.start).max(1);
            if len < best_len {
                best_len = len;
                best = Some(r);
            }
        }
        best
    }

    pub fn binding_of(&self, ref_: Option<&Reference>) -> Option<&Binding> {
        let id = ref_?.binding_id?;
        self.bindings.get(&id)
    }

    /// Binding whose **definition** name span covers the position (cursor on a def).
    pub fn binding_at(&self, src: &str, line: u32, character: u32) -> Option<&Binding> {
        let mut best: Option<&Binding> = None;
        let mut best_len = usize::MAX;
        for b in self.bindings.values() {
            let Some(span) = b.def_span else { continue };
            if !span::span_covers(src, span, line, character) {
                continue;
            }
            let len = span.end.saturating_sub(span.start).max(1);
            if len < best_len {
                best_len = len;
                best = Some(b);
            }
        }
        best
    }

    pub fn refs_to(&self, binding_id: u32) -> Vec<&Reference> {
        self.references
            .iter()
            .filter(|r| r.binding_id == Some(binding_id))
            .collect()
    }

    pub fn module_definitions(&self) -> Vec<&Binding> {
        self.module_binding_ids
            .iter()
            .filter_map(|id| self.bindings.get(id))
            .collect()
    }

    pub fn members_of(&self, type_name: &str) -> Vec<&Binding> {
        self.bindings
            .values()
            .filter(|b| b.owner.as_deref() == Some(type_name))
            .collect()
    }

    /// Link a pending member reference at `span` to `binding_id` (type-driven).
    pub fn link_member_at(&mut self, span: Span, binding_id: u32) {
        for r in &mut self.references {
            if r.ctx == RefCtx::Member && r.span == span && r.binding_id.is_none() {
                r.binding_id = Some(binding_id);
                return;
            }
        }
    }
}

/// Built-in type and value names with no source definition in the buffer.
const BUILTINS: &[&str] = &[
    "Int64", "Int32", "UInt64", "UInt32", "Byte", "Bool", "Void", "String", "Bytes", "Buffer",
    "Array", "Address", "Trial", "May", "Pass", "Fail", "Has", "Not", "true", "false",
];

struct Scope {
    parent: Option<usize>,
    names: HashMap<String, u32>,
}

struct Resolver<'a> {
    module: &'a Module,
    res: Resolution,
    next_id: u32,
    scopes: Vec<Scope>,
}

/// `Trial<T, E>?` / `May<T>?` → success head `T` (first type-arg head).
/// Non-fallible nav types pass through unchanged.
pub fn unwrap_fallible_nav_ty(t: NavTy) -> NavTy {
    if (t.head == "Trial" || t.head == "May") && !t.args.is_empty() {
        NavTy {
            head: t.args[0].clone(),
            args: Vec::new(),
        }
    } else {
        t
    }
}

/// Resolve a parsed module (single-file; imports are local bindings only).
pub fn resolve_module(module: &Module) -> Resolution {
    // Experiment: expand type-body route clauses so LSP sees `route` + handlers.
    let mut desugared = module.clone();
    let module = match crate::route_desugar::desugar_module(&mut desugared) {
        Ok(()) => &desugared,
        Err(_) => module,
    };
    let mut r = Resolver {
        module,
        res: Resolution::default(),
        next_id: 0,
        scopes: Vec::new(),
    };
    r.run();
    r.res
}

/// Lex + parse + resolve `text`. Parse errors propagate; resolve never fails.
pub fn resolve_source(text: &str) -> Result<Resolution> {
    let tokens = Lexer::new(text).tokenize()?;
    let module = Parser::new(tokens).parse_module()?;
    Ok(resolve_module(&module))
}

impl<'a> Resolver<'a> {
    fn run(&mut self) {
        self.push_scope(None);
        self.seed_module();
        // Walk bodies (definitions already seeded).
        for item in &self.module.items {
            self.walk_item(item);
        }
    }

    fn push_scope(&mut self, parent: Option<usize>) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent,
            names: HashMap::new(),
        });
        id
    }

    fn root_scope(&self) -> usize {
        0
    }

    fn define(&mut self, scope: usize, name: &str, binding_id: u32) {
        self.scopes[scope]
            .names
            .insert(name.to_string(), binding_id);
    }

    fn lookup(&self, scope: usize, name: &str) -> Option<u32> {
        let mut cur = Some(scope);
        while let Some(s) = cur {
            if let Some(&id) = self.scopes[s].names.get(name) {
                return Some(id);
            }
            cur = self.scopes[s].parent;
        }
        None
    }

    fn new_binding(
        &mut self,
        name: &str,
        kind: BindingKind,
        def_span: Option<Span>,
        exported: bool,
        owner: Option<String>,
        module_level: bool,
    ) -> u32 {
        self.new_binding_full(name, kind, def_span, exported, owner, module_level, None)
    }

    fn new_binding_full(
        &mut self,
        name: &str,
        kind: BindingKind,
        def_span: Option<Span>,
        exported: bool,
        owner: Option<String>,
        module_level: bool,
        import_origin: Option<ImportOrigin>,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.res.bindings.insert(
            id,
            Binding {
                id,
                name: name.to_string(),
                kind,
                def_span,
                exported,
                owner,
                import_origin,
                ty_hint: None,
                ty_args: Vec::new(),
            },
        );
        if module_level {
            self.res.module_binding_ids.push(id);
        }
        id
    }

    fn set_nav_ty(&mut self, id: u32, nav: Option<NavTy>) {
        if let Some(n) = nav {
            if let Some(b) = self.res.bindings.get_mut(&id) {
                b.ty_hint = Some(n.head);
                b.ty_args = n.args;
            }
        }
    }

    fn binding_nav_ty(b: &Binding) -> Option<NavTy> {
        let head = b.ty_hint.clone()?;
        Some(NavTy {
            head,
            args: b.ty_args.clone(),
        })
    }

    /// Head + type-arg heads for nav. `Box<Point>` → head `Box`, args `["Point"]`.
    fn nav_ty_from_type_expr(te: &TypeExpr) -> Option<NavTy> {
        if te.lit.is_some() || te.name.is_empty() {
            return None;
        }
        let mut args = Vec::new();
        for a in &te.args {
            if let TypeArg::Type(inner) = a {
                if let Some(n) = Self::nav_ty_from_type_expr(inner) {
                    args.push(n.head);
                }
            }
        }
        Some(NavTy {
            head: te.name.clone(),
            args,
        })
    }

    /// Product field types in declaration order (params then body fields).
    fn type_def_field_types(&self, type_name: &str) -> Option<Vec<TypeExpr>> {
        for item in &self.module.items {
            if let Item::Type(td) = item {
                if td.name == type_name {
                    let mut out = Vec::new();
                    for p in &td.params {
                        out.push(p.ty.clone());
                    }
                    for f in &td.fields {
                        out.push(f.ty.clone());
                    }
                    return Some(out);
                }
            }
        }
        None
    }

    fn type_def_type_params(&self, type_name: &str) -> Vec<String> {
        for item in &self.module.items {
            if let Item::Type(td) = item {
                if td.name == type_name {
                    return td.type_params.clone();
                }
            }
        }
        Vec::new()
    }

    /// Substitute type-param heads using receiver type arguments.
    fn apply_type_param_subst(nav: &NavTy, type_params: &[String], recv_args: &[String]) -> NavTy {
        if let Some(i) = type_params.iter().position(|p| p == &nav.head) {
            if let Some(concrete) = recv_args.get(i) {
                return NavTy {
                    head: concrete.clone(),
                    args: Vec::new(),
                };
            }
        }
        let mut args = Vec::new();
        for a in &nav.args {
            let sub = Self::apply_type_param_subst(
                &NavTy {
                    head: a.clone(),
                    args: Vec::new(),
                },
                type_params,
                recv_args,
            );
            args.push(sub.head);
        }
        NavTy {
            head: nav.head.clone(),
            args,
        }
    }

    fn member_nav_ty(&self, owner: &NavTy, member: &str) -> Option<NavTy> {
        let mid = self.member_id(&owner.head, member)?;
        let mb = self.res.bindings.get(&mid)?;
        let raw = Self::binding_nav_ty(mb)?;
        let tparams = self.type_def_type_params(&owner.head);
        Some(Self::apply_type_param_subst(&raw, &tparams, &owner.args))
    }

    fn ty_hint_from_init(&self, init: &Expr, scope: usize) -> Option<NavTy> {
        self.expr_nav_ty(init, scope)
    }

    /// Best-effort value type for untyped `let` inits (and nested expressions).
    fn expr_nav_ty(&self, expr: &Expr, scope: usize) -> Option<NavTy> {
        match &expr.kind {
            ExprKind::Name(_)
            | ExprKind::Call { .. }
            | ExprKind::MethodCall { .. }
            | ExprKind::Field { .. } => self.receiver_nav_ty(expr, scope),
            ExprKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let t = self.block_nav_ty(then_branch, scope)?;
                match else_branch {
                    Some(eb) => {
                        let e = self.block_nav_ty(eb, scope)?;
                        if t == e {
                            Some(t)
                        } else {
                            None
                        }
                    }
                    None => Some(t),
                }
            }
            ExprKind::Block(b) => self.block_nav_ty(b, scope),
            // `x?` extracts the success arm of Trial/May (first type argument).
            ExprKind::Try(inner) => {
                let t = self.expr_nav_ty(inner, scope)?;
                Some(unwrap_fallible_nav_ty(t))
            }
            ExprKind::Unary { expr: inner, .. } => self.expr_nav_ty(inner, scope),
            // Value match: all arms must agree on a concrete nav type (pattern
            // binds not re-scoped here — constructor tails work).
            ExprKind::Match { arms, .. } => {
                let mut acc: Option<NavTy> = None;
                for arm in arms {
                    let t = self.block_nav_ty(&arm.body, scope)?;
                    match &acc {
                        None => acc = Some(t),
                        Some(prev) if prev == &t => {}
                        _ => return None,
                    }
                }
                acc
            }
            _ => None,
        }
    }

    fn block_nav_ty(&self, block: &Block, scope: usize) -> Option<NavTy> {
        block
            .tail
            .as_ref()
            .and_then(|t| self.expr_nav_ty(t, scope))
    }

    /// Result type of a bare call for member linking: type ctor, or free/extern return.
    fn call_result_nav_ty(&self, callee: &str, scope: usize) -> Option<NavTy> {
        let id = self.lookup(scope, callee)?;
        let b = self.res.bindings.get(&id)?;
        match b.kind {
            BindingKind::Type => Some(NavTy {
                head: b.name.clone(),
                args: Vec::new(),
            }),
            BindingKind::Fn | BindingKind::Extern => Self::binding_nav_ty(b),
            _ => None,
        }
    }

    fn receiver_nav_ty(&self, expr: &Expr, scope: usize) -> Option<NavTy> {
        match &expr.kind {
            ExprKind::Name(n) => {
                let id = self.lookup(scope, n)?;
                Self::binding_nav_ty(self.res.bindings.get(&id)?)
            }
            ExprKind::Call { callee, .. } => self.call_result_nav_ty(callee, scope),
            // `p.moved().x` / `b.get().x` — method return after type-param subst.
            ExprKind::MethodCall {
                receiver, method, ..
            } => {
                let recv = self.receiver_nav_ty(receiver, scope)?;
                self.member_nav_ty(&recv, method)
            }
            // `p.pos.x` — field type after type-param subst.
            ExprKind::Field { base, name, .. } => {
                let recv = self.receiver_nav_ty(base, scope)?;
                self.member_nav_ty(&recv, name)
            }
            _ => None,
        }
    }

    fn receiver_ty_hint(&self, expr: &Expr, scope: usize) -> Option<String> {
        self.receiver_nav_ty(expr, scope).map(|n| n.head)
    }

    fn member_id(&self, type_name: &str, member: &str) -> Option<u32> {
        self.res
            .member_index
            .get(&(type_name.to_string(), member.to_string()))
            .copied()
    }

    fn seed_module(&mut self) {
        let root = self.root_scope();
        for name in BUILTINS {
            let id = self.new_binding(name, BindingKind::Builtin, None, false, None, false);
            self.define(root, name, id);
        }

        for item in &self.module.items {
            match item {
                Item::Function(f) => {
                    let id = self.new_binding(
                        &f.name,
                        BindingKind::Fn,
                        Some(f.name_span),
                        f.exported,
                        None,
                        true,
                    );
                    // Return type feeds typed-member nav: `let p = make()` / `make().x`.
                    if let Some(ret) = &f.return_type {
                        self.set_nav_ty(id, Self::nav_ty_from_type_expr(ret));
                    }
                    self.define(root, &f.name, id);
                }
                Item::Extern(e) => {
                    let id = self.new_binding(
                        &e.name,
                        BindingKind::Extern,
                        Some(e.name_span),
                        e.exported,
                        None,
                        true,
                    );
                    if let Some(ret) = &e.return_type {
                        self.set_nav_ty(id, Self::nav_ty_from_type_expr(ret));
                    }
                    self.define(root, &e.name, id);
                }
                Item::Type(td) => {
                    let id = self.new_binding(
                        &td.name,
                        BindingKind::Type,
                        Some(td.name_span),
                        td.exported,
                        None,
                        true,
                    );
                    self.define(root, &td.name, id);
                    self.declare_members(td);
                }
                Item::SessionStmt(_) => {
                    // Lifted into Shell.step before MIR; ignore for top-level binds.
                }
                Item::Global(g) => {
                    let kind = if g.mutable {
                        BindingKind::Mut
                    } else {
                        BindingKind::Let
                    };
                    let id = self.new_binding(
                        &g.name,
                        kind,
                        Some(g.name_span),
                        g.exported,
                        None,
                        true,
                    );
                    self.define(root, &g.name, id);
                }
                Item::Import(imp) => match &imp.kind {
                    ImportKind::Selective(names) => {
                        self.declare_selective_imports(root, &imp.path, names);
                    }
                    ImportKind::Namespace {
                        binding,
                        binding_span,
                    } => {
                        self.declare_namespace_import(root, &imp.path, binding, *binding_span);
                    }
                    ImportKind::Combined {
                        binding,
                        binding_span,
                        names,
                    } => {
                        self.declare_namespace_import(root, &imp.path, binding, *binding_span);
                        self.declare_selective_imports(root, &imp.path, names);
                    }
                },
            }
        }
    }

    fn declare_selective_imports(
        &mut self,
        root: usize,
        path: &str,
        names: &[ast::ImportName],
    ) {
        for n in names {
            let (local, span) = match &n.alias {
                Some((a, sp)) => (a.as_str(), *sp),
                None => (n.name.as_str(), n.name_span),
            };
            let origin = ImportOrigin {
                spec: path.to_string(),
                original: Some(n.name.clone()),
                is_namespace: false,
            };
            let id = self.new_binding_full(
                local,
                BindingKind::Import,
                Some(span),
                false,
                None,
                true,
                Some(origin),
            );
            self.define(root, local, id);
        }
    }

    fn declare_namespace_import(
        &mut self,
        root: usize,
        path: &str,
        binding: &str,
        binding_span: Span,
    ) {
        let origin = ImportOrigin {
            spec: path.to_string(),
            original: None,
            is_namespace: true,
        };
        let id = self.new_binding_full(
            binding,
            BindingKind::Import,
            Some(binding_span),
            false,
            None,
            true,
            Some(origin),
        );
        self.define(root, binding, id);
    }

    fn declare_members(&mut self, td: &ast::TypeDef) {
        for p in &td.params {
            let id =
                self.member_binding(&td.name, &p.name, BindingKind::Field, p.name_span, false);
            self.set_nav_ty(id, Self::nav_ty_from_type_expr(&p.ty));
        }
        for f in &td.fields {
            let id =
                self.member_binding(&td.name, &f.name, BindingKind::Field, f.name_span, false);
            self.set_nav_ty(id, Self::nav_ty_from_type_expr(&f.ty));
        }
        for m in &td.methods {
            let id = self.member_binding(
                &td.name,
                &m.name,
                BindingKind::Method,
                m.name_span,
                m.exported,
            );
            // Method return (may be type param `T`) for chain nav after subst.
            if let Some(ret) = &m.return_type {
                self.set_nav_ty(id, Self::nav_ty_from_type_expr(ret));
            }
        }
    }

    fn member_binding(
        &mut self,
        type_name: &str,
        name: &str,
        kind: BindingKind,
        span: Span,
        exported: bool,
    ) -> u32 {
        let key = (type_name.to_string(), name.to_string());
        if let Some(&existing) = self.res.member_index.get(&key) {
            // Same-name field/ctor: unify (refs go to first).
            if kind == BindingKind::Field {
                self.res.references.push(Reference {
                    span,
                    binding_id: Some(existing),
                    ctx: RefCtx::Member,
                });
                return existing;
            }
        }
        let id = self.new_binding(
            name,
            kind,
            Some(span),
            exported,
            Some(type_name.to_string()),
            false,
        );
        self.res.member_index.insert(key, id);
        id
    }

    fn walk_item(&mut self, item: &Item) {
        let root = self.root_scope();
        match item {
            Item::Function(f) => {
                self.walk_type_expr_opt(f.return_type.as_ref(), root);
                let inner = self.push_scope(Some(root));
                self.bind_params(&f.params, inner, root);
                self.walk_block(&f.body, inner);
            }
            Item::Extern(e) => {
                for p in &e.params {
                    self.walk_type_expr(&p.ty, root);
                }
                self.walk_type_expr_opt(e.return_type.as_ref(), root);
            }
            Item::Type(td) => {
                let body = self.push_scope(Some(root));
                // Members are visible in field defaults / method bodies (seeded above).
                let member_ids: Vec<(String, u32)> = self
                    .res
                    .bindings
                    .values()
                    .filter(|b| b.owner.as_deref() == Some(td.name.as_str()))
                    .map(|b| (b.name.clone(), b.id))
                    .collect();
                for (name, id) in member_ids {
                    self.define(body, &name, id);
                }
                for p in &td.params {
                    self.walk_type_expr(&p.ty, root);
                }
                for f in &td.fields {
                    self.walk_type_expr(&f.ty, root);
                }
                for m in &td.methods {
                    self.walk_type_expr_opt(m.return_type.as_ref(), root);
                    let method_scope = self.push_scope(Some(body));
                    self.bind_params(&m.params, method_scope, root);
                    self.walk_block(&m.body, method_scope);
                }
            }
            Item::Global(g) => {
                self.walk_expr(&g.init, root);
                // Module-level let/mut: feed typed-member nav from init.
                if let Some(id) = self.lookup(root, &g.name) {
                    let nav = self.expr_nav_ty(&g.init, root);
                    self.set_nav_ty(id, nav);
                }
            }
            Item::Import(_) => {
                // Import path string is not a name reference; locals already seeded.
            }
            Item::SessionStmt(bi) => {
                // Pre-session-prepare only; walk like a block item for LSP.
                self.walk_block_item(bi, root);
            }
        }
    }

    fn bind_params(&mut self, params: &[ast::Param], scope: usize, type_scope: usize) {
        for p in params {
            self.walk_type_expr(&p.ty, type_scope);
            let id = self.new_binding(
                &p.name,
                BindingKind::Param,
                Some(p.name_span),
                false,
                None,
                false,
            );
            self.set_nav_ty(id, Self::nav_ty_from_type_expr(&p.ty));
            self.define(scope, &p.name, id);
        }
    }

    fn walk_block(&mut self, block: &Block, scope: usize) {
        let inner = self.push_scope(Some(scope));
        for item in &block.items {
            self.walk_block_item(item, inner);
        }
        if let Some(tail) = &block.tail {
            self.walk_expr(tail, inner);
        }
    }

    fn walk_block_item(&mut self, item: &BlockItem, scope: usize) {
        match item {
            BlockItem::Let {
                name,
                name_span,
                mutable,
                ty,
                init,
            } => {
                self.walk_expr(init, scope);
                if let Some(t) = ty {
                    self.walk_type_expr(t, scope);
                }
                let kind = if *mutable {
                    BindingKind::Mut
                } else {
                    BindingKind::Let
                };
                let id = self.new_binding(name, kind, Some(*name_span), false, None, false);
                let nav = ty
                    .as_ref()
                    .and_then(Self::nav_ty_from_type_expr)
                    .or_else(|| self.ty_hint_from_init(init, scope));
                self.set_nav_ty(id, nav);
                self.define(scope, name, id);
            }
            BlockItem::Assign { target, value } => {
                self.walk_assign_target(target, scope);
                self.walk_expr(value, scope);
            }
            BlockItem::Expr(e) => self.walk_expr(e, scope),
        }
    }

    fn walk_assign_target(&mut self, target: &AssignTarget, scope: usize) {
        match target {
            AssignTarget::Name { name, name_span } => {
                self.add_ref(*name_span, self.lookup(scope, name), RefCtx::Value);
            }
            AssignTarget::Field {
                base,
                base_span,
                field,
                field_span,
                ..
            } => {
                self.add_ref(*base_span, self.lookup(scope, base), RefCtx::Value);
                let mid = self
                    .lookup(scope, base)
                    .and_then(|id| self.res.bindings.get(&id)?.ty_hint.clone())
                    .and_then(|ty| self.member_id(&ty, field));
                self.add_ref(*field_span, mid, RefCtx::Member);
            }
            AssignTarget::Index {
                base,
                base_span,
                field,
                field_span,
                index,
                ..
            } => {
                self.add_ref(*base_span, self.lookup(scope, base), RefCtx::Value);
                let mid = self
                    .lookup(scope, base)
                    .and_then(|id| self.res.bindings.get(&id)?.ty_hint.clone())
                    .and_then(|ty| self.member_id(&ty, field));
                self.add_ref(*field_span, mid, RefCtx::Member);
                self.walk_expr(index, scope);
            }
            AssignTarget::DynIndex {
                name,
                name_span,
                index,
            } => {
                self.add_ref(*name_span, self.lookup(scope, name), RefCtx::Value);
                self.walk_expr(index, scope);
            }
        }
    }

    fn walk_expr(&mut self, expr: &Expr, scope: usize) {
        match &expr.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::String(_) | ExprKind::Break
            | ExprKind::Continue => {}
            ExprKind::Interpolate(parts) => {
                for p in parts {
                    if let ast::InterpPart::Expr(e) = p {
                        self.walk_expr(e, scope);
                    }
                }
            }
            ExprKind::Name(name) => {
                self.add_ref(expr.span, self.lookup(scope, name), RefCtx::Value);
            }
            ExprKind::Unary { expr: inner, .. } => self.walk_expr(inner, scope),
            ExprKind::Binary { left, right, .. } => {
                self.walk_expr(left, scope);
                self.walk_expr(right, scope);
            }
            ExprKind::Call { callee, args } => {
                // Callee span is not stored separately; best-effort: ref at expr start length.
                let callee_span = Span::new(expr.span.start, expr.span.start + callee.len());
                let bid = self.lookup(scope, callee);
                self.add_ref(callee_span, bid, RefCtx::Call);
                self.walk_args(args, scope, bid);
            }
            ExprKind::Field {
                base,
                name,
                name_span,
            } => {
                self.walk_expr(base, scope);
                let mid = self
                    .receiver_ty_hint(base, scope)
                    .and_then(|ty| self.member_id(&ty, name));
                self.add_ref(*name_span, mid, RefCtx::Member);
            }
            ExprKind::MethodCall {
                receiver,
                method,
                method_span,
                args,
            } => {
                self.walk_expr(receiver, scope);
                let mid = self
                    .receiver_ty_hint(receiver, scope)
                    .and_then(|ty| self.member_id(&ty, method));
                self.add_ref(*method_span, mid, RefCtx::Member);
                self.walk_args(args, scope, None);
            }
            ExprKind::Index { base, index } => {
                self.walk_expr(base, scope);
                self.walk_expr(index, scope);
            }
            ExprKind::ArrayLit(elems) => {
                for e in elems {
                    self.walk_expr(e, scope);
                }
            }
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(cond, scope);
                self.walk_block(then_branch, scope);
                if let Some(e) = else_branch {
                    self.walk_block(e, scope);
                }
            }
            ExprKind::While { cond, body } => {
                self.walk_expr(cond, scope);
                self.walk_block(body, scope);
            }
            ExprKind::Loop { body } => self.walk_block(body, scope),
            ExprKind::For {
                name,
                name_span,
                start,
                end,
                body,
                ..
            } => {
                self.walk_expr(start, scope);
                if let Some(e) = end {
                    self.walk_expr(e, scope);
                }
                let inner = self.push_scope(Some(scope));
                let id = self.new_binding(
                    name,
                    BindingKind::ForVar,
                    Some(*name_span),
                    false,
                    None,
                    false,
                );
                self.define(inner, name, id);
                self.walk_block(body, inner);
            }
            ExprKind::Return { value } => {
                if let Some(v) = value {
                    self.walk_expr(v, scope);
                }
            }
            ExprKind::Block(b) => self.walk_block(b, scope),
            ExprKind::Try(inner) => self.walk_expr(inner, scope),
            ExprKind::Match { scrutinee, arms } => {
                self.walk_expr(scrutinee, scope);
                for arm in arms {
                    self.walk_match_arm(arm, scope);
                }
            }
        }
    }

    fn walk_args(&mut self, args: &[Arg], scope: usize, callee_bid: Option<u32>) {
        let param_map = self.params_of(callee_bid);
        for a in args {
            match a {
                Arg::Pos(e) => self.walk_expr(e, scope),
                Arg::Named {
                    name,
                    name_span,
                    value,
                } => {
                    if let Some(&pid) = param_map.get(name.as_str()) {
                        self.add_ref(*name_span, Some(pid), RefCtx::Label);
                    }
                    self.walk_expr(value, scope);
                }
            }
        }
    }

    fn params_of(&self, callee_bid: Option<u32>) -> HashMap<String, u32> {
        let mut out = HashMap::new();
        let Some(cid) = callee_bid else {
            return out;
        };
        let Some(callee) = self.res.bindings.get(&cid) else {
            return out;
        };
        if !matches!(callee.kind, BindingKind::Fn | BindingKind::Extern) {
            return out;
        }
        // Params are already created when we walked the function — but call sites
        // may appear before the callee body walk finishes (we walk items in order,
        // seeding all names first, then walking each body). When walking a body,
        // other functions' params already exist if those functions were walked first.
        // Best-effort: find param bindings by name that were defined for this fn —
        // we don't store owner on params. Scan module items for matching fn.
        for item in &self.module.items {
            let (name, params) = match item {
                Item::Function(f) => (f.name.as_str(), f.params.as_slice()),
                Item::Extern(e) => (e.name.as_str(), e.params.as_slice()),
                _ => continue,
            };
            if name != callee.name {
                continue;
            }
            for p in params {
                // Find binding with this param's def span.
                for b in self.res.bindings.values() {
                    if b.kind == BindingKind::Param
                        && b.name == p.name
                        && b.def_span == Some(p.name_span)
                    {
                        out.insert(p.name.clone(), b.id);
                        break;
                    }
                }
            }
            break;
        }
        out
    }

    fn walk_match_arm(&mut self, arm: &MatchArm, scope: usize) {
        let inner = self.push_scope(Some(scope));
        match &arm.pattern {
            Pattern::Variant {
                name,
                binds,
                bind_span,
                span,
            } => {
                // Pass / Fail / Has / Not / union constructors.
                let name_end = span.start + name.len().min(span.end - span.start);
                let name_span = Span::new(span.start, name_end);
                self.add_ref(name_span, self.lookup(scope, name), RefCtx::Pattern);
                self.bind_variant_pattern_fields(name, binds, *bind_span, inner);
            }
            // Literal / else patterns introduce no bindings.
            Pattern::StringLit { .. }
            | Pattern::IntLit { .. }
            | Pattern::BoolLit { .. }
            | Pattern::Else { .. } => {}
            // Value bare bind (D110): introduce pattern binding for arm body.
            Pattern::Bind { name, span } => {
                let id = self.new_binding(
                    name,
                    BindingKind::Pattern,
                    Some(*span),
                    false,
                    None,
                    false,
                );
                self.define(inner, name, id);
            }
            // Value or-patterns (D108) — only literals; no binds.
            Pattern::Or { alts, .. } => {
                for alt in alts {
                    match alt {
                        Pattern::StringLit { .. }
                        | Pattern::IntLit { .. }
                        | Pattern::BoolLit { .. }
                        | Pattern::Bind { .. }
                        | Pattern::Else { .. }
                        | Pattern::Or { .. } => {}
                        Pattern::Variant {
                            name,
                            binds,
                            bind_span,
                            span,
                        } => {
                            let name_end = span.start + name.len().min(span.end - span.start);
                            let name_span = Span::new(span.start, name_end);
                            self.add_ref(name_span, self.lookup(scope, name), RefCtx::Pattern);
                            self.bind_variant_pattern_fields(name, binds, *bind_span, inner);
                        }
                    }
                }
            }
        }
        self.walk_block(&arm.body, inner);
    }

    /// Bind variant payload names; ty_hint from product field types when known.
    fn bind_variant_pattern_fields(
        &mut self,
        type_name: &str,
        binds: &[Option<String>],
        bind_span: Span,
        scope: usize,
    ) {
        let field_tys = self.type_def_field_types(type_name);
        for (i, bopt) in binds.iter().enumerate() {
            let Some(bname) = bopt else {
                continue;
            };
            let id = self.new_binding(
                bname,
                BindingKind::Pattern,
                Some(bind_span),
                false,
                None,
                false,
            );
            if let Some(fields) = field_tys.as_ref() {
                if i < fields.len() {
                    self.set_nav_ty(id, Self::nav_ty_from_type_expr(&fields[i]));
                }
            }
            self.define(scope, bname, id);
        }
    }

    fn walk_type_expr_opt(&mut self, ty: Option<&TypeExpr>, scope: usize) {
        if let Some(t) = ty {
            self.walk_type_expr(t, scope);
        }
    }

    fn walk_type_expr(&mut self, ty: &TypeExpr, scope: usize) {
        if let Some(members) = &ty.union_members {
            for m in members {
                self.walk_type_expr(m, scope);
            }
            return;
        }
        if ty.lit.is_some() {
            return;
        }
        // Type name is a prefix of ty.span.
        let name_end = ty.span.start + ty.name.len().min(ty.span.end.saturating_sub(ty.span.start));
        let name_span = Span::new(ty.span.start, name_end);
        self.add_ref(name_span, self.lookup(scope, &ty.name), RefCtx::Type);
        for a in &ty.args {
            match a {
                TypeArg::Type(t) => self.walk_type_expr(t, scope),
                TypeArg::Int { .. } => {}
            }
        }
    }

    fn add_ref(&mut self, span: Span, binding_id: Option<u32>, ctx: RefCtx) {
        self.res.references.push(Reference {
            span,
            binding_id,
            ctx,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::byte_to_position;

    fn resolve_ok(src: &str) -> Resolution {
        resolve_source(src).expect("resolve")
    }

    fn pos_of(src: &str, needle: &str) -> (u32, u32) {
        let start = src.find(needle).expect(needle);
        let p = byte_to_position(src, start);
        (p.line, p.character)
    }

    #[test]
    fn fn_def_and_call() {
        let src = "fn helper() -> Int64 { 1 }\nfn main() -> Int64 { helper() }\n";
        let res = resolve_ok(src);
        let helper = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "helper")
            .expect("helper");
        assert_eq!(helper.kind, BindingKind::Fn);
        // Second `helper` is the call site (first is the definition name).
        let call_at = src.rfind("helper").unwrap();
        let p = byte_to_position(src, call_at);
        let r = res.ref_at(src, p.line, p.character).expect("ref at call");
        assert_eq!(r.binding_id, Some(helper.id));
        assert_eq!(r.ctx, RefCtx::Call);
        assert!(res.refs_to(helper.id).len() >= 1);
    }

    #[test]
    fn let_shadowing() {
        let src = "fn main() -> Int64 {\n  let x = 1\n  let x = 2\n  x\n}\n";
        let res = resolve_ok(src);
        let (line, col) = pos_of(src, "x\n}");
        // Last `x` before closing — find the use that's the tail
        let use_x = src.rfind("\n  x\n").unwrap() + 3; // start of tail x
        let p = byte_to_position(src, use_x);
        let r = res.ref_at(src, p.line, p.character).expect("tail x");
        let b = res.binding_of(Some(r)).expect("bound");
        assert_eq!(b.kind, BindingKind::Let);
        // Should be the second let (x = 2)
        assert_eq!(b.def_span.unwrap().start, src.find("let x = 2").unwrap() + 4);
        let _ = (line, col);
    }

    #[test]
    fn param_and_type_ref() {
        let src = "fn add(a: Int64, b: Int64) -> Int64 { a + b }\n";
        let res = resolve_ok(src);
        let (line, col) = pos_of(src, "a +");
        let r = res.ref_at(src, line, col).expect("a use");
        let b = res.binding_of(Some(r)).expect("param");
        assert_eq!(b.kind, BindingKind::Param);
        assert_eq!(b.name, "a");
        // Int64 type refs resolve to builtin
        let type_refs: Vec<_> = res
            .references
            .iter()
            .filter(|r| r.ctx == RefCtx::Type)
            .collect();
        assert!(!type_refs.is_empty());
        for tr in type_refs {
            let b = res.binding_of(Some(tr)).expect("type bound");
            assert!(
                b.kind == BindingKind::Builtin || b.kind == BindingKind::Type,
                "{b:?}"
            );
        }
    }

    #[test]
    fn user_type_and_method_linked() {
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
  export fn moved() -> Point { Point(x: x + 1) }
}
fn main() -> Int64 {
  let p = Point(x: 1)
  p.len()
  let q: Point = p
  q.x
  p.moved().x
}
";
        let res = resolve_ok(src);
        let point = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "Point")
            .expect("Point");
        assert_eq!(point.kind, BindingKind::Type);
        let members = res.members_of("Point");
        assert!(members.iter().any(|m| m.name == "x" && m.kind == BindingKind::Field));
        assert!(members.iter().any(|m| m.name == "len" && m.kind == BindingKind::Method));
        // method call linked via ctor ty_hint
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("len ref");
        assert_eq!(r.ctx, RefCtx::Member);
        let b = res.binding_of(Some(r)).expect("method linked via ty_hint");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
        // field access on annotated binding
        let x_use_main = src.find("q.x").unwrap() + 2;
        let p = byte_to_position(src, x_use_main);
        let r = res.ref_at(src, p.line, p.character).expect("q.x ref");
        let b = res.binding_of(Some(r)).expect("field linked");
        assert_eq!(b.kind, BindingKind::Field);
        assert_eq!(b.name, "x");
        // chain: p.moved().x — trailing field linked via method return ty_hint
        let chain_x = src.rfind(".x").unwrap() + 1;
        let p = byte_to_position(src, chain_x);
        let r = res.ref_at(src, p.line, p.character).expect("chain .x");
        let b = res.binding_of(Some(r)).expect("chain field");
        assert_eq!(b.kind, BindingKind::Field);
        assert_eq!(b.name, "x");
        assert_eq!(b.owner.as_deref(), Some("Point"));
        // field use in method body binds to field
        let x_use = src.find("{ x }").unwrap() + 2;
        let p = byte_to_position(src, x_use);
        let r = res.ref_at(src, p.line, p.character).expect("x in method");
        let b = res.binding_of(Some(r)).expect("field");
        assert_eq!(b.kind, BindingKind::Field);
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn typed_member_via_try_unwrap() {
        // `let p = make()?` where make() -> Trial<Point, String> links p.len.
        let src = "\
import { Pass, Fail, Trial } from \"std/Fallible\"
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn make() -> Trial<Point, String> { Pass(Point(x: 3)) }
fn main() -> Trial<Int64, String> {
  let p = make()?
  p.len()
  Pass(0)
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len");
        let b = res.binding_of(Some(r)).expect("method via try unwrap");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn typed_member_via_function_return() {
        // Free fn return type → let binding / direct call receiver.
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn make() -> Point { Point(x: 3) }
fn main() -> Int64 {
  let p = make()
  p.len()
  make().x
  let q = p
  q.x
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len");
        let b = res.binding_of(Some(r)).expect("method via fn return");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));

        let call_x = src.find("make().x").unwrap() + "make().".len();
        let p = byte_to_position(src, call_x);
        let r = res.ref_at(src, p.line, p.character).expect("make().x");
        let b = res.binding_of(Some(r)).expect("field via direct call");
        assert_eq!(b.kind, BindingKind::Field);
        assert_eq!(b.name, "x");
        assert_eq!(b.owner.as_deref(), Some("Point"));

        let q_x = src.find("q.x").unwrap() + 2;
        let p = byte_to_position(src, q_x);
        let r = res.ref_at(src, p.line, p.character).expect("q.x");
        let b = res.binding_of(Some(r)).expect("copy hint");
        assert_eq!(b.kind, BindingKind::Field);
        assert_eq!(b.name, "x");
    }

    #[test]
    fn typed_member_via_generic_return_head() {
        // `fn make() -> Box<Point>` → head name Box for member linking.
        let src = "\
type Point(x: Int64)
type Box<T>(value: T) {
  export fn get() -> T { value }
}
fn make() -> Box<Point> { Box(value: Point(x: 1)) }
fn main() -> Int64 {
  let b = make()
  b.get()
  0
}
";
        let res = resolve_ok(src);
        let get_call = src.find("b.get()").unwrap() + 2;
        let p = byte_to_position(src, get_call);
        let r = res.ref_at(src, p.line, p.character).expect("b.get");
        let b = res.binding_of(Some(r)).expect("method via generic return head");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "get");
        assert_eq!(b.owner.as_deref(), Some("Box"));
    }

    #[test]
    fn typed_member_via_type_param_return() {
        // `get() -> T` with `Box<Point>` receiver → chain `.x` on Point.
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
type Box<T>(value: T) {
  export fn get() -> T { value }
}
fn make() -> Box<Point> { Box(value: Point(x: 1)) }
fn main() -> Int64 {
  let b = make()
  b.get().len()
  0
}
";
        let res = resolve_ok(src);
        let len_call = src.find("b.get().len()").unwrap() + "b.get().".len();
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("get().len");
        let b = res.binding_of(Some(r)).expect("method via type-param return");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn typed_member_via_if_init() {
        // Untyped let from if/else with same constructor type in both arms.
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn main() -> Int64 {
  let p = if true { Point(x: 1) } else { Point(x: 2) }
  p.len()
  0
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len");
        let b = res.binding_of(Some(r)).expect("method via if-init");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn typed_member_via_block_init() {
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn main() -> Int64 {
  let p = { Point(x: 3) }
  p.len()
  0
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len");
        let b = res.binding_of(Some(r)).expect("method via block-init");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
    }

    #[test]
    fn typed_member_via_match_init() {
        // Value-match arms returning the same constructor type.
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn main() -> Int64 {
  let n = 0
  let p = match n {
    0 { Point(x: 1) }
    else { Point(x: 2) }
  }
  p.len()
  0
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len");
        let b = res.binding_of(Some(r)).expect("method via match-init");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn typed_member_via_global_init() {
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
let origin = Point(x: 0)
fn main() -> Int64 {
  origin.len()
  0
}
";
        let res = resolve_ok(src);
        let len_call = src.find("origin.len()").unwrap() + "origin.".len();
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("origin.len");
        let b = res.binding_of(Some(r)).expect("method via global init");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
    }

    #[test]
    fn typed_member_via_match_pattern_payload() {
        // `Box(p)` payload inherits field type Point → p.x links.
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
type Wrap(value: Point)
fn main() -> Int64 {
  let w = Wrap(value: Point(x: 2))
  match w {
    Wrap(p) { p.len() }
  }
}
";
        let res = resolve_ok(src);
        let len_call = src.find("p.len()").unwrap() + 2;
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("p.len in match");
        let b = res.binding_of(Some(r)).expect("method via pattern payload");
        assert_eq!(b.kind, BindingKind::Method);
        assert_eq!(b.name, "len");
        assert_eq!(b.owner.as_deref(), Some("Point"));
    }

    #[test]
    fn match_pattern_bind() {
        let src = "\
fn f(t: Trial<Int64, Int64>) -> Int64 {
  match t {
    Pass(v) { v }
    Fail(_) { 0 }
  }
}
";
        let res = resolve_ok(src);
        let v_use = src.find("{ v }").unwrap() + 2;
        let p = byte_to_position(src, v_use);
        let r = res.ref_at(src, p.line, p.character).expect("v use");
        let b = res.binding_of(Some(r)).expect("pattern");
        assert_eq!(b.kind, BindingKind::Pattern);
        assert_eq!(b.name, "v");
    }

    #[test]
    fn for_var() {
        let src = "fn main() -> Int64 {\n  for i in 0..3 { i }\n  0\n}\n";
        let res = resolve_ok(src);
        let i_use = src.find("{ i }").unwrap() + 2;
        let p = byte_to_position(src, i_use);
        let r = res.ref_at(src, p.line, p.character).expect("i");
        let b = res.binding_of(Some(r)).expect("forvar");
        assert_eq!(b.kind, BindingKind::ForVar);
    }

    #[test]
    fn import_local_binding() {
        let src = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        let res = resolve_ok(src);
        let answer = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "answer")
            .expect("import");
        assert_eq!(answer.kind, BindingKind::Import);
        let origin = answer.import_origin.as_ref().expect("origin");
        assert_eq!(origin.spec, "./Helper");
        assert_eq!(origin.original.as_deref(), Some("answer"));
        assert!(!origin.is_namespace);
        let use_at = src.rfind("answer").unwrap();
        let p = byte_to_position(src, use_at);
        let r = res.ref_at(src, p.line, p.character).expect("use");
        assert_eq!(r.binding_id, Some(answer.id));
    }

    #[test]
    fn import_alias_and_namespace_origin() {
        let src = "import { answer as ans } from \"./Helper\"\nimport h from \"./Helper\"\nfn main() -> Int64 { 0 }\n";
        let res = resolve_ok(src);
        let ans = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "ans")
            .expect("alias");
        let o = ans.import_origin.as_ref().unwrap();
        assert_eq!(o.original.as_deref(), Some("answer"));
        assert!(!o.is_namespace);
        let h = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "h")
            .expect("ns");
        let no = h.import_origin.as_ref().unwrap();
        assert!(no.is_namespace);
        assert!(no.original.is_none());
    }

    #[test]
    fn import_combined_namespace_and_selective() {
        let src = "import h, { answer as ans } from \"./Helper\"\nfn main() -> Int64 { 0 }\n";
        let res = resolve_ok(src);
        let ans = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "ans")
            .expect("selective");
        let o = ans.import_origin.as_ref().unwrap();
        assert_eq!(o.original.as_deref(), Some("answer"));
        assert!(!o.is_namespace);
        let h = res
            .module_definitions()
            .into_iter()
            .find(|b| b.name == "h")
            .expect("ns");
        let no = h.import_origin.as_ref().unwrap();
        assert!(no.is_namespace);
        assert!(no.original.is_none());
    }

    #[test]
    fn binding_at_definition() {
        let src = "fn main() -> Int64 { 0 }\n";
        let res = resolve_ok(src);
        let (line, col) = pos_of(src, "main");
        let b = res.binding_at(src, line, col).expect("on def");
        assert_eq!(b.name, "main");
        assert_eq!(b.kind, BindingKind::Fn);
    }

    #[test]
    fn parse_error_propagates() {
        assert!(resolve_source("fn main() {").is_err());
    }

    #[test]
    fn let_from_route_call_has_may_string_ty_hint() {
        let src = r#"
type MyRouter {
    GET "/home" { "Home" }
}
fn main() -> Int64 {
    let r = MyRouter()
    let a = r.route(method: "GET", path: "/home")
    a
    0
}
"#;
        let res = resolve_ok(src);
        let use_a = src.rfind("\n    a\n").unwrap() + 5;
        let p = byte_to_position(src, use_a);
        let r = res.ref_at(src, p.line, p.character).expect("ref a");
        let b = res.binding_of(Some(r)).expect("binding a");
        assert_eq!(
            b.ty_hint.as_deref(),
            Some("May"),
            "expected May ty_hint on let a from r.route(), got {:?}",
            b.ty_hint
        );
        assert_eq!(
            b.ty_args,
            vec!["String".to_string()],
            "expected May<String> ty_args, got {:?}",
            b.ty_args
        );
    }
}
