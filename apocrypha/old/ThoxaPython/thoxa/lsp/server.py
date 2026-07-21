"""The Thoxa language server: diagnostics + semantic tokens over stdio.

Full-document sync (Thoxa files are small). Every handler is wrapped so an
exception in analysis logs to stderr and never kills the message loop.
"""
from __future__ import annotations

import sys

from . import analysis, convert, crossmodule, index
from .. import ast
from .protocol import Connection

# JSON-RPC error codes
_METHOD_NOT_FOUND = -32601
_INTERNAL_ERROR = -32603

# `Void` is a built-in unit type with no source definition, so hover describes it
# inline rather than pointing at a (nonexistent) declaration.
_VOID_HOVER = (
    "```thoxa\nVoid\n```\n\n"
    "The unit type — its sole value `Void`, returned by side-effecting code and "
    "carried by a successful `Trial<Void, E>`."
)


class Server:
    _VALUE_KINDS = {"fn", "extern", "method", "param", "field", "let", "mut",
                    "forvar", "pattern"}
    _TYPE_KINDS = {"type", "union"}

    def __init__(self, conn: Connection) -> None:
        self.conn = conn
        self.docs: dict[str, str] = {}
        self.shutdown_requested = False

    # -- lifecycle --------------------------------------------------------
    def initialize(self, params: dict) -> dict:
        return {
            "capabilities": {
                "textDocumentSync": 1,  # full document sync
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": convert.TOKEN_TYPES,
                        "tokenModifiers": convert.TOKEN_MODIFIERS,
                    },
                    "full": True,
                },
                "definitionProvider": True,
                "declarationProvider": True,
                "referencesProvider": True,
                "documentSymbolProvider": True,
                "hoverProvider": True,
                "renameProvider": {"prepareProvider": True},
            },
            "serverInfo": {"name": "thoxa-lsp"},
        }

    def _shutdown(self, params: dict) -> None:
        self.shutdown_requested = True
        return None

    # -- document notifications ------------------------------------------
    def did_open(self, params: dict) -> None:
        doc = params["textDocument"]
        self.docs[doc["uri"]] = doc["text"]
        self.publish(doc["uri"])

    def did_change(self, params: dict) -> None:
        uri = params["textDocument"]["uri"]
        changes = params.get("contentChanges") or []
        if changes:  # full sync: the final change carries the whole document
            self.docs[uri] = changes[-1]["text"]
        self.publish(uri)

    def did_close(self, params: dict) -> None:
        uri = params["textDocument"]["uri"]
        self.docs.pop(uri, None)
        self._send_diagnostics(uri, [])  # clear squiggles for the closed doc

    # -- requests ---------------------------------------------------------
    def semantic_tokens_full(self, params: dict) -> dict:
        uri = params["textDocument"]["uri"]
        text = self.docs.get(uri, "")
        try:
            data = convert.encode_semantic_tokens(analysis.semantic_tokens(text, uri))
        except Exception as exc:  # noqa: BLE001 — never fail the request
            print(f"thoxa-lsp: semantic tokens failed: {exc}", file=sys.stderr)
            data = []
        return {"data": data}

    def definition(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        text = self.docs.get(uri, "")
        try:
            path = convert.uri_to_path(uri)
            idx = index.build_index(text, path)
            ref = idx.reference_at(pos["line"], pos["character"])
            remote = crossmodule.resolve_remote(
                idx, ref, idx.resolution.binding_of(ref), self.docs, path)
            if remote is not None and remote.def_span is not None:
                return convert.location(convert.path_to_uri(remote.path), remote.def_span)
            binding = idx.binding_at(pos["line"], pos["character"])
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: definition failed: {exc}", file=sys.stderr)
            return None
        if binding is None or binding.def_span is None:
            return None
        return convert.location(uri, binding.def_span)

    def references(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        include_decl = (params.get("context") or {}).get("includeDeclaration", True)
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            binding = idx.binding_at(pos["line"], pos["character"])
            if binding is None:
                return []
            spans = []
            for ref in idx.resolution.refs_to(binding.id):
                if not include_decl and ref.span == binding.def_span:
                    continue
                if convert.span_text(text, ref.span) != binding.name:
                    continue
                spans.append(ref.span)
            if include_decl and binding.def_span is not None \
                    and binding.def_span not in spans \
                    and convert.span_text(text, binding.def_span) == binding.name:
                spans.append(binding.def_span)
            return [convert.location(uri, s) for s in spans]
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: references failed: {exc}", file=sys.stderr)
            return []

    def document_symbol(self, params: dict):
        uri = params["textDocument"]["uri"]
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            out = []
            for b in idx.resolution.module_definitions():
                if b.def_span is None:
                    continue
                children = []
                if b.kind == "type":
                    for m in idx.resolution.members_of(b.name):
                        if m.def_span is None:
                            continue
                        children.append(convert.document_symbol(
                            m.name, m.kind, m.def_span, m.def_span, []))
                out.append(convert.document_symbol(
                    b.name, b.kind, b.def_span, b.def_span, children))
            return out
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: documentSymbol failed: {exc}", file=sys.stderr)
            return []

    def hover(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        text = self.docs.get(uri, "")
        try:
            path = convert.uri_to_path(uri)
            idx = index.build_index(text, path)
            ref = idx.reference_at(pos["line"], pos["character"])
            binding = idx.resolution.binding_of(ref)
            span = ref.span if ref is not None else None
            if binding is None:                       # cursor may be ON a definition name
                binding = idx.binding_at(pos["line"], pos["character"])
                if span is None and binding is not None:
                    span = binding.def_span
            # cross-module: an imported name, a namespace member/name, a prelude
            # type, or a member on a value of an imported type
            remote = crossmodule.resolve_remote(idx, ref, binding, self.docs, path)
            if remote is not None:
                parts = [f"```thoxa\n{remote.signature}\n```"]
                if remote.doc:
                    parts.append(remote.doc)
                return convert.hover("\n\n".join(parts), span)
            # a bare `Void` literal parses to no binding: describe the built-in
            if binding is None:
                vspan = index.voidlit_at(idx, pos["line"], pos["character"])
                if vspan is not None:
                    return convert.hover(_VOID_HOVER, vspan)
            if binding is None or binding.kind == "builtin":
                return None
            ty = idx.type_of(binding)
            param_types = None
            if binding.kind == "type":              # a fallible ctor's Trial<Self,E>
                ty = idx.ctor_returns.get(binding.name)
                param_types = index.ctor_param_types(idx, binding)
            elif binding.kind in ("fn", "method"):  # an inferred, unannotated return
                ty = idx.fn_returns.get(binding.id)
            sig = index.render_signature(binding, ty, idx.text, param_types)
            if not sig:
                return None
            parts = [f"```thoxa\n{sig}\n```"]
            # A ctor-param field's def shares the type's declaration line, so its
            # `doc_above` is really the enclosing type's doc — suppress it; the
            # signature already names the field's type.
            ctor_field = binding.kind == "field" and isinstance(binding.node, ast.Param)
            if binding.def_span is not None and not ctor_field:
                doc = convert.doc_above(text, binding.def_span.line)
                if doc:
                    parts.append(doc)
            return convert.hover("\n\n".join(parts), span)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: hover failed: {exc}", file=sys.stderr)
            return None

    def _renameable(self, binding):
        if binding is None or binding.def_span is None:
            return False
        return binding.kind in self._VALUE_KINDS or binding.kind in self._TYPE_KINDS

    def _valid_new_name(self, binding, new_name: str) -> bool:
        if not new_name or not (new_name[0].isalpha() or new_name[0] == "_"):
            return False
        if any(not (c.isalnum() or c == "_") for c in new_name):
            return False
        first = new_name[0]
        if binding.kind in self._TYPE_KINDS:
            return first.isupper()
        return first.islower() or first == "_"

    def prepare_rename(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        try:
            idx = index.build_index(self.docs.get(uri, ""), convert.uri_to_path(uri))
            ref = idx.reference_at(pos["line"], pos["character"])
            binding = idx.resolution.binding_of(ref)
            span = ref.span if ref is not None else None
            if binding is None:                       # cursor may be ON a definition name
                binding = idx.binding_at(pos["line"], pos["character"])
                span = binding.def_span if binding is not None else None
            if not self._renameable(binding) or span is None:
                return None
            return convert.span_to_range(span)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: prepareRename failed: {exc}", file=sys.stderr)
            return None

    def rename(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        new_name = params.get("newName", "")
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            binding = idx.binding_at(pos["line"], pos["character"])
            if not self._renameable(binding) or not self._valid_new_name(binding, new_name):
                return None
            spans = [r.span for r in idx.resolution.refs_to(binding.id)]
            if binding.def_span not in spans:
                spans.append(binding.def_span)
            spans = [s for s in spans if convert.span_text(text, s) == binding.name]
            edits = [convert.text_edit(s, new_name) for s in spans]
            return convert.workspace_edit(uri, edits)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: rename failed: {exc}", file=sys.stderr)
            return None

    # -- diagnostics ------------------------------------------------------
    def publish(self, uri: str) -> None:
        text = self.docs.get(uri, "")
        # The checker resolves relative imports against the file's directory, so
        # it needs a real path — not the `file://` URI. Publish back under `uri`.
        path = convert.uri_to_path(uri)
        try:
            diags = [convert.diagnostic_to_lsp(d) for d in analysis.collect_diagnostics(text, path)]
        except Exception as exc:  # noqa: BLE001 — never fail on a bad buffer
            print(f"thoxa-lsp: diagnostics failed: {exc}", file=sys.stderr)
            diags = []
        self._send_diagnostics(uri, diags)

    def _send_diagnostics(self, uri: str, diagnostics: list) -> None:
        self.conn.send({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": uri, "diagnostics": diagnostics},
        })

    # -- dispatch loop ----------------------------------------------------
    def serve(self) -> int:
        requests = {
            "initialize": self.initialize,
            "shutdown": self._shutdown,
            "textDocument/semanticTokens/full": self.semantic_tokens_full,
            "textDocument/definition": self.definition,
            "textDocument/declaration": self.definition,
            "textDocument/references": self.references,
            "textDocument/documentSymbol": self.document_symbol,
            "textDocument/hover": self.hover,
            "textDocument/prepareRename": self.prepare_rename,
            "textDocument/rename": self.rename,
        }
        notifications = {
            "initialized": lambda p: None,
            "textDocument/didOpen": self.did_open,
            "textDocument/didChange": self.did_change,
            "textDocument/didClose": self.did_close,
        }
        while True:
            msg = self.conn.read()
            if msg is None:
                return 0
            method = msg.get("method")
            if method == "exit":
                return 0 if self.shutdown_requested else 1
            msg_id = msg.get("id")
            try:
                if msg_id is not None:
                    handler = requests.get(method)
                    if handler is None:
                        self._error(msg_id, _METHOD_NOT_FOUND, f"method not found: {method}")
                        continue
                    self.conn.send({"jsonrpc": "2.0", "id": msg_id, "result": handler(msg.get("params") or {})})
                else:
                    handler = notifications.get(method)
                    if handler is not None:
                        handler(msg.get("params") or {})
            except Exception as exc:  # noqa: BLE001 — keep the loop alive
                print(f"thoxa-lsp: handler error in {method}: {exc}", file=sys.stderr)
                if msg_id is not None:
                    self._error(msg_id, _INTERNAL_ERROR, str(exc))

    def _error(self, msg_id, code: int, message: str) -> None:
        self.conn.send({"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}})


def run() -> int:
    return Server(Connection(sys.stdin.buffer, sys.stdout.buffer)).serve()
