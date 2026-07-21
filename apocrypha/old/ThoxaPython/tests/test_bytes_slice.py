"""Bytes.slice — the Stage 3 slice primitive. Strict bounds (trap on out-of-range),
copying (source unaffected)."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<slice>", out=buf.append)
    return "".join(buf), code


def test_slice_full_and_sub():
    out, _ = _run(
        'let s = Bytes.fromString("abcdef")\n'
        'println("{s.slice(0,6).toString()} {s.slice(2,4).toString()}")'
    )
    assert out == "abcdef cd\n"


@pytest.mark.parametrize("call", [
    "s.slice(0, 7)",      # end > count
    "s.slice(4, 2)",      # start > end
    "s.slice(0 - 1, 3)",  # negative start
])
def test_slice_out_of_bounds_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let s = Bytes.fromString("abcdef")\nprintln("{{{call}.count}}")')
    assert "out of bounds" in exc.value.render()


def test_slice_is_a_copy():
    # the slice keeps its bytes even after the source buffer is mutated
    out, _ = _run(
        'let s = Bytes.fromString("abcdef")\n'
        'let sl = s.slice(0, 3)\n'
        'mut b = Buffer()\n'
        'b.appendBytes(s)\n'
        "b.set(0, b'Z')\n"
        'println(sl.toString())'
    )
    assert out == "abc\n"
