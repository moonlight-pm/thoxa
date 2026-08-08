//! Buffer → diagnostics via parse + MIR lower (no cgen).
//!
//! This is the check engine for `thoxa lsp` (and optional `thoxa check`). The
//! same frontend as `build`/`run` so diagnostics never drift.

use crate::ast::{Item, Module};
use crate::error::{Error, Result};
use crate::lexer::{Lexer, TokenKind};
use crate::loader::{self, Package, SourceModule};
use crate::mir;
use crate::parser::Parser;
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

/// One diagnostic ready for LSP publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    pub message: String,
    /// LSP range in the checked buffer (0-based, UTF-16 characters).
    pub range: Range,
    /// Original byte span into the buffer (when known).
    pub span: Span,
    /// Path / label passed to [`check_source`].
    pub filename: String,
}

/// Lex, parse, and type-check `text` without codegen.
///
/// Returns an empty vec on success, or one-or-more diagnostics. Independent
/// function/method body errors are collected together; earlier pipeline
/// failures (lex/parse/types) remain first-error only. Build/run still report
/// the primary error only.
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
    match check_source_inner(filename, text, overlays) {
        Ok(()) => Vec::new(),
        Err(e) => errors_to_diagnostics(filename, text, e),
    }
}

fn module_has_main(module: &Module) -> bool {
    module.items.iter().any(|item| {
        matches!(
            item,
            Item::Function(f) if f.name == "main"
        )
    })
}

fn check_source_inner(
    filename: &str,
    text: &str,
    overlays: &HashMap<PathBuf, String>,
) -> Result<()> {
    // Session command sugar (same as loader entry path).
    let text = crate::command::desugar_session_source(text)?;
    let tokens = Lexer::new(&text).tokenize()?;
    let module = Parser::new(tokens).parse_module()?;

    let has_imports = module
        .items
        .iter()
        .any(|item| matches!(item, Item::Import(_)));

    if !has_imports {
        // Single-file: session prepare (no-main scripts) then lower.
        let mut package = Package {
            project_root: PathBuf::from("."),
            modules: vec![SourceModule {
                id: "Main".into(),
                path: PathBuf::from(filename),
                ast: module,
                is_entry: true,
            }],
        };
        crate::session::prepare_package(&mut package)?;
        let has_main = package
            .modules
            .iter()
            .any(|m| m.is_entry && module_has_main(&m.ast));
        let opts = mir::LowerOptions {
            require_main: has_main,
        };
        mir::lower_package_with(&package, opts)?;
        return Ok(());
    }

    let path = Path::new(filename);
    if !path.is_file() {
        let span = first_import_span(&module).unwrap_or_else(Span::dummy);
        return Err(Error::at(
            "checking imports requires a real file path (open the file on disk for multi-file check)",
            span,
        ));
    }

    let has_main = module_has_main(&module);
    let mut package = loader::load_package(path)?;
    overlay_open_ast(&mut package, module, path, has_main);
    apply_open_overlays(&mut package, overlays, path);
    crate::session::prepare_package(&mut package)?;
    let has_main_after = package
        .modules
        .iter()
        .any(|m| m.is_entry && module_has_main(&m.ast));
    let opts = mir::LowerOptions {
        require_main: has_main_after,
    };
    mir::lower_package_with(&package, opts)?;
    Ok(())
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
    path.canonicalize().ok().or_else(|| Some(path.to_path_buf()))
}

fn first_import_span(module: &Module) -> Option<Span> {
    module.items.iter().find_map(|item| match item {
        Item::Import(imp) => Some(imp.path_span),
        _ => None,
    })
}

fn errors_to_diagnostics(filename: &str, text: &str, e: Error) -> Vec<CheckDiagnostic> {
    let mut out = Vec::with_capacity(1 + e.more.len());
    out.push(error_parts_to_diagnostic(
        filename,
        text,
        e.message,
        e.span,
    ));
    for extra in e.more {
        out.push(error_parts_to_diagnostic(
            filename,
            text,
            extra.message,
            extra.span,
        ));
    }
    out
}

fn error_parts_to_diagnostic(
    filename: &str,
    text: &str,
    message: String,
    span_opt: Option<Span>,
) -> CheckDiagnostic {
    let span = span_opt.unwrap_or_else(Span::dummy);
    let range = if span_opt.is_some() {
        span::span_to_range(text, span)
    } else {
        Range::default()
    };
    CheckDiagnostic {
        message,
        range,
        span,
        filename: filename.to_string(),
    }
}

/// Classify identifiers the grammar cannot resolve from casing alone.
///
/// - PascalCase → `type`
/// - camelCase before `(` → `function`
/// - other camelCase → `variable`
///
/// Keywords, strings, numbers, comments stay with tree-sitter (`combined` mode).
/// Lex failures yield an empty list (leave base coloring only).
pub fn semantic_tokens(text: &str) -> Vec<SemanticToken> {
    let tokens = match Lexer::new(text).tokenize() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for i in 0..tokens.len() {
        let TokenKind::Ident(name) = &tokens[i].kind else {
            continue;
        };
        let Some(first) = name.chars().next() else {
            continue;
        };
        let kind = if first.is_uppercase() {
            "type"
        } else {
            let next = tokens.get(i + 1).map(|t| &t.kind);
            if matches!(next, Some(TokenKind::LParen)) {
                "function"
            } else {
                "variable"
            }
        };
        let range = span::span_to_range(text, tokens[i].span);
        let slice = text
            .get(tokens[i].span.start..tokens[i].span.end)
            .unwrap_or(name.as_str());
        let length: u32 = slice.chars().map(|c| c.len_utf16() as u32).sum();
        out.push(SemanticToken {
            line: range.start.line,
            character: range.start.character,
            length,
            token_type: kind.into(),
            modifiers: 0,
        });
    }
    out
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

    #[test]
    fn good_hello_style() {
        let src = "fn main() -> Int64 {\n    println(\"Hello, world!\")\n    0\n}\n";
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
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.to_lowercase().contains("unexpected")
                || diags[0].message.contains('@'),
            "{}",
            diags[0].message
        );
        assert_ne!(diags[0].range, Range::default());
    }

    #[test]
    fn parse_error_one_diagnostic() {
        let src = "fn main() -> Int64 { if true\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.to_lowercase().contains("expected"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn import_without_real_path_clear_diagnostic() {
        let src = "import { foo } from \"./Helper\"\nfn main() -> Int64 { 0 }\n";
        let diags = check_source("<input>", src);
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0].message.contains("real file path")
                || diags[0].message.contains("multi-file"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn open_buffer_overlay_affects_check() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "thoxa-overlay-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        let main = dir.join("Main.thx");
        // Disk: helper returns Int64 (compatible).
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let main_src = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        // Open buffer: change helper to return Bool → type error at use site.
        let edited = "export fn answer() -> Bool { true }\n";
        let mut overlays = HashMap::new();
        overlays.insert(helper.clone(), edited.to_string());
        let diags = check_source_with_overlays(main.to_str().unwrap(), main_src, &overlays);
        assert_eq!(diags.len(), 1, "expected type error from overlay: {diags:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_with_disk_package_ok() {
        use std::fs;
        let dir = std::env::temp_dir().join(format!(
            "thoxa-check-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        let main = dir.join("Main.thx");
        fs::write(
            &helper,
            "export fn answer() -> Int64 { 42 }\n",
        )
        .unwrap();
        // Buffer has a type error in the open file; deps load from disk.
        let bad = "import { answer } from \"./Helper\"\nfn main() -> Int64 { if answer() { 0 } 0 }\n";
        fs::write(&main, "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n")
            .unwrap();
        let diags = check_source(main.to_str().unwrap(), bad);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("Bool"), "{}", diags[0].message);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_module_without_main_ok() {
        // Export-only modules must typecheck without `fn main` (LSP on Io.thx / Http.thx).
        let src = "export fn answer() -> Int64 { 42 }\nexport let n = 1\n";
        let diags = check_source("Helper.thx", src);
        assert!(diags.is_empty(), "library should not require main: {diags:?}");
    }

    #[test]
    fn library_module_still_reports_type_errors() {
        let src = "export fn bad() -> Int64 { if 3 { 0 } 0 }\n";
        let diags = check_source("Lib.thx", src);
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
        let dir = std::env::temp_dir().join(format!(
            "thoxa-lib-check-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        let lib = dir.join("Lib.thx");
        fs::write(&helper, "export fn answer() -> Int64 { 42 }\n").unwrap();
        let src = "import { answer } from \"./Helper\"\nexport fn use() -> Int64 { answer() }\n";
        fs::write(&lib, src).unwrap();
        let diags = check_source(lib.to_str().unwrap(), src);
        assert!(diags.is_empty(), "library with imports: {diags:?}");
        let _ = fs::remove_dir_all(&dir);
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
            diags[0].message.to_lowercase().contains("int64")
                || diags[0].message.contains("main"),
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
    fn semantic_tokens_fn_and_call() {
        let src = "fn main() -> Int64 { println(\"hi\") 0 }\n";
        let toks = semantic_tokens(src);
        let types: Vec<&str> = toks.iter().map(|t| t.token_type.as_str()).collect();
        // main + Int64 + println
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
}
