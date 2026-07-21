"""Source spans and friendly, caret-annotated diagnostics.

This is the foundation of Thoxa's "friendly errors" pitch: every error carries a
source span and renders with the offending line and a caret underline.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Span:
    """A 1-based (line, col) location with a character length on that line."""

    line: int
    col: int
    length: int = 1

    def __repr__(self) -> str:
        return f"{self.line}:{self.col}"


class Source:
    """A named blob of Thoxa source, with line lookup for diagnostics."""

    def __init__(self, text: str, filename: str = "<input>") -> None:
        self.text = text
        self.filename = filename
        self.lines = text.split("\n")

    def line_text(self, n: int) -> str:
        if 1 <= n <= len(self.lines):
            return self.lines[n - 1]
        return ""


@dataclass
class Diagnostic:
    message: str
    span: Span
    source: Source
    notes: list[str] = field(default_factory=list)

    def render(self) -> str:
        src = self.source
        sp = self.span
        line_txt = src.line_text(sp.line)
        gutter = f"{sp.line} "
        pad = " " * len(gutter)
        caret_pad = " " * (sp.col - 1)
        carets = "^" * max(1, sp.length)
        out = [
            f"error: {self.message}",
            f"{pad}--> {src.filename}:{sp.line}:{sp.col}",
            f"{pad}|",
            f"{gutter}| {line_txt}",
            f"{pad}| {caret_pad}{carets}",
        ]
        for note in self.notes:
            out.append(f"{pad}= note: {note}")
        return "\n".join(out)


class ThoxaError(Exception):
    """Any user-facing Thoxa error (lex, parse, type, or runtime)."""

    def __init__(self, diag: Diagnostic) -> None:
        self.diag = diag
        super().__init__(diag.message)

    def render(self) -> str:
        return self.diag.render()
