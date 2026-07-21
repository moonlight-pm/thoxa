"""Value splice — Bytes.insertAt/removeAt and Array.insertAt/removeAt (fork #4).
Copying (receiver/source unchanged) and strict-bounds (trap on a bad index)."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<splice>", out=buf.append)
    return "".join(buf), code


def test_bytes_insert_and_remove():
    out, _ = _run(
        'let b = Bytes.fromString("helo")\n'
        "let b2 = b.insertAt(2, b'l')\n"      # h e [l] l o
        'let b3 = b.removeAt(1)\n'            # h l o
        'println("{b2.toString()} {b3.toString()} src={b.toString()}")'
    )
    assert out == "hello hlo src=helo\n"


def test_bytes_insert_at_end_appends():
    out, _ = _run(
        'let b = Bytes.fromString("ab")\n'
        "let b2 = b.insertAt(2, b'c')\n"      # at == count -> append
        'println(b2.toString())'
    )
    assert out == "abc\n"


def test_bytes_insert_masks_to_byte():
    out, _ = _run(
        'let b = Bytes.fromString("")\n'
        'let b2 = b.insertAt(0, 300)\n'       # 300 & 0xFF == 44 (",")
        'println("{b2.count} {b2.get(0)}")'
    )
    assert out == "1 44\n"


@pytest.mark.parametrize("call", [
    "b.insertAt(0 - 1, 65)",   # negative
    "b.insertAt(5, 65)",       # past insert bound (count 4)
    "b.removeAt(4)",           # == count
    "b.removeAt(0 - 1)",       # negative
])
def test_bytes_splice_out_of_range_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let b = Bytes.fromString("abcd")\nprintln("{{{call}.count}}")')
    assert "out of range" in exc.value.render()


def test_array_insert_and_remove():
    out, _ = _run(
        'let xs = [10, 20, 30]\n'
        'let ys = xs.insertAt(1, 99)\n'       # [10, 99, 20, 30]
        'let zs = xs.removeAt(0)\n'           # [20, 30]
        'println("ys={ys.count},{ys[1]} zs={zs.count},{zs[0]} src={xs.count},{xs[1]}")'
    )
    assert out == "ys=4,99 zs=2,20 src=3,20\n"


def test_array_insert_at_end_appends():
    out, _ = _run(
        'let xs = [1, 2]\n'
        'let ys = xs.insertAt(2, 3)\n'        # at == count -> append
        'println("{ys.count} {ys[2]}")'
    )
    assert out == "3 3\n"


@pytest.mark.parametrize("call", [
    "xs.insertAt(0 - 1, 9)",   # negative
    "xs.insertAt(4, 9)",       # past insert bound (count 3)
    "xs.removeAt(3)",          # == count
    "xs.removeAt(0 - 1)",      # negative
])
def test_array_splice_out_of_range_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let xs = [1, 2, 3]\nprintln("{{{call}.count}}")')
    assert "out of range" in exc.value.render()
