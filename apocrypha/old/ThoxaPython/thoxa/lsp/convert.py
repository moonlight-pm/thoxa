"""Pure mappings from Thoxa's model to LSP wire structures.

Free of I/O and of compiler internals, so it is trivially testable. Thoxa spans
are 1-based ``(line, col)`` with a character length; LSP positions are 0-based
``(line, character)`` with half-open ranges.
"""
from __future__ import annotations

from typing import Optional
from urllib.parse import quote, unquote, urlparse

from ..errors import Diagnostic, Span

# Our semantic-token legend: the standard LSP token types, advertised verbatim
# in the `initialize` response. We only emit a handful (type / variable /
# function / parameter); the rest are reserved so the legend is stable.
TOKEN_TYPES: list[str] = [
    "namespace", "type", "class", "enum", "interface", "struct",
    "typeParameter", "parameter", "variable", "property", "enumMember",
    "event", "function", "method", "macro", "keyword", "modifier",
    "comment", "string", "number", "regexp", "operator", "decorator",
]
TOKEN_MODIFIERS: list[str] = []
_TYPE_INDEX = {name: i for i, name in enumerate(TOKEN_TYPES)}

# LSP DiagnosticSeverity
SEVERITY_ERROR = 1


def uri_to_path(uri: str) -> str:
    """Map an LSP ``file://`` DocumentUri to a local filesystem path.

    The checker resolves relative imports (``./Net``) against the source file's
    directory, so it needs a real path — not the ``file://…`` URI the client
    sends, which ``os.path.abspath`` would mangle into a bogus directory. A
    non-``file`` string (e.g. the in-memory ``<input>`` used by tests and
    untitled buffers) is returned unchanged.
    """
    if not uri.startswith("file://"):
        return uri
    return unquote(urlparse(uri).path)


def path_to_uri(path: str) -> str:
    """Inverse of `uri_to_path`: a local filesystem path back to a `file://` URI.

    Used to point go-to-definition at a *different* file than the one under the
    cursor. A non-absolute string (the `<input>` sentinel, an untitled buffer)
    has no real location and is returned unchanged.
    """
    if not path.startswith("/"):
        return path
    return "file://" + quote(path)


def span_to_range(span: Span) -> dict:
    """Convert a 1-based single-line Span to a 0-based LSP Range."""
    line = max(0, span.line - 1)
    start = max(0, span.col - 1)
    return {
        "start": {"line": line, "character": start},
        "end": {"line": line, "character": start + max(0, span.length)},
    }


def span_text(text: str, span: Span) -> Optional[str]:
    """The document slice covered by a 1-based (line, col, length) span, or None
    if the span falls outside the text. Used to verify a span actually covers the
    identifier before emitting an edit/location (defends against upstream bad spans)."""
    lines = text.splitlines()
    if span.line < 1 or span.line > len(lines):
        return None
    line = lines[span.line - 1]
    start = span.col - 1
    if start < 0 or start + span.length > len(line):
        return None
    return line[start:start + span.length]


def diagnostic_to_lsp(diag: Diagnostic, severity: int = SEVERITY_ERROR) -> dict:
    """Convert a compiler Diagnostic to an LSP diagnostic, folding notes into
    the message (LSP has no first-class 'note' concept)."""
    message = diag.message
    if diag.notes:
        message = "\n".join([message, *(f"note: {n}" for n in diag.notes)])
    return {
        "range": span_to_range(diag.span),
        "severity": severity,
        "source": "thoxa",
        "message": message,
    }


def encode_semantic_tokens(tokens) -> list[int]:
    """Encode ``(line, char, length, type_name, modifiers)`` tuples as the LSP
    relative-delta integer array. Tokens are sorted by ``(line, char)`` first.
    """
    data: list[int] = []
    prev_line = 0
    prev_char = 0
    for line, char, length, type_name, modifiers in sorted(tokens, key=lambda t: (t[0], t[1])):
        delta_line = line - prev_line
        delta_char = char - prev_char if delta_line == 0 else char
        data += [delta_line, delta_char, length, _TYPE_INDEX[type_name], modifiers]
        prev_line, prev_char = line, char
    return data


# -- LSP structure mappings: Location/Hover/WorkspaceEdit/DocumentSymbol --

# LSP SymbolKind (subset we emit)
_SYMBOL_KIND = {
    "fn": 12, "extern": 12, "method": 6,       # Function / Method
    "type": 23, "union": 10,                    # Struct / Enum
    "field": 8, "param": 13, "let": 13,         # Field / Variable
    "mut": 13, "forvar": 13, "pattern": 13,
    "import": 2,                                # Module
}


def position_covers(span: Span, line0: int, char0: int) -> bool:
    """True if the 0-based LSP (line0, char0) falls within the 1-based span."""
    start = max(0, span.col - 1)
    return span.line - 1 == line0 and start <= char0 < start + max(1, span.length)


def location(uri: str, span: Span) -> dict:
    return {"uri": uri, "range": span_to_range(span)}


def hover(markdown: str, span: Optional[Span]) -> dict:
    out = {"contents": {"kind": "markdown", "value": markdown}}
    if span is not None:
        out["range"] = span_to_range(span)
    return out


def text_edit(span: Span, new_text: str) -> dict:
    return {"range": span_to_range(span), "newText": new_text}


def workspace_edit(uri: str, edits: list) -> dict:
    return {"changes": {uri: edits}}


def symbol_kind(kind_str: str) -> int:
    return _SYMBOL_KIND.get(kind_str, 13)       # default Variable


def document_symbol(name: str, kind_str: str, range_span: Span,
                    selection_span: Span, children: list) -> dict:
    sym = {
        "name": name,
        "kind": symbol_kind(kind_str),
        "range": span_to_range(range_span),
        "selectionRange": span_to_range(selection_span),
    }
    if children:
        sym["children"] = children
    return sym


def doc_above(text: str, def_line: int) -> Optional[str]:
    """The contiguous `##` doc block immediately above 1-based `def_line`."""
    lines = text.splitlines()
    collected: list[str] = []
    i = def_line - 2                            # 0-based line just above the def
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("##"):
            collected.append(stripped[2:].strip())
            i -= 1
        else:
            break
    if not collected:
        return None
    return "\n".join(reversed(collected))
