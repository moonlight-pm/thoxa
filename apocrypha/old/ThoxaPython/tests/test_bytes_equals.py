"""Bytes.equals — the Stage 7 structural byte-comparison primitive. True iff same
length and bytes, length-counted (embedded-NUL safe), strict about its argument type."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.ffi import Bytes
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<equals>", out=buf.append)
    return "".join(buf), code


# -- through the interpreter (Bool surface) ----------------------------------
# A plain-word `if` avoids nested string literals inside interpolation (which the lexer
# reads as an unterminated string) — the same shape as spec/conformance/bytes_equals.thx.

def test_equal_buffers_are_true():
    out, _ = _run(
        'let a = Bytes.fromString("int")\n'
        'let b = Bytes.fromString("int")\n'
        'let r = if a.equals(b) { "yes" } else { "no" }\n'
        'println(r)'
    )
    assert out == "yes\n"


def test_different_content_same_length_is_false():
    out, _ = _run(
        'let a = Bytes.fromString("int")\n'
        'let b = Bytes.fromString("ins")\n'
        'let r = if a.equals(b) { "yes" } else { "no" }\n'
        'println(r)'
    )
    assert out == "no\n"


def test_different_length_is_false():
    out, _ = _run(
        'let a = Bytes.fromString("in")\n'
        'let b = Bytes.fromString("int")\n'
        'let r = if a.equals(b) { "yes" } else { "no" }\n'
        'println(r)'
    )
    assert out == "no\n"


def test_empty_equals_empty():
    out, _ = _run(
        'let a = Bytes.fromString("")\n'
        'let b = Bytes.fromString("")\n'
        'let r = if a.equals(b) { "yes" } else { "no" }\n'
        'println(r)'
    )
    assert out == "yes\n"


# -- the Python Bytes.equals directly (edges a Thoxa string can't express) ----

def test_equals_python_api():
    assert Bytes.from_string("abc").equals(Bytes.from_string("abc")) is True
    assert Bytes.from_string("abc").equals(Bytes.from_string("abd")) is False
    assert Bytes.from_string("abc").equals(Bytes.from_string("ab")) is False


def test_equals_survives_embedded_nul():
    # length-counted, not NUL-terminated: bytes past a \x00 are still compared.
    assert Bytes.from_bytes(b"a\x00c").equals(Bytes.from_bytes(b"a\x00c")) is True
    assert Bytes.from_bytes(b"a\x00c").equals(Bytes.from_bytes(b"a\x00d")) is False


def test_equals_rejects_non_bytes_argument():
    with pytest.raises(ThoxaError) as exc:
        _run('let b = Bytes.fromString("hi").equals(3)\nprintln("{b}")')
    assert "Bytes.equals" in exc.value.render()
