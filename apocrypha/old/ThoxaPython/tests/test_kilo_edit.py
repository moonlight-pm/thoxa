"""State editing (kilo editorInsertChar/InsertNewline/DelChar) — Stage 4.
Deterministic: drive the exported ops and assert row text, cursor, and dirty."""
from __future__ import annotations

import pathlib

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = (
        'import state from "./State"\n'
        'import { Row } from "./Row"\n'
        'fn main() -> Int64 {\n'
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "t")\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "edit_probe.thx"), out=buf.append)
    return "".join(buf)


def test_state_type_checks_clean():
    p = KILO / "State.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_insert_char_into_empty_buffer():
    # cy starts at 0 == numRows() (phantom line): the first insert materializes a row
    out = _probe(
        "state.insertChar(b'h')\n"
        "state.insertChar(b'i')\n"
        'println("n={state.numRows()} r0={state.row(0).text()} '
        'cx={state.cursorX()} cy={state.cursorY()} d={state.isDirty()}")'
    )
    assert out == "n=1 r0=hi cx=2 cy=0 d=true\n"


def test_insert_newline_splits_line():
    out = _probe(
        "state.insertChar(b'a')\n"
        "state.insertChar(b'b')\n"       # row0 = "ab", cx=2
        'state.moveHome()\n'             # cx=0
        'state.moveRight()\n'            # cx=1 (between a and b)
        'state.insertNewline()\n'        # split: row0="a", row1="b", cy=1, cx=0
        'println("n={state.numRows()} r0={state.row(0).text()} r1={state.row(1).text()} '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=2 r0=a r1=b cx=0 cy=1\n"


def test_newline_at_column_zero_inserts_blank_above():
    out = _probe(
        "state.insertChar(b'x')\n"       # row0 = "x", cx=1
        'state.moveHome()\n'             # cx=0
        'state.insertNewline()\n'        # blank line above; cursor moves to old text (row1)
        'println("n={state.numRows()} r0=[{state.row(0).text()}] r1=[{state.row(1).text()}] '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=2 r0=[] r1=[x] cx=0 cy=1\n"


def test_backspace_within_line():
    out = _probe(
        "state.insertChar(b'a')\nstate.insertChar(b'b')\nstate.insertChar(b'c')\n"  # "abc", cx=3
        'state.deleteChar()\n'           # remove 'c': "ab", cx=2
        'println("r0={state.row(0).text()} cx={state.cursorX()}")'
    )
    assert out == "r0=ab cx=2\n"


def test_backspace_at_column_zero_joins_previous():
    out = _probe(
        "state.insertChar(b'a')\nstate.insertChar(b'b')\n"   # row0 "ab"
        'state.insertNewline()\n'                            # row0 "ab", row1 "", cy=1, cx=0
        "state.insertChar(b'c')\nstate.insertChar(b'd')\n"   # row1 "cd", cx=2
        'state.moveHome()\n'                                 # cx=0 on row1
        'state.deleteChar()\n'                               # join: row0 "abcd", cy=0, cx=2
        'println("n={state.numRows()} r0={state.row(0).text()} '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=1 r0=abcd cx=2 cy=0\n"


def test_delete_forward_at_line_end_joins_next():
    out = _probe(
        "state.insertChar(b'a')\n"       # row0 "a"
        'state.insertNewline()\n'        # row0 "a", row1 "", cy=1
        "state.insertChar(b'b')\n"       # row1 "b"
        'state.moveUp()\nstate.moveEnd()\n'  # cursor at end of row0 ("a"), cx=1, cy=0
        'state.deleteForward()\n'        # forward-delete joins row1 onto row0 -> "ab"
        'println("n={state.numRows()} r0={state.row(0).text()}")'
    )
    assert out == "n=1 r0=ab\n"


def test_start_of_file_backspace_is_noop():
    out = _probe(
        'state.deleteChar()\n'           # empty buffer, cy==numRows(): no-op, no trap
        'println("n={state.numRows()} d={state.isDirty()}")'
    )
    assert out == "n=0 d=false\n"
