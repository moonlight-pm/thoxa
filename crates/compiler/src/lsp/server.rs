//! Thoxa language server: diagnostics, semantic tokens, and navigation over stdio.
//!
//! Full-document sync. Analysis failures log to stderr and never tear down the
//! message loop (handlers use `catch_unwind` around check/token work).

use crate::lsp::analysis::{self, check_source_with_overlays};
use crate::lsp::convert::{self, TOKEN_MODIFIERS, TOKEN_TYPES};
use crate::lsp::crossmodule::{self, resolve_remote};
use crate::lsp::index::{self, build_index, DocumentIndex};
use crate::lsp::protocol::{read_message, write_message};
use crate::resolver::BindingKind;
use crate::span;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

const METHOD_NOT_FOUND: i64 = -32601;
const INTERNAL_ERROR: i64 = -32603;

/// Run the language server on stdin/stdout. Returns process exit code.
pub fn run() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    match Server::new().serve(&mut reader, &mut writer) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("thoxa-lsp: transport error: {e}");
            1
        }
    }
}

pub struct Server {
    docs: HashMap<String, String>,
    shutdown_requested: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            docs: HashMap::new(),
            shutdown_requested: false,
        }
    }

    /// Drive the message loop until EOF or `exit`.
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
    ) -> io::Result<i32> {
        loop {
            let msg = match read_message(reader)? {
                Some(m) => m,
                None => return Ok(0),
            };
            let method = msg.get("method").and_then(|m| m.as_str()).map(str::to_string);
            if method.as_deref() == Some("exit") {
                return Ok(if self.shutdown_requested { 0 } else { 1 });
            }

            if let Err(e) = self.handle_message(&msg, writer) {
                eprintln!(
                    "thoxa-lsp: handler error in {:?}: {e}",
                    method.as_deref().unwrap_or("?")
                );
                if let Some(id) = msg.get("id").cloned() {
                    let _ = self.send_error(writer, id, INTERNAL_ERROR, &e.to_string());
                }
            }
        }
    }

    fn handle_message<W: Write>(&mut self, msg: &Value, writer: &mut W) -> io::Result<()> {
        let method = msg.get("method").and_then(|m| m.as_str());
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        if let Some(id) = id {
            let result = match method {
                Some("initialize") => self.initialize(&params),
                Some("shutdown") => {
                    self.shutdown_requested = true;
                    Value::Null
                }
                Some("textDocument/semanticTokens/full") => self.semantic_tokens_full(&params),
                Some("textDocument/definition") | Some("textDocument/declaration") => {
                    self.definition(&params)
                }
                Some("textDocument/references") => self.references(&params),
                Some("textDocument/hover") => self.hover(&params),
                Some("textDocument/documentSymbol") => self.document_symbol(&params),
                Some("textDocument/prepareRename") => self.prepare_rename(&params),
                Some("textDocument/rename") => self.rename(&params),
                Some(other) => {
                    self.send_error(
                        writer,
                        id,
                        METHOD_NOT_FOUND,
                        &format!("method not found: {other}"),
                    )?;
                    return Ok(());
                }
                None => {
                    self.send_error(writer, id, METHOD_NOT_FOUND, "missing method")?;
                    return Ok(());
                }
            };
            write_message(
                writer,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result,
                }),
            )?;
        } else {
            match method {
                Some("initialized") => {}
                Some("textDocument/didOpen") => self.did_open(&params, writer)?,
                Some("textDocument/didChange") => self.did_change(&params, writer)?,
                Some("textDocument/didClose") => self.did_close(&params, writer)?,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn initialize(&self, _params: &Value) -> Value {
        json!({
            "capabilities": {
                "textDocumentSync": 1,
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": TOKEN_TYPES,
                        "tokenModifiers": TOKEN_MODIFIERS,
                    },
                    "full": true,
                },
                "definitionProvider": true,
                "declarationProvider": true,
                "referencesProvider": true,
                "documentSymbolProvider": true,
                "hoverProvider": true,
                "renameProvider": { "prepareProvider": true },
            },
            "serverInfo": {
                "name": "thoxa-lsp",
                "version": env!("CARGO_PKG_VERSION"),
            },
        })
    }

    fn doc_index(&self, uri: &str) -> DocumentIndex {
        let text = self.docs.get(uri).map(|s| s.as_str()).unwrap_or("");
        let path = convert::check_filename(uri);
        match catch_unwind(AssertUnwindSafe(|| build_index(text, &path))) {
            Ok(idx) => idx,
            Err(panic) => {
                eprintln!("thoxa-lsp: index failed: {}", panic_message(&panic));
                DocumentIndex::default()
            }
        }
    }

    fn definition(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let (line, character) = convert::pos_from_params(params);
        let idx = self.doc_index(uri);
        let ref_ = idx.resolution.ref_at(&idx.text, line, character);
        let binding = idx
            .resolution
            .binding_of(ref_)
            .or_else(|| idx.resolution.binding_at(&idx.text, line, character));

        // Cross-module: jump into the defining module when the name is imported.
        if let Some(remote) = resolve_remote(&idx, ref_, binding, &self.docs) {
            if let Some(span) = remote.def_span {
                let remote_uri = convert::path_to_uri(&remote.path.to_string_lossy());
                return convert::location(&remote_uri, &remote.text, span);
            }
        }

        let Some(binding) = binding else {
            return Value::Null;
        };
        let Some(span) = binding.def_span else {
            return Value::Null; // builtin / no in-file def
        };
        convert::location(uri, &idx.text, span)
    }

    fn references(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let (line, character) = convert::pos_from_params(params);
        let include_decl = params["context"]["includeDeclaration"]
            .as_bool()
            .unwrap_or(true);
        let idx = self.doc_index(uri);
        let ref_ = idx.resolution.ref_at(&idx.text, line, character);
        let binding = idx
            .resolution
            .binding_of(ref_)
            .or_else(|| idx.resolution.binding_at(&idx.text, line, character));
        let Some(binding) = binding else {
            return json!([]);
        };
        let mut locs = Vec::new();
        if include_decl {
            if let Some(span) = binding.def_span {
                locs.push(convert::location(uri, &idx.text, span));
            }
        }
        for r in idx.resolution.refs_to(binding.id) {
            if let Some(slice) = idx.text.get(r.span.start..r.span.end) {
                if slice != binding.name {
                    continue;
                }
            }
            if !include_decl {
                if let Some(def) = binding.def_span {
                    if r.span == def {
                        continue;
                    }
                }
            }
            locs.push(convert::location(uri, &idx.text, r.span));
        }

        // Import: include defining-module declaration (open buffer preferred).
        if binding.kind == BindingKind::Import {
            if let Some(remote) = resolve_remote(&idx, ref_, Some(binding), &self.docs) {
                if let Some(span) = remote.def_span {
                    let remote_uri = convert::path_to_uri(&remote.path.to_string_lossy());
                    locs.push(convert::location(&remote_uri, &remote.text, span));
                }
            }
        }

        // Export: among *open* documents, collect uses of selective imports that
        // resolve back to this file's export (multi-file refs, open-buffer scope).
        if binding.exported
            && binding.owner.is_none()
            && binding.kind != BindingKind::Import
            && binding.def_span.is_some()
        {
            let my_path = PathBuf::from(convert::check_filename(uri));
            for (other_uri, other_text) in &self.docs {
                if other_uri == uri {
                    continue;
                }
                let other_path = convert::check_filename(other_uri);
                let other_idx = match catch_unwind(AssertUnwindSafe(|| {
                    build_index(other_text, &other_path)
                })) {
                    Ok(i) => i,
                    Err(_) => continue,
                };
                if !other_idx.ok {
                    continue;
                }
                for b in other_idx.resolution.bindings.values() {
                    if b.kind != BindingKind::Import {
                        continue;
                    }
                    let Some(origin) = b.import_origin.as_ref() else {
                        continue;
                    };
                    if origin.is_namespace {
                        continue;
                    }
                    let export_name = origin
                        .original
                        .clone()
                        .unwrap_or_else(|| b.name.clone());
                    if export_name != binding.name {
                        continue;
                    }
                    let Some(resolved) = crossmodule::resolve_import_file(
                        &origin.spec,
                        Path::new(&other_path),
                    ) else {
                        continue;
                    };
                    if !crossmodule::paths_equal_public(&resolved, &my_path) {
                        continue;
                    }
                    // Import binding site (local name) + uses.
                    if include_decl {
                        if let Some(span) = b.def_span {
                            locs.push(convert::location(other_uri, other_text, span));
                        }
                    }
                    for r in other_idx.resolution.refs_to(b.id) {
                        if let Some(slice) = other_text.get(r.span.start..r.span.end) {
                            if slice != b.name {
                                continue;
                            }
                        }
                        locs.push(convert::location(other_uri, other_text, r.span));
                    }
                }
            }
        }

        Value::Array(locs)
    }

    fn hover(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let (line, character) = convert::pos_from_params(params);
        let idx = self.doc_index(uri);
        let ref_ = idx.resolution.ref_at(&idx.text, line, character);
        let binding = idx
            .resolution
            .binding_of(ref_)
            .or_else(|| idx.resolution.binding_at(&idx.text, line, character));

        // Prefer local occurrence span for the hover range.
        let range = ref_
            .map(|r| span::span_to_range(&idx.text, r.span))
            .or_else(|| {
                binding
                    .and_then(|b| b.def_span)
                    .map(|s| span::span_to_range(&idx.text, s))
            });

        // Cross-module: show remote signature + docs when imported.
        if let Some(remote) = resolve_remote(&idx, ref_, binding, &self.docs) {
            let md = crossmodule::remote_hover_markdown(&remote);
            return convert::hover(&md, range);
        }

        let Some(binding) = binding else {
            return Value::Null;
        };
        let Some(md) = idx.hover_markdown(binding) else {
            return Value::Null;
        };
        convert::hover(&md, range)
    }

    fn document_symbol(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let idx = self.doc_index(uri);
        let mut out = Vec::new();
        for b in idx.resolution.module_definitions() {
            let Some(sel) = b.def_span else { continue };
            let selection = span::span_to_range(&idx.text, sel);
            // Whole-line range for the definition (good enough for outline).
            let range = selection;
            let mut children = Vec::new();
            if b.kind == BindingKind::Type {
                for m in idx.resolution.members_of(&b.name) {
                    let Some(ms) = m.def_span else { continue };
                    let mr = span::span_to_range(&idx.text, ms);
                    children.push(convert::document_symbol(
                        &m.name,
                        m.kind,
                        mr,
                        mr,
                        vec![],
                    ));
                }
            }
            out.push(convert::document_symbol(
                &b.name,
                b.kind,
                range,
                selection,
                children,
            ));
        }
        Value::Array(out)
    }

    fn prepare_rename(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let (line, character) = convert::pos_from_params(params);
        let idx = self.doc_index(uri);
        let Some(binding) = idx.binding_at(line, character) else {
            return Value::Null;
        };
        if !idx.renameable(binding) {
            return Value::Null;
        }
        let Some(span) = binding.def_span else {
            return Value::Null;
        };
        // Prefer occurrence under cursor if present.
        let span = idx
            .resolution
            .ref_at(&idx.text, line, character)
            .map(|r| r.span)
            .unwrap_or(span);
        let range = span::span_to_range(&idx.text, span);
        json!({
            "range": convert::range_to_json(range),
            "placeholder": binding.name,
        })
    }

    fn rename(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let (line, character) = convert::pos_from_params(params);
        let new_name = params["newName"].as_str().unwrap_or("");
        let idx = self.doc_index(uri);
        let Some(binding) = idx.binding_at(line, character) else {
            return Value::Null;
        };
        if !idx.renameable(binding) {
            return Value::Null;
        }
        if let Err(msg) = idx.valid_new_name(binding, new_name) {
            eprintln!("thoxa-lsp: rename rejected: {msg}");
            return Value::Null;
        }
        let spans = index::rename_spans(&idx, binding);
        let mut changes: Vec<(String, Vec<Value>)> = Vec::new();
        let local_edits: Vec<Value> = spans
            .into_iter()
            .map(|s| convert::text_edit(span::span_to_range(&idx.text, s), new_name))
            .collect();
        if !local_edits.is_empty() {
            changes.push((uri.to_string(), local_edits));
        }

        // Open-buffer multi-file: renaming an export also renames selective-import
        // local names + uses in other open docs (mirrors multi-file refs).
        if binding.exported
            && binding.owner.is_none()
            && binding.kind != BindingKind::Import
            && binding.def_span.is_some()
        {
            let my_path = PathBuf::from(convert::check_filename(uri));
            for (other_uri, other_text) in &self.docs {
                if other_uri == uri {
                    continue;
                }
                let other_path = convert::check_filename(other_uri);
                let other_idx = match catch_unwind(AssertUnwindSafe(|| {
                    build_index(other_text, &other_path)
                })) {
                    Ok(i) => i,
                    Err(_) => continue,
                };
                if !other_idx.ok {
                    continue;
                }
                for b in other_idx.resolution.bindings.values() {
                    if b.kind != BindingKind::Import {
                        continue;
                    }
                    let Some(origin) = b.import_origin.as_ref() else {
                        continue;
                    };
                    if origin.is_namespace {
                        continue;
                    }
                    let export_name = origin
                        .original
                        .clone()
                        .unwrap_or_else(|| b.name.clone());
                    if export_name != binding.name {
                        continue;
                    }
                    let Some(resolved) =
                        crossmodule::resolve_import_file(&origin.spec, Path::new(&other_path))
                    else {
                        continue;
                    };
                    if !crossmodule::paths_equal_public(&resolved, &my_path) {
                        continue;
                    }
                    // Rename the local import binding name + its uses (not the
                    // string inside `from "./…"`).
                    let spans = index::rename_spans(&other_idx, b);
                    let edits: Vec<Value> = spans
                        .into_iter()
                        .map(|s| {
                            convert::text_edit(span::span_to_range(other_text, s), new_name)
                        })
                        .collect();
                    if !edits.is_empty() {
                        changes.push((other_uri.clone(), edits));
                    }
                }
            }
        }

        if changes.is_empty() {
            return Value::Null;
        }
        convert::workspace_edit_changes(changes)
    }

    fn did_open<W: Write>(&mut self, params: &Value, writer: &mut W) -> io::Result<()> {
        let doc = &params["textDocument"];
        let uri = doc["uri"].as_str().unwrap_or("").to_string();
        let text = doc["text"].as_str().unwrap_or("").to_string();
        self.docs.insert(uri.clone(), text);
        self.publish(&uri, writer)
    }

    fn did_change<W: Write>(&mut self, params: &Value, writer: &mut W) -> io::Result<()> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if let Some(changes) = params["contentChanges"].as_array() {
            if let Some(last) = changes.last() {
                if let Some(text) = last["text"].as_str() {
                    self.docs.insert(uri.clone(), text.to_string());
                }
            }
        }
        self.publish(&uri, writer)
    }

    fn did_close<W: Write>(&mut self, params: &Value, writer: &mut W) -> io::Result<()> {
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_string();
        self.docs.remove(&uri);
        self.send_diagnostics(&uri, &[], writer)
    }

    fn semantic_tokens_full(&self, params: &Value) -> Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
        let text = self.docs.get(uri).map(|s| s.as_str()).unwrap_or("");
        let data = match catch_unwind(AssertUnwindSafe(|| {
            convert::encode_semantic_tokens(&analysis::semantic_tokens(text))
        })) {
            Ok(data) => data,
            Err(panic) => {
                eprintln!(
                    "thoxa-lsp: semantic tokens failed: {}",
                    panic_message(&panic)
                );
                Vec::new()
            }
        };
        json!({ "data": data })
    }

    fn publish<W: Write>(&self, uri: &str, writer: &mut W) -> io::Result<()> {
        let text = self.docs.get(uri).map(|s| s.as_str()).unwrap_or("");
        let path = convert::check_filename(uri);
        let overlays = self.open_overlays(uri);
        let diags = match catch_unwind(AssertUnwindSafe(|| {
            check_source_with_overlays(&path, text, &overlays)
        })) {
            Ok(d) => d,
            Err(panic) => {
                eprintln!(
                    "thoxa-lsp: diagnostics failed: {}",
                    panic_message(&panic)
                );
                Vec::new()
            }
        };
        let lsp_diags: Vec<Value> = diags.iter().map(convert::diagnostic_to_lsp).collect();
        self.send_diagnostics(uri, &lsp_diags, writer)
    }

    /// Path → buffer text for every open document except `except_uri` (the
    /// entry is already overlaid via the `text` argument to check).
    fn open_overlays(&self, except_uri: &str) -> HashMap<PathBuf, String> {
        let mut map = HashMap::new();
        for (uri, text) in &self.docs {
            if uri == except_uri {
                continue;
            }
            let p = convert::uri_to_path(uri);
            if p.as_os_str().is_empty() {
                continue;
            }
            // Prefer absolute paths; relative/`file://` untitled skip.
            map.insert(p, text.clone());
        }
        map
    }

    fn send_diagnostics<W: Write>(
        &self,
        uri: &str,
        diagnostics: &[Value],
        writer: &mut W,
    ) -> io::Result<()> {
        write_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": uri,
                    "diagnostics": diagnostics,
                },
            }),
        )
    }

    fn send_error<W: Write>(
        &self,
        writer: &mut W,
        id: Value,
        code: i64,
        message: &str,
    ) -> io::Result<()> {
        write_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": code,
                    "message": message,
                },
            }),
        )
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::protocol::read_message;
    use serde_json::json;
    use std::io::Cursor;

    fn drain_messages(buf: &[u8]) -> Vec<Value> {
        let mut cursor = Cursor::new(buf);
        let mut out = Vec::new();
        while let Some(m) = read_message(&mut cursor).unwrap() {
            out.push(m);
        }
        out
    }

    fn exchange(inputs: &[Value]) -> Vec<Value> {
        let mut input = Vec::new();
        for m in inputs {
            write_message(&mut input, m).unwrap();
        }
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let mut server = Server::new();
        let code = server.serve(&mut reader, &mut output).unwrap();
        assert_eq!(code, 0, "serve should exit 0 on EOF");
        drain_messages(&output)
    }

    #[test]
    fn initialize_then_initialized() {
        let msgs = exchange(&[
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }),
        ]);
        assert_eq!(msgs.len(), 1);
        let result = &msgs[0]["result"];
        assert_eq!(result["serverInfo"]["name"], "thoxa-lsp");
        assert_eq!(result["capabilities"]["textDocumentSync"], 1);
        assert_eq!(
            result["capabilities"]["semanticTokensProvider"]["full"],
            true
        );
        let types = result["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
            .as_array()
            .expect("tokenTypes");
        assert!(types.iter().any(|t| t == "function"));
        assert!(types.iter().any(|t| t == "type"));
        assert!(types.iter().any(|t| t == "variable"));
        assert_eq!(result["capabilities"]["definitionProvider"], true);
        assert_eq!(result["capabilities"]["hoverProvider"], true);
        assert_eq!(
            result["capabilities"]["renameProvider"]["prepareProvider"],
            true
        );
    }

    #[test]
    fn definition_and_hover_and_refs() {
        let uri = "file:///tmp/nav.thx";
        let text = "fn helper() -> Int64 { 1 }\nfn main() -> Int64 { helper() }\n";
        // Position of call-site `helper` (second occurrence).
        let call_byte = text.rfind("helper").unwrap();
        let p = span::byte_to_position(text, call_byte);
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": text
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/definition",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "textDocument/references",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character },
                    "context": { "includeDeclaration": true }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "textDocument/documentSymbol",
                "params": { "textDocument": { "uri": uri } }
            }),
        ]);
        let def = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        assert_eq!(def["result"]["uri"], uri);
        assert_eq!(def["result"]["range"]["start"]["line"], 0);

        let hover = msgs.iter().find(|m| m.get("id") == Some(&json!(3))).unwrap();
        let md = hover["result"]["contents"]["value"].as_str().unwrap();
        assert!(md.contains("helper"), "{md}");

        let refs = msgs.iter().find(|m| m.get("id") == Some(&json!(4))).unwrap();
        let arr = refs["result"].as_array().unwrap();
        assert!(arr.len() >= 2, "def + call: {arr:?}");

        let syms = msgs.iter().find(|m| m.get("id") == Some(&json!(5))).unwrap();
        let list = syms["result"].as_array().unwrap();
        let names: Vec<_> = list.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"main"));
    }

    #[test]
    fn method_member_definition() {
        // Typed-member nav: go-to-def on `p.len` jumps to the method on Point.
        let uri = "file:///tmp/member.thx";
        let text = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn main() -> Int64 {
  let p = Point(x: 1)
  p.len()
}
";
        let call_byte = text.rfind("len").unwrap();
        let p = span::byte_to_position(text, call_byte);
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": text
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/definition",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
        ]);
        let def = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        assert_eq!(def["result"]["uri"], uri, "{def}");
        // Method def is on the first `len` line (line 1 in 0-based).
        assert_eq!(def["result"]["range"]["start"]["line"], 1, "{def}");
    }

    #[test]
    fn rename_helper() {
        let uri = "file:///tmp/ren.thx";
        let text = "fn helper() -> Int64 { 1 }\nfn main() -> Int64 { helper() }\n";
        let call_byte = text.rfind("helper").unwrap();
        let p = span::byte_to_position(text, call_byte);
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": text
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/prepareRename",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/rename",
                "params": {
                    "textDocument": { "uri": uri },
                    "position": { "line": p.line, "character": p.character },
                    "newName": "assist"
                }
            }),
        ]);
        let prep = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        assert_eq!(prep["result"]["placeholder"], "helper");

        let ren = msgs.iter().find(|m| m.get("id") == Some(&json!(3))).unwrap();
        let edits = ren["result"]["changes"][uri].as_array().unwrap();
        assert!(edits.len() >= 2, "{edits:?}");
        assert!(edits.iter().all(|e| e["newText"] == "assist"));
    }

    #[test]
    fn did_open_type_error_publishes_diagnostic() {
        let uri = "file:///tmp/bad.thx";
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": "fn main() -> Int64 { if 3 { 0 } 0 }\n"
                    }
                }
            }),
        ]);
        let diags = msgs
            .iter()
            .find(|m| m.get("method").and_then(|x| x.as_str()) == Some("textDocument/publishDiagnostics"))
            .expect("publishDiagnostics");
        let list = diags["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert!(
            list[0]["message"].as_str().unwrap().contains("Bool"),
            "{}",
            list[0]["message"]
        );
        assert_eq!(diags["params"]["uri"], uri);
    }

    #[test]
    fn did_change_fix_clears_diagnostics() {
        let uri = "file:///tmp/fix.thx";
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": "fn main() -> Int64 { if 3 { 0 } 0 }\n"
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": uri, "version": 2 },
                    "contentChanges": [{
                        "text": "fn main() -> Int64 { 0 }\n"
                    }]
                }
            }),
        ]);
        let publishes: Vec<_> = msgs
            .iter()
            .filter(|m| m.get("method").and_then(|x| x.as_str()) == Some("textDocument/publishDiagnostics"))
            .collect();
        assert!(publishes.len() >= 2, "expected open + change publishes: {publishes:?}");
        let last = publishes.last().unwrap();
        let list = last["params"]["diagnostics"].as_array().unwrap();
        assert!(list.is_empty(), "fixed buffer should clear diags: {last}");
    }

    #[test]
    fn semantic_tokens_full_request() {
        let uri = "file:///tmp/tok.thx";
        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": "fn main() -> Int64 { println(\"x\") 0 }\n"
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/semanticTokens/full",
                "params": { "textDocument": { "uri": uri } }
            }),
        ]);
        let resp = msgs
            .iter()
            .find(|m| m.get("id") == Some(&json!(2)))
            .expect("semantic tokens response");
        let data = resp["result"]["data"].as_array().expect("data array");
        assert!(!data.is_empty(), "expected token data: {resp}");
        // 5 ints per token
        assert_eq!(data.len() % 5, 0);
    }

    #[test]
    fn unknown_method_returns_error() {
        let msgs = exchange(&[json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "workspace/symbol",
            "params": {}
        })]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn shutdown_then_exit_ok() {
        let mut input = Vec::new();
        write_message(
            &mut input,
            &json!({"jsonrpc":"2.0","id":1,"method":"shutdown","params":null}),
        )
        .unwrap();
        write_message(&mut input, &json!({"jsonrpc":"2.0","method":"exit"})).unwrap();
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let code = Server::new().serve(&mut reader, &mut output).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn cross_module_definition_and_hover() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_xmod_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        fs::write(
            &helper,
            "## the answer\nexport fn answer() -> Int64 { 42 }\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let text = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, text).unwrap();
        let main_uri = convert::path_to_uri(&main.to_string_lossy());
        let call = text.rfind("answer").unwrap();
        let p = span::byte_to_position(text, call);

        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": main_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": text
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/definition",
                "params": {
                    "textDocument": { "uri": main_uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": main_uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
        ]);

        let def = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let def_uri = def["result"]["uri"].as_str().unwrap();
        assert!(
            def_uri.ends_with("Helper.thx"),
            "expected jump into Helper, got {def_uri}"
        );
        assert_eq!(def["result"]["range"]["start"]["line"], 1); // export fn answer

        let hover = msgs.iter().find(|m| m.get("id") == Some(&json!(3))).unwrap();
        let md = hover["result"]["contents"]["value"].as_str().unwrap();
        assert!(md.contains("answer"), "{md}");
        assert!(md.contains("the answer") || md.contains("Int64"), "{md}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_rename_among_open_buffers() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_ren_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        let helper_src = "export fn answer() -> Int64 { 42 }\n";
        fs::write(&helper, helper_src).unwrap();
        let main = dir.join("Main.thx");
        let main_src = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
        let main_uri = convert::path_to_uri(&main.to_string_lossy());
        let def_byte = helper_src.find("answer").unwrap();
        let p = span::byte_to_position(helper_src, def_byte);

        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": helper_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": helper_src
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": main_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": main_src
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/rename",
                "params": {
                    "textDocument": { "uri": helper_uri },
                    "position": { "line": p.line, "character": p.character },
                    "newName": "result"
                }
            }),
        ]);

        let ren = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let changes = ren["result"]["changes"]
            .as_object()
            .expect("workspace edit changes");
        assert!(
            changes.keys().any(|k| k.ends_with("Helper.thx")),
            "{changes:?}"
        );
        assert!(
            changes.keys().any(|k| k.ends_with("Main.thx")),
            "{changes:?}"
        );
        // Main should rewrite import binding + call, not the path string.
        let main_edits = changes
            .iter()
            .find(|(k, _)| k.ends_with("Main.thx"))
            .map(|(_, v)| v.as_array().unwrap())
            .expect("main edits");
        assert!(main_edits.len() >= 2, "{main_edits:?}");
        for e in main_edits {
            assert_eq!(e["newText"], "result");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_refs_among_open_buffers() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_refs_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        let helper_src = "export fn answer() -> Int64 { 42 }\n";
        fs::write(&helper, helper_src).unwrap();
        let main = dir.join("Main.thx");
        let main_src = "import { answer } from \"./Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
        let main_uri = convert::path_to_uri(&main.to_string_lossy());
        // Cursor on export name in Helper.
        let def_byte = helper_src.find("answer").unwrap();
        let p = span::byte_to_position(helper_src, def_byte);

        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": helper_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": helper_src
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": main_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": main_src
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/references",
                "params": {
                    "textDocument": { "uri": helper_uri },
                    "position": { "line": p.line, "character": p.character },
                    "context": { "includeDeclaration": true }
                }
            }),
        ]);

        let refs = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let arr = refs["result"].as_array().expect("refs array");
        // At least: Helper def + Main import/use sites.
        assert!(
            arr.len() >= 2,
            "expected multi-file refs, got {}: {arr:?}",
            arr.len()
        );
        let uris: Vec<&str> = arr
            .iter()
            .filter_map(|l| l["uri"].as_str())
            .collect();
        assert!(
            uris.iter().any(|u| u.ends_with("Helper.thx")),
            "{uris:?}"
        );
        assert!(
            uris.iter().any(|u| u.ends_with("Main.thx")),
            "{uris:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hover_open_buffer_wins() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_buf_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let helper = dir.join("Helper.thx");
        fs::write(
            &helper,
            "export fn helper(x: Int64) -> Int64 { x }\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let text = "import { helper } from \"./Helper\"\nfn main() -> Int64 { helper(1) }\n";
        fs::write(&main, text).unwrap();
        let main_uri = convert::path_to_uri(&main.to_string_lossy());
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
        let edited = "export fn helper(x: Int64, y: Int64) -> Int64 { x }\n";
        let call = text.rfind("helper").unwrap();
        let p = span::byte_to_position(text, call);

        let msgs = exchange(&[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": main_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": text
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": helper_uri,
                        "languageId": "thoxa",
                        "version": 1,
                        "text": edited
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": main_uri },
                    "position": { "line": p.line, "character": p.character }
                }
            }),
        ]);
        let hover = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let md = hover["result"]["contents"]["value"].as_str().unwrap();
        assert!(md.contains("y"), "expected open-buffer signature: {md}");
        let _ = fs::remove_dir_all(&dir);
    }
}
