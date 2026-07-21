"""Command-line entry point: ``thoxa run <file>``, ``thoxa repl``, ``thoxa --version``."""

from __future__ import annotations

import argparse
import os
import sys

from . import __version__
from .errors import ThoxaError
from .interp import VOID, run


class EntryNotFound(Exception):
    """A run/check target that names no runnable Thoxa entry."""


def resolve_entry(path: str) -> str:
    """Map a run/check target to a concrete ``.thx`` file (design §4.2).

    A directory is a *project*: it resolves to its ``Main.thx``. A path that is
    already a file is used as-is. A directory with no ``Main.thx`` raises
    ``EntryNotFound``.
    """
    if os.path.isdir(path):
        main = os.path.join(path, "Main.thx")
        if not os.path.isfile(main):
            raise EntryNotFound(
                f"no Main.thx in {path} — a runnable Thoxa project needs a "
                "Main.thx that exports main"
            )
        return main
    return path


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    parser = argparse.ArgumentParser(prog="thoxa", description="Thoxa language (pre-alpha, unfrozen)")
    parser.add_argument("--version", action="store_true", help="print the version and exit")
    sub = parser.add_subparsers(dest="cmd")
    p_run = sub.add_parser("run", help="run a .thx file")
    p_run.add_argument("file")
    # Trailing tokens after the module are the program's own `arguments` (kilo takes a
    # filename here). REMAINDER (not "*") so option-like args pass through untouched.
    p_run.add_argument("arguments", nargs=argparse.REMAINDER)
    p_check = sub.add_parser("check", help="type-check a .thx file without running it")
    p_check.add_argument("file")
    sub.add_parser("repl", help="start the interactive REPL")
    p_nb = sub.add_parser("notebook", help="launch the local literate notebook")
    p_nb.add_argument("--port", type=int, default=8099)
    p_nb.add_argument("--no-open", action="store_true", help="do not open a browser")
    sub.add_parser("lsp", help="run the language server (stdio, for editors like Zed)")

    args = parser.parse_args(argv)
    if args.version:
        print(f"thoxa {__version__}")
        return 0
    if args.cmd == "run":
        return cmd_run(args.file, args.arguments)
    if args.cmd == "check":
        return cmd_check(args.file)
    if args.cmd == "repl":
        from .repl import repl

        return repl()
    if args.cmd == "notebook":
        from .notebook import serve

        return serve(port=args.port, open_browser=not args.no_open)
    if args.cmd == "lsp":
        from .lsp import run

        return run()
    parser.print_help()
    return 0


def cmd_check(path: str) -> int:
    from .checker import check

    try:
        path = resolve_entry(path)
    except EntryNotFound as exc:
        print(f"thoxa: {exc}", file=sys.stderr)
        return 2
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except OSError as exc:
        print(f"thoxa: cannot open {path}: {exc}", file=sys.stderr)
        return 2
    try:
        errors = check(text, path)
    except ThoxaError as exc:
        print(exc.render(), file=sys.stderr)
        return 1
    if not errors:
        print(f"{path}: ok")
        return 0
    for diag in errors:
        print(diag.render(), file=sys.stderr)
        print(file=sys.stderr)
    print(f"{len(errors)} error(s)", file=sys.stderr)
    return 1


def cmd_run(path: str, arguments: list[str]) -> int:
    # arguments[0] is the program: the module path exactly as typed (C-style argv[0]);
    # the trailing tokens follow. Capture it before resolve_entry rewrites `path`.
    typed = path
    try:
        path = resolve_entry(path)
    except EntryNotFound as exc:
        print(f"thoxa: {exc}", file=sys.stderr)
        return 2
    try:
        with open(path, encoding="utf-8") as f:
            text = f.read()
    except OSError as exc:
        print(f"thoxa: cannot open {path}: {exc}", file=sys.stderr)
        return 2
    try:
        result = run(text, path, arguments=[typed, *arguments])
    except ThoxaError as exc:
        print(exc.render(), file=sys.stderr)
        return 1
    if isinstance(result, int) and not isinstance(result, bool):
        return result
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
