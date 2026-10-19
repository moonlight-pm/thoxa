//! Pure mappings from Thoxa check/token model to LSP wire JSON.

use crate::lsp::analysis::{CheckDiagnostic, SemanticToken};
use crate::resolver::BindingKind;
use crate::span::{self, Position, Range, Span};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Standard LSP semantic token types (stable legend indices).
/// We emit only `type` / `function` / `variable` in phase 1.
pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "event",
    "function",
    "method",
    "macro",
    "keyword",
    "modifier",
    "comment",
    "string",
    "number",
    "regexp",
    "operator",
    "decorator",
];

pub const TOKEN_MODIFIERS: &[&str] = &[];

const SEVERITY_ERROR: i32 = 1;
const SEVERITY_WARNING: i32 = 2;
const SEVERITY_INFORMATION: i32 = 3;

fn token_type_index(name: &str) -> Option<u32> {
    TOKEN_TYPES
        .iter()
        .position(|&t| t == name)
        .map(|i| i as u32)
}

/// Map a local filesystem path to a `file://` URI (or leave non-absolute alone).
///
/// Spaces are percent-encoded so the URI is valid; other characters pass through.
pub fn path_to_uri(path: &str) -> String {
    if path.starts_with('/') {
        let encoded = path.replace(' ', "%20");
        format!("file://{encoded}")
    } else {
        path.to_string()
    }
}

/// Map an LSP DocumentUri to a local path for import resolution.
pub fn uri_to_path(uri: &str) -> PathBuf {
    if let Some(rest) = uri.strip_prefix("file://") {
        // file:///home/… → /home/… ; leave percent-encoding simple for phase 1
        let path = percent_decode(rest);
        PathBuf::from(path)
    } else {
        PathBuf::from(uri)
    }
}

fn percent_decode(s: &str) -> String {
    // Minimal decode for common path chars; full URL decoding deferred.
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push(char::from(hi * 16 + lo));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn range_to_json(range: Range) -> Value {
    json!({
        "start": position_to_json(range.start),
        "end": position_to_json(range.end),
    })
}

fn position_to_json(pos: Position) -> Value {
    json!({
        "line": pos.line,
        "character": pos.character,
    })
}

/// Compiler diagnostic → LSP Diagnostic object (D124: code + relatedInformation).
pub fn diagnostic_to_lsp(diag: &CheckDiagnostic) -> Value {
    // LSP has no standard `help` field — fold help into the message.
    let mut message = diag.message.clone();
    for h in &diag.help {
        message.push_str("\nhelp: ");
        message.push_str(h);
    }
    if let (Some(exp), Some(act)) = (&diag.expected, &diag.actual) {
        if !message.contains(exp) || !message.contains(act) {
            message.push_str(&format!("\nexpected `{exp}`, found `{act}`"));
        }
    }

    let severity = match diag.severity {
        crate::diagnostic::Severity::Error => SEVERITY_ERROR,
        crate::diagnostic::Severity::Warning => SEVERITY_WARNING,
        crate::diagnostic::Severity::Note => SEVERITY_INFORMATION,
    };
    let mut obj = json!({
        "range": range_to_json(diag.range),
        "severity": severity,
        "source": "thoxa",
        "message": message,
    });
    if let Some(code) = &diag.code {
        obj["code"] = json!(code);
    }
    if !diag.related.is_empty() {
        let related: Vec<Value> = diag
            .related
            .iter()
            .map(|r| {
                let uri = path_to_uri(&r.filename);
                json!({
                    "location": {
                        "uri": uri,
                        "range": range_to_json(r.range),
                    },
                    "message": r.message,
                })
            })
            .collect();
        obj["relatedInformation"] = json!(related);
    }
    obj
}

/// Encode semantic tokens as the LSP relative-delta `data` array.
///
/// Each token is `(line, char, length_utf16, type_name, modifiers)`.
/// Sorted by `(line, char)` before encoding.
pub fn encode_semantic_tokens(tokens: &[SemanticToken]) -> Vec<u32> {
    let mut sorted: Vec<&SemanticToken> = tokens.iter().collect();
    sorted.sort_by_key(|t| (t.line, t.character));

    let mut data = Vec::with_capacity(sorted.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for t in sorted {
        let Some(type_idx) = token_type_index(&t.token_type) else {
            continue;
        };
        let delta_line = t.line.saturating_sub(prev_line);
        let delta_char = if delta_line == 0 {
            t.character.saturating_sub(prev_char)
        } else {
            t.character
        };
        data.push(delta_line);
        data.push(delta_char);
        data.push(t.length);
        data.push(type_idx);
        data.push(t.modifiers);
        prev_line = t.line;
        prev_char = t.character;
    }
    data
}

/// Filename hint for `check_source` from a document URI.
pub fn check_filename(uri: &str) -> String {
    let path = uri_to_path(uri);
    if path.as_os_str().is_empty() {
        uri.to_string()
    } else {
        path.to_string_lossy().into_owned()
    }
}

pub fn path_is_file(path: &Path) -> bool {
    path.is_file()
}

/// LSP Location for a span in `src` under `uri`.
pub fn location(uri: &str, src: &str, span: Span) -> Value {
    json!({
        "uri": uri,
        "range": range_to_json(span::span_to_range(src, span)),
    })
}

/// Markdown hover (optional range for the hovered name).
pub fn hover(markdown: &str, range: Option<Range>) -> Value {
    let mut out = json!({
        "contents": {
            "kind": "markdown",
            "value": markdown,
        }
    });
    if let Some(r) = range {
        out["range"] = range_to_json(r);
    }
    out
}

/// LSP SymbolKind for a binding kind.
pub fn symbol_kind(kind: BindingKind) -> i32 {
    match kind {
        BindingKind::Fn | BindingKind::Extern => 12, // Function
        BindingKind::Method => 6,                    // Method
        BindingKind::Type => 23,                     // Struct
        BindingKind::Trait => 11,                    // Interface
        BindingKind::Field => 8,                     // Field
        BindingKind::Param
        | BindingKind::Let
        | BindingKind::Mut
        | BindingKind::ForVar
        | BindingKind::Pattern => 13, // Variable
        BindingKind::Import => 2,                    // Module
        BindingKind::Builtin => 14,                  // Constant
    }
}

pub fn document_symbol(
    name: &str,
    kind: BindingKind,
    range: Range,
    selection_range: Range,
    children: Vec<Value>,
) -> Value {
    json!({
        "name": name,
        "kind": symbol_kind(kind),
        "range": range_to_json(range),
        "selectionRange": range_to_json(selection_range),
        "children": children,
    })
}

pub fn text_edit(range: Range, new_text: &str) -> Value {
    json!({
        "range": range_to_json(range),
        "newText": new_text,
    })
}

pub fn workspace_edit(uri: &str, edits: Vec<Value>) -> Value {
    workspace_edit_changes(vec![(uri.to_string(), edits)])
}

/// WorkspaceEdit with edits across multiple document URIs.
pub fn workspace_edit_changes(changes: Vec<(String, Vec<Value>)>) -> Value {
    let mut map = serde_json::Map::new();
    for (uri, edits) in changes {
        if edits.is_empty() {
            continue;
        }
        map.insert(uri, Value::Array(edits));
    }
    json!({ "changes": Value::Object(map) })
}

/// Extract 0-based line/character from LSP Position params.
pub fn pos_from_params(params: &Value) -> (u32, u32) {
    let line = params["position"]["line"].as_u64().unwrap_or(0) as u32;
    let character = params["position"]["character"].as_u64().unwrap_or(0) as u32;
    (line, character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::analysis::SemanticToken;
    use crate::span::{Position, Range, Span};

    #[test]
    fn encode_tokens_relative() {
        let tokens = vec![
            SemanticToken {
                line: 0,
                character: 3,
                length: 4,
                token_type: "function".into(),
                modifiers: 0,
            },
            SemanticToken {
                line: 0,
                character: 0,
                length: 2,
                token_type: "type".into(),
                modifiers: 0,
            },
            SemanticToken {
                line: 1,
                character: 2,
                length: 1,
                token_type: "variable".into(),
                modifiers: 0,
            },
        ];
        // After sort: type@0:0, function@0:3, variable@1:2
        let data = encode_semantic_tokens(&tokens);
        let type_idx = token_type_index("type").unwrap();
        let fn_idx = token_type_index("function").unwrap();
        let var_idx = token_type_index("variable").unwrap();
        assert_eq!(
            data,
            vec![
                0, 0, 2, type_idx, 0, // first
                0, 3, 4, fn_idx, 0, // same line, delta char 3
                1, 2, 1, var_idx, 0, // next line
            ]
        );
    }

    #[test]
    fn diagnostic_json_shape() {
        let d = CheckDiagnostic {
            message: "boom".into(),
            range: Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 1,
                    character: 5,
                },
            },
            span: Span::new(10, 13),
            filename: "x.thx".into(),
            code: Some("THX001".into()),
            severity: crate::diagnostic::Severity::Error,
            help: vec!["try this".into()],
            expected: None,
            actual: None,
            related: vec![],
        };
        let j = diagnostic_to_lsp(&d);
        assert_eq!(j["severity"], 1);
        assert_eq!(j["source"], "thoxa");
        assert!(j["message"].as_str().unwrap().contains("boom"));
        assert!(j["message"].as_str().unwrap().contains("help: try this"));
        assert_eq!(j["code"], "THX001");
        assert_eq!(j["range"]["start"]["line"], 1);
        assert_eq!(j["range"]["start"]["character"], 2);
    }

    #[test]
    fn uri_roundtrip_unix() {
        let uri = "file:///home/me/proj/main.thx";
        let path = uri_to_path(uri);
        assert_eq!(path, PathBuf::from("/home/me/proj/main.thx"));
        assert_eq!(path_to_uri(path.to_str().unwrap()), uri);
    }

    #[test]
    fn path_to_uri_encodes_spaces() {
        let p = "/tmp/a b/c.thx";
        let uri = path_to_uri(p);
        assert!(!uri.contains(' '), "{uri}");
        assert_eq!(uri_to_path(&uri), PathBuf::from(p));
    }

    #[test]
    fn path_to_uri_passes_through_non_absolute() {
        assert_eq!(path_to_uri("<input>"), "<input>");
        assert_eq!(path_to_uri("relative/x.thx"), "relative/x.thx");
    }
}
