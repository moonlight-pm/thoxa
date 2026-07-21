"""Stage 0 of the kilo port: the `Terminal` module (examples/kilo/Terminal.thx).

These pin what the conformance program (spec/conformance/kilo_terminal.thx, which is
self-contained and re-declares the structs) cannot: that the *real* module type-checks,
that its C structs classify and lay out correctly (Termios must be exactly 60 bytes or
tcgetattr/tcsetattr would corrupt memory), that windowSize()/enable() fail gracefully
off a tty. (Raw-mode engagement through a real pty now lives in test_kilo_keys.py,
which drives the Stage 1 editor built on this module.)
"""

from __future__ import annotations

import contextlib
import ctypes
import os
import pathlib

import pytest

from thoxa import ast, ffi
from thoxa.checker import check
from thoxa.errors import Source
from thoxa.interp import run
from thoxa.lexer import Lexer
from thoxa.parser import Parser

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _typedefs(path: pathlib.Path) -> dict[str, ast.TypeDef]:
    text = path.read_text(encoding="utf-8")
    s = Source(text, str(path))
    module = Parser(Lexer(s).tokenize(), s).parse_program()
    return {t.name: t for t in module.body if isinstance(t, ast.TypeDef)}


@contextlib.contextmanager
def _stdin_from_devnull():
    """Point fd 0 at /dev/null for the duration. enable() does tcgetattr(fd 0), which the
    `out=` fd-1 capture bridge does NOT redirect; without this, `pytest -s` on a real
    terminal would let enable() succeed and flip the developer's terminal into raw mode.
    """
    devnull = os.open(os.devnull, os.O_RDONLY)
    saved = os.dup(0)
    try:
        os.dup2(devnull, 0)
        yield
    finally:
        os.dup2(saved, 0)
        os.close(saved)
        os.close(devnull)


def _run_probe(src: str) -> tuple[str, object]:
    """Run a snippet whose imports resolve from examples/kilo/ (so `./Terminal` works).
    stdin is /dev/null so tcgetattr(fd 0) fails deterministically and never touches a tty."""
    buf: list[str] = []
    with _stdin_from_devnull():
        code = run(src, str(KILO / "probe.thx"), out=buf.append)
    return "".join(buf), code


# -- the module type-checks --------------------------------------------------

def test_terminal_module_type_checks_clean():
    text = (KILO / "Terminal.thx").read_text(encoding="utf-8")
    assert check(text, str(KILO / "Terminal.thx")) == []


def test_main_driver_type_checks_clean():
    text = (KILO / "Main.thx").read_text(encoding="utf-8")
    assert check(text, str(KILO / "Main.thx")) == []


# -- struct classification & ABI layout --------------------------------------

def test_struct_classification():
    """Termios/Winsize cross the FFI by address (ABI); RawMode/WindowSize do not."""
    types = _typedefs(KILO / "Terminal.thx")
    assert ffi.abi_struct_specs(types["Termios"]) is not None
    assert ffi.abi_struct_specs(types["Winsize"]) is not None
    assert ffi.abi_struct_specs(types["RawMode"]) is None       # has methods
    assert ffi.abi_struct_specs(types["WindowSize"]) is None    # has ctor params


def test_termios_layout_matches_struct_termios():
    """The safety property: Termios must be exactly `sizeof(struct termios)` = 60 bytes
    on x86-64 glibc, so the kernel's write through the pointer stays inside the block."""
    types = _typedefs(KILO / "Terminal.thx")
    cls = ffi.make_struct_type("Termios", ffi.abi_struct_specs(types["Termios"]))
    assert ctypes.sizeof(cls) == 60


def test_winsize_layout_is_eight_bytes():
    types = _typedefs(KILO / "Terminal.thx")
    cls = ffi.make_struct_type("Winsize", ffi.abi_struct_specs(types["Winsize"]))
    assert ctypes.sizeof(cls) == 8


# -- the real module's graceful-failure paths (deterministic off a tty) -------

def test_window_size_fails_without_tty():
    out, _ = _run_probe(
        'import terminal from "./Terminal"\n'
        "fn main() -> Int64 {\n"
        "    match terminal.windowSize() {\n"
        '        Pass(size) { println("ok {size.rows} {size.cols}") }\n'
        '        Fail(e) { println("fail") }\n'
        "    }\n"
        "    0\n"
        "}\n"
    )
    assert out.strip() == "fail"


def test_enable_fails_without_tty():
    out, _ = _run_probe(
        'import terminal from "./Terminal"\n'
        "fn main() -> Int64 {\n"
        "    mut t = terminal.RawMode()\n"
        "    match t.enable() {\n"
        '        Pass { println("enabled") }\n'
        '        Fail(e) { println("fail") }\n'
        "    }\n"
        "    0\n"
        "}\n"
    )
    assert out.strip() == "fail"


def test_restore_before_enable_is_a_noop():
    """restore() is idempotent — safe on a fresh handle that never enabled raw mode."""
    out, _ = _run_probe(
        'import terminal from "./Terminal"\n'
        "fn main() -> Int64 {\n"
        "    mut t = terminal.RawMode()\n"
        "    match t.restore() {\n"
        '        Pass { println("noop") }\n'
        '        Fail(e) { println("fail") }\n'
        "    }\n"
        "    0\n"
        "}\n"
    )
    assert out.strip() == "noop"


# -- the raw-mode success path (through a real pty) --------------------------
# Moved to tests/test_kilo_keys.py: the Stage 0 driver this test drove was
# replaced by the Stage 1 movable-cursor driver, so the pty smoke — which spawns
# the real `Main.thx` and asserts raw mode engages (ICANON/ISIG off), keys decode,
# and the terminal restores — now lives beside the current driver.
