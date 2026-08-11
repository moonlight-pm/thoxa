//! Structured diagnostic IR (D124).
//!
//! One IR drives pretty `thoxa check`, `thoxa check --json`, and LSP.

use crate::error::{AdditionalDiagnostic, Error};
use crate::span::{self, Span};
use crate::version;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ── Codes (stable once published) ───────────────────────────────────────────

/// Undefined name / not in scope (general).
pub const CODE_UNDEFINED: &str = "THX001";
/// Type mismatch (`expected …, found …`).
pub const CODE_TYPE_MISMATCH: &str = "THX002";
/// Unknown type name.
pub const CODE_UNKNOWN_TYPE: &str = "THX003";
/// Fallible / `std/Fallible` import required.
pub const CODE_FALLIBLE_IMPORT: &str = "THX004";
/// Top-level statement in a program that defines `fn main`.
pub const CODE_TOP_LEVEL_WITH_MAIN: &str = "THX005";
/// Program requires `fn main`.
pub const CODE_REQUIRES_MAIN: &str = "THX006";
/// Lex / unexpected character.
pub const CODE_LEX: &str = "THX007";
/// Parse error (expected / unexpected token).
pub const CODE_PARSE: &str = "THX008";
/// Import / module resolution failure.
pub const CODE_IMPORT: &str = "THX009";
/// Imported name never referenced in this module.
pub const CODE_UNUSED_IMPORT: &str = "THX010";

/// Severity for a diagnostic (v0: Error primary; Warning for lints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }

    /// LSP DiagnosticSeverity (Error=1, Warning=2, Information=3, Hint=4).
    pub fn lsp_number(self) -> i32 {
        match self {
            Severity::Error => 1,
            Severity::Warning => 2,
            Severity::Note => 3,
        }
    }
}

/// Secondary span + message (`defined here`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub path: Option<PathBuf>,
    pub message: String,
}

impl Label {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            path: None,
            message: message.into(),
        }
    }
}

/// First-class diagnostic (shared by CLI + LSP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Option<String>,
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
    pub path: Option<PathBuf>,
    pub labels: Vec<Label>,
    pub help: Vec<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: None,
            severity: Severity::Error,
            message: message.into(),
            span: None,
            path: None,
            labels: Vec::new(),
            help: Vec::new(),
            expected: None,
            actual: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            code: None,
            severity: Severity::Warning,
            message: message.into(),
            span: None,
            path: None,
            labels: Vec::new(),
            help: Vec::new(),
            expected: None,
            actual: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Fill code/help/expected/actual from message patterns when not already set.
    pub fn enrich(mut self) -> Self {
        enrich_in_place(&mut self);
        self
    }

    /// True when this diagnostic has a stable code (after enrich).
    pub fn has_code(&self) -> bool {
        self.code.is_some()
    }

    /// Human message including help lines (for LSP clients without a help field).
    pub fn display_message(&self) -> String {
        if self.help.is_empty() {
            return self.message.clone();
        }
        let mut s = self.message.clone();
        for h in &self.help {
            s.push_str("\nhelp: ");
            s.push_str(h);
        }
        s
    }
}

// ── Error → Diagnostic ──────────────────────────────────────────────────────

/// Flatten an [`Error`] into independent structured diagnostics.
///
/// Each error / `more` entry may carry its own `path` (span file). When set, it
/// wins over `path_fallback` so multi-file package errors are not attributed to
/// the open buffer.
pub fn from_error(err: Error, path_fallback: Option<&Path>) -> Vec<Diagnostic> {
    let mut out = Vec::with_capacity(1 + err.more.len());
    let primary_path = err.path.as_deref().or(path_fallback);
    out.push(diag_from_parts(
        err.message,
        err.span,
        err.code,
        err.help,
        err.labels,
        err.expected,
        err.actual,
        primary_path,
    ));
    for m in err.more {
        let owned = m.path.clone();
        let p = owned.as_deref().or(path_fallback);
        out.push(from_additional(m, p));
    }
    out
}

fn from_additional(m: AdditionalDiagnostic, path: Option<&Path>) -> Diagnostic {
    diag_from_parts(
        m.message,
        m.span,
        m.code,
        m.help,
        m.labels,
        m.expected,
        m.actual,
        path,
    )
}

#[allow(clippy::too_many_arguments)]
fn diag_from_parts(
    message: String,
    span: Option<Span>,
    code: Option<&'static str>,
    help: Vec<String>,
    labels: Vec<Label>,
    expected: Option<String>,
    actual: Option<String>,
    path: Option<&Path>,
) -> Diagnostic {
    let mut d = Diagnostic {
        code: code.map(|c| c.to_string()),
        severity: Severity::Error,
        message,
        span,
        path: path.map(|p| p.to_path_buf()),
        labels,
        help,
        expected,
        actual,
    };
    enrich_in_place(&mut d);
    d
}

fn enrich_in_place(d: &mut Diagnostic) {
    let msg = d.message.as_str();

    if d.code.is_none() {
        if let Some(code) = classify_code(msg) {
            d.code = Some(code.to_string());
        }
    }

    if d.help.is_empty() {
        if let Some(helps) = default_help(d.code.as_deref(), msg) {
            d.help = helps;
        }
    }

    if d.expected.is_none() && d.actual.is_none() {
        if let Some((exp, act)) = parse_expected_found(msg) {
            d.expected = Some(exp);
            d.actual = Some(act);
        }
    }
}

fn classify_code(msg: &str) -> Option<&'static str> {
    let lower = msg.to_lowercase();
    if msg.contains("undefined name")
        || msg.contains("unknown function")
        || msg.contains("unknown method")
        || msg.contains("unknown static method")
        || msg.contains("unknown argument")
        || msg.contains("no field `")
        || msg.contains("no method `")
    {
        return Some(CODE_UNDEFINED);
    }
    if msg.contains("is not in scope") && msg.contains("std/Fallible") {
        return Some(CODE_FALLIBLE_IMPORT);
    }
    if msg.contains("unknown type") {
        return Some(CODE_UNKNOWN_TYPE);
    }
    if msg.contains("top-level statement") && msg.contains("fn main") {
        return Some(CODE_TOP_LEVEL_WITH_MAIN);
    }
    if msg.contains("program requires") && msg.contains("main") {
        return Some(CODE_REQUIRES_MAIN);
    }
    if lower.contains("unexpected character") || lower.starts_with("unexpected char") {
        return Some(CODE_LEX);
    }
    if msg.contains("cannot find")
        || msg.contains("empty local import")
        || msg.contains("empty project import")
        || msg.contains("empty std import")
        || msg.contains("import cycle")
        || (msg.contains("no such file") && (msg.contains("import") || msg.contains(".thx")))
    {
        return Some(CODE_IMPORT);
    }
    if msg.starts_with("unused import") {
        return Some(CODE_UNUSED_IMPORT);
    }
    // Parse `expect_kind` format first: `expected Colon, found Eq` (TokenKind
    // Debug). Must not fall into type-mismatch (which also says expected/found).
    if is_token_expect_found(msg)
        || lower.contains("unexpected token")
        || lower.contains("expected `")
        || lower.contains("unclosed")
        || msg.starts_with("expected identifier")
        || msg.starts_with("expected expression")
        || msg.starts_with("expected string literal")
    {
        return Some(CODE_PARSE);
    }
    // Type mismatch: expected/found, "must be Bool", return type, condition types.
    if (msg.contains("expected") && msg.contains("found"))
        || msg.contains("type mismatch")
        || msg.contains("type annotation mismatch")
        || msg.contains("must be Bool")
        || msg.contains("must return Int64")
        || msg.contains("must be an integer")
        || msg.contains("condition must be")
    {
        return Some(CODE_TYPE_MISMATCH);
    }
    if lower.starts_with("expected ") {
        return Some(CODE_PARSE);
    }
    None
}

/// `expected Colon, found Eq` / `expected Ident("x"), found Int(1)` from the parser.
fn is_token_expect_found(msg: &str) -> bool {
    let Some(rest) = msg.strip_prefix("expected ") else {
        return false;
    };
    let Some((exp, found)) = rest.split_once(", found ") else {
        return false;
    };
    is_token_kind_debug(exp.trim()) && is_token_kind_debug(found.trim())
}

fn is_token_kind_debug(s: &str) -> bool {
    // TokenKind Debug: bare variant (`Colon`) or with payload (`Ident("a")`, `Int(1)`).
    if s.is_empty() {
        return false;
    }
    // Payload form: Ident("…"), String("…"), Int(…), UInt(…), Float(…), …
    if let Some(i) = s.find('(') {
        let name = &s[..i];
        return name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_alphanumeric());
    }
    // Bare TokenKind names are PascalCase; exclude common type names that use the
    // same expected/found shape in MIR (`expected Int64, found Bool`).
    if matches!(
        s,
        "Int64"
            | "Int32"
            | "UInt32"
            | "UInt64"
            | "Bool"
            | "String"
            | "Bytes"
            | "Void"
            | "Byte"
            | "Float64"
    ) {
        return false;
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

fn default_help(code: Option<&str>, msg: &str) -> Option<Vec<String>> {
    match code {
        Some(CODE_UNDEFINED) => {
            if msg.contains("unknown function") || msg.contains("unknown method") {
                Some(vec![
                    "define the function/method or import it; check the spelling".into(),
                ])
            } else if msg.contains("no field") {
                Some(vec!["check the field name on this type".into()])
            } else {
                Some(vec![
                    "declare the name, import it, or fix the spelling".into(),
                ])
            }
        }
        Some(CODE_TYPE_MISMATCH) => {
            if msg.contains("must be Bool") || msg.contains("condition must be") {
                Some(vec!["use a Bool expression (comparisons yield Bool)".into()])
            } else {
                Some(vec![
                    "adjust the expression type or the annotation / expected type".into(),
                ])
            }
        }
        Some(CODE_UNKNOWN_TYPE) => Some(vec![
            "define the type in this module or import it".into(),
        ]),
        Some(CODE_FALLIBLE_IMPORT) => Some(vec![
            "add `import { Has, Not, May, Pass, Fail, Trial } from \"std/Fallible\"` \
             (or a selective subset)"
                .into(),
        ]),
        Some(CODE_TOP_LEVEL_WITH_MAIN) => Some(vec![
            "move the statement into `fn main` or remove `fn main` for a session script".into(),
        ]),
        Some(CODE_REQUIRES_MAIN) => Some(vec![
            "add `fn main() -> Int64 { … }` or check as a library (no entry required)".into(),
        ]),
        Some(CODE_LEX) => Some(vec!["remove or replace the unexpected character".into()]),
        Some(CODE_PARSE) => Some(vec!["fix the syntax near this location".into()]),
        Some(CODE_IMPORT) => Some(vec![
            "check the import path (`./`, `~/`, `std/…`) and that the file exists".into(),
        ]),
        Some(CODE_UNUSED_IMPORT) => Some(vec![
            "remove the unused name from the import list".into(),
        ]),
        _ => {
            if msg.contains("std/Fallible") {
                Some(vec![
                    "import the Fallible names you need from \"std/Fallible\"".into(),
                ])
            } else {
                None
            }
        }
    }
}

/// Parse `expected X, found Y` (and close variants) into structured fields.
fn parse_expected_found(msg: &str) -> Option<(String, String)> {
    // Patterns: "expected Foo, found Bar" / "expected Foo: found Bar" less common
    let lower = msg;
    let key = "expected ";
    let idx = lower.find(key)?;
    let rest = &lower[idx + key.len()..];
    let found_key = ", found ";
    let fidx = rest.find(found_key)?;
    let exp = rest[..fidx].trim().to_string();
    let act = rest[fidx + found_key.len()..].trim();
    // Trim trailing context after actual if needed (keep simple: whole remainder)
    let act = act
        .split(" (")
        .next()
        .unwrap_or(act)
        .trim()
        .trim_end_matches('.')
        .to_string();
    if exp.is_empty() || act.is_empty() {
        return None;
    }
    Some((exp, act))
}

// ── Pretty human format ─────────────────────────────────────────────────────

/// Pretty multi-line diagnostic for stderr (rustc-inspired).
pub fn format_pretty(diag: &Diagnostic, source: Option<&str>) -> String {
    let mut out = String::new();
    let sev = diag.severity.as_str();
    match &diag.code {
        Some(code) => {
            out.push_str(&format!("{sev}[{code}]: {}\n", diag.message));
        }
        None => {
            out.push_str(&format!("{sev}: {}\n", diag.message));
        }
    }

    let path_str = diag
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<input>".into());

    if let Some(span) = diag.span {
        let (line, col) = line_col_1based(source.unwrap_or(""), span.start);
        out.push_str(&format!(" --> {path_str}:{line}:{col}\n"));
        if let Some(src) = source {
            if let Some(snippet) = snippet_lines(src, span) {
                out.push_str(&snippet);
            }
        }
    } else {
        out.push_str(&format!(" --> {path_str}\n"));
    }

    if let (Some(exp), Some(act)) = (&diag.expected, &diag.actual) {
        out.push_str(&format!("  = note: expected `{exp}`\n"));
        out.push_str(&format!("           found    `{act}`\n"));
    }

    for h in &diag.help {
        out.push_str(&format!("  = help: {h}\n"));
    }

    for lab in &diag.labels {
        let lab_path = lab
            .path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| path_str.clone());
        let (line, col) = line_col_1based(source.unwrap_or(""), lab.span.start);
        out.push_str(&format!("  = note: {}\n", lab.message));
        out.push_str(&format!(" --> {lab_path}:{line}:{col}\n"));
    }

    out
}

fn line_col_1based(src: &str, byte: usize) -> (u32, u32) {
    let pos = span::byte_to_position(src, byte);
    (pos.line + 1, pos.character + 1)
}

/// Caret underline under the primary span on its source line.
fn snippet_lines(src: &str, span: Span) -> Option<String> {
    if src.is_empty() {
        return None;
    }
    let start = span.start.min(src.len());
    let end = span.end.min(src.len()).max(start);
    let pos = span::byte_to_position(src, start);
    let line_no = pos.line;
    let line_start = {
        let mut cur = 0u32;
        let mut i = 0usize;
        let bytes = src.as_bytes();
        while i < bytes.len() && cur < line_no {
            if bytes[i] == b'\n' {
                cur += 1;
            }
            i += 1;
        }
        i
    };
    let line_end = src[line_start..]
        .find('\n')
        .map(|o| line_start + o)
        .unwrap_or(src.len());
    let line_text = &src[line_start..line_end];
    // Skip pure empty lines for tiny spans at EOF
    if line_text.is_empty() && start >= src.len() {
        return None;
    }

    let col = pos.character as usize;
    let mark_len = {
        let s = start.saturating_sub(line_start);
        let e = end.saturating_sub(line_start).min(line_text.len());
        (e.saturating_sub(s)).max(1)
    };
    // Prefer UTF-8-safe underline length from span when span is on this line
    let underline = "^".repeat(mark_len.min(200).max(1));
    let pad = " ".repeat(col.min(200));
    let gutter = format!("{}", line_no + 1);
    let gw = gutter.len().max(2);
    let mut s = String::new();
    s.push_str(&format!("{:>gw$} |\n", "", gw = gw));
    s.push_str(&format!("{gutter:>gw$} | {line_text}\n"));
    s.push_str(&format!("{:>gw$} | {pad}{underline}\n", "", gw = gw));
    Some(s)
}

// ── JSON report ─────────────────────────────────────────────────────────────

/// Full `thoxa check --json` document.
pub fn format_json_report(diags: &[Diagnostic], source: Option<&str>) -> Value {
    let has_error = diags.iter().any(|d| d.severity == Severity::Error);
    let path_disp = |p: &Option<PathBuf>| {
        p.as_ref()
            .map(|x| x.display().to_string())
            .unwrap_or_else(|| "<input>".into())
    };
    let compiler_path = version::executable_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown path)".into());

    let diagnostics: Vec<Value> = diags
        .iter()
        .map(|d| diag_to_json(d, source, &path_disp(&d.path)))
        .collect();

    json!({
        "ok": !has_error,
        "compiler": {
            "version": version::version_string(),
            "path": compiler_path,
        },
        "diagnostics": diagnostics,
    })
}

fn diag_to_json(d: &Diagnostic, source: Option<&str>, path_fallback: &str) -> Value {
    let path = d
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| path_fallback.to_string());

    let (line, column, end_line, end_column, byte_start, byte_end) = match d.span {
        Some(span) => {
            let src = source.unwrap_or("");
            let r = span::span_to_range(src, span);
            (
                json!(r.start.line + 1),
                json!(r.start.character + 1),
                json!(r.end.line + 1),
                json!(r.end.character + 1),
                json!(span.start),
                json!(span.end),
            )
        }
        None => (
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ),
    };

    let related: Vec<Value> = d
        .labels
        .iter()
        .map(|lab| {
            let lab_path = lab
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| path.clone());
            let src = source.unwrap_or("");
            let r = span::span_to_range(src, lab.span);
            json!({
                "path": lab_path,
                "line": r.start.line + 1,
                "column": r.start.character + 1,
                "byte_start": lab.span.start,
                "byte_end": lab.span.end,
                "message": lab.message,
            })
        })
        .collect();

    json!({
        "code": d.code,
        "severity": d.severity.as_str(),
        "message": d.message,
        "path": path,
        "byte_start": byte_start,
        "byte_end": byte_end,
        "line": line,
        "column": column,
        "end_line": end_line,
        "end_column": end_column,
        "help": d.help,
        "expected": d.expected,
        "actual": d.actual,
        "related": related,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    #[test]
    fn classify_undefined_name() {
        let d = Diagnostic::error("undefined name `fooo`").enrich();
        assert_eq!(d.code.as_deref(), Some(CODE_UNDEFINED));
        assert!(!d.help.is_empty());
    }

    #[test]
    fn classify_parse_token_expect_not_type_mismatch() {
        let d = Diagnostic::error("expected Colon, found Eq").enrich();
        assert_eq!(d.code.as_deref(), Some(CODE_PARSE), "{d:?}");
        assert!(
            d.help.iter().any(|h| h.contains("syntax")),
            "parse help, got {:?}",
            d.help
        );
    }

    #[test]
    fn classify_type_mismatch_expected_found() {
        let d = Diagnostic::error("expected Bool, found Int64").enrich();
        assert_eq!(d.code.as_deref(), Some(CODE_TYPE_MISMATCH));
        assert_eq!(d.expected.as_deref(), Some("Bool"));
        assert_eq!(d.actual.as_deref(), Some("Int64"));
    }

    #[test]
    fn classify_fallible_import() {
        let d = Diagnostic::error(
            "`May` is not in scope — import it from \"std/Fallible\" (e.g. `import …`)",
        )
        .enrich();
        assert_eq!(d.code.as_deref(), Some(CODE_FALLIBLE_IMPORT));
    }

    #[test]
    fn classify_top_level_with_main() {
        let d = Diagnostic::error(
            "unexpected top-level statement in a program with `fn main` \
             (move it into main or use a session script without main)",
        )
        .enrich();
        assert_eq!(d.code.as_deref(), Some(CODE_TOP_LEVEL_WITH_MAIN));
    }

    #[test]
    fn pretty_includes_code_and_help() {
        let src = "fn main() -> Int64 { fooo() }\n";
        let start = src.find("fooo").unwrap();
        let d = Diagnostic {
            code: Some(CODE_UNDEFINED.into()),
            severity: Severity::Error,
            message: "undefined name `fooo`".into(),
            span: Some(Span::new(start, start + 4)),
            path: Some(PathBuf::from("Main.thx")),
            labels: vec![],
            help: vec!["declare the name, import it, or fix the spelling".into()],
            expected: None,
            actual: None,
        };
        let text = format_pretty(&d, Some(src));
        assert!(text.contains("error[THX001]"), "{text}");
        assert!(text.contains("Main.thx:"), "{text}");
        assert!(text.contains("help:"), "{text}");
        assert!(text.contains("^^^^") || text.contains("^"), "{text}");
    }

    #[test]
    fn json_report_shape() {
        let src = "fn main() -> Int64 { 0 }\n";
        let d = Diagnostic::error("undefined name `x`")
            .with_path("t.thx")
            .enrich();
        let mut d = d;
        d.span = Some(Span::new(0, 1));
        let report = format_json_report(&[d], Some(src));
        assert_eq!(report["ok"], false);
        assert!(report["compiler"]["version"].as_str().unwrap().contains('+'));
        assert_eq!(report["diagnostics"][0]["code"], CODE_UNDEFINED);
        assert_eq!(report["diagnostics"][0]["severity"], "error");
        assert!(report["diagnostics"][0]["line"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn from_error_flattens_more() {
        let e = Error::at("undefined name `a`", Span::new(0, 1)).with_more(vec![
            AdditionalDiagnostic {
                message: "undefined name `b`".into(),
                span: Some(Span::new(2, 3)),
                path: None,
                code: None,
                help: vec![],
                labels: vec![],
                expected: None,
                actual: None,
            },
        ]);
        let diags = from_error(e, Some(Path::new("x.thx")));
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].code.as_deref(), Some(CODE_UNDEFINED));
        assert_eq!(diags[1].code.as_deref(), Some(CODE_UNDEFINED));
        assert_eq!(diags[0].path.as_deref(), Some(Path::new("x.thx")));
    }
}
