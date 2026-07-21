"""A small REPL that keeps bindings across lines (the seed of the M5 notebook kernel).

The same ``Interpreter`` instance persists, so definitions and `let` bindings accumulate
across inputs. Bare expressions print their value.
"""

from __future__ import annotations

from .errors import Source, ThoxaError
from .interp import VOID, Interpreter
from .lexer import Lexer
from .parser import Parser

_OPENERS = {"{": "}", "(": ")", "[": "]"}


def _incomplete(src: str) -> bool:
    """Very small heuristic: unbalanced brackets => keep reading."""
    depth = 0
    in_str = False
    escape = False
    for c in src:
        if in_str:
            if escape:
                escape = False
            elif c == "\\":
                escape = True
            elif c == '"':
                in_str = False
            continue
        if c == '"':
            in_str = True
        elif c in _OPENERS:
            depth += 1
        elif c in _OPENERS.values():
            depth -= 1
    return depth > 0 or in_str


def repl() -> int:
    print("Thoxa REPL (pre-alpha). Press Ctrl-D to exit.")
    interp = Interpreter()
    buffer = ""
    while True:
        try:
            line = input("...  " if buffer else "thoxa> ")
        except EOFError:
            print()
            break
        except KeyboardInterrupt:
            print("^C")
            buffer = ""
            continue
        buffer += line + "\n"
        if _incomplete(buffer):
            continue
        src, buffer = buffer, ""
        if not src.strip():
            continue
        try:
            source = Source(src, "<repl>")
            module = Parser(Lexer(source).tokenize(), source).parse_program()
            interp.src = source
            result = interp.run_module(module)
            if result is not VOID and result is not None:
                print(interp.display(result))
        except ThoxaError as exc:
            print(exc.render())
    return 0
