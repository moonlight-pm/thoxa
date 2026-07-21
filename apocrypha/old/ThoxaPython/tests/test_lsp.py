"""Tests for the Thoxa language server (thoxa.lsp).

The server is a thin, zero-dependency adapter over the existing compiler
frontend, so the tests focus on the two things that are genuinely new:
position/diagnostic conversion and the LSP wire framing.
"""

import io
import pathlib

import pytest

from thoxa.errors import Diagnostic, Source, Span
from thoxa.lsp import analysis, convert, protocol

CONF = pathlib.Path(__file__).resolve().parent.parent / "spec" / "conformance"


# -- convert: Span (1-based) -> LSP Range (0-based) -----------------------

def test_span_to_range_is_zero_based():
    r = convert.span_to_range(Span(line=1, col=1, length=3))
    assert r == {
        "start": {"line": 0, "character": 0},
        "end": {"line": 0, "character": 3},
    }


def test_span_to_range_uses_length_for_end():
    r = convert.span_to_range(Span(line=5, col=4, length=2))
    assert r["start"] == {"line": 4, "character": 3}
    assert r["end"] == {"line": 4, "character": 5}


def test_span_to_range_never_goes_negative():
    r = convert.span_to_range(Span(line=1, col=0, length=1))
    assert r["start"]["character"] >= 0


# -- convert: Diagnostic -> LSP diagnostic --------------------------------

def test_diagnostic_to_lsp_maps_fields():
    src = Source("let x = 1\n", "f.thx")
    d = convert.diagnostic_to_lsp(Diagnostic("boom", Span(1, 1, 3), src))
    assert d["severity"] == 1  # Error
    assert d["source"] == "thoxa"
    assert d["message"] == "boom"
    assert d["range"]["start"] == {"line": 0, "character": 0}


def test_diagnostic_to_lsp_folds_notes_into_message():
    src = Source("x\n", "f.thx")
    d = convert.diagnostic_to_lsp(
        Diagnostic("bad name", Span(1, 1, 1), src, notes=["use camelCase"])
    )
    assert "bad name" in d["message"]
    assert "use camelCase" in d["message"]


# -- analysis: diagnostics via the real compiler --------------------------

@pytest.mark.parametrize("path", sorted(CONF.glob("*.thx")), ids=lambda p: p.name)
def test_conformance_programs_have_no_diagnostics(path):
    assert analysis.collect_diagnostics(path.read_text(encoding="utf-8"), str(path)) == []


def test_type_error_yields_diagnostics():
    diags = analysis.collect_diagnostics("fn main() -> Int64 { if 3 { }  0 }")
    assert diags, "expected at least one diagnostic"
    assert any("Bool" in d.message for d in diags)


def test_lex_or_parse_error_yields_single_diagnostic():
    # underscores are rejected by the lexer -> raised ThoxaError, one diagnostic
    diags = analysis.collect_diagnostics("fn main() -> Int64 { let x_y = 1  0 }")
    assert len(diags) == 1


# -- analysis + convert: semantic tokens ----------------------------------

def test_semantic_tokens_classify_identifiers():
    from collections import Counter

    toks = analysis.semantic_tokens("fn add(x!: Int64) -> Int64 { x }")
    kinds = Counter(t[3] for t in toks)
    assert kinds == {"function": 1, "parameter": 1, "type": 2, "variable": 1}


def test_semantic_tokens_ignore_unlexable_buffers():
    # mid-edit garbage should not raise; base coloring is left to the grammar
    assert analysis.semantic_tokens('fn main() { "unterminated ') == []


def test_encode_semantic_tokens_is_relative():
    t = convert.TOKEN_TYPES
    tokens = [
        (0, 4, 3, "function", 0),
        (0, 8, 1, "parameter", 0),
        (1, 2, 1, "variable", 0),
    ]
    data = convert.encode_semantic_tokens(tokens)
    assert data == [
        0, 4, 3, t.index("function"), 0,
        0, 4, 1, t.index("parameter"), 0,
        1, 2, 1, t.index("variable"), 0,
    ]


# -- protocol: Content-Length framed JSON-RPC -----------------------------

def test_write_then_read_round_trips():
    buf = io.BytesIO()
    msg = {"jsonrpc": "2.0", "id": 7, "result": {"ok": True}}
    protocol.write_message(buf, msg)
    raw = buf.getvalue()
    assert raw.startswith(b"Content-Length: ")
    assert b"\r\n\r\n" in raw
    assert protocol.read_message(io.BytesIO(raw)) == msg


def test_read_message_returns_none_at_eof():
    assert protocol.read_message(io.BytesIO(b"")) is None


# -- convert: file:// DocumentUri -> local filesystem path ----------------

def test_uri_to_path_decodes_file_uri():
    assert convert.uri_to_path("file:///home/x/Main.thx") == "/home/x/Main.thx"
    # percent-encoded segments (e.g. a space) are decoded
    assert convert.uri_to_path("file:///home/a%20b/M.thx") == "/home/a b/M.thx"


def test_uri_to_path_passes_through_non_file():
    # in-memory / untitled buffers (and tests) use a non-file sentinel
    assert convert.uri_to_path("<input>") == "<input>"


def test_diagnostics_resolve_relative_imports_via_file_uri(tmp_path):
    """The LSP hands the checker a file:// URI; relative imports must resolve
    against the file's directory. Regression: a raw URI made os.path.abspath
    mangle the base dir, so `import … from "./Lib"` reported 'cannot find
    module' in the editor even though the CLI (real path) was clean."""
    (tmp_path / "Lib.thx").write_text("export fn ping() -> Int64 { 1 }\n", encoding="utf-8")
    main = tmp_path / "Main.thx"
    main.write_text(
        'import lib from "./Lib"\nexport fn main() -> Int64 { lib.ping() }\n',
        encoding="utf-8",
    )
    uri = main.as_uri()
    assert uri.startswith("file://")
    text = main.read_text(encoding="utf-8")
    # The path the LSP now derives (uri_to_path) resolves the import cleanly.
    assert analysis.collect_diagnostics(text, convert.uri_to_path(uri)) == []


# -- convert: LSP structure mappings (Location/Hover/WorkspaceEdit/etc.) --

def test_position_covers_is_zero_based_half_open():
    sp = Span(line=3, col=5, length=4)     # 1-based cols 5..8 -> 0-based chars 4..7
    assert convert.position_covers(sp, 2, 4) is True
    assert convert.position_covers(sp, 2, 7) is True
    assert convert.position_covers(sp, 2, 8) is False
    assert convert.position_covers(sp, 1, 4) is False


def test_symbol_kind_maps_known_kinds():
    assert convert.symbol_kind("fn") == 12       # Function
    assert convert.symbol_kind("type") == 23      # Struct
    assert convert.symbol_kind("field") == 8      # Field
    assert convert.symbol_kind("let") == 13       # Variable


def test_doc_above_collects_contiguous_double_hash():
    text = (
        "## first line\n"
        "## second line\n"
        "fn f() -> Int64 { 0 }\n"
    )
    assert convert.doc_above(text, 3) == "first line\nsecond line"


def test_doc_above_stops_at_blank_or_single_hash():
    text = "# not a doc\n\n## real doc\nfn f() -> Int64 { 0 }\n"
    assert convert.doc_above(text, 4) == "real doc"   # a blank line stops the scan
    assert convert.doc_above(text, 1) is None
    # A single-`#` line (ordinary comment, not a doc) also stops collection:
    # `far` sits above the `# stop` line and must be excluded.
    single = "## far\n# stop\n## near\nfn f() -> Int64 { 0 }\n"
    assert convert.doc_above(single, 4) == "near"


def test_workspace_edit_shape():
    we = convert.workspace_edit("file:///m.thx", [convert.text_edit(Span(1, 1, 3), "abc")])
    assert we == {"changes": {"file:///m.thx": [
        {"range": {"start": {"line": 0, "character": 0},
                   "end": {"line": 0, "character": 3}}, "newText": "abc"}]}}
