"""Bytes.find — the Stage 6 substring-search primitive. May-typed (Has(index)/Not),
leftmost match, length-counted (embedded-NUL safe), strict about its argument type."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.ffi import Bytes
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<find>", out=buf.append)
    return "".join(buf), code


# -- through the interpreter (the May surface) -------------------------------

def test_find_present_returns_leftmost_index():
    out, _ = _run(
        'let hay = Bytes.fromString("hello world")\n'
        'let show = match hay.find(Bytes.fromString("o")) { Has(i) { "{i}" } Not { "none" } }\n'
        'println(show)'
    )
    assert out == "4\n"


def test_find_absent_returns_not():
    out, _ = _run(
        'let hay = Bytes.fromString("hello")\n'
        'let show = match hay.find(Bytes.fromString("z")) { Has(i) { "{i}" } Not { "none" } }\n'
        'println(show)'
    )
    assert out == "none\n"


def test_find_needle_longer_than_haystack_is_not():
    out, _ = _run(
        'let show = match Bytes.fromString("hi").find(Bytes.fromString("high")) '
        '{ Has(i) { "{i}" } Not { "none" } }\n'
        'println(show)'
    )
    assert out == "none\n"


def test_find_empty_needle_is_zero():
    out, _ = _run(
        'let show = match Bytes.fromString("abc").find(Bytes.fromString("")) '
        '{ Has(i) { "{i}" } Not { "none" } }\n'
        'println(show)'
    )
    assert out == "0\n"


# -- the Python Bytes.find directly (edges a Thoxa string can't easily express) ---

def test_find_python_api_leftmost_and_absent():
    hay = Bytes.from_string("banana")
    assert hay.find(Bytes.from_string("na")) == 2   # leftmost of "na"
    assert hay.find(Bytes.from_string("x")) == -1
    assert hay.find(Bytes.from_string("banana")) == 0
    assert hay.find(Bytes.from_string("")) == 0


def test_find_survives_embedded_nul():
    # Bytes is length-counted, not NUL-terminated: a NUL in the middle does not cut search.
    hay = Bytes.from_bytes(b"a\x00b\x00c")
    assert hay.find(Bytes.from_bytes(b"b\x00c")) == 2
    assert hay.find(Bytes.from_bytes(b"\x00c")) == 3
    assert hay.find(Bytes.from_bytes(b"zz")) == -1


def test_find_rejects_non_bytes_argument():
    with pytest.raises(ThoxaError) as exc:
        _run('let n = Bytes.fromString("hi").find(3)\nprintln("{n}")')
    assert "Bytes.find" in exc.value.render()
