"""Bridge from the Thoxa compiler frontend to LSP-shaped data.

``collect_diagnostics`` reuses the real ``check`` so the server's errors can
never drift from the language. ``semantic_tokens`` reuses the lexer so coloring
stays robust on unparseable, mid-edit buffers (it simply yields nothing when the
buffer will not even lex, leaving base coloring to the tree-sitter grammar).
"""
from __future__ import annotations

from ..errors import Diagnostic, Source, ThoxaError
from ..tokens import T


def collect_diagnostics(text: str, filename: str = "<input>") -> list[Diagnostic]:
    """All diagnostics for a document. Type errors come back as a list; a lexer
    or parser error surfaces as a single raised ThoxaError (no error recovery
    yet), which we normalize to a one-element list."""
    from ..checker import check

    try:
        return check(text, filename)
    except ThoxaError as exc:
        return [exc.diag]


def semantic_tokens(text: str, filename: str = "<input>"):
    """Classify the identifiers the grammar cannot resolve on casing alone.

    Returns ``(line0, char0, length, type_name, modifiers)`` tuples for:
      * PascalCase names -> ``type``
      * camelCase before ``(`` -> ``function``
      * camelCase before ``!:`` (a named-argument label) -> ``parameter``
      * other camelCase names -> ``variable``

    Keywords, strings, numbers, comments and operators are intentionally left to
    the tree-sitter base layer (``semantic_tokens = "combined"``); in particular
    we never emit tokens over string literals, so interpolation coloring is
    preserved.
    """
    from ..lexer import Lexer

    try:
        toks = Lexer(Source(text, filename)).tokenize()
    except ThoxaError:
        return []

    out = []
    for i, tok in enumerate(toks):
        if tok.kind is T.UPPER:
            kind = "type"
        elif tok.kind is T.LOWER:
            nxt = toks[i + 1].kind if i + 1 < len(toks) else None
            nxt2 = toks[i + 2].kind if i + 2 < len(toks) else None
            if nxt is T.LPAREN:
                kind = "function"
            elif nxt is T.BANG and nxt2 is T.COLON:
                kind = "parameter"
            else:
                kind = "variable"
        else:
            continue
        sp = tok.span
        out.append((sp.line - 1, max(0, sp.col - 1), sp.length, kind, 0))
    return out
