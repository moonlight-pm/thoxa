"""Row — kilo's erow: tab-expanded render, cxToRx, and the visible window slice."""
from __future__ import annotations

import pathlib
from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = 'import { Row } from "./Row"\nfn main() -> Int64 {\n' + body + "\n0\n}\n"
    buf: list[str] = []
    run(src, str(KILO / "row_probe.thx"), out=buf.append)
    return "".join(buf)


def test_row_module_type_checks_clean():
    p = KILO / "Row.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_tab_expands_to_next_stop():
    # "a\tb": 'a' at col0, tab fills cols 1..7 (to stop 8), 'b' at col8 -> render len 9
    out = _probe(
        'let r = Row(Bytes.fromString("a\\tb"))\n'
        'println("rc={r.renderCount()} cc={r.charCount()} b8={r.window(8,1).toString()}")'
    )
    assert out == "rc=9 cc=3 b8=b\n"


def test_cx_to_rx_counts_tab_width():
    # chars "a\tb": cx=0 -> rx 0; cx=1 (at tab) -> rx 1; cx=2 (after tab) -> rx 8
    out = _probe(
        'let r = Row(Bytes.fromString("a\\tb"))\n'
        'println("{r.cxToRx(0)} {r.cxToRx(1)} {r.cxToRx(2)}")'
    )
    assert out == "0 1 8\n"


def test_window_clamps_both_ends():
    out = _probe(
        'let r = Row(Bytes.fromString("hello"))\n'
        'println("mid={r.window(1,3).toString()} past={r.window(9,4).count} tail={r.window(3,99).toString()}")'
    )
    assert out == "mid=ell past=0 tail=lo\n"


# -- Stage 4 editing: each op returns a NEW Row (render re-derived) ------------

def test_row_insert_and_delete_char():
    out = _probe(
        'let r = Row(Bytes.fromString("helo"))\n'
        "let a = r.insertChar(2, b'l')\n"      # "hello"
        'let b = r.deleteChar(0)\n'            # "elo"
        'println("{a.text()} {b.text()} src={r.text()}")'
    )
    assert out == "hello elo src=helo\n"


def test_row_take_drop_concat():
    out = _probe(
        'let r = Row(Bytes.fromString("hello"))\n'
        'let head = r.take(2)\n'               # "he"
        'let tail = r.drop(2)\n'               # "llo"
        'let joined = head.concat(tail)\n'     # "hello"
        'println("{head.text()} {tail.text()} {joined.text()}")'
    )
    assert out == "he llo hello\n"


def test_row_insert_rebuilds_render():
    # inserting a tab must re-expand render (proves render is not stale from the ctor)
    out = _probe(
        'let r = Row(Bytes.fromString("ab"))\n'
        "let e = r.insertChar(1, b'\\t')\n"    # chars "a\tb" -> render "a"+7sp+"b" = 9
        'println("{e.renderCount()}")'
    )
    assert out == "9\n"
