//! Lazy cross-module resolution for hover / go-to-definition.
//!
//! When the cursor lands on an imported name (or a namespace-qualified member),
//! load the defining module (open buffer preferred over disk), find the export,
//! and return signature + doc + jump location. Any failure returns `None` so
//! handlers fall back to single-file behaviour.

use crate::ast::{self, Expr, ExprKind, Item, Module, TypeArg, TypeExpr};
use crate::lexer::Lexer;
use crate::loader;
use crate::lsp::analysis;
use crate::lsp::convert;
use crate::lsp::index::{self, DocumentIndex};
use crate::parser::Parser;
use crate::resolver::{
    self, unwrap_fallible_nav_ty, Binding, BindingKind, NavTy, RefCtx, Reference, Resolution,
};
use crate::span::{self, Span};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A definition that lives in another module.
#[derive(Debug, Clone)]
pub struct Remote {
    pub signature: String,
    pub doc: String,
    /// Absolute (or soft-absolute) path of the defining module.
    pub path: PathBuf,
    /// Name span in the remote file; `None` for module-summary (file top).
    pub def_span: Option<Span>,
    /// Remote source text (for span → range).
    pub text: String,
}

/// Resolve a cross-module target under the cursor, or `None` to fall back locally.
///
/// `docs` maps LSP document URI → buffer text so **open buffers win over disk**.
pub fn resolve_remote(
    idx: &DocumentIndex,
    ref_: Option<&Reference>,
    binding: Option<&Binding>,
    docs: &HashMap<String, String>,
) -> Option<Remote> {
    // Catch panics from path/parse edge cases; never break the handler.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resolve_remote_inner(idx, ref_, binding, docs)
    })) {
        Ok(r) => r,
        Err(_) => None,
    }
}

fn resolve_remote_inner(
    idx: &DocumentIndex,
    ref_: Option<&Reference>,
    binding: Option<&Binding>,
    docs: &HashMap<String, String>,
) -> Option<Remote> {
    let current = Path::new(&idx.path);
    if !current.is_absolute() && !current.exists() {
        // Untitled / synthetic paths cannot resolve `./` imports.
        // Still allow absolute-looking paths even if not yet on disk.
        if !idx.path.starts_with('/') {
            return None;
        }
    }

    match origin(idx, ref_, binding, docs)? {
        Origin::Member { spec, name } => lookup_member(&spec, &name, docs, current),
        Origin::Module { spec } => lookup_module(&spec, docs, current),
        Origin::TypeMember {
            spec,
            type_name,
            member,
        } => lookup_type_member(&spec, &type_name, &member, docs, current),
    }
}

enum Origin {
    /// Selective import use / namespace member / aliased import.
    Member { spec: String, name: String },
    /// Namespace binding itself (`import ns from "…"`).
    Module { spec: String },
    /// Method/field on a value whose type is defined in another module
    /// (e.g. `let s = net.createSocket()?; s.reuseAddr()`).
    TypeMember {
        spec: String,
        type_name: String,
        member: String,
    },
}

fn origin(
    idx: &DocumentIndex,
    ref_: Option<&Reference>,
    binding: Option<&Binding>,
    docs: &HashMap<String, String>,
) -> Option<Origin> {
    // (1) Imported name (kind Import).
    if let Some(b) = binding {
        if b.kind == BindingKind::Import {
            let origin = b.import_origin.as_ref()?;
            if origin.is_namespace {
                return Some(Origin::Module {
                    spec: origin.spec.clone(),
                });
            }
            let name = origin.original.clone().unwrap_or_else(|| b.name.clone());
            return Some(Origin::Member {
                spec: origin.spec.clone(),
                name,
            });
        }
    }

    // (2) Pending member on a namespace import: `mu.add` / `geometry.Point`.
    // (3) Pending member on a value typed from a remote module (after `?`, etc.).
    if binding.is_none() {
        if let Some(r) = ref_ {
            if r.ctx == RefCtx::Member {
                if let Some(module) = idx.module.as_ref() {
                    if let Some((spec, name)) = namespace_member_origin(idx, module, r) {
                        return Some(Origin::Member { spec, name });
                    }
                    if let Some((spec, type_name, member)) =
                        typed_remote_member_origin(idx, module, r, docs)
                    {
                        return Some(Origin::TypeMember {
                            spec,
                            type_name,
                            member,
                        });
                    }
                }
            }
        }
    }

    None
}

/// `recv.member` where `recv`'s value type is defined in another module.
fn typed_remote_member_origin(
    idx: &DocumentIndex,
    module: &Module,
    member_ref: &Reference,
    docs: &HashMap<String, String>,
) -> Option<(String, String, String)> {
    let (recv_name, member) = find_member_access(module, member_ref.span)?;
    let current = Path::new(&idx.path);
    // Prefer the receiver name's linked binding (local let/param), not only
    // module-level names — `server.reuseAddr` uses a function-local let.
    let nav = infer_receiver_nav_ty(idx, module, member_ref, &recv_name, docs, current)?;
    let spec = type_defining_spec(idx, &nav, docs, current)?;
    Some((spec, nav.head, member))
}

/// Infer type of the receiver of the member access at `member_ref`.
fn infer_receiver_nav_ty(
    idx: &DocumentIndex,
    module: &Module,
    member_ref: &Reference,
    recv_name: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<RemoteNavTy> {
    // Find the Name reference for the receiver immediately before the member.
    // Walk refs whose span text is recv_name and binding is Let/Mut/Param.
    if let Some(b) = binding_for_receiver_name(idx, member_ref, recv_name) {
        if matches!(b.kind, BindingKind::Let | BindingKind::Mut | BindingKind::Param) {
            if let Some(init) = find_binding_init(module, b) {
                if let Some(t) = infer_expr_nav_ty(idx, module, init, docs, current) {
                    return Some(t);
                }
            }
            if let Some(head) = &b.ty_hint {
                return Some(RemoteNavTy {
                    head: head.clone(),
                    args: b.ty_args.clone(),
                    origin_spec: None,
                });
            }
        }
    }
    // Fallback: module-level name (import / global).
    infer_name_nav_ty(idx, module, recv_name, docs, current)
}

fn binding_for_receiver_name<'a>(
    idx: &'a DocumentIndex,
    member_ref: &Reference,
    recv_name: &str,
) -> Option<&'a Binding> {
    // Nearest reference to `recv_name` ending at or before the member span.
    let mut best: Option<&Reference> = None;
    for r in &idx.resolution.references {
        if r.span.end > member_ref.span.start {
            continue;
        }
        let Some(slice) = idx.text.get(r.span.start..r.span.end) else {
            continue;
        };
        if slice != recv_name {
            continue;
        }
        if best.map(|b| b.span.start < r.span.start).unwrap_or(true) {
            best = Some(r);
        }
    }
    let r = best?;
    idx.resolution.binding_of(Some(r))
}

/// Where is `nav.head` defined? Selective type import, producer origin, or local (None).
fn type_defining_spec(
    idx: &DocumentIndex,
    nav: &RemoteNavTy,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<String> {
    if let Some(id) = idx.resolution.lookup_name_at_module(&nav.head) {
        if let Some(b) = idx.resolution.bindings.get(&id) {
            if b.kind == BindingKind::Import {
                if let Some(o) = &b.import_origin {
                    if !o.is_namespace {
                        return Some(o.spec.clone());
                    }
                }
            }
            if b.kind == BindingKind::Type {
                return None;
            }
        }
    }
    if let Some(spec) = &nav.origin_spec {
        if remote_exports_type(spec, &nav.head, docs, current) {
            return Some(spec.clone());
        }
    }
    for b in idx.resolution.module_definitions() {
        if b.kind != BindingKind::Import {
            continue;
        }
        let Some(o) = b.import_origin.as_ref() else {
            continue;
        };
        if !o.is_namespace {
            continue;
        }
        if remote_exports_type(&o.spec, &nav.head, docs, current) {
            return Some(o.spec.clone());
        }
    }
    None
}

fn remote_exports_type(
    spec: &str,
    type_name: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> bool {
    let Some(path) = resolve_spec(spec, current) else {
        return false;
    };
    let Some(text) = module_text(&path, docs) else {
        return false;
    };
    let Some(res) = resolve_text(&text) else {
        return false;
    };
    res.bindings.values().any(|b| {
        b.name == type_name
            && b.kind == BindingKind::Type
            && b.exported
            && b.owner.is_none()
    })
}

/// Nav type plus optional import-spec of the module that produced it.
#[derive(Debug, Clone)]
struct RemoteNavTy {
    head: String,
    args: Vec<String>,
    /// Module that defined the producing API (namespace method / free fn).
    origin_spec: Option<String>,
}

impl RemoteNavTy {
    fn with_origin(mut self, spec: Option<String>) -> Self {
        if spec.is_some() {
            self.origin_spec = spec;
        }
        self
    }
}

fn nav_ty_from_type_expr(te: &TypeExpr) -> Option<RemoteNavTy> {
    if te.lit.is_some() || te.name.is_empty() {
        return None;
    }
    let mut args = Vec::new();
    for a in &te.args {
        if let TypeArg::Type(inner) = a {
            if let Some(n) = nav_ty_from_type_expr(inner) {
                args.push(n.head);
            }
        }
    }
    Some(RemoteNavTy {
        head: te.name.clone(),
        args,
        origin_spec: None,
    })
}

fn unwrap_remote_fallible(t: RemoteNavTy) -> RemoteNavTy {
    let plain = NavTy {
        head: t.head,
        args: t.args,
    };
    let u = unwrap_fallible_nav_ty(plain);
    RemoteNavTy {
        head: u.head,
        args: u.args,
        origin_spec: t.origin_spec,
    }
}

/// Infer the value type of a simple name binding (let/param/…), loading remote
/// modules when the init is a namespace call (optionally with `?`).
fn infer_name_nav_ty(
    idx: &DocumentIndex,
    module: &Module,
    name: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<RemoteNavTy> {
    let b = binding_by_name(idx, name)?;
    // Prefer re-inferring from init when present — picks up remote origins that
    // the single-file resolver cannot see (namespace returns after `?`).
    if matches!(b.kind, BindingKind::Let | BindingKind::Mut) {
        if let Some(init) = find_binding_init(module, b) {
            if let Some(t) = infer_expr_nav_ty(idx, module, init, docs, current) {
                return Some(t);
            }
        }
    }
    if let Some(head) = &b.ty_hint {
        return Some(RemoteNavTy {
            head: head.clone(),
            args: b.ty_args.clone(),
            origin_spec: None,
        });
    }
    // Namespace import used as a value has no type.
    if b.kind == BindingKind::Import {
        return None;
    }
    None
}

/// Module-level first, else last local Let/Mut/Param with this name (shadowing).
fn binding_by_name<'a>(idx: &'a DocumentIndex, name: &str) -> Option<&'a Binding> {
    if let Some(id) = idx.resolution.lookup_name_at_module(name) {
        if let Some(b) = idx.resolution.bindings.get(&id) {
            // Module-level hit (import/fn/type/global). Locals are not in this list.
            if !matches!(
                b.kind,
                BindingKind::Let | BindingKind::Mut | BindingKind::Param | BindingKind::ForVar
            ) {
                return Some(b);
            }
        }
    }
    idx.resolution
        .bindings
        .values()
        .filter(|b| {
            b.name == name
                && matches!(
                    b.kind,
                    BindingKind::Let | BindingKind::Mut | BindingKind::Param
                )
                && b.def_span.is_some()
        })
        .max_by_key(|b| b.def_span.map(|s| s.start).unwrap_or(0))
}

fn find_binding_init<'a>(module: &'a Module, binding: &Binding) -> Option<&'a Expr> {
    let def = binding.def_span?;
    find_let_init_in_module(module, def)
}

fn find_let_init_in_module(module: &Module, def_span: Span) -> Option<&Expr> {
    for item in &module.items {
        if let Some(e) = find_let_init_in_item(item, def_span) {
            return Some(e);
        }
    }
    None
}

fn find_let_init_in_item(item: &Item, def_span: Span) -> Option<&Expr> {
    match item {
        Item::Function(f) => find_let_init_in_block(&f.body, def_span),
        Item::Type(td) => {
            for m in &td.methods {
                if let Some(e) = find_let_init_in_block(&m.body, def_span) {
                    return Some(e);
                }
            }
            None
        }
        Item::Global(g) if g.name_span == def_span => Some(&g.init),
        Item::SessionStmt(bi) => find_let_init_in_block_item(bi, def_span),
        _ => None,
    }
}

fn find_let_init_in_block(block: &ast::Block, def_span: Span) -> Option<&Expr> {
    for bi in &block.items {
        if let Some(e) = find_let_init_in_block_item(bi, def_span) {
            return Some(e);
        }
    }
    None
}

fn find_let_init_in_block_item(item: &ast::BlockItem, def_span: Span) -> Option<&Expr> {
    match item {
        ast::BlockItem::Let {
            name_span, init, ..
        } if *name_span == def_span => Some(init),
        ast::BlockItem::Let { init, .. } => find_let_init_in_expr(init, def_span),
        ast::BlockItem::Assign { value, .. } => find_let_init_in_expr(value, def_span),
        ast::BlockItem::Expr(e) => find_let_init_in_expr(e, def_span),
    }
}

fn find_let_init_in_expr(expr: &Expr, def_span: Span) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Block(b) => find_let_init_in_block(b, def_span),
        ExprKind::If {
            then_branch,
            else_branch,
            cond,
            ..
        } => find_let_init_in_expr(cond, def_span)
            .or_else(|| find_let_init_in_block(then_branch, def_span))
            .or_else(|| else_branch.as_ref().and_then(|b| find_let_init_in_block(b, def_span))),
        ExprKind::While { cond, body } => {
            find_let_init_in_expr(cond, def_span).or_else(|| find_let_init_in_block(body, def_span))
        }
        ExprKind::Loop { body } => find_let_init_in_block(body, def_span),
        ExprKind::For {
            start, end, body, ..
        } => find_let_init_in_expr(start, def_span)
            .or_else(|| end.as_ref().and_then(|e| find_let_init_in_expr(e, def_span)))
            .or_else(|| find_let_init_in_block(body, def_span)),
        ExprKind::Match { scrutinee, arms } => {
            if let Some(e) = find_let_init_in_expr(scrutinee, def_span) {
                return Some(e);
            }
            for arm in arms {
                if let Some(e) = find_let_init_in_block(&arm.body, def_span) {
                    return Some(e);
                }
            }
            None
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Try(inner)
        | ExprKind::Field { base: inner, .. } => find_let_init_in_expr(inner, def_span),
        ExprKind::Binary { left, right, .. } => {
            find_let_init_in_expr(left, def_span).or_else(|| find_let_init_in_expr(right, def_span))
        }
        ExprKind::MethodCall { receiver, args, .. } => {
            if let Some(e) = find_let_init_in_expr(receiver, def_span) {
                return Some(e);
            }
            for a in args {
                match a {
                    ast::Arg::Pos(e) | ast::Arg::Named { value: e, .. } => {
                        if let Some(hit) = find_let_init_in_expr(e, def_span) {
                            return Some(hit);
                        }
                    }
                }
            }
            None
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                match a {
                    ast::Arg::Pos(e) | ast::Arg::Named { value: e, .. } => {
                        if let Some(hit) = find_let_init_in_expr(e, def_span) {
                            return Some(hit);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn infer_expr_nav_ty(
    idx: &DocumentIndex,
    module: &Module,
    expr: &Expr,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<RemoteNavTy> {
    match &expr.kind {
        ExprKind::Name(n) => infer_name_nav_ty(idx, module, n, docs, current),
        ExprKind::Try(inner) => {
            let t = infer_expr_nav_ty(idx, module, inner, docs, current)?;
            Some(unwrap_remote_fallible(t))
        }
        ExprKind::Unary { expr: inner, .. } => {
            infer_expr_nav_ty(idx, module, inner, docs, current)
        }
        ExprKind::Call { callee, .. } => {
            let id = idx.resolution.lookup_name_at_module(callee)?;
            let b = idx.resolution.bindings.get(&id)?;
            match b.kind {
                BindingKind::Type => Some(RemoteNavTy {
                    head: b.name.clone(),
                    args: Vec::new(),
                    origin_spec: None,
                }),
                BindingKind::Fn | BindingKind::Extern => {
                    let head = b.ty_hint.clone()?;
                    Some(RemoteNavTy {
                        head,
                        args: b.ty_args.clone(),
                        origin_spec: None,
                    })
                }
                BindingKind::Import => {
                    let o = b.import_origin.as_ref()?;
                    if o.is_namespace {
                        return None;
                    }
                    let name = o.original.as_deref().unwrap_or(b.name.as_str());
                    remote_export_return_nav(&o.spec, name, docs, current)
                        .map(|t| t.with_origin(Some(o.spec.clone())))
                }
                _ => None,
            }
        }
        ExprKind::MethodCall {
            receiver, method, ..
        } => {
            if let ExprKind::Name(recv) = &receiver.kind {
                if let Some(id) = idx.resolution.lookup_name_at_module(recv) {
                    if let Some(b) = idx.resolution.bindings.get(&id) {
                        if b.kind == BindingKind::Import {
                            if let Some(o) = &b.import_origin {
                                if o.is_namespace {
                                    return remote_export_return_nav(
                                        &o.spec, method, docs, current,
                                    )
                                    .map(|t| t.with_origin(Some(o.spec.clone())));
                                }
                            }
                        }
                    }
                }
            }
            let recv_ty = infer_expr_nav_ty(idx, module, receiver, docs, current)?;
            let spec = type_defining_spec(idx, &recv_ty, docs, current)?;
            remote_type_member_return_nav(&spec, &recv_ty.head, method, docs, current)
                .map(|t| t.with_origin(Some(spec)))
        }
        ExprKind::Field { base, name, .. } => {
            let recv_ty = infer_expr_nav_ty(idx, module, base, docs, current)?;
            let spec = type_defining_spec(idx, &recv_ty, docs, current)?;
            remote_type_member_return_nav(&spec, &recv_ty.head, name, docs, current)
                .map(|t| t.with_origin(Some(spec)))
        }
        _ => None,
    }
}

/// Return type of an exported free function or type name (ctor → type head).
fn remote_export_return_nav(
    spec: &str,
    name: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<RemoteNavTy> {
    let path = resolve_spec(spec, current)?;
    let text = module_text(&path, docs)?;
    let tokens = Lexer::new(&text).tokenize().ok()?;
    let remote_mod = Parser::new(tokens).parse_module().ok()?;
    for item in &remote_mod.items {
        match item {
            Item::Function(f) if f.exported && f.name == name => {
                return f.return_type.as_ref().and_then(nav_ty_from_type_expr);
            }
            Item::Extern(e) if e.exported && e.name == name => {
                return e.return_type.as_ref().and_then(nav_ty_from_type_expr);
            }
            Item::Type(td) if td.exported && td.name == name => {
                return Some(RemoteNavTy {
                    head: td.name.clone(),
                    args: Vec::new(),
                    origin_spec: Some(spec.to_string()),
                });
            }
            _ => {}
        }
    }
    None
}

/// Return type (or field type) of a member on an exported type in a remote module.
fn remote_type_member_return_nav(
    spec: &str,
    type_name: &str,
    member: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<RemoteNavTy> {
    let path = resolve_spec(spec, current)?;
    let text = module_text(&path, docs)?;
    let tokens = Lexer::new(&text).tokenize().ok()?;
    let remote_mod = Parser::new(tokens).parse_module().ok()?;
    for item in &remote_mod.items {
        let Item::Type(td) = item else { continue };
        if td.name != type_name {
            continue;
        }
        for m in &td.methods {
            if m.name == member {
                return m.return_type.as_ref().and_then(nav_ty_from_type_expr);
            }
        }
        for p in &td.params {
            if p.name == member {
                return nav_ty_from_type_expr(&p.ty);
            }
        }
        for f in &td.fields {
            if f.name == member {
                return nav_ty_from_type_expr(&f.ty);
            }
        }
    }
    None
}

fn lookup_type_member(
    spec: &str,
    type_name: &str,
    member: &str,
    docs: &HashMap<String, String>,
    current_path: &Path,
) -> Option<Remote> {
    let path = resolve_spec(spec, current_path)?;
    let text = module_text(&path, docs)?;
    let res = resolve_text(&text)?;
    let b = type_member_binding(&res, type_name, member)?;
    let inferred = remote_inferred_ret(&path, &text, docs, b);
    let sig = index::render_signature_with_ret(b, &text, inferred.as_deref())?;
    let doc = b
        .def_span
        .map(|s| index::doc_above(&text, s.start))
        .unwrap_or_default();
    Some(Remote {
        signature: sig,
        doc,
        path,
        def_span: b.def_span,
        text,
    })
}

fn type_member_binding<'a>(
    res: &'a Resolution,
    type_name: &str,
    member: &str,
) -> Option<&'a Binding> {
    res.bindings.values().find(|b| {
        b.name == member
            && b.owner.as_deref() == Some(type_name)
            && matches!(b.kind, BindingKind::Method | BindingKind::Field)
            && b.def_span.is_some()
    })
}

/// If `ref_` is the member of `ns.member` where `ns` is a namespace import, return (spec, member).
fn namespace_member_origin(
    idx: &DocumentIndex,
    module: &Module,
    member_ref: &Reference,
) -> Option<(String, String)> {
    let (recv_name, member) = find_member_access(module, member_ref.span)?;
    let recv_bid = idx.resolution.lookup_name_at_module(&recv_name)?;
    let recv = idx.resolution.bindings.get(&recv_bid)?;
    if recv.kind != BindingKind::Import {
        return None;
    }
    let origin = recv.import_origin.as_ref()?;
    if !origin.is_namespace {
        return None;
    }
    Some((origin.spec.clone(), member))
}

/// Walk the module for Field / MethodCall whose member name span matches `span`.
fn find_member_access(module: &Module, span: Span) -> Option<(String, String)> {
    for item in &module.items {
        if let Some(hit) = walk_item_member(item, span) {
            return Some(hit);
        }
    }
    None
}

fn walk_item_member(item: &Item, span: Span) -> Option<(String, String)> {
    match item {
        Item::Function(f) => walk_block_member(&f.body, span),
        Item::Type(td) => {
            for m in &td.methods {
                if let Some(h) = walk_block_member(&m.body, span) {
                    return Some(h);
                }
            }
            None
        }
        Item::Global(g) => walk_expr_member(&g.init, span),
        Item::Extern(_) | Item::Import(_) | Item::SessionStmt(_) => None,
    }
}

fn walk_block_member(block: &crate::ast::Block, span: Span) -> Option<(String, String)> {
    for bi in &block.items {
        match bi {
            crate::ast::BlockItem::Let { init, .. } => {
                if let Some(h) = walk_expr_member(init, span) {
                    return Some(h);
                }
            }
            crate::ast::BlockItem::Assign { target, value } => {
                if let Some(h) = walk_assign_member(target, span) {
                    return Some(h);
                }
                if let Some(h) = walk_expr_member(value, span) {
                    return Some(h);
                }
            }
            crate::ast::BlockItem::Expr(e) => {
                if let Some(h) = walk_expr_member(e, span) {
                    return Some(h);
                }
            }
        }
    }
    if let Some(t) = &block.tail {
        return walk_expr_member(t, span);
    }
    None
}

fn walk_assign_member(
    target: &crate::ast::AssignTarget,
    span: Span,
) -> Option<(String, String)> {
    match target {
        crate::ast::AssignTarget::Field {
            base,
            field,
            field_span,
            ..
        } if *field_span == span => Some((base.clone(), field.clone())),
        crate::ast::AssignTarget::Index {
            index, field_span, ..
        } => {
            if *field_span == span {
                // Not a simple name.member form for namespace.
                return None;
            }
            walk_expr_member(index, span)
        }
        crate::ast::AssignTarget::DynIndex { index, .. } => walk_expr_member(index, span),
        _ => None,
    }
}

fn walk_expr_member(expr: &Expr, span: Span) -> Option<(String, String)> {
    match &expr.kind {
        ExprKind::Field {
            base,
            name,
            name_span,
        } if *name_span == span => {
            if let ExprKind::Name(recv) = &base.kind {
                return Some((recv.clone(), name.clone()));
            }
            walk_expr_member(base, span)
        }
        ExprKind::MethodCall {
            receiver,
            method,
            method_span,
            args,
        } => {
            if *method_span == span {
                if let ExprKind::Name(recv) = &receiver.kind {
                    return Some((recv.clone(), method.clone()));
                }
            }
            if let Some(h) = walk_expr_member(receiver, span) {
                return Some(h);
            }
            for a in args {
                match a {
                    crate::ast::Arg::Pos(e) | crate::ast::Arg::Named { value: e, .. } => {
                        if let Some(h) = walk_expr_member(e, span) {
                            return Some(h);
                        }
                    }
                }
            }
            None
        }
        ExprKind::Field { base, .. } => walk_expr_member(base, span),
        ExprKind::Unary { expr: inner, .. } | ExprKind::Try(inner) => {
            walk_expr_member(inner, span)
        }
        ExprKind::Binary { left, right, .. } => {
            walk_expr_member(left, span).or_else(|| walk_expr_member(right, span))
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                match a {
                    crate::ast::Arg::Pos(e) | crate::ast::Arg::Named { value: e, .. } => {
                        if let Some(h) = walk_expr_member(e, span) {
                            return Some(h);
                        }
                    }
                }
            }
            None
        }
        ExprKind::Index { base, index } => {
            walk_expr_member(base, span).or_else(|| walk_expr_member(index, span))
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                if let Some(h) = walk_expr_member(e, span) {
                    return Some(h);
                }
            }
            None
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => walk_expr_member(cond, span)
            .or_else(|| walk_block_member(then_branch, span))
            .or_else(|| else_branch.as_ref().and_then(|b| walk_block_member(b, span))),
        ExprKind::While { cond, body } => {
            walk_expr_member(cond, span).or_else(|| walk_block_member(body, span))
        }
        ExprKind::Loop { body } => walk_block_member(body, span),
        ExprKind::For {
            start, end, body, ..
        } => walk_expr_member(start, span)
            .or_else(|| end.as_ref().and_then(|e| walk_expr_member(e, span)))
            .or_else(|| walk_block_member(body, span)),
        ExprKind::Return { value } => value.as_ref().and_then(|v| walk_expr_member(v, span)),
        ExprKind::Block(b) => walk_block_member(b, span),
        ExprKind::Match { scrutinee, arms } => {
            if let Some(h) = walk_expr_member(scrutinee, span) {
                return Some(h);
            }
            for arm in arms {
                if let Some(h) = walk_block_member(&arm.body, span) {
                    return Some(h);
                }
            }
            None
        }
        ExprKind::Interpolate(parts) => {
            for p in parts {
                if let crate::ast::InterpPart::Expr(e) = p {
                    if let Some(h) = walk_expr_member(e, span) {
                        return Some(h);
                    }
                }
            }
            None
        }
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Break
        | ExprKind::Continue => None,
    }
}

/// Lookup a module-level name in the resolution by walking value scopes is hard;
/// we only need root-scope names for namespace receivers — use module_definitions
/// + any import binding with that name.
trait ResolutionLookup {
    fn lookup_name_at_module(&self, name: &str) -> Option<u32>;
}

impl ResolutionLookup for Resolution {
    fn lookup_name_at_module(&self, name: &str) -> Option<u32> {
        // Prefer module-level definitions (imports, fns, types, …).
        for id in &self.module_binding_ids {
            if let Some(b) = self.bindings.get(id) {
                if b.name == name {
                    return Some(*id);
                }
            }
        }
        // Fall back: any binding of that name without owner (builtins have no def).
        self.bindings
            .values()
            .find(|b| b.name == name && b.owner.is_none())
            .map(|b| b.id)
    }
}

fn lookup_member(
    spec: &str,
    imported: &str,
    docs: &HashMap<String, String>,
    current_path: &Path,
) -> Option<Remote> {
    let path = resolve_spec(spec, current_path)?;
    let text = module_text(&path, docs)?;
    let res = resolve_text(&text)?;
    let b = exported_binding(&res, imported)?;
    let inferred = remote_inferred_ret(&path, &text, docs, b);
    let sig = index::render_signature_with_ret(b, &text, inferred.as_deref())?;
    let doc = b
        .def_span
        .map(|s| index::doc_above(&text, s.start))
        .unwrap_or_default();
    Some(Remote {
        signature: sig,
        doc,
        path,
        def_span: b.def_span,
        text,
    })
}

/// MIR return type for a remote binding (free fn or Type.method).
fn remote_inferred_ret(
    path: &Path,
    text: &str,
    docs: &HashMap<String, String>,
    b: &Binding,
) -> Option<String> {
    if !matches!(
        b.kind,
        BindingKind::Fn | BindingKind::Extern | BindingKind::Method
    ) {
        return None;
    }
    let path_str = path.to_string_lossy();
    let mut overlays = HashMap::new();
    for (uri, buf) in docs {
        let p = convert::uri_to_path(uri);
        if !p.as_os_str().is_empty() {
            overlays.insert(p, buf.clone());
        }
    }
    let rets = analysis::return_types_with_overlays(&path_str, text, &overlays);
    let key = match b.kind {
        BindingKind::Method => {
            if let Some(owner) = &b.owner {
                format!("{owner}.{}", b.name)
            } else {
                b.name.clone()
            }
        }
        _ => b.name.clone(),
    };
    rets.get(&key).cloned()
}

fn lookup_module(
    spec: &str,
    docs: &HashMap<String, String>,
    current_path: &Path,
) -> Option<Remote> {
    let path = resolve_spec(spec, current_path)?;
    let text = module_text(&path, docs)?;
    // Module summary: leading ## docs + synthetic signature.
    let doc = module_leading_doc(&text);
    let sig = format!("module {spec}");
    // Jump to start of file (first non-empty line or byte 0).
    let def_span = Some(Span::new(0, 0));
    Some(Remote {
        signature: sig,
        doc,
        path,
        def_span,
        text,
    })
}

fn module_leading_doc(text: &str) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("##") {
            lines.push(t.trim_start_matches('#').trim_start());
        } else if t.is_empty() {
            if !lines.is_empty() {
                break;
            }
        } else {
            break;
        }
    }
    lines.join("\n")
}

fn resolve_spec(spec: &str, current_path: &Path) -> Option<PathBuf> {
    resolve_import_file(spec, current_path)
}

/// Resolve an import specifier relative to the importing file (for nav / multi-file refs).
pub fn resolve_import_file(spec: &str, current_path: &Path) -> Option<PathBuf> {
    let importer_dir = current_path.parent().unwrap_or_else(|| Path::new("."));
    // `~/` root = entry file's directory (same as D31/D68 for single open file).
    let project_root = importer_dir;
    let path = loader::resolve_import_path(spec, importer_dir, project_root)?;
    // Soft-canonicalize when possible.
    fs::canonicalize(&path).ok().or(Some(path))
}

/// Best-effort path equality for open-buffer multi-file refs.
pub fn paths_equal_public(a: &Path, b: &Path) -> bool {
    paths_equal(a, b)
}

fn module_text(path: &Path, docs: &HashMap<String, String>) -> Option<String> {
    // Open buffers win: match by resolved path, not raw URI.
    for (uri, buffered) in docs {
        let p = convert::uri_to_path(uri);
        if paths_equal(&p, path) {
            return Some(buffered.clone());
        }
    }
    fs::read_to_string(path).ok()
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => {
            // Compare lossy string forms for non-existent paths.
            a.to_string_lossy() == b.to_string_lossy()
        }
    }
}

fn resolve_text(text: &str) -> Option<Resolution> {
    let tokens = Lexer::new(text).tokenize().ok()?;
    let module = Parser::new(tokens).parse_module().ok()?;
    Some(resolver::resolve_module(&module))
}

fn exported_binding<'a>(res: &'a Resolution, name: &str) -> Option<&'a Binding> {
    res.bindings.values().find(|b| {
        b.name == name
            && b.exported
            && b.owner.is_none()
            && b.def_span.is_some()
            && b.kind != BindingKind::Import
    })
}

/// Markdown hover for a remote definition.
pub fn remote_hover_markdown(remote: &Remote) -> String {
    let mut parts = vec![format!("```thoxa\n{}\n```", remote.signature)];
    if !remote.doc.is_empty() {
        parts.push(remote.doc.clone());
    }
    parts.join("\n\n")
}

// ── Call-argument hover (formal param type under the cursor) ────────────────

/// Hover info for a call argument at `line`/`character` (e.g. `16` in
/// `server.listen(16)` → `backlog: Int64`).
///
/// Returns markdown + the argument expression span for the hover range.
pub fn hover_call_argument(
    idx: &DocumentIndex,
    line: u32,
    character: u32,
    docs: &HashMap<String, String>,
) -> Option<(String, Span)> {
    let module = idx.module.as_ref()?;
    let byte = span::position_to_byte(&idx.text, line, character);
    let hit = find_call_arg_at(module, byte)?;
    let current = Path::new(&idx.path);
    let formals = resolve_call_formals(idx, module, &hit.call, docs, current)?;
    let formal = match &hit.slot {
        ArgSlot::Positional(i) => formals.get(*i)?,
        ArgSlot::Named(name) => formals.iter().find(|p| p.name == *name)?,
    };
    let ty = format_type_expr(&formal.ty);
    let md = format!("```thoxa\n{}: {}\n```", formal.name, ty);
    Some((md, hit.span))
}

#[derive(Debug, Clone)]
enum ArgSlot {
    Positional(usize),
    Named(String),
}

#[derive(Debug, Clone)]
enum CallShape {
    Free { callee: String },
    Method { receiver: Expr, method: String },
}

#[derive(Debug, Clone)]
struct CallArgHit {
    span: Span,
    slot: ArgSlot,
    call: CallShape,
}

#[derive(Debug, Clone)]
struct FormalParam {
    name: String,
    ty: TypeExpr,
}

fn find_call_arg_at(module: &Module, byte: usize) -> Option<CallArgHit> {
    let mut best: Option<CallArgHit> = None;
    for item in &module.items {
        walk_item_call_args(item, byte, &mut best);
    }
    best
}

fn consider_hit(best: &mut Option<CallArgHit>, hit: CallArgHit) {
    let len = hit.span.end.saturating_sub(hit.span.start);
    match best {
        None => *best = Some(hit),
        Some(prev) => {
            let prev_len = prev.span.end.saturating_sub(prev.span.start);
            // Innermost (smallest span) wins for nested calls.
            if len < prev_len {
                *best = Some(hit);
            }
        }
    }
}

/// Half-open byte range: `start <= byte < end`.
fn span_contains_byte(span: Span, byte: usize) -> bool {
    span.end > span.start && byte >= span.start && byte < span.end
}

fn walk_item_call_args(item: &Item, byte: usize, best: &mut Option<CallArgHit>) {
    match item {
        Item::Function(f) => walk_block_call_args(&f.body, byte, best),
        Item::Type(td) => {
            for m in &td.methods {
                walk_block_call_args(&m.body, byte, best);
            }
        }
        Item::Global(g) => walk_expr_call_args(&g.init, byte, best),
        Item::SessionStmt(bi) => walk_block_item_call_args(bi, byte, best),
        Item::Extern(_) | Item::Import(_) => {}
    }
}

fn walk_block_call_args(block: &ast::Block, byte: usize, best: &mut Option<CallArgHit>) {
    for bi in &block.items {
        walk_block_item_call_args(bi, byte, best);
    }
    if let Some(t) = &block.tail {
        walk_expr_call_args(t, byte, best);
    }
}

fn walk_block_item_call_args(item: &ast::BlockItem, byte: usize, best: &mut Option<CallArgHit>) {
    match item {
        ast::BlockItem::Let { init, .. } => walk_expr_call_args(init, byte, best),
        ast::BlockItem::Assign { value, .. } => walk_expr_call_args(value, byte, best),
        ast::BlockItem::Expr(e) => walk_expr_call_args(e, byte, best),
    }
}

fn walk_expr_call_args(expr: &Expr, byte: usize, best: &mut Option<CallArgHit>) {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            collect_args(
                args,
                CallShape::Free {
                    callee: callee.clone(),
                },
                byte,
                best,
            );
            for a in args {
                walk_arg_expr(a, byte, best);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            collect_args(
                args,
                CallShape::Method {
                    receiver: *receiver.clone(),
                    method: method.clone(),
                },
                byte,
                best,
            );
            walk_expr_call_args(receiver, byte, best);
            for a in args {
                walk_arg_expr(a, byte, best);
            }
        }
        ExprKind::Unary { expr: inner, .. }
        | ExprKind::Try(inner)
        | ExprKind::Field { base: inner, .. } => walk_expr_call_args(inner, byte, best),
        ExprKind::Binary { left, right, .. } => {
            walk_expr_call_args(left, byte, best);
            walk_expr_call_args(right, byte, best);
        }
        ExprKind::Index { base, index } => {
            walk_expr_call_args(base, byte, best);
            walk_expr_call_args(index, byte, best);
        }
        ExprKind::ArrayLit(elems) => {
            for e in elems {
                walk_expr_call_args(e, byte, best);
            }
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr_call_args(cond, byte, best);
            walk_block_call_args(then_branch, byte, best);
            if let Some(e) = else_branch {
                walk_block_call_args(e, byte, best);
            }
        }
        ExprKind::While { cond, body } => {
            walk_expr_call_args(cond, byte, best);
            walk_block_call_args(body, byte, best);
        }
        ExprKind::Loop { body } => walk_block_call_args(body, byte, best),
        ExprKind::For {
            start, end, body, ..
        } => {
            walk_expr_call_args(start, byte, best);
            if let Some(e) = end {
                walk_expr_call_args(e, byte, best);
            }
            walk_block_call_args(body, byte, best);
        }
        ExprKind::Return { value } => {
            if let Some(v) = value {
                walk_expr_call_args(v, byte, best);
            }
        }
        ExprKind::Block(b) => walk_block_call_args(b, byte, best),
        ExprKind::Match { scrutinee, arms } => {
            walk_expr_call_args(scrutinee, byte, best);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    walk_expr_call_args(g, byte, best);
                }
                walk_block_call_args(&arm.body, byte, best);
            }
        }
        ExprKind::Interpolate(parts) => {
            for p in parts {
                if let ast::InterpPart::Expr(e) = p {
                    walk_expr_call_args(e, byte, best);
                }
            }
        }
        ExprKind::Name(_)
        | ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Break
        | ExprKind::Continue => {}
    }
}

fn walk_arg_expr(arg: &ast::Arg, byte: usize, best: &mut Option<CallArgHit>) {
    match arg {
        ast::Arg::Pos(e) | ast::Arg::Named { value: e, .. } => {
            walk_expr_call_args(e, byte, best);
        }
    }
}

fn collect_args(
    args: &[ast::Arg],
    call: CallShape,
    byte: usize,
    best: &mut Option<CallArgHit>,
) {
    let mut pos_i = 0usize;
    for arg in args {
        match arg {
            ast::Arg::Pos(e) => {
                if span_contains_byte(e.span, byte) {
                    consider_hit(
                        best,
                        CallArgHit {
                            span: e.span,
                            slot: ArgSlot::Positional(pos_i),
                            call: call.clone(),
                        },
                    );
                }
                pos_i += 1;
            }
            ast::Arg::Named { name, value, .. } => {
                if span_contains_byte(value.span, byte) {
                    consider_hit(
                        best,
                        CallArgHit {
                            span: value.span,
                            slot: ArgSlot::Named(name.clone()),
                            call: call.clone(),
                        },
                    );
                }
                // Named args still consume a "slot" for positional mapping of
                // later pos args only by name; pos_i tracks positional only.
            }
        }
    }
}

fn resolve_call_formals(
    idx: &DocumentIndex,
    module: &Module,
    call: &CallShape,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<Vec<FormalParam>> {
    match call {
        CallShape::Free { callee } => resolve_free_formals(idx, module, callee, docs, current),
        CallShape::Method { receiver, method } => {
            resolve_method_formals(idx, module, receiver, method, docs, current)
        }
    }
}

fn resolve_free_formals(
    idx: &DocumentIndex,
    module: &Module,
    callee: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<Vec<FormalParam>> {
    let id = idx.resolution.lookup_name_at_module(callee)?;
    let b = idx.resolution.bindings.get(&id)?;
    match b.kind {
        BindingKind::Fn | BindingKind::Extern => {
            local_free_fn_params(module, callee)
        }
        BindingKind::Type => local_type_ctor_params(module, callee),
        BindingKind::Import => {
            let o = b.import_origin.as_ref()?;
            if o.is_namespace {
                return None;
            }
            let name = o.original.as_deref().unwrap_or(b.name.as_str());
            remote_export_params(&o.spec, name, docs, current)
        }
        _ => None,
    }
}

fn resolve_method_formals(
    idx: &DocumentIndex,
    module: &Module,
    receiver: &Expr,
    method: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<Vec<FormalParam>> {
    // Namespace free-fn sugar: `net.listen` is not a method — but
    // `net.createSocket` is free. MethodCall form is used for both.
    if let ExprKind::Name(recv) = &receiver.kind {
        if let Some(id) = idx.resolution.lookup_name_at_module(recv) {
            if let Some(b) = idx.resolution.bindings.get(&id) {
                if b.kind == BindingKind::Import {
                    if let Some(o) = &b.import_origin {
                        if o.is_namespace {
                            // Prefer free fn on the module; else type method N/A.
                            if let Some(p) =
                                remote_export_params(&o.spec, method, docs, current)
                            {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
    }
    let recv_ty = infer_expr_nav_ty(idx, module, receiver, docs, current)?;
    // Local type methods.
    if let Some(params) = local_type_method_params(module, &recv_ty.head, method) {
        return Some(params);
    }
    let spec = type_defining_spec(idx, &recv_ty, docs, current)?;
    remote_type_method_params(&spec, &recv_ty.head, method, docs, current)
}

fn formal_from_param(p: &ast::Param) -> FormalParam {
    FormalParam {
        name: p.name.clone(),
        ty: p.ty.clone(),
    }
}

fn local_free_fn_params(module: &Module, name: &str) -> Option<Vec<FormalParam>> {
    for item in &module.items {
        match item {
            Item::Function(f) if f.name == name => {
                return Some(f.params.iter().map(formal_from_param).collect());
            }
            Item::Extern(e) if e.name == name => {
                return Some(e.params.iter().map(formal_from_param).collect());
            }
            _ => {}
        }
    }
    None
}

fn local_type_ctor_params(module: &Module, name: &str) -> Option<Vec<FormalParam>> {
    for item in &module.items {
        let Item::Type(td) = item else { continue };
        if td.name != name {
            continue;
        }
        return Some(td.params.iter().map(formal_from_param).collect());
    }
    None
}

fn local_type_method_params(
    module: &Module,
    type_name: &str,
    method: &str,
) -> Option<Vec<FormalParam>> {
    for item in &module.items {
        let Item::Type(td) = item else { continue };
        if td.name != type_name {
            continue;
        }
        for m in &td.methods {
            if m.name == method {
                return Some(m.params.iter().map(formal_from_param).collect());
            }
        }
    }
    None
}

fn remote_export_params(
    spec: &str,
    name: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<Vec<FormalParam>> {
    let path = resolve_spec(spec, current)?;
    let text = module_text(&path, docs)?;
    let tokens = Lexer::new(&text).tokenize().ok()?;
    let remote_mod = Parser::new(tokens).parse_module().ok()?;
    for item in &remote_mod.items {
        match item {
            Item::Function(f) if f.exported && f.name == name => {
                return Some(f.params.iter().map(formal_from_param).collect());
            }
            Item::Extern(e) if e.exported && e.name == name => {
                return Some(e.params.iter().map(formal_from_param).collect());
            }
            Item::Type(td) if td.exported && td.name == name => {
                return Some(td.params.iter().map(formal_from_param).collect());
            }
            _ => {}
        }
    }
    None
}

fn remote_type_method_params(
    spec: &str,
    type_name: &str,
    method: &str,
    docs: &HashMap<String, String>,
    current: &Path,
) -> Option<Vec<FormalParam>> {
    let path = resolve_spec(spec, current)?;
    let text = module_text(&path, docs)?;
    let tokens = Lexer::new(&text).tokenize().ok()?;
    let remote_mod = Parser::new(tokens).parse_module().ok()?;
    for item in &remote_mod.items {
        let Item::Type(td) = item else { continue };
        if td.name != type_name {
            continue;
        }
        for m in &td.methods {
            if m.name == method {
                return Some(m.params.iter().map(formal_from_param).collect());
            }
        }
    }
    None
}

fn format_type_expr(te: &TypeExpr) -> String {
    if let Some(lit) = &te.lit {
        let s = match lit {
            ast::TypeLit::Int(n) => n.to_string(),
            ast::TypeLit::Bool(b) => b.to_string(),
            ast::TypeLit::String(s) => format!("\"{s}\""),
        };
        return with_type_mark(s, te.mark);
    }
    let mut s = te.name.clone();
    if !te.args.is_empty() {
        s.push('<');
        for (i, a) in te.args.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            match a {
                TypeArg::Type(inner) => s.push_str(&format_type_expr(inner)),
                TypeArg::Int { value, .. } => s.push_str(&value.to_string()),
            }
        }
        s.push('>');
    }
    with_type_mark(s, te.mark)
}

fn with_type_mark(s: String, mark: Option<ast::TypeMark>) -> String {
    match mark {
        Some(ast::TypeMark::Extract) => format!("{s}!"),
        Some(ast::TypeMark::Residual) => format!("{s}?"),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::index::build_index;
    use crate::span::byte_to_position;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(label: &str) -> PathBuf {
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_xmod_{label}_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn remote_at(
        src: &str,
        path: &Path,
        line: u32,
        character: u32,
        docs: &HashMap<String, String>,
    ) -> Option<Remote> {
        let idx = build_index(src, &path.to_string_lossy());
        let ref_ = idx.resolution.ref_at(&idx.text, line, character);
        let binding = idx
            .resolution
            .binding_of(ref_)
            .or_else(|| idx.resolution.binding_at(&idx.text, line, character));
        resolve_remote(&idx, ref_, binding, docs)
    }

    #[test]
    fn selective_import_remote() {
        let dir = unique_dir("sel");
        let helper = dir.join("Helper.thx");
        fs::write(
            &helper,
            "## returns forty-two\nexport fn answer() -> Int64 { 42 }\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let src = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, src).unwrap();

        let use_at = src.rfind("answer").unwrap();
        let p = byte_to_position(src, use_at);
        let r = remote_at(src, &main, p.line, p.character, &HashMap::new()).expect("remote");
        assert!(r.signature.contains("answer"), "{}", r.signature);
        assert!(r.doc.contains("forty-two"), "{}", r.doc);
        assert!(r.path.ends_with("Helper.thx"), "{:?}", r.path);
        assert!(r.def_span.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn aliased_import_shows_original() {
        let dir = unique_dir("alias");
        let helper = dir.join("Helper.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let main = dir.join("Main.thx");
        let src =
            "import { answer as ans } from \"./Helper\"\nfn main() -> Int64 { ans() }\n";
        fs::write(&main, src).unwrap();
        let use_at = src.rfind("ans").unwrap();
        let p = byte_to_position(src, use_at);
        let r = remote_at(src, &main, p.line, p.character, &HashMap::new()).expect("remote");
        assert!(r.signature.contains("answer"), "{}", r.signature);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_name_not_remote() {
        let dir = unique_dir("local");
        let main = dir.join("Main.thx");
        let src = "fn f() -> Int64 { 0 }\nfn main() -> Int64 { f() }\n";
        fs::write(&main, src).unwrap();
        let use_at = src.rfind("f").unwrap();
        let p = byte_to_position(src, use_at);
        assert!(remote_at(src, &main, p.line, p.character, &HashMap::new()).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn namespace_member_and_name() {
        let dir = unique_dir("ns");
        let helper = dir.join("Helper.thx");
        fs::write(
            &helper,
            "## tiny helper\nexport fn answer() -> Int64 { 42 }\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let src = "import h from \"./Helper\"\nfn main() -> Int64 { h.answer() }\n";
        fs::write(&main, src).unwrap();

        // member `answer` in h.answer()
        let ans = src.rfind("answer").unwrap();
        let p = byte_to_position(src, ans);
        let r = remote_at(src, &main, p.line, p.character, &HashMap::new()).expect("member");
        assert!(r.signature.contains("answer"), "{}", r.signature);

        // namespace name `h`
        let h_use = src.find("h.answer").unwrap();
        let p = byte_to_position(src, h_use);
        let r = remote_at(src, &main, p.line, p.character, &HashMap::new()).expect("ns");
        assert!(r.signature.contains("module"), "{}", r.signature);
        assert!(r.doc.contains("tiny helper"), "{}", r.doc);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hover_arg_shows_formal_param_type() {
        // `server.listen(16)` — hover on `16` → backlog: Int64
        let dir = unique_dir("arg_hover");
        let net = dir.join("Net.thx");
        fs::write(
            &net,
            r#"import { Pass, Fail, Trial } from "std/Fallible"
export type Socket(fd: Int64) {
  export fn listen(backlog: Int64) -> Trial<Void, String> { Pass(Void) }
}
export fn createSocket() -> Trial<Socket, String> { Pass(Socket(fd: 0)) }
"#,
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let src = r#"import net from "./Net"
import { Pass, Fail, Trial } from "std/Fallible"
fn main() -> Trial<Int64, String> {
  let server = net.createSocket()?
  server.listen(16)?
  Pass(0)
}
"#;
        fs::write(&main, src).unwrap();
        let idx = build_index(src, &main.to_string_lossy());
        let sixteen = src.find("16").unwrap();
        let p = byte_to_position(src, sixteen);
        let (md, span) = hover_call_argument(&idx, p.line, p.character, &HashMap::new())
            .expect("arg hover");
        assert!(md.contains("backlog"), "{md}");
        assert!(md.contains("Int64"), "{md}");
        assert_eq!(span.start, sixteen);
        assert_eq!(span.end, sixteen + 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hover_arg_local_free_fn() {
        let src = "\
fn listen(backlog: Int64) -> Int64 { backlog }
fn main() -> Int64 { listen(16) }
";
        let idx = build_index(src, "Main.thx");
        let sixteen = src.find("16").unwrap();
        let p = byte_to_position(src, sixteen);
        let (md, _) = hover_call_argument(&idx, p.line, p.character, &HashMap::new())
            .expect("local arg hover");
        assert!(md.contains("backlog: Int64"), "{md}");
    }

    #[test]
    fn typed_member_after_namespace_try() {
        // `let s = net.createSocket()?; s.reuseAddr()` — method is on Socket in Net.
        let dir = unique_dir("try_ns");
        let net = dir.join("Net.thx");
        fs::write(
            &net,
            r#"import { Pass, Fail, Trial } from "std/Fallible"
export type Socket(fd: Int64) {
  ## enable SO_REUSEADDR
  export fn reuseAddr() -> Trial<Void, String> { Pass(Void) }
}
export fn createSocket() -> Trial<Socket, String> { Pass(Socket(fd: 0)) }
"#,
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let src = r#"import net from "./Net"
import { Pass, Fail, Trial } from "std/Fallible"
fn main() -> Trial<Int64, String> {
  let server = net.createSocket()?
  server.reuseAddr()?
  Pass(0)
}
"#;
        fs::write(&main, src).unwrap();

        let reuse = src.find("reuseAddr").unwrap();
        let p = byte_to_position(src, reuse);
        let r = remote_at(src, &main, p.line, p.character, &HashMap::new())
            .expect("typed remote method");
        assert!(
            r.signature.contains("reuseAddr"),
            "sig={}",
            r.signature
        );
        assert!(
            r.doc.contains("SO_REUSEADDR") || r.doc.contains("reuse"),
            "doc={}",
            r.doc
        );
        assert!(r.path.ends_with("Net.thx"), "{:?}", r.path);
        assert!(r.def_span.is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_buffer_wins_over_disk() {
        let dir = unique_dir("buf");
        let helper = dir.join("Helper.thx");
        fs::write(
            &helper,
            "export fn helper(x: Int64) -> Int64 { x }\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let src = "import { helper } from \"./Helper\"\nfn main() -> Int64 { helper(1) }\n";
        fs::write(&main, src).unwrap();

        let edited = "export fn helper(x: Int64, y: Int64) -> Int64 { x }\n";
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
        let mut docs = HashMap::new();
        docs.insert(helper_uri, edited.to_string());

        let use_at = src.rfind("helper").unwrap();
        let p = byte_to_position(src, use_at);
        let r = remote_at(src, &main, p.line, p.character, &docs).expect("remote");
        assert!(
            r.signature.contains("y"),
            "expected unsaved buffer signature, got {}",
            r.signature
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_module_degrades() {
        let dir = unique_dir("miss");
        let main = dir.join("Main.thx");
        let src = "import { nope } from \"./Missing\"\nfn main() -> Int64 { 0 }\n";
        fs::write(&main, src).unwrap();
        let use_at = src.find("nope").unwrap();
        let p = byte_to_position(src, use_at);
        assert!(remote_at(src, &main, p.line, p.character, &HashMap::new()).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
