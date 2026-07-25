//! Lazy cross-module resolution for hover / go-to-definition.
//!
//! When the cursor lands on an imported name (or a namespace-qualified member),
//! load the defining module (open buffer preferred over disk), find the export,
//! and return signature + doc + jump location. Any failure returns `None` so
//! handlers fall back to single-file behaviour.

use crate::ast::{Expr, ExprKind, Item, Module};
use crate::lexer::Lexer;
use crate::loader;
use crate::lsp::convert;
use crate::lsp::index::{self, DocumentIndex};
use crate::parser::Parser;
use crate::resolver::{self, Binding, BindingKind, RefCtx, Reference, Resolution};
use crate::span::Span;
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

    match origin(idx, ref_, binding)? {
        Origin::Member { spec, name } => lookup_member(&spec, &name, docs, current),
        Origin::Module { spec } => lookup_module(&spec, docs, current),
    }
}

enum Origin {
    /// Selective import use / namespace member / aliased import.
    Member { spec: String, name: String },
    /// Namespace binding itself (`import mu from "…"`).
    Module { spec: String },
}

fn origin(
    idx: &DocumentIndex,
    ref_: Option<&Reference>,
    binding: Option<&Binding>,
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
    if binding.is_none() {
        if let Some(r) = ref_ {
            if r.ctx == RefCtx::Member {
                if let Some(module) = idx.module.as_ref() {
                    if let Some((spec, name)) = namespace_member_origin(idx, module, r) {
                        return Some(Origin::Member { spec, name });
                    }
                }
            }
        }
    }

    None
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
    let sig = index::render_signature(b, &text)?;
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
    let importer_dir = current_path.parent().unwrap_or_else(|| Path::new("."));
    // `~/` root = entry file's directory (same as D31/D68 for single open file).
    let project_root = importer_dir;
    let path = loader::resolve_import_path(spec, importer_dir, project_root)?;
    // Soft-canonicalize when possible.
    fs::canonicalize(&path).ok().or(Some(path))
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
