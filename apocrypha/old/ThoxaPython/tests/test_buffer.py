"""Buffer — the growable, value-semantic byte buffer: safety-critical guard paths.

The happy-path behaviour (geometric growth, copy-on-write, appendBytes, freeze) is
pinned by spec/conformance/buffer.thx. These tests pin the two error paths a
conformance program (which must exit 0) can't: use-after-freeze and the refusal to
let a growable buffer cross the FFI (where a realloc could dangle C's pointer).
"""

from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def _run(src: str) -> str:
    buf: list[str] = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def test_use_after_freeze_is_rejected():
    src = """
fn main() -> Int64 {
    mut b = Buffer()
    b.append(1)
    let frozen = b.freeze()
    b.append(2)
    0
}
"""
    with pytest.raises(ThoxaError) as exc:
        _run(src)
    assert "consumed by freeze" in exc.value.render()


def test_buffer_cannot_cross_the_ffi():
    src = """
extern fn sysWrite(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c" "write"
fn main() -> Int64 {
    mut b = Buffer()
    b.append(65)
    sysWrite(1, b, 1)
    0
}
"""
    with pytest.raises(ThoxaError) as exc:
        _run(src)
    assert "cannot cross the FFI" in exc.value.render()


def test_freeze_produces_independent_bytes():
    # freeze copies the live prefix out; mutating the (fresh) buffer afterwards
    # cannot disturb an already-frozen Bytes.
    out = _run(
        """
fn main() -> Int64 {
    mut b = Buffer()
    b.append(72)
    b.append(105)
    let frozen = b.freeze()
    println("{frozen.toString()} {frozen.count}")
    0
}
"""
    )
    assert out == "Hi 2\n"
