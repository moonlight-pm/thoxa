"""The Thoxa language server: a thin, zero-dependency stdio LSP.

It reuses the compiler frontend directly, so its diagnostics can never drift
from the language. Entry point: :func:`run` (wired to the ``thoxa lsp`` CLI
subcommand). See ``docs/specs/2026-07-12-thoxa-zed-editor-support-design.md``.
"""
from __future__ import annotations


def run() -> int:
    """Serve the language server over stdio until the client disconnects."""
    from .server import run as _run

    return _run()


__all__ = ["run"]
