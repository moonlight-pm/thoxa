//! Document index: parse + lexical resolve for navigation queries.
//!
//! On unparseable buffers returns an empty index (navigation degrades gracefully).

use crate::ast::Module;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::resolver::{self, Binding, BindingKind, Resolution};
use crate::span::Span;

/// Position-queryable analysis of one open buffer.
#[derive(Debug, Clone, Default)]
pub struct DocumentIndex {
    pub text: String,
    pub path: String,
    pub resolution: Resolution,
    /// Parsed AST when parse succeeded (needed for namespace-member walks).
    pub module: Option<Module>,
    /// False when lex/parse failed (resolution is empty).
    pub ok: bool,
}

/// Build an index for `text`. `path` is a filesystem hint for cross-module resolution.
pub fn build_index(text: &str, path: &str) -> DocumentIndex {
    match parse_and_resolve(text) {
        Ok((module, resolution)) => DocumentIndex {
            text: text.to_string(),
            path: path.to_string(),
            resolution,
            module: Some(module),
            ok: true,
        },
        Err(_) => DocumentIndex {
            text: text.to_string(),
            path: path.to_string(),
            resolution: Resolution::default(),
            module: None,
            ok: false,
        },
    }
}

fn parse_and_resolve(text: &str) -> crate::error::Result<(Module, Resolution)> {
    let tokens = Lexer::new(text).tokenize()?;
    let module = Parser::new(tokens)
        .with_source(text.to_string())
        .parse_module()?;
    let resolution = resolver::resolve_module(&module);
    Ok((module, resolution))
}

impl DocumentIndex {
    pub fn binding_at(&self, line: u32, character: u32) -> Option<&Binding> {
        let ref_ = self.resolution.ref_at(&self.text, line, character);
        if let Some(b) = self.resolution.binding_of(ref_) {
            return Some(b);
        }
        self.resolution.binding_at(&self.text, line, character)
    }

    /// Contiguous `##` doc-comment lines immediately above `def_span`.
    pub fn doc_above(&self, def_span: Span) -> String {
        doc_above(&self.text, def_span.start)
    }

    /// Hover markdown for a binding (signature fence + optional docs).
    ///
    /// `inferred_ret` is the MIR-resolved return type display (D129) when known.
    pub fn hover_markdown(&self, binding: &Binding) -> Option<String> {
        self.hover_markdown_with_ret(binding, None)
    }

    /// Like [`hover_markdown`], with an optional inferred/annotated return type
    /// from typecheck (`Trial<Void, String>`, `Int64`, …).
    pub fn hover_markdown_with_ret(
        &self,
        binding: &Binding,
        inferred_ret: Option<&str>,
    ) -> Option<String> {
        if binding.kind == BindingKind::Builtin {
            return Some(format!(
                "```thoxa\n{}\n```\n\n{}",
                binding.name,
                builtin_help(&binding.name)
            ));
        }
        let sig = render_signature_with_ret(binding, &self.text, inferred_ret)?;
        let mut parts = vec![format!("```thoxa\n{sig}\n```")];
        if let Some(span) = binding.def_span {
            let doc = self.doc_above(span);
            if !doc.is_empty() {
                parts.push(doc);
            }
        }
        Some(parts.join("\n\n"))
    }

    /// True if this binding may be renamed in this file.
    pub fn renameable(&self, binding: &Binding) -> bool {
        match binding.kind {
            BindingKind::Builtin => false,
            // Imports: rename the local binding name only (single-file).
            BindingKind::Fn
            | BindingKind::Extern
            | BindingKind::Type
            | BindingKind::Field
            | BindingKind::Method
            | BindingKind::Param
            | BindingKind::Let
            | BindingKind::Mut
            | BindingKind::ForVar
            | BindingKind::Pattern
            | BindingKind::Import => binding.def_span.is_some(),
        }
    }

    /// Validate a new name against Thoxa casing for this binding kind.
    pub fn valid_new_name(&self, binding: &Binding, new_name: &str) -> Result<(), String> {
        if new_name.is_empty() {
            return Err("name must not be empty".into());
        }
        if !new_name.chars().next().unwrap().is_ascii_alphabetic() && new_name != "_" {
            // allow `_` only as full name? Thoxa ident starts with letter
            return Err("name must start with a letter".into());
        }
        if !new_name.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("name must be alphanumeric ASCII".into());
        }
        let first = new_name.chars().next().unwrap();
        match binding.kind {
            BindingKind::Type => {
                if !first.is_ascii_uppercase() {
                    return Err("type names must be PascalCase".into());
                }
            }
            BindingKind::Fn
            | BindingKind::Extern
            | BindingKind::Method
            | BindingKind::Field
            | BindingKind::Param
            | BindingKind::Let
            | BindingKind::Mut
            | BindingKind::ForVar
            | BindingKind::Pattern
            | BindingKind::Import => {
                if !first.is_ascii_lowercase() {
                    return Err("value names must be camelCase".into());
                }
            }
            BindingKind::Builtin => return Err("cannot rename a built-in".into()),
        }
        Ok(())
    }
}

fn builtin_help(name: &str) -> &'static str {
    match name {
        "Node" => {
            "Built-in HTML tree (JSX). `Element | Text | Fragment`; render with `.html() -> String`."
        }
        "Element" => "HTML element node: `Element(tag, attrs, kids)`. A `Node`.",
        "Text" => "HTML text node: `Text(s)`. A `Node`.",
        "Fragment" => "HTML fragment: `Fragment(kids)`. A `Node`.",
        "Attr" => "HTML attribute: `Attr(name, value)`.",
        "String" => "Built-in UTF-8 string.",
        "Bytes" => "Built-in byte buffer.",
        "Buffer" => "Built-in growable buffer.",
        "Array" => "Built-in dynamic array.",
        "Map" => "Built-in map (Swift-style COW).",
        _ => "Built-in.",
    }
}

/// Signature line for hover: prefer the source line at the definition.
///
/// When the source omits `-> …`, prefer `inferred_ret` from typecheck (D129).
/// Fallback: **`main` → Int64**, other fn/method/extern → **Void**.
pub fn render_signature(binding: &Binding, text: &str) -> Option<String> {
    render_signature_with_ret(binding, text, None)
}

/// Like [`render_signature`], with optional MIR-resolved return type display.
pub fn render_signature_with_ret(
    binding: &Binding,
    text: &str,
    inferred_ret: Option<&str>,
) -> Option<String> {
    if let Some(span) = binding.def_span {
        let line = line_containing(text, span.start);
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            // Desugared route clauses point def spans at `GET "/…"` lines — not
            // `fn …` syntax. Prefer a synthetic signature for those.
            if let Some(syn) = synthetic_route_signature(binding, trimmed, inferred_ret) {
                return Some(syn);
            }
            // Locals: prefer `let a: String` (nav ty_hint) over a long init line.
            if matches!(
                binding.kind,
                BindingKind::Let | BindingKind::Mut | BindingKind::Param | BindingKind::ForVar
            ) {
                if let Some(annotated) = local_hover_line(binding, trimmed) {
                    return Some(annotated);
                }
            }
            return Some(enrich_fn_return_display(binding, trimmed, inferred_ret));
        }
    }
    // Fallback by kind.
    let s = match binding.kind {
        BindingKind::Fn | BindingKind::Extern => {
            let ret = return_display(binding, inferred_ret);
            format!("fn {}() -> {ret}", binding.name)
        }
        BindingKind::Type => format!("type {}", binding.name),
        BindingKind::Method => {
            let ret = return_display(binding, inferred_ret);
            if let Some(owner) = &binding.owner {
                format!("fn {owner}.{}() -> {ret}", binding.name)
            } else {
                format!("fn {}() -> {ret}", binding.name)
            }
        }
        BindingKind::Field => {
            if let Some(owner) = &binding.owner {
                format!("{owner}.{}", binding.name)
            } else {
                binding.name.clone()
            }
        }
        BindingKind::Param => format!("param {}", typed_local_name(binding)),
        BindingKind::Let => format!("let {}", typed_local_name(binding)),
        BindingKind::Mut => format!("mut {}", typed_local_name(binding)),
        BindingKind::ForVar => format!("for {}", typed_local_name(binding)),
        BindingKind::Pattern => format!("pattern {}", typed_local_name(binding)),
        BindingKind::Import => format!("import {}", binding.name),
        BindingKind::Builtin => binding.name.clone(),
    };
    Some(s)
}

/// `a` or `a: String` when nav has a type head (from annotation or init inference).
fn typed_local_name(binding: &Binding) -> String {
    match binding.ty_hint.as_deref() {
        Some(ty) if !ty.is_empty() => {
            if binding.ty_args.is_empty() {
                format!("{}: {ty}", binding.name)
            } else {
                format!("{}: {ty}<{}>", binding.name, binding.ty_args.join(", "))
            }
        }
        _ => binding.name.clone(),
    }
}

/// Hover for `let`/`mut`/`param` when we know a type head from resolve nav.
///
/// Examples:
/// - `let a = r.route(…)` with ty_hint Response → `let a: Response`
/// - `let a: Int64 = 1` (already annotated in source) → keep source if it has `: Type`
fn local_hover_line(binding: &Binding, source_line: &str) -> Option<String> {
    let ty = binding.ty_hint.as_deref().filter(|t| !t.is_empty())?;
    // Source already has an explicit annotation after the name — keep it.
    let name = &binding.name;
    if let Some(after_name) = source_line
        .find(name)
        .map(|i| &source_line[i + name.len()..])
    {
        let rest = after_name.trim_start();
        if rest.starts_with(':') {
            return None; // use enrich path / raw line
        }
    }
    let ty_disp = if binding.ty_args.is_empty() {
        ty.to_string()
    } else {
        format!("{ty}<{}>", binding.ty_args.join(", "))
    };
    let kw = match binding.kind {
        BindingKind::Mut => "mut",
        BindingKind::Param => "param",
        BindingKind::ForVar => "for",
        _ => "let",
    };
    Some(format!("{kw} {name}: {ty_disp}"))
}

/// Hover text when a binding's def span sits on a route clause (`GET "/…"`), not `fn`.
fn synthetic_route_signature(
    binding: &Binding,
    source_line: &str,
    inferred_ret: Option<&str>,
) -> Option<String> {
    let first = source_line.split_whitespace().next()?;
    let is_http = matches!(
        first,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    );
    if !is_http {
        return None;
    }
    let default_ret = if binding.kind == BindingKind::Method && binding.name == "route" {
        route_return_display(binding)
    } else {
        "String".into()
    };
    let ret = return_display(binding, inferred_ret.or(Some(default_ret.as_str())));
    match binding.kind {
        BindingKind::Method if binding.name == "route" => {
            let owner = binding.owner.as_deref().unwrap_or("?");
            Some(format!(
                "fn {owner}.route(req: Request) -> {ret}\n\
                 // generated from route clauses"
            ))
        }
        BindingKind::Fn if binding.name.contains("_route_") => Some(format!(
            "fn {}(…) -> {ret}\n// desugared route handler ({first} …)",
            binding.name
        )),
        BindingKind::Method => Some(format!(
            "fn {}(…) -> {ret}\n// desugared route handler ({first} …)",
            binding.name
        )),
        _ => None,
    }
}

/// Generated `route` returns `Response` (nav ty_hint when resolve saw the
/// annotated return; otherwise the experiment default).
fn route_return_display(binding: &Binding) -> String {
    match binding.ty_hint.as_deref() {
        Some(head) if !head.is_empty() => {
            if binding.ty_args.is_empty() {
                head.to_string()
            } else {
                format!("{head}<{}>", binding.ty_args.join(", "))
            }
        }
        _ => "Response".into(),
    }
}

/// Return type for hover: inferred (D129) > main/Int64 > Void.
fn return_display(binding: &Binding, inferred_ret: Option<&str>) -> String {
    if let Some(r) = inferred_ret {
        if !r.is_empty() {
            return r.to_string();
        }
    }
    if binding.kind == BindingKind::Fn && binding.name == "main" {
        "Int64".into()
    } else {
        "Void".into()
    }
}

/// If this is a fn/method/extern source line without `->`, append the return type
/// (before a trailing `{` when present).
fn enrich_fn_return_display(
    binding: &Binding,
    source_line: &str,
    inferred_ret: Option<&str>,
) -> String {
    match binding.kind {
        BindingKind::Fn | BindingKind::Extern | BindingKind::Method => {
            if source_line.contains("->") {
                return source_line.to_string();
            }
            // Only enrich true signature lines (`fn …` / `export fn …`).
            let looks_fn = source_line.starts_with("fn ")
                || source_line.starts_with("export fn ")
                || source_line.contains(" fn ");
            if !looks_fn {
                return source_line.to_string();
            }
            let ret = return_display(binding, inferred_ret);
            if let Some(brace) = source_line.find('{') {
                let head = source_line[..brace].trim_end();
                let tail = &source_line[brace..];
                format!("{head} -> {ret} {tail}")
            } else {
                format!("{source_line} -> {ret}")
            }
        }
        _ => source_line.to_string(),
    }
}

fn line_containing(text: &str, byte: usize) -> &str {
    let byte = byte.min(text.len());
    let start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[byte..]
        .find('\n')
        .map(|i| byte + i)
        .unwrap_or(text.len());
    &text[start..end]
}

/// Contiguous `##` lines immediately above the line of `byte`.
pub fn doc_above(text: &str, byte: usize) -> String {
    let byte = byte.min(text.len());
    let line_start = text[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    // Walk previous lines.
    let mut cursor = line_start;
    let mut doc_lines: Vec<&str> = Vec::new();
    while cursor > 0 {
        let prev_end = cursor - 1; // points at '\n' or we adjust
        let prev_start = text[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = text[prev_start..prev_end].trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with("##") {
            // Strip ## and optional space
            let body = trimmed.trim_start_matches('#').trim_start();
            doc_lines.push(body);
            cursor = prev_start;
        } else if trimmed.is_empty() {
            // blank line stops the doc block (oracle: contiguous ## only)
            break;
        } else {
            break;
        }
    }
    doc_lines.reverse();
    doc_lines.join("\n")
}

/// Spans to rewrite for rename: def + all refs with matching text.
pub fn rename_spans(idx: &DocumentIndex, binding: &Binding) -> Vec<Span> {
    let mut spans = Vec::new();
    if let Some(def) = binding.def_span {
        spans.push(def);
    }
    for r in idx.resolution.refs_to(binding.id) {
        if let Some(slice) = idx.text.get(r.span.start..r.span.end) {
            if slice == binding.name {
                spans.push(r.span);
            }
        }
    }
    // Dedupe
    spans.sort_by_key(|s| (s.start, s.end));
    spans.dedup();
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::BindingKind;
    use crate::span::byte_to_position;

    #[test]
    fn index_good_file() {
        let src = "fn main() -> Int64 { 0 }\n";
        let idx = build_index(src, "<input>");
        assert!(idx.ok);
        assert!(!idx.resolution.module_definitions().is_empty());
        let b = idx.binding_at(0, 3).expect("main");
        assert_eq!(b.name, "main");
    }

    #[test]
    fn index_bad_parse_empty() {
        let idx = build_index("fn main() {", "<input>");
        assert!(!idx.ok);
        assert!(idx.resolution.module_definitions().is_empty());
    }

    #[test]
    fn doc_above_collects_hashhash() {
        let src = "## greets\n## the world\nfn main() -> Int64 { 0 }\n";
        let idx = build_index(src, "<input>");
        let b = idx.resolution.module_definitions()[0];
        let doc = idx.doc_above(b.def_span.unwrap());
        assert!(doc.contains("greets"), "{doc}");
        assert!(doc.contains("the world"), "{doc}");
    }

    #[test]
    fn hover_builtin_node() {
        let src = "fn view() -> Node { <p>Hi</p> }\n";
        let idx = build_index(src, "<input>");
        let at = src.find("Node").unwrap();
        let p = byte_to_position(src, at);
        let b = idx.binding_at(p.line, p.character).expect("Node");
        assert_eq!(b.kind, BindingKind::Builtin);
        let md = idx.hover_markdown(b).expect("hover");
        assert!(md.contains("Node"), "{md}");
        assert!(md.contains("HTML"), "{md}");
        assert!(md.contains(".html()"), "{md}");
    }

    #[test]
    fn hover_includes_signature_line() {
        let src = "fn helper() -> Int64 { 1 }\nfn main() -> Int64 { helper() }\n";
        let idx = build_index(src, "<input>");
        let call = src.rfind("helper").unwrap();
        let p = byte_to_position(src, call);
        let b = idx.binding_at(p.line, p.character).expect("helper");
        let md = idx.hover_markdown(b).expect("hover");
        assert!(md.contains("helper"), "{md}");
        assert!(md.contains("```thoxa"), "{md}");
    }

    #[test]
    fn hover_let_from_route_shows_string_type() {
        let src = r#"
type Request(method: String, path: String)
type MyRouter {
    GET "/home" { "Home" }
}
fn main() -> Int64 {
    let r = MyRouter()
    let req = Request(method: "GET", path: "/home")
    let a = r.route(req)
    a
    0
}
"#;
        let idx = build_index(src, "<input>");
        let use_a = src.rfind("\n    a\n").unwrap() + 5;
        let p = byte_to_position(src, use_a);
        let b = idx.binding_at(p.line, p.character).expect("a");
        assert_eq!(b.ty_hint.as_deref(), Some("Response"));
        let md = idx.hover_markdown(b).expect("hover");
        assert!(
            md.contains("a: Response") || md.contains("let a: Response"),
            "expected typed local hover Response, got: {md}"
        );
    }

    #[test]
    fn hover_route_method_from_type_body_clauses() {
        let src = r#"
type Request(method: String, path: String)
type MyRouter {
    GET "/home" { "Home" }
}
fn main() -> Int64 {
    let r = MyRouter()
    let req = Request(method: "GET", path: "/home")
    r.route(req)
    0
}
"#;
        let idx = build_index(src, "<input>");
        assert!(idx.ok);
        // Members of MyRouter include generated `route`.
        let members = idx.resolution.members_of("MyRouter");
        assert!(
            members.iter().any(|m| m.name == "route"),
            "expected route method member, got {:?}",
            members.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        let call = src.rfind("route").unwrap();
        let p = byte_to_position(src, call);
        let b = idx.binding_at(p.line, p.character).expect("route ref");
        assert_eq!(b.name, "route");
        let md = idx
            .hover_markdown_with_ret(b, Some("Response"))
            .expect("hover");
        assert!(md.contains("route"), "{md}");
        assert!(md.contains("Response"), "{md}");
        assert!(
            md.contains("generated") || md.contains("req: Request"),
            "{md}"
        );
        // Free handler from desugar is indexed.
        assert!(
            idx.resolution
                .module_definitions()
                .iter()
                .any(|d| d.name.starts_with("__MyRouter_route_")),
            "expected desugared handler fn in definitions"
        );
    }

    #[test]
    fn hover_synthesizes_default_void_return() {
        // Empty body → Void when no inferred ret is supplied.
        let src = "fn serveOne() {\n}\nfn main() -> Int64 {\n    serveOne()\n    0\n}\n";
        let idx = build_index(src, "<input>");
        let call = src.rfind("serveOne").unwrap();
        let p = byte_to_position(src, call);
        let b = idx
            .binding_at(p.line, p.character)
            .or_else(|| {
                // Def site if call ref not linked.
                idx.resolution
                    .module_definitions()
                    .into_iter()
                    .find(|b| b.name == "serveOne")
            })
            .expect("serveOne");
        let md = idx.hover_markdown(b).expect("hover");
        assert!(
            md.contains("serveOne") && md.contains("-> Void"),
            "expected synthesized Void return, got: {md}"
        );
    }

    #[test]
    fn hover_uses_inferred_return_type() {
        let src = "fn fortyTwo() {\n    42\n}\nfn main() -> Int64 {\n    fortyTwo()\n    0\n}\n";
        let idx = build_index(src, "<input>");
        let call = src.find("fortyTwo").unwrap();
        let p = byte_to_position(src, call);
        let b = idx.binding_at(p.line, p.character).expect("fortyTwo");
        let md = idx
            .hover_markdown_with_ret(b, Some("Int64"))
            .expect("hover");
        assert!(
            md.contains("fortyTwo") && md.contains("-> Int64"),
            "expected inferred Int64, got: {md}"
        );
        assert!(!md.contains("-> Void"), "{md}");
    }

    #[test]
    fn hover_keeps_written_return_type() {
        let src = "fn helper() -> Int64 { 1 }\nfn main() -> Int64 { helper() }\n";
        let idx = build_index(src, "<input>");
        let call = src.find("helper").unwrap();
        let p = byte_to_position(src, call);
        let b = idx.binding_at(p.line, p.character).expect("helper");
        let md = idx.hover_markdown(b).expect("hover");
        assert!(md.contains("-> Int64"), "{md}");
        assert!(!md.contains("-> Void"), "{md}");
    }
}
