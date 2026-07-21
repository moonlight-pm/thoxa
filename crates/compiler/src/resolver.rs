//! Shared lexical name resolver.
//!
//! Produces a position-queryable table of definitions ([`Binding`]) and
//! occurrences ([`Reference`]). Resolution is **type-independent** so both MIR
//! lower (future) and the LSP can consume one scope implementation.
//!
//! Member access (`.field` / `.method`) is emitted as a *pending* reference
//! (`binding_id = None`, ctx `Member`); a later pass links it when the receiver
//! type is provable.
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
    "Array", "Address", "Trial", "May", "Pass", "Fail", "Has", "Not", "true", "false", "println",
    "print",
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

/// Resolve a parsed module (single-file; imports are local bindings only).
pub fn resolve_module(module: &Module) -> Resolution {
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
            },
        );
        if module_level {
            self.res.module_binding_ids.push(id);
        }
        id
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
                        for n in names {
                            let (local, span) = match &n.alias {
                                Some((a, sp)) => (a.as_str(), *sp),
                                None => (n.name.as_str(), n.name_span),
                            };
                            let origin = ImportOrigin {
                                spec: imp.path.clone(),
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
                    ImportKind::Namespace {
                        binding,
                        binding_span,
                    } => {
                        let origin = ImportOrigin {
                            spec: imp.path.clone(),
                            original: None,
                            is_namespace: true,
                        };
                        let id = self.new_binding_full(
                            binding,
                            BindingKind::Import,
                            Some(*binding_span),
                            false,
                            None,
                            true,
                            Some(origin),
                        );
                        self.define(root, binding, id);
                    }
                },
            }
        }
    }

    fn declare_members(&mut self, td: &ast::TypeDef) {
        for p in &td.params {
            self.member_binding(&td.name, &p.name, BindingKind::Field, p.name_span, false);
        }
        for f in &td.fields {
            self.member_binding(&td.name, &f.name, BindingKind::Field, f.name_span, false);
        }
        for m in &td.methods {
            self.member_binding(
                &td.name,
                &m.name,
                BindingKind::Method,
                m.name_span,
                m.exported,
            );
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
            }
            Item::Import(_) => {
                // Import path string is not a name reference; locals already seeded.
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
                field_span,
                ..
            } => {
                self.add_ref(*base_span, self.lookup(scope, base), RefCtx::Value);
                self.add_ref(*field_span, None, RefCtx::Member);
            }
            AssignTarget::Index {
                base,
                base_span,
                field_span,
                index,
                ..
            } => {
                self.add_ref(*base_span, self.lookup(scope, base), RefCtx::Value);
                self.add_ref(*field_span, None, RefCtx::Member);
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
                name_span,
                ..
            } => {
                self.walk_expr(base, scope);
                self.add_ref(*name_span, None, RefCtx::Member);
            }
            ExprKind::MethodCall {
                receiver,
                method_span,
                args,
                ..
            } => {
                self.walk_expr(receiver, scope);
                self.add_ref(*method_span, None, RefCtx::Member);
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
                bind,
                bind_span,
                span,
                has_payload: _,
            } => {
                // Pass / Fail / Has / Not as constructors (builtins).
                let name_end = span.start + name.len().min(span.end - span.start);
                let name_span = Span::new(span.start, name_end);
                self.add_ref(name_span, self.lookup(scope, name), RefCtx::Pattern);
                if let Some(bname) = bind {
                    let id = self.new_binding(
                        bname,
                        BindingKind::Pattern,
                        Some(*bind_span),
                        false,
                        None,
                        false,
                    );
                    self.define(inner, bname, id);
                }
            }
            // Literal / else patterns introduce no bindings.
            Pattern::StringLit { .. }
            | Pattern::IntLit { .. }
            | Pattern::BoolLit { .. }
            | Pattern::Else { .. } => {}
        }
        self.walk_block(&arm.body, inner);
    }

    fn walk_type_expr_opt(&mut self, ty: Option<&TypeExpr>, scope: usize) {
        if let Some(t) = ty {
            self.walk_type_expr(t, scope);
        }
    }

    fn walk_type_expr(&mut self, ty: &TypeExpr, scope: usize) {
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
    fn user_type_and_method_pending() {
        let src = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn main() -> Int64 {
  let p = Point(1)
  p.len()
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
        // method call is pending member (`.len` use, not `fn len` definition)
        let len_call = src.rfind("len").unwrap();
        let p = byte_to_position(src, len_call);
        let r = res.ref_at(src, p.line, p.character).expect("len ref");
        assert_eq!(r.ctx, RefCtx::Member);
        assert!(r.binding_id.is_none(), "pending until type-driven link");
        // field use in method body binds to field
        let x_use = src.find("{ x }").unwrap() + 2;
        let p = byte_to_position(src, x_use);
        let r = res.ref_at(src, p.line, p.character).expect("x in method");
        let b = res.binding_of(Some(r)).expect("field");
        assert_eq!(b.kind, BindingKind::Field);
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
}
