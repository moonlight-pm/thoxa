//! Buffer → diagnostics via parse + MIR lower (no cgen).
//!
//! This is the check engine for `thoxa lsp` (and optional `thoxa check`). The
//! same frontend as `build`/`run` so diagnostics never drift.

use crate::ast::{Item, Module};
use crate::diagnostic::{self, Diagnostic, Severity};
use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token, TokenKind};
use crate::loader::{self, Package, SourceModule};
use crate::mir;
use crate::parser::Parser;
use crate::resolver::{self, BindingKind};
use crate::span::{self, Range, Span};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One lexer-driven semantic token refinement (line/char 0-based, length UTF-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: u32,
    pub character: u32,
    pub length: u32,
    /// LSP token type name (`type` / `function` / `variable`).
    pub token_type: String,
    pub modifiers: u32,
}

/// Related location for a diagnostic (LSP `relatedInformation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedInfo {
    pub message: String,
    pub range: Range,
    pub span: Span,
    pub filename: String,
}

/// One diagnostic ready for LSP publish / pretty CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    pub message: String,
    /// LSP range in the checked buffer (0-based, UTF-16 characters).
    pub range: Range,
    /// Original byte span into the buffer (when known).
    pub span: Span,
    /// Path / label passed to [`check_source`].
    pub filename: String,
    /// Stable code when known (`THX001`, …).
    pub code: Option<String>,
    pub severity: Severity,
    pub help: Vec<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub related: Vec<RelatedInfo>,
}

impl CheckDiagnostic {
    /// Structured IR for pretty / JSON formatting.
    pub fn to_diagnostic(&self) -> Diagnostic {
        use crate::diagnostic::Label;
        use std::path::PathBuf;
        Diagnostic {
            code: self.code.clone(),
            severity: self.severity,
            message: self.message.clone(),
            span: if self.span == Span::dummy() && self.range == Range::default() {
                None
            } else {
                Some(self.span)
            },
            path: Some(PathBuf::from(&self.filename)),
            labels: self
                .related
                .iter()
                .map(|r| Label {
                    span: r.span,
                    path: Some(PathBuf::from(&r.filename)),
                    message: r.message.clone(),
                })
                .collect(),
            help: self.help.clone(),
            expected: self.expected.clone(),
            actual: self.actual.clone(),
        }
    }
}

/// Lex, parse, and type-check `text` without codegen.
///
/// Returns diagnostics (errors and warnings). Independent function/method body
/// errors are collected together; earlier pipeline failures (lex/parse/types)
/// remain first-error only. Build/run still report the primary error only.
/// Warnings (e.g. unused imports) do not fail `thoxa check` exit status.
///
/// **Entry vs library:**
/// - Buffer defines `fn main` → program check (`require_main`).
/// - No `main` → library check (typecheck exports/types/fns without requiring
///   an entry point). Fixes false diagnostics on `iolib/`, `httplib/`, etc.
///
/// **Imports:**
/// - No `import` items → single-file lower on the buffer only.
/// - Has imports and `filename` is an existing file path → load the package from
///   disk for dependencies, but **overlay** the open module's AST with the
///   buffer parse (so unsaved edits on the open file are checked).
/// - Use [`check_source_with_overlays`] so other open buffers also replace disk.
/// - Has imports but `filename` is not a real path → one diagnostic explaining
///   that multi-file check needs a file path.
pub fn check_source(filename: &str, text: &str) -> Vec<CheckDiagnostic> {
    check_source_with_overlays(filename, text, &HashMap::new())
}

/// Like [`check_source`], but each path in `overlays` (absolute or soft path →
/// source text) wins over disk when loading package dependencies.
pub fn check_source_with_overlays(
    filename: &str,
    text: &str,
    overlays: &HashMap<PathBuf, String>,
) -> Vec<CheckDiagnostic> {
    check_and_returns_with_overlays(filename, text, overlays).0
}

/// Diagnostics plus MIR return-type map (one lower). Keys: free fn name or
/// `Type.method` (D129 hover cache).
pub fn check_and_returns_with_overlays(
    filename: &str,
    text: &str,
    overlays: &HashMap<PathBuf, String>,
) -> (Vec<CheckDiagnostic>, HashMap<String, String>) {
    let (error, lint_module, program) = check_source_inner(filename, text, overlays);
    let mut diags = Vec::new();
    if let Some(e) = error {
        diags.extend(errors_to_diagnostics(filename, text, e, overlays));
    }
    if let Some(module) = lint_module {
        diags.extend(lint_unused_imports(filename, text, &module));
    }
    let rets = program
        .map(|p| mir::function_return_displays(&p))
        .unwrap_or_default();
    (diags, rets)
}

/// Inferred / resolved return types only (same lower path as diagnostics).
pub fn return_types_with_overlays(
    filename: &str,
    text: &str,
    overlays: &HashMap<PathBuf, String>,
) -> HashMap<String, String> {
    check_and_returns_with_overlays(filename, text, overlays).1
}

fn module_has_main(module: &Module) -> bool {
    module.items.iter().any(|item| {
        matches!(
            item,
            Item::Function(f) if f.name == "main"
        )
    })
}

/// Returns `(hard_error, module_for_lints, program_if_lower_ok)`.
///
/// The module is the open buffer AST when parse (soft or hard) produced one —
/// used for unused-import lints even when lower failed.
fn check_source_inner(
    filename: &str,
    text: &str,
    overlays: &HashMap<PathBuf, String>,
) -> (Option<Error>, Option<Module>, Option<mir::Program>) {
    let tokens = match Lexer::new(text).tokenize() {
        Ok(t) => t,
        Err(e) => return (Some(e), None, None),
    };
    // Soft parse: recover partial AST so we can still report undefined names
    // after a mid-file syntax error (e.g. keyword `as`).
    let (module, mut pre_errors) =
        match Parser::new(tokens).with_source(text).parse_module() {
            Ok(m) => (m, Vec::new()),
            Err(mut e) => match e.recovered_module.take() {
                Some(recovered) => (*recovered, e.into_diagnostics()),
                None => return (Some(e), None, None),
            },
        };

    // Keep a copy for lints (lower may take ownership via package overlay).
    let lint_module = module.clone();
    let has_main = module_has_main(&module);

    let has_imports = module
        .items
        .iter()
        .any(|item| matches!(item, Item::Import(_)));

    let lower_result: Result<mir::Program> = if !has_imports {
        // Single file: a program when it defines `main`, else a library.
        let mut package = Package {
            project_root: PathBuf::from("."),
            modules: vec![SourceModule {
                id: "Main".into(),
                path: PathBuf::from(filename),
                ast: module,
                is_entry: true,
            }],
        };
        crate::loader::include_core_modules(&mut package).and_then(|_| {
            let opts = mir::LowerOptions {
                require_main: has_main,
            };
            mir::lower_package_with(&package, opts)
        })
    } else {
        let path = Path::new(filename);
        if !path.is_file() {
            let span = first_import_span(&module).unwrap_or_else(Span::dummy);
            pre_errors.push(Error::at(
                "checking imports requires a real file path (open the file on disk for multi-file check)",
                span,
            ));
            return (Some(Error::multi(pre_errors)), Some(lint_module), None);
        }

        // Prefer the buffer AST so soft-recovered parse errors still lower.
        // Re-parsing the entry from disk would hard-fail on the same soft errors.
        let mut package = if pre_errors.is_empty() {
            match loader::load_package(path) {
                Ok(mut p) => {
                    overlay_open_ast(&mut p, module, path, has_main);
                    p
                }
                Err(e) => return (Some(e), Some(lint_module), None),
            }
        } else {
            match loader::load_package_with_entry_ast(path, module) {
                Ok(p) => p,
                Err(e) => {
                    pre_errors.extend(e.into_diagnostics());
                    return (Some(Error::multi(pre_errors)), Some(lint_module), None);
                }
            }
        };
        apply_open_overlays(&mut package, overlays, path);
        let opts = mir::LowerOptions {
            require_main: has_main,
        };
        mir::lower_package_with(&package, opts)
    };

    let (error, program) = match (pre_errors.is_empty(), lower_result) {
        (true, Ok(p)) => (None, Some(p)),
        (true, Err(e)) => (Some(e), None),
        (false, Ok(p)) => (Some(Error::multi(pre_errors)), Some(p)),
        (false, Err(e)) => {
            pre_errors.extend(e.into_diagnostics());
            (Some(Error::multi(pre_errors)), None)
        }
    };
    (error, Some(lint_module), program)
}

/// Warn on each imported local name that is never referenced in this module.
///
/// Fallible companions: `May` counts as used when `Has`/`Not` is referenced
/// (patterns / ctors / `Bytes.find` wrap), and `Trial` when `Pass`/`Fail` is —
/// those type names are required in scope even if never written as text.
fn lint_unused_imports(filename: &str, text: &str, module: &Module) -> Vec<CheckDiagnostic> {
    let has_import = module
        .items
        .iter()
        .any(|item| matches!(item, Item::Import(_)));
    if !has_import {
        return Vec::new();
    }

    let res = resolver::resolve_module(module);
    let mut import_by_name: HashMap<&str, (u32, Option<Span>)> = HashMap::new();
    for b in res.module_definitions() {
        if b.kind == BindingKind::Import {
            import_by_name.insert(b.name.as_str(), (b.id, b.def_span));
        }
    }

    let is_referenced = |name: &str| -> bool {
        import_by_name
            .get(name)
            .map(|(id, _)| !res.refs_to(*id).is_empty())
            .unwrap_or(false)
    };

    // Implicit type-name uses via constructors / match arms.
    let may_live = is_referenced("Has") || is_referenced("Not");
    let trial_live = is_referenced("Pass") || is_referenced("Fail");

    let mut unused: Vec<(String, Span)> = Vec::new();
    for b in res.module_definitions() {
        if b.kind != BindingKind::Import {
            continue;
        }
        if !res.refs_to(b.id).is_empty() {
            continue;
        }
        // Keep parent type if a constructor is live (even with no direct name ref).
        if b.name == "May" && may_live {
            continue;
        }
        if b.name == "Trial" && trial_live {
            continue;
        }
        let Some(span) = b.def_span else { continue };
        unused.push((b.name.clone(), span));
    }
    // Stable report order: source order (def_span start).
    unused.sort_by_key(|(_, span)| span.start);

    unused
        .into_iter()
        .map(|(name, span)| {
            let d = Diagnostic::warning(format!("unused import `{name}`"))
                .with_code(diagnostic::CODE_UNUSED_IMPORT)
                .with_span(span)
                .with_path(filename)
                .enrich();
            structured_to_check(filename, text, d, &HashMap::new())
        })
        .collect()
}

/// Replace the open module's AST with the in-memory buffer parse.
///
/// When the buffer defines `fn main`, keep/promote it as package entry. When it
/// does not (library module open as the load root), demote `is_entry` so names
/// mangle as a library and `require_main` can be off.
fn overlay_open_ast(package: &mut Package, module: Module, open_path: &Path, has_main: bool) {
    let open_canon = canonicalize_soft(open_path);
    for m in &mut package.modules {
        if m.is_entry || paths_equal(&m.path, open_path, open_canon.as_ref()) {
            m.ast = module;
            m.is_entry = has_main;
            return;
        }
    }
    // Fallback: if load_package somehow lacked the open file, push one.
    package.modules.push(SourceModule {
        id: open_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Main")
            .to_string(),
        path: open_path.to_path_buf(),
        ast: module,
        is_entry: has_main,
    });
}

/// Re-parse any non-entry package module whose path has an open-buffer overlay.
fn apply_open_overlays(
    package: &mut Package,
    overlays: &HashMap<PathBuf, String>,
    entry_path: &Path,
) {
    if overlays.is_empty() {
        return;
    }
    let entry_canon = canonicalize_soft(entry_path);
    for m in &mut package.modules {
        if m.is_entry {
            continue;
        }
        if paths_equal(&m.path, entry_path, entry_canon.as_ref()) {
            continue;
        }
        let Some(text) = overlay_text_for(&m.path, overlays) else {
            continue;
        };
        if let Ok(ast) = parse_module_text(text) {
            m.ast = ast;
        }
        // Parse failure: leave disk AST; diagnostics on that buffer will surface separately.
    }
}

fn overlay_text_for<'a>(path: &Path, overlays: &'a HashMap<PathBuf, String>) -> Option<&'a str> {
    if let Some(t) = overlays.get(path) {
        return Some(t.as_str());
    }
    let path_canon = canonicalize_soft(path);
    for (k, v) in overlays {
        if paths_equal(path, k, path_canon.as_ref()) {
            return Some(v.as_str());
        }
        // Also try canonicalize of overlay key.
        if let Some(kc) = canonicalize_soft(k) {
            if let Some(ref pc) = path_canon {
                if kc == *pc {
                    return Some(v.as_str());
                }
            }
        }
    }
    None
}

fn parse_module_text(text: &str) -> Result<Module> {
    let tokens = Lexer::new(text).tokenize()?;
    Parser::new(tokens).parse_module()
}

fn paths_equal(a: &Path, b: &Path, b_canon: Option<&PathBuf>) -> bool {
    if a == b {
        return true;
    }
    match (canonicalize_soft(a), b_canon) {
        (Some(ca), Some(cb)) => ca == *cb,
        _ => false,
    }
}

fn canonicalize_soft(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.to_path_buf()))
}

fn first_import_span(module: &Module) -> Option<Span> {
    module.items.iter().find_map(|item| match item {
        Item::Import(imp) => Some(imp.path_span),
        _ => None,
    })
}

fn errors_to_diagnostics(
    filename: &str,
    text: &str,
    e: Error,
    overlays: &HashMap<PathBuf, String>,
) -> Vec<CheckDiagnostic> {
    let path = Path::new(filename);
    let structured = e.into_structured(Some(path));
    structured
        .into_iter()
        .map(|d| structured_to_check(filename, text, d, overlays))
        .collect()
}

/// Source text for a diagnostic path: open entry buffer, other open overlays, else disk.
fn source_for_diag_path(
    open_filename: &str,
    open_text: &str,
    diag_path: &Path,
    overlays: &HashMap<PathBuf, String>,
) -> String {
    let open = Path::new(open_filename);
    if paths_equal(open, diag_path, canonicalize_soft(diag_path).as_ref()) {
        return open_text.to_string();
    }
    if let Some(t) = overlay_text_for(diag_path, overlays) {
        return t.to_string();
    }
    std::fs::read_to_string(diag_path).unwrap_or_default()
}

fn structured_to_check(
    open_filename: &str,
    open_text: &str,
    d: Diagnostic,
    overlays: &HashMap<PathBuf, String>,
) -> CheckDiagnostic {
    let fname = d
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| open_filename.to_string());
    let text = source_for_diag_path(open_filename, open_text, Path::new(&fname), overlays);
    let span = d.span.unwrap_or_else(Span::dummy);
    let range = if d.span.is_some() {
        span::span_to_range(&text, span)
    } else {
        Range::default()
    };
    let related = d
        .labels
        .into_iter()
        .map(|lab| {
            let lab_fname = lab
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| fname.clone());
            let lab_src =
                source_for_diag_path(open_filename, open_text, Path::new(&lab_fname), overlays);
            RelatedInfo {
                message: lab.message,
                range: span::span_to_range(&lab_src, lab.span),
                span: lab.span,
                filename: lab_fname,
            }
        })
        .collect();
    CheckDiagnostic {
        message: d.message,
        range,
        span,
        filename: fname,
        code: d.code,
        severity: d.severity,
        help: d.help,
        expected: d.expected,
        actual: d.actual,
        related,
    }
}

/// Classify identifiers the grammar cannot resolve from casing alone.
///
/// - PascalCase → `type`
/// - camelCase before `(` → `function`
/// - other camelCase → `variable`
/// - tag names → `type` (component) / `macro` (HTML); attribute names → `property`
///
/// Tags start where the parser starts them (D147): `<name` / `<>` with no space
/// after `<`. Text between tags is prose and gets no token; `{…}` holes are
/// code again.
///
/// Keywords, strings, numbers, comments stay with tree-sitter (`combined` mode).
/// Lex failures yield an empty list (leave base coloring only).
pub fn semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let tokens = match Lexer::new(text).tokenize() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    /// Where a token sits relative to JSX.
    enum Scope {
        /// Thoxa code; `braces` counts `{` still open inside it.
        Code { braces: usize },
        /// Between `<name` and its `>` / `/>`.
        TagHead,
        /// Between an opening tag's `>` and its closing tag.
        Children,
        /// `</name>`.
        ClosingTag,
    }

    let mut out = Vec::new();
    let mut scopes = vec![Scope::Code { braces: 0 }];
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let next = tokens.get(index + 1).map(|t| &t.kind);
        let top = scopes.last_mut().expect("code scope at the bottom");
        match (top, &token.kind) {
            (Scope::Code { .. } | Scope::Children, TokenKind::LAngle)
                if starts_tag(&tokens, index) =>
            {
                // Step over the name too, or the `>` of `<>`.
                index += 1;
                if let TokenKind::Ident(name) = &tokens[index].kind {
                    out.push(semantic_token(text, &tokens[index], tag_kind(name)));
                    scopes.push(Scope::TagHead);
                } else {
                    scopes.push(Scope::Children);
                }
            }
            (Scope::Code { braces }, TokenKind::LBrace) => *braces += 1,
            (Scope::Code { braces }, TokenKind::RBrace) => {
                if *braces > 0 {
                    *braces -= 1;
                } else if scopes.len() > 1 {
                    // The `}` of a hole: back to the tag around it.
                    scopes.pop();
                }
            }
            (Scope::Code { .. }, TokenKind::Nowait) => {
                out.push(semantic_token(text, token, "keyword"));
            }
            (Scope::Code { .. }, TokenKind::Ident(name)) => {
                let kind = if name.starts_with(|c: char| c.is_uppercase()) {
                    "type"
                } else if matches!(next, Some(TokenKind::LParen)) {
                    "function"
                } else {
                    "variable"
                };
                out.push(semantic_token(text, token, kind));
            }
            (Scope::Code { .. }, _) => {}
            (Scope::TagHead, TokenKind::Ident(_)) => {
                out.push(semantic_token(text, token, "property"));
            }
            (Scope::TagHead | Scope::Children, TokenKind::LBrace) => {
                scopes.push(Scope::Code { braces: 0 });
            }
            (Scope::TagHead, TokenKind::Slash) if matches!(next, Some(TokenKind::RAngle)) => {
                index += 1;
                scopes.pop();
            }
            (Scope::TagHead, TokenKind::RAngle) => {
                scopes.pop();
                scopes.push(Scope::Children);
            }
            (Scope::TagHead, _) => {}
            (Scope::Children, TokenKind::LAngle) if matches!(next, Some(TokenKind::Slash)) => {
                index += 1;
                scopes.push(Scope::ClosingTag);
            }
            // Prose between tags.
            (Scope::Children, _) => {}
            (Scope::ClosingTag, TokenKind::Ident(name)) => {
                out.push(semantic_token(text, token, tag_kind(name)));
            }
            (Scope::ClosingTag, TokenKind::RAngle) => {
                scopes.pop();
                scopes.pop();
            }
            (Scope::ClosingTag, _) => {}
        }
        index += 1;
    }
    out
}

/// `<name` / `<>` with no space after `<` starts a tag, as in the parser,
/// unless a name is glued before it: `Array<Int64>` opens type arguments.
fn starts_tag(tokens: &[Token], index: usize) -> bool {
    let angle = &tokens[index];
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    let after_name = index > 0
        && tokens[index - 1].span.end == angle.span.start
        && matches!(tokens[index - 1].kind, TokenKind::Ident(_));
    angle.span.end == next.span.start
        && matches!(next.kind, TokenKind::Ident(_) | TokenKind::RAngle)
        && !after_name
}

/// A component tag is a type; an HTML tag is `macro`, distinct from
/// `variable` so combined mode keeps the HTML hue.
fn tag_kind(name: &str) -> &'static str {
    if name.starts_with(|c: char| c.is_uppercase()) {
        "type"
    } else {
        "macro"
    }
}

fn semantic_token(text: &str, token: &Token, kind: &str) -> SemanticToken {
    let range = span::span_to_range(text, token.span);
    let slice = text.get(token.span.start..token.span.end).unwrap_or("");
    SemanticToken {
        line: range.start.line,
        character: range.start.character,
        length: slice.chars().map(|c| c.len_utf16() as u32).sum(),
        token_type: kind.into(),
        modifiers: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Position;

    #[test]
    fn good_snippet_no_diagnostics() {
        let diags = check_source("<input>", "fn main() -> Int64 { 0 }\n");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    /// A file with no imports still has String's std methods (D196).
    #[test]
    fn string_methods_without_imports() {
        let text = "fn main() -> Int64 {\n    if \"Straße\".equals(\"STRASSE\", ignoreCase: true) {\n        0\n    } else {\n        1\n    }\n}\n";
        let diags = check_source("<input>", text);
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn unused_import_is_warning() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-unused-imp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        fs::write(
            &helper,
            "export fn answer() -> Int64 { 42 }\nexport fn other() -> Int64 { 1 }\n",
        )
        .unwrap();
        let src = "\
import { answer, other } from \"./helper\"
fn main() -> Int64 { answer() }
";
        fs::write(&main, src).unwrap();
        let diags = check_source(main.to_str().unwrap(), src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("THX010"));
        assert!(
            diags[0].message.contains("unused import `other`"),
            "{}",
            diags[0].message
        );
        assert!(!diags[0].help.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unused_namespace_import_is_warning() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-unused-ns-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let src = "\
import h from \"./helper\"
fn main() -> Int64 { 0 }
";
        fs::write(&main, src).unwrap();
        let diags = check_source(main.to_str().unwrap(), src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(
            diags[0].message.contains("unused import `h`"),
            "{}",
            diags[0].message
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn used_import_no_warning() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-used-imp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let src = "import { answer } from \"./helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, src).unwrap();
        let diags = check_source(main.to_str().unwrap(), src);
        assert!(diags.is_empty(), "unexpected: {diags:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn combined_import_unused_selective_is_warning() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-combined-imp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        fs::write(
            &helper,
            "export fn answer() -> Int64 { 42 }\nexport fn other() -> Int64 { 1 }\n",
        )
        .unwrap();
        let src = "\
import h, { answer, other } from \"./helper\"
fn main() -> Int64 { h.answer() + answer() }
";
        fs::write(&main, src).unwrap();
        let diags = check_source(main.to_str().unwrap(), src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Warning);
        assert_eq!(diags[0].code.as_deref(), Some("THX010"));
        assert!(
            diags[0].message.contains("unused import `other`"),
            "{}",
            diags[0].message
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn structured_codes_on_undefined_and_type_error() {
        let src = "fn main() -> Int64 { x }\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("THX001"));
        assert!(!diags[0].help.is_empty());

        let src = "fn main() -> Int64 { if 3 { 0 } 0 }\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("THX002"));
        assert!(!diags[0].help.is_empty());
    }

    #[test]
    fn structured_code_top_level_with_main() {
        let src = "FOOBAR\nfn main() -> Int64 { 0 }\n";
        let diags = check_source("<input>", src);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("THX005")),
            "{diags:?}"
        );
    }

    #[test]
    fn good_hello_style() {
        let src = "fn main() -> Int64 {\n    0\n}\n";
        let diags = check_source("<input>", src);
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn type_error_one_diagnostic_with_range() {
        let src = "fn main() -> Int64 { if 3 { 0 } 0 }\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert!(
            d.message.contains("Bool"),
            "message should mention Bool: {}",
            d.message
        );
        assert!(
            d.range != Range::default() || d.span != Span::dummy(),
            "expected a non-empty span/range, got {d:?}"
        );
        // `3` is on line 0
        assert_eq!(d.range.start.line, 0);
        let three_at = src.find('3').unwrap();
        assert_eq!(d.span.start, three_at);
        assert_eq!(
            d.range.start,
            Position {
                line: 0,
                character: three_at as u32
            }
        );
    }

    #[test]
    fn type_error_multiline_range() {
        let src = "fn main() -> Int64 {\n  if 3 {\n    0\n  }\n  0\n}\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert!(d.message.contains("Bool"), "{}", d.message);
        assert_eq!(d.range.start.line, 1);
        assert_eq!(d.range.start.character, 5); // "  if "
        assert_eq!(d.range.end.character, 6);
    }

    #[test]
    fn lex_error_one_diagnostic() {
        let src = "fn main() -> Int64 { @ }\n";
        let diags = check_source("<input>", src);
        assert!(!diags.is_empty());
        assert!(
            diags[0].message.to_lowercase().contains("unexpected")
                || diags[0].message.contains('@'),
            "{}",
            diags[0].message
        );
        assert_ne!(diags[0].range, Range::default());
    }

    #[test]
    fn lex_error_multi_unexpected_chars() {
        // Junk chars (`@`, `$`) are tokens now (JSX raw text) and fail at parse.
        let src = "fn main() -> Int64 { @ $ }\n";
        let diags = check_source("<input>", src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.to_lowercase().contains("unexpected")),
            "{diags:?}"
        );
    }

    #[test]
    fn parse_error_one_diagnostic() {
        let src = "fn main() -> Int64 { if true\n";
        let diags = check_source("<input>", src);
        assert!(!diags.is_empty());
        assert!(
            diags[0].message.to_lowercase().contains("expected")
                || diags[0].message.contains("unexpected")
                || !diags[0].message.is_empty(),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn parse_error_multi_independent_items() {
        // Two broken free functions; recovery finds the second `fn`.
        let src = "\
fn a( -> Int64 { 0 }
fn b( -> Int64 { 0 }
fn main() -> Int64 { 0 }
";
        let diags = check_source("<input>", src);
        assert!(
            diags.len() >= 2,
            "expected multi parse diags, got {}: {diags:?}",
            diags.len()
        );
    }

    #[test]
    fn parse_error_multi_inside_block() {
        // Two incomplete `let`s: each is a separate block item; recovery lands on
        // the next `let` (not nested as a then-block of the first).
        let src = "\
fn main() -> Int64 {
  let
  let
  0
}
";
        let diags = check_source("<input>", src);
        assert!(
            diags.len() >= 2,
            "expected mid-block multi parse diags, got {}: {diags:?}",
            diags.len()
        );
    }

    #[test]
    fn import_without_real_path_clear_diagnostic() {
        // Use the import so the only error is path-related (not unused-import).
        let src = "import { foo } from \"./helper\"\nfn main() -> Int64 { foo() }\n";
        let diags = check_source("<input>", src);
        assert!(
            diags.iter().any(|d| {
                d.message.contains("real file path") || d.message.contains("multi-file")
            }),
            "{diags:?}"
        );
    }

    #[test]
    fn fallible_may_kept_when_has_used() {
        // `May` is required in scope for `Has`/`Not` even when never written as text.
        use std::fs;
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let main = workspace.join("target/thoxa-test-may-live/main.thx");
        let _ = fs::create_dir_all(main.parent().unwrap());
        // No `May` type annotation — only constructors; companion rule must keep May.
        let src = "\
import { Has, Not, May } from \"std/fallible\"
fn main() -> Int64 {
  let m = Not()
  match m {
    Has(i) { i }
    Not { 0 }
  }
}
";
        fs::write(&main, src).unwrap();
        let diags = check_source(main.to_str().unwrap(), src);
        let unused: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("THX010"))
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            !unused.iter().any(|m| m.contains("`May`")),
            "May should be kept live via Has/Not: {unused:?} full={diags:?}"
        );
        // And no hard errors for this program shape.
        assert!(
            !diags.iter().any(|d| d.severity == Severity::Error),
            "{diags:?}"
        );
        let _ = fs::remove_dir_all(main.parent().unwrap());
    }

    #[test]
    fn open_buffer_overlay_affects_check() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-overlay-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        // Disk: helper returns Int64 (compatible).
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let main_src = "import { answer } from \"./helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        // Open buffer: change helper to return Bool → type error at use site.
        let edited = "export fn answer() -> Bool { true }\n";
        let mut overlays = HashMap::new();
        overlays.insert(helper.clone(), edited.to_string());
        let diags = check_source_with_overlays(main.to_str().unwrap(), main_src, &overlays);
        assert_eq!(
            diags.len(),
            1,
            "expected type error from overlay: {diags:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Dependency-module import errors must keep the dependency file path and
    /// span — not paint the open buffer at the same byte offset (wrong token).
    #[test]
    fn dep_import_error_keeps_foreign_path_and_span() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-dep-span-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let app = dir.join("app.thx");
        // Helper imports a name that does not exist on std/fallible.
        fs::write(
            &helper,
            "import { NotARealExport } from \"std/fallible\"\nexport fn hi() -> Int64 { 1 }\n",
        )
        .unwrap();
        // App is longer / different so a stolen span would underline the wrong token.
        let app_src = "\
## filler so offsets diverge from Helper
import { hi } from \"./helper\"
fn isReadMethod(method!: String) -> Bool {
    if method == \"GET\" {
        true
    } else {
        method == \"HEAD\"
    }
}
fn main() -> Int64 {
    hi()
}
";
        fs::write(&app, app_src).unwrap();
        let diags = check_source(app.to_str().unwrap(), app_src);
        let err = diags
            .iter()
            .find(|d| d.message.contains("NotARealExport"))
            .unwrap_or_else(|| panic!("expected not-exported diag: {diags:?}"));
        assert!(
            err.filename.contains("helper.thx"),
            "diag must stay on Helper, got filename={}",
            err.filename
        );
        // Span text in Helper is the bad import name — not App's `String`.
        let helper_src = fs::read_to_string(&helper).unwrap();
        let slice = helper_src.get(err.span.start..err.span.end).unwrap_or("");
        assert_eq!(
            slice, "NotARealExport",
            "span must cover the import name, got {slice:?} range={:?}",
            err.range
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_with_disk_package_ok() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let main = dir.join("main.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        // Buffer has a type error in the open file; deps load from disk.
        let bad =
            "import { answer } from \"./helper\"\nfn main() -> Int64 { if answer() { 0 } 0 }\n";
        fs::write(
            &main,
            "import { answer } from \"./helper\"\nfn main() -> Int64 { answer() }\n",
        )
        .unwrap();
        let diags = check_source(main.to_str().unwrap(), bad);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("Bool"), "{}", diags[0].message);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_file_tilde_import_uses_package_main() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-check-tilde-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("app")).unwrap();
        fs::create_dir_all(dir.join("framework")).unwrap();
        fs::write(dir.join("main.thx"), "fn main() -> Int64 { 0 }\n").unwrap();
        fs::write(
            dir.join("framework").join("wire.thx"),
            "export fn ping() -> Int64 { 1 }\n",
        )
        .unwrap();
        let pages = dir.join("app").join("pages.thx");
        let src =
            "import wire from \"~/framework/wire\"\nexport fn go() -> Int64 { wire.ping() }\n";
        fs::write(&pages, src).unwrap();
        let diags = check_source(pages.to_str().unwrap(), src);
        assert!(
            diags.is_empty(),
            "nested ~/ should resolve via main.thx: {diags:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_module_without_main_ok() {
        // Export-only modules must typecheck without `fn main` (LSP on io.thx / http.thx).
        let src = "export fn answer() -> Int64 { 42 }\nexport let n = 1\n";
        let diags = check_source("helper.thx", src);
        assert!(
            diags.is_empty(),
            "library should not require main: {diags:?}"
        );
    }

    #[test]
    fn library_module_still_reports_type_errors() {
        let src = "export fn bad() -> Int64 { if 3 { 0 } 0 }\n";
        let diags = check_source("lib.thx", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("Bool"), "{}", diags[0].message);
        assert!(
            !diags[0].message.contains("main"),
            "should not be a missing-main error: {}",
            diags[0].message
        );
    }

    #[test]
    fn library_with_imports_no_main_ok() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!("thoxa-lib-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("helper.thx");
        let lib = dir.join("lib.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let src = "import { answer } from \"./helper\"\nexport fn use() -> Int64 { answer() }\n";
        fs::write(&lib, src).unwrap();
        let diags = check_source(lib.to_str().unwrap(), src);
        assert!(diags.is_empty(), "library with imports: {diags:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn route_clause_int64_req_and_mid_path_typecheck() {
        let src = r#"
type Request(method: String, path: String)
type Response(status: Int64)
fn ok(body!: Bytes, contentType!: String) -> Response { Response(status: 200) }
fn notFound() -> Response { Response(status: 404) }
fn badRequest(msg!: String) -> Response { Response(status: 400) }
type Router {
    GET "/home" (req: Request) { "x" }
    GET "/user/:userId" (userId: Int64) -> String { "u" }
    GET "/user/:userId/edit" (req: Request, userId: Int64) -> String { "e" }
}
fn main() -> Int64 {
    let r = Router()
    let a = r.route(Request(method: "GET", path: "/user/42"))
    a.status
}
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.is_empty(),
            "Int64 / req / mid-path should typecheck, got {diags:?}"
        );
    }

    #[test]
    fn route_clause_mismatch_mentions_response() {
        let src = r#"
type Request(method: String, path: String)
type Response
fn ok(body!: Bytes, contentType!: String) -> Response { Response() }
fn notFound() -> Response { Response() }
type Router {
    GET "/home" {
        1
    }
}
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.iter().any(|d| {
                d.message.contains("String or Response") && d.message.contains("Int64")
            }),
            "expected route-clause String-or-Response diagnostic, got {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| { d.help.iter().any(|h| h.contains("-> Response")) }),
            "expected help mentioning -> Response, got {diags:?}"
        );
    }

    #[test]
    fn program_without_main_still_errors() {
        // Single-file program path used to always require main via lower_module.
        // With require_main only when main is present, bare helpers are libs.
        // A true program check is `thoxa build` / buffer that *has* a broken main.
        let src = "fn main() -> Bool { true }\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.to_lowercase().contains("int64") || diags[0].message.contains("main"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn multi_diag_collects_independent_function_bodies() {
        // Two free functions with independent type errors → both diagnostics.
        let src = r#"
fn a() -> Int64 {
    if 1 { 0 }
    0
}
fn b() -> Int64 {
    if 2 { 0 }
    0
}
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.len() >= 2,
            "expected multi-diag, got {}: {diags:?}",
            diags.len()
        );
        assert!(
            diags.iter().all(|d| d.message.contains("Bool")),
            "{diags:?}"
        );
    }

    #[test]
    fn multi_diag_collects_method_and_free_function() {
        // Method body error + free-function body error both reported.
        let src = r#"
type Point(x: Int64) {
  export fn bad() -> Int64 {
    if 1 { 0 }
    0
  }
}
fn also() -> Int64 {
  if 2 { 0 }
  0
}
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.len() >= 2,
            "expected method+fn multi-diag, got {}: {diags:?}",
            diags.len()
        );
        assert!(
            diags.iter().all(|d| d.message.contains("Bool")),
            "{diags:?}"
        );
    }

    #[test]
    fn multi_diag_collects_independent_type_reg_errors() {
        // Two concrete types with independent unknown field types.
        let src = r#"
type Alpha(x: NoSuchAlpha)
type Beta(y: NoSuchBeta)
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.len() >= 2,
            "expected type-reg multi-diag, got {}: {diags:?}",
            diags.len()
        );
        let joined = diags
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            joined.contains("NoSuchAlpha") && joined.contains("NoSuchBeta"),
            "{joined}"
        );
    }

    #[test]
    fn union_match_lists_missing_arms() {
        let src = r#"
type A
type B
type C
type U = A | B | C
fn main() -> Int64 {
    let u: U = A()
    match u {
        A { 1 }
        B { 2 }
    }
    0
}
"#;
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("non-exhaustive") && diags[0].message.contains("C"),
            "expected missing C named: {}",
            diags[0].message
        );
    }

    #[test]
    fn union_match_unreachable_after_else() {
        let src = r#"
type A
type B
type U = A | B
fn main() -> Int64 {
    let u: U = A()
    match u {
        else { 0 }
        A { 1 }
    }
    0
}
"#;
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].message.contains("unreachable"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn fn_type_not_allowed_as_ctor_field() {
        let src = r#"
fn id(x: Int64) -> Int64 { x }
type Box(h: (x: Int64) -> Int64)
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("constructor-param")),
            "{diags:?}"
        );
    }

    #[test]
    fn nested_user_ctor_field_ok() {
        let src = r#"
type Inner(n: Int64)
type Outer(inner: Inner)
fn main() -> Int64 {
    let o = Outer(inner: Inner(n: 1))
    o.inner.n
}
"#;
        let diags = check_source("<input>", src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn node_ctor_field_ok() {
        let src = r#"
type Wrap(n: Node)
fn main() -> Int64 {
    let w = Wrap(n: <p>x</p>)
    0
}
"#;
        let diags = check_source("<input>", src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn recursive_user_field_is_cycle() {
        let src = r#"
type Cons(n: Int64, tail: Cons)
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.iter().any(|d| d.message.contains("itself")),
            "{diags:?}"
        );
    }

    #[test]
    fn mutual_user_field_is_cycle() {
        let src = r#"
type A(b: B)
type B(a: A)
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags.iter().any(|d| d.message.contains("itself")),
            "{diags:?}"
        );
    }

    #[test]
    fn clayout_not_allowed_as_ctor_field() {
        let src = r#"
type Point {
    mut x: Int64
    mut y: Int64
}
type Wrap(p: Point)
fn main() -> Int64 { 0 }
"#;
        let diags = check_source("<input>", src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("constructor-param")),
            "{diags:?}"
        );
    }

    #[test]
    fn semantic_tokens_nowait_is_keyword() {
        let src = "fn main() { --> serveOne(peer) }\n";
        let toks = semantic_tokens(src);
        assert!(
            toks.iter()
                .any(|t| t.token_type == "keyword" && t.length == 3),
            "--> should be keyword: {toks:?}"
        );
        assert!(
            toks.iter()
                .any(|t| t.token_type == "function" && t.length == 8),
            "serveOne should stay a function: {toks:?}"
        );
    }

    #[test]
    fn semantic_tokens_fn_and_call() {
        let src = "fn main() -> Int64 { 0 }\n";
        let toks = semantic_tokens(src);
        let types: Vec<&str> = toks.iter().map(|t| t.token_type.as_str()).collect();
        // main + Int64
        assert!(types.contains(&"function"), "{toks:?}");
        assert!(types.contains(&"type"), "{toks:?}");
        let main = toks
            .iter()
            .find(|t| t.token_type == "function" && t.character == 3)
            .expect("main function token");
        assert_eq!(main.length, 4); // "main"
        let int64 = toks
            .iter()
            .find(|t| t.token_type == "type")
            .expect("Int64 type token");
        assert_eq!(int64.length, 5);
    }

    #[test]
    fn semantic_tokens_on_lex_error_empty() {
        assert!(semantic_tokens("@").is_empty());
    }

    #[test]
    fn semantic_tokens_jsx_tag_not_variable() {
        let src = r#"let n = <div class="hero">Hi</div>"#;
        let toks = semantic_tokens(src);
        let divs: Vec<_> = toks.iter().filter(|t| t.token_type == "macro").collect();
        assert_eq!(divs.len(), 2, "open + close tag: {toks:?}");
        assert!(
            toks.iter().any(|t| t.token_type == "property"),
            "class attr: {toks:?}"
        );
    }

    /// The text each token covers, with its kind.
    fn token_texts(src: &str) -> Vec<(String, String)> {
        let lines: Vec<&str> = src.split('\n').collect();
        semantic_tokens(src)
            .iter()
            .map(|t| {
                let line: Vec<char> = lines[t.line as usize].chars().collect();
                let start = t.character as usize;
                let word: String = line[start..start + t.length as usize].iter().collect();
                (word, t.token_type.clone())
            })
            .collect()
    }

    fn kind_of<'a>(toks: &'a [(String, String)], word: &str) -> Vec<&'a str> {
        toks.iter()
            .filter(|(text, _)| text == word)
            .map(|(_, kind)| kind.as_str())
            .collect()
    }

    #[test]
    fn semantic_tokens_comparison_is_not_a_tag() {
        let src = "fn f(limit: Int64) {\n    while index < limit {\n        index = index + 1\n    }\n}\n";
        let toks = token_texts(src);
        assert_eq!(kind_of(&toks, "limit"), ["variable", "variable"], "{toks:?}");
        assert!(
            kind_of(&toks, "index").iter().all(|kind| *kind == "variable"),
            "{toks:?}"
        );
    }

    #[test]
    fn semantic_tokens_type_arguments_are_not_a_tag() {
        let src = "fn view(children: Array<Node>) -> May<String> {\n    total = 1\n}\n";
        let toks = token_texts(src);
        assert_eq!(kind_of(&toks, "Node"), ["type"], "{toks:?}");
        assert_eq!(kind_of(&toks, "String"), ["type"], "{toks:?}");
        assert_eq!(kind_of(&toks, "total"), ["variable"], "{toks:?}");
    }

    #[test]
    fn semantic_tokens_jsx_text_and_holes() {
        let src = "let page = <p class=\"x\">Hello world, {name} said {greet(who)}</p>\nlet after = 1\n";
        let toks = token_texts(src);
        for prose in ["Hello", "world", "said"] {
            assert!(kind_of(&toks, prose).is_empty(), "{prose}: {toks:?}");
        }
        assert_eq!(kind_of(&toks, "p"), ["macro", "macro"], "{toks:?}");
        assert_eq!(kind_of(&toks, "class"), ["property"], "{toks:?}");
        assert_eq!(kind_of(&toks, "name"), ["variable"], "{toks:?}");
        assert_eq!(kind_of(&toks, "greet"), ["function"], "{toks:?}");
        assert_eq!(kind_of(&toks, "after"), ["variable"], "{toks:?}");
    }

    #[test]
    fn semantic_tokens_fragment_and_self_closing() {
        let src = "let page = <><Card title={heading} /><br/>Text</>\nlet after = 1\n";
        let toks = token_texts(src);
        assert_eq!(kind_of(&toks, "Card"), ["type"], "{toks:?}");
        assert_eq!(kind_of(&toks, "title"), ["property"], "{toks:?}");
        assert_eq!(kind_of(&toks, "heading"), ["variable"], "{toks:?}");
        assert_eq!(kind_of(&toks, "br"), ["macro"], "{toks:?}");
        assert!(kind_of(&toks, "Text").is_empty(), "{toks:?}");
        assert_eq!(kind_of(&toks, "after"), ["variable"], "{toks:?}");
    }
}

#[cfg(test)]
mod return_type_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn return_types_include_inferred_int64() {
        let src = "\
fn fortyTwo() {
    42
}
fn main() -> Int64 {
    fortyTwo()
    0
}
";
        let rets = return_types_with_overlays("<input>", src, &HashMap::new());
        assert_eq!(
            rets.get("fortyTwo").map(String::as_str),
            Some("Int64"),
            "got {rets:?}"
        );
        assert_eq!(rets.get("main").map(String::as_str), Some("Int64"));
    }

    /// D174: hover shows an unannotated method's inferred type, not Void.
    #[test]
    fn return_types_include_inferred_method() {
        let src = "\
type Box(width: Int64, height: Int64) {
    export fn area() {
        width * height
    }
    export fn grow() {
        width = width + 1
    }
}
fn main() -> Int64 {
    Box(width: 2, height: 3).area()
}
";
        let rets = return_types_with_overlays("<input>", src, &HashMap::new());
        assert_eq!(
            rets.get("Box.area").map(String::as_str),
            Some("Int64"),
            "got {rets:?}"
        );
        assert_eq!(rets.get("Box.grow").map(String::as_str), Some("Void"));
    }

    #[test]
    fn return_types_include_inferred_trial_from_try() {
        use std::fs;
        // Imports need a real path so package load can find std/.
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let main = workspace.join("target/thoxa-test-return-infer/main.thx");
        let _ = fs::create_dir_all(main.parent().unwrap());
        let src = "\
import { Pass, Fail, Trial } from \"std/fallible\"
fn setup() -> Trial<Void, String> {
    Pass(Void)
}
fn runServer() {
    setup()?
    loop {
        return Pass(Void)
    }
}
fn main() -> Int64 {
    match runServer() {
        Pass(_) { 0 }
        Fail(_) { 1 }
    }
}
";
        fs::write(&main, src).unwrap();
        let rets = return_types_with_overlays(main.to_str().unwrap(), src, &HashMap::new());
        let run = rets.get("runServer").map(String::as_str);
        assert!(
            matches!(run, Some("Trial<Void, String>")),
            "expected Trial<Void, String>, got {run:?} full={rets:?}"
        );
        let _ = fs::remove_file(&main);
    }
}

#[cfg(test)]
mod recover_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn soft_parse_reports_undefined_after_keyword() {
        let path = PathBuf::from("/tmp/thoxa_recover_test.thx");
        let text = r#"
import { Pass } from "std/fallible"
fn main() -> Int64 {
    bar
    FOOBAR
    else
    0
}
"#;
        std::fs::write(&path, text).unwrap();
        let diags = check_source(path.to_str().unwrap(), text);
        let msgs: Vec<_> = diags.iter().map(|d| d.message.as_str()).collect();
        assert!(
            msgs.iter().any(|m| m.contains("undefined name `bar`")),
            "missing bar: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("undefined name `FOOBAR`")),
            "missing FOOBAR: {msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("found Else")),
            "missing Else: {msgs:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
