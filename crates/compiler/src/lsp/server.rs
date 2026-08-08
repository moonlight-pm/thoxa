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
use crate::span::{self, Span};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
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
                Some("workspace/symbol") => self.workspace_symbol(&params),
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
                "workspaceSymbolProvider": true,
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

        // Export: clients that selective-import this export (open buffers first,
        // then package-tree `.thx` on disk under Main.thx / .git root).
        if binding.exported
            && binding.owner.is_none()
            && binding.kind != BindingKind::Import
            && binding.def_span.is_some()
        {
            let my_path = PathBuf::from(convert::check_filename(uri));
            locs.extend(self.export_client_locations(
                &binding.name,
                &my_path,
                uri,
                include_decl,
            ));
        }

        Value::Array(locs)
    }

    /// Locations in other modules that selective-import `export_name` from `my_path`.
    fn export_client_locations(
        &self,
        export_name: &str,
        my_path: &Path,
        skip_uri: &str,
        include_decl: bool,
    ) -> Vec<Value> {
        let mut locs = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();

        // Open buffers win (may be unsaved).
        for (other_uri, other_text) in &self.docs {
            if other_uri == skip_uri {
                continue;
            }
            let other_path = convert::check_filename(other_uri);
            seen_paths.insert(other_path.clone());
            locs.extend(import_uses_of_export(
                other_uri,
                other_text,
                Path::new(&other_path),
                my_path,
                export_name,
                include_decl,
            ));
        }

        // Package-tree disk scan for `.thx` not already open.
        for p in package_disk_thx_files(my_path) {
            if crossmodule::paths_equal_public(&p, my_path) {
                continue;
            }
            let path_s = p.to_string_lossy().into_owned();
            if seen_paths.contains(&path_s) {
                continue;
            }
            // Also skip if an open buffer is this file under another URI form.
            let already_open = self.docs.keys().any(|u| {
                let op = convert::check_filename(u);
                crossmodule::paths_equal_public(Path::new(&op), &p)
            });
            if already_open {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else {
                continue;
            };
            let uri = convert::path_to_uri(&path_s);
            locs.extend(import_uses_of_export(
                &uri,
                &text,
                &p,
                my_path,
                export_name,
                include_decl,
            ));
        }

        locs
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

    /// Package-wide symbol search: open buffers + package-tree disk `.thx`.
    fn workspace_symbol(&self, params: &Value) -> Value {
        let query = params["query"].as_str().unwrap_or("").to_lowercase();
        let mut out: Vec<Value> = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();

        // Open buffers first (may be unsaved).
        for (uri, text) in &self.docs {
            let path = convert::check_filename(uri);
            seen_paths.insert(path.clone());
            collect_workspace_symbols(uri, text, &path, &query, &mut out);
            if out.len() >= WORKSPACE_SYMBOL_LIMIT {
                return Value::Array(out);
            }
        }

        // Package-tree disk modules under each open file's package walk root.
        for uri in self.docs.keys() {
            let path = PathBuf::from(convert::check_filename(uri));
            for p in package_disk_thx_files(&path) {
                let path_s = p.to_string_lossy().into_owned();
                if seen_paths.contains(&path_s) {
                    continue;
                }
                let already = self.docs.keys().any(|u| {
                    let op = convert::check_filename(u);
                    crossmodule::paths_equal_public(Path::new(&op), &p)
                });
                if already {
                    continue;
                }
                seen_paths.insert(path_s.clone());
                let Ok(text) = fs::read_to_string(&p) else {
                    continue;
                };
                let file_uri = convert::path_to_uri(&path_s);
                collect_workspace_symbols(&file_uri, &text, &path_s, &query, &mut out);
                if out.len() >= WORKSPACE_SYMBOL_LIMIT {
                    return Value::Array(out);
                }
            }
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

        // Multi-file: rename export → selective-import local names in open
        // buffers and package-tree disk modules (mirrors find-refs).
        if binding.exported
            && binding.owner.is_none()
            && binding.kind != BindingKind::Import
            && binding.def_span.is_some()
        {
            let my_path = PathBuf::from(convert::check_filename(uri));
            changes.extend(self.export_client_renames(&binding.name, &my_path, uri, new_name));
        }

        if changes.is_empty() {
            return Value::Null;
        }
        convert::workspace_edit_changes(changes)
    }

    fn export_client_renames(
        &self,
        export_name: &str,
        my_path: &Path,
        skip_uri: &str,
        new_name: &str,
    ) -> Vec<(String, Vec<Value>)> {
        let mut changes = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();

        for (other_uri, other_text) in &self.docs {
            if other_uri == skip_uri {
                continue;
            }
            let other_path = convert::check_filename(other_uri);
            seen_paths.insert(other_path.clone());
            if let Some(edits) = import_rename_edits(
                other_uri,
                other_text,
                Path::new(&other_path),
                my_path,
                export_name,
                new_name,
            ) {
                changes.push((other_uri.clone(), edits));
            }
        }

        for p in package_disk_thx_files(my_path) {
            if crossmodule::paths_equal_public(&p, my_path) {
                continue;
            }
            let path_s = p.to_string_lossy().into_owned();
            if seen_paths.contains(&path_s) {
                continue;
            }
            let already_open = self.docs.keys().any(|u| {
                let op = convert::check_filename(u);
                crossmodule::paths_equal_public(Path::new(&op), &p)
            });
            if already_open {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else {
                continue;
            };
            let uri = convert::path_to_uri(&path_s);
            if let Some(edits) =
                import_rename_edits(&uri, &text, &p, my_path, export_name, new_name)
            {
                changes.push((uri, edits));
            }
        }
        changes
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

/// Cap on disk `.thx` files visited for multi-file refs/rename (avoid home-tree scans).
const PACKAGE_DISK_THX_LIMIT: usize = 256;

/// Cap on workspace/symbol results.
const WORKSPACE_SYMBOL_LIMIT: usize = 100;

/// Collect module-level (and type member) symbols matching `query` (case-insensitive
/// substring; empty query matches all). Appends to `out` up to [`WORKSPACE_SYMBOL_LIMIT`].
fn collect_workspace_symbols(
    uri: &str,
    text: &str,
    path: &str,
    query: &str,
    out: &mut Vec<Value>,
) {
    if out.len() >= WORKSPACE_SYMBOL_LIMIT {
        return;
    }
    let idx = match catch_unwind(AssertUnwindSafe(|| build_index(text, path))) {
        Ok(i) => i,
        Err(_) => return,
    };
    if !idx.ok {
        return;
    }
    let container = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    for b in idx.resolution.module_definitions() {
        if out.len() >= WORKSPACE_SYMBOL_LIMIT {
            return;
        }
        // Prefer real product symbols: skip pure import re-exports as container-less noise.
        if !matches!(
            b.kind,
            BindingKind::Fn
                | BindingKind::Extern
                | BindingKind::Type
                | BindingKind::Let
                | BindingKind::Mut
        ) {
            continue;
        }
        let Some(sel) = b.def_span else { continue };
        if !query.is_empty() && !b.name.to_lowercase().contains(query) {
            // Still check type members below.
            if b.kind == BindingKind::Type {
                for m in idx.resolution.members_of(&b.name) {
                    if out.len() >= WORKSPACE_SYMBOL_LIMIT {
                        return;
                    }
                    if !query.is_empty() && !m.name.to_lowercase().contains(query) {
                        continue;
                    }
                    let Some(ms) = m.def_span else { continue };
                    out.push(workspace_symbol_info(
                        &m.name,
                        m.kind,
                        uri,
                        text,
                        ms,
                        &b.name,
                    ));
                }
            }
            continue;
        }
        out.push(workspace_symbol_info(
            &b.name,
            b.kind,
            uri,
            text,
            sel,
            &container,
        ));
        if b.kind == BindingKind::Type {
            for m in idx.resolution.members_of(&b.name) {
                if out.len() >= WORKSPACE_SYMBOL_LIMIT {
                    return;
                }
                if !query.is_empty() && !m.name.to_lowercase().contains(query) {
                    continue;
                }
                let Some(ms) = m.def_span else { continue };
                out.push(workspace_symbol_info(
                    &m.name,
                    m.kind,
                    uri,
                    text,
                    ms,
                    &b.name,
                ));
            }
        }
    }
}

fn workspace_symbol_info(
    name: &str,
    kind: BindingKind,
    uri: &str,
    text: &str,
    def_span: Span,
    container: &str,
) -> Value {
    let range = span::span_to_range(text, def_span);
    json!({
        "name": name,
        "kind": convert::symbol_kind(kind),
        "location": {
            "uri": uri,
            "range": convert::range_to_json(range),
        },
        "containerName": container,
    })
}

/// Nearest package-ish root above `export_path`: ancestor with `Main.thx` or `.git`,
/// else the export's parent directory (same-dir fallback).
fn package_walk_root(export_path: &Path) -> Option<PathBuf> {
    let start = export_path.parent()?.to_path_buf();
    let mut dir = start.clone();
    for _ in 0..24 {
        if dir.join("Main.thx").is_file() || dir.join(".git").exists() {
            return Some(dir);
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p.to_path_buf(),
            _ => break,
        }
    }
    Some(start)
}

fn skip_walk_dir(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | "node_modules"
            | "third_party"
            | "apocrypha"
            | ".grok"
            | ".cache"
            | "vendor"
    ) || name.starts_with('.')
}

/// Recursive `.thx` files under the package walk root for `export_path`.
fn package_disk_thx_files(export_path: &Path) -> Vec<PathBuf> {
    let Some(root) = package_walk_root(export_path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        if out.len() >= PACKAGE_DISK_THX_LIMIT {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for ent in entries.flatten() {
            if out.len() >= PACKAGE_DISK_THX_LIMIT {
                break;
            }
            let p = ent.path();
            let Ok(ft) = ent.file_type() else {
                continue;
            };
            if ft.is_dir() {
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if skip_walk_dir(&name) {
                    continue;
                }
                stack.push(p);
            } else if ft.is_file() {
                if p.extension().and_then(|e| e.to_str()) == Some("thx") {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Collect import binding + uses in `other` when it selective-imports `export_name`
/// from a module that resolves to `export_path`.
fn import_rename_edits(
    other_uri: &str,
    other_text: &str,
    other_path: &Path,
    export_path: &Path,
    export_name: &str,
    new_name: &str,
) -> Option<Vec<Value>> {
    let other_idx = catch_unwind(AssertUnwindSafe(|| {
        build_index(other_text, &other_path.to_string_lossy())
    }))
    .ok()?;
    if !other_idx.ok {
        return None;
    }
    let mut edits = Vec::new();
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
        let name = origin
            .original
            .clone()
            .unwrap_or_else(|| b.name.clone());
        if name != export_name {
            continue;
        }
        let resolved = crossmodule::resolve_import_file(&origin.spec, other_path)?;
        if !crossmodule::paths_equal_public(&resolved, export_path) {
            continue;
        }
        let _ = other_uri;
        for s in index::rename_spans(&other_idx, b) {
            edits.push(convert::text_edit(
                span::span_to_range(other_text, s),
                new_name,
            ));
        }
    }
    if edits.is_empty() {
        None
    } else {
        Some(edits)
    }
}

fn import_uses_of_export(
    other_uri: &str,
    other_text: &str,
    other_path: &Path,
    export_path: &Path,
    export_name: &str,
    include_decl: bool,
) -> Vec<Value> {
    let mut locs = Vec::new();
    let other_idx = match catch_unwind(AssertUnwindSafe(|| {
        build_index(other_text, &other_path.to_string_lossy())
    })) {
        Ok(i) => i,
        Err(_) => return locs,
    };
    if !other_idx.ok {
        return locs;
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
        let name = origin
            .original
            .clone()
            .unwrap_or_else(|| b.name.clone());
        if name != export_name {
            continue;
        }
        let Some(resolved) =
            crossmodule::resolve_import_file(&origin.spec, other_path)
        else {
            continue;
        };
        if !crossmodule::paths_equal_public(&resolved, export_path) {
            continue;
        }
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
    locs
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
    fn method_member_via_function_return() {
        // `let p = make()` where make() -> Point links p.len to Point.len.
        let uri = "file:///tmp/member_fn.thx";
        let text = "\
type Point(x: Int64) {
  export fn len() -> Int64 { x }
}
fn make() -> Point { Point(x: 1) }
fn main() -> Int64 {
  let p = make()
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
            "method": "workspace/didChangeConfiguration",
            "params": {}
        })]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn workspace_symbol_open_buffer_and_disk() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_wsym_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        let lib = dir.join("lib");
        fs::create_dir_all(&lib).unwrap();
        let helper = lib.join("Helper.thx");
        fs::write(
            &helper,
            "export fn answer() -> Int64 { 42 }\nexport type Point(x: Int64)\n",
        )
        .unwrap();
        let main = dir.join("Main.thx");
        let main_src =
            "import { answer } from \"./lib/Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        let main_uri = convert::path_to_uri(&main.to_string_lossy());

        // Only Main open — Helper is on disk under package tree.
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
                        "text": main_src
                    }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "workspace/symbol",
                "params": { "query": "ans" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "workspace/symbol",
                "params": { "query": "Point" }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "workspace/symbol",
                "params": { "query": "main" }
            }),
        ]);

        let ans = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let arr = ans["result"].as_array().expect("symbol array");
        assert!(
            arr.iter().any(|s| s["name"] == "answer"),
            "expected answer from disk Helper: {arr:?}"
        );

        let pt = msgs.iter().find(|m| m.get("id") == Some(&json!(3))).unwrap();
        let arr = pt["result"].as_array().expect("Point symbols");
        assert!(
            arr.iter().any(|s| s["name"] == "Point"),
            "expected Point type: {arr:?}"
        );

        let mn = msgs.iter().find(|m| m.get("id") == Some(&json!(4))).unwrap();
        let arr = mn["result"].as_array().expect("main symbols");
        assert!(
            arr.iter().any(|s| s["name"] == "main"),
            "expected main from open buffer: {arr:?}"
        );

        let _ = fs::remove_dir_all(&dir);
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
    fn std_named_root_definition() {
        // Named root `std/…` go-to-def (lsp-xmodule polish).
        use std::fs;
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let main = workspace.join("tests/conformance/_lsp_std_nav.thx");
        // Ephemeral file under workspace so relative paths / std resolve.
        let text = "import { isEmpty } from \"std/Str\"\nfn main() -> Int64 { if isEmpty(\"\") { 1 } else { 0 } }\n";
        fs::write(&main, text).unwrap();
        let main_uri = convert::path_to_uri(&main.to_string_lossy());
        let call = text.rfind("isEmpty").unwrap();
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
        ]);

        let _ = fs::remove_file(&main);
        let def = msgs.iter().find(|m| m.get("id") == Some(&json!(2))).unwrap();
        let def_uri = def["result"]["uri"].as_str().unwrap_or("");
        assert!(
            def_uri.ends_with("Str.thx") || def_uri.contains("/std/Str"),
            "expected jump into std/Str, got {def_uri} full={def}"
        );
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
    fn multi_file_rename_same_dir_on_disk() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_diskren_{}_{}",
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
            changes.keys().any(|k| k.ends_with("Main.thx")),
            "expected Main.thx on disk in rename: {changes:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_refs_same_dir_on_disk() {
        // Export refs include same-directory importers on disk (not only open buffers).
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_diskrefs_{}_{}",
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
        // Only Helper is open — Main stays on disk.
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
        let uris: Vec<&str> = arr
            .iter()
            .filter_map(|l| l["uri"].as_str())
            .collect();
        assert!(
            uris.iter().any(|u| u.ends_with("Main.thx")),
            "expected disk Main.thx among refs: {uris:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_refs_package_tree_on_disk() {
        // Export in a subdir; Main at package root imports via `./lib/Helper`.
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_pkgrefts_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        let lib = dir.join("lib");
        fs::create_dir_all(&lib).unwrap();
        let helper = lib.join("Helper.thx");
        let helper_src = "export fn answer() -> Int64 { 42 }\n";
        fs::write(&helper, helper_src).unwrap();
        let main = dir.join("Main.thx");
        let main_src =
            "import { answer } from \"./lib/Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
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
        let uris: Vec<&str> = arr
            .iter()
            .filter_map(|l| l["uri"].as_str())
            .collect();
        assert!(
            uris.iter().any(|u| u.ends_with("Main.thx")),
            "expected package-root Main.thx among refs: {uris:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_file_rename_package_tree_on_disk() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "thoxa_lsp_pkgren_{}_{}",
            std::process::id(),
            ns
        ));
        let _ = fs::remove_dir_all(&dir);
        let lib = dir.join("lib");
        fs::create_dir_all(&lib).unwrap();
        let helper = lib.join("Helper.thx");
        let helper_src = "export fn answer() -> Int64 { 42 }\n";
        fs::write(&helper, helper_src).unwrap();
        let main = dir.join("Main.thx");
        let main_src =
            "import { answer } from \"./lib/Helper\"\nfn main() -> Int64 { answer() }\n";
        fs::write(&main, main_src).unwrap();
        let helper_uri = convert::path_to_uri(&helper.to_string_lossy());
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
            changes.keys().any(|k| k.ends_with("Main.thx")),
            "expected Main.thx on disk in package rename: {changes:?}"
        );

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
