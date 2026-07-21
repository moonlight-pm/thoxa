"""Stage 6 of the kilo port: the `Find` and `Prompt` modules
(examples/kilo/{Find,Prompt}.thx).

The interactive loops (Find.run / Prompt.promptLine) are terminal-driven, so they are
covered end to end by the whole-port pty smoke in tests/test_kilo_keys.py (which owns the
single pty harness). Here we pin the *pure* core: `Find.searchFrom` — the wrap-around row
scan the search loop drives — deterministically, with no terminal, no singleton, no keys.
This is the same testability seam as Stage 5's `messageFor(clock)`."""
from __future__ import annotations

import pathlib

import pytest

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


@pytest.mark.parametrize("name", ["Find", "Prompt"])
def test_module_type_checks_clean(name):
    path = KILO / f"{name}.thx"
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


def _search(rows: list[str], query: str, last: int, direction: int) -> str:
    """Run Find.searchFrom over `rows` and return "(row,col)" or "none"."""
    lits = ", ".join(f'Row(Bytes.fromString("{r}"))' for r in rows)
    src = (
        'import { Row } from "./Row"\n'
        'import { searchFrom, Match } from "./Find"\n'
        "fn main() -> Int64 {\n"
        f"let rows: Array<Row> = [{lits}]\n"
        f'let q = Bytes.fromString("{query}")\n'
        f"let show = match searchFrom(rows, q, {last}, {direction}) "
        '{ Has(m) { "({m.row},{m.col})" } Not { "none" } }\n'
        "println(show)\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "find_probe.thx"), out=buf.append)
    return "".join(buf).strip()


ROWS = ["alpha beta", "gamma delta", "beta again"]


def test_fresh_scan_finds_first_occurrence_from_row_zero():
    # last == -1 is a fresh scan: it starts forward from row 0 regardless of direction.
    assert _search(ROWS, "beta", -1, 1) == "(0,6)"


def test_step_forward_to_the_next_row_with_a_match():
    # From a match in row 0, stepping forward skips row 1 (no match) to row 2.
    assert _search(ROWS, "beta", 0, 1) == "(2,0)"


def test_forward_wraps_past_the_end():
    # From row 2, forward wraps around to row 0.
    assert _search(ROWS, "alpha", 2, 1) == "(0,0)"


def test_backward_step_and_wrap():
    # dir == -1 steps up; from row 0 it wraps to the last row first.
    assert _search(["target", "x", "y"], "target", 0, -1) == "(0,0)"
    assert _search(["aaa", "bbb", "aaa"], "a", 2, -1) == "(0,0)"


def test_direction_actually_steps_opposite_ways():
    # THE discriminating pair: identical rows + start row, opposite directions must land on
    # DIFFERENT rows. Three matching rows starting from the middle: forward -> the row below,
    # backward -> the row above. If searchFrom ignored its `direction` argument (or Find.run's
    # Up/Down arms were swapped), both would collapse to the same result and this fails — the
    # backward-scan path is otherwise never observed (every other input matches under both dirs).
    assert _search(["a", "a", "a"], "a", 1, 1) == "(2,0)"    # forward from row 1 -> row 2
    assert _search(["a", "a", "a"], "a", 1, -1) == "(0,0)"   # backward from row 1 -> row 0


def test_leftmost_occurrence_within_a_row():
    assert _search(["abcabc"], "abc", -1, 1) == "(0,0)"
    assert _search(["  spaced"], "spaced", -1, 1) == "(0,2)"


def test_absent_query_is_not():
    assert _search(ROWS, "zzz", -1, 1) == "none"


def test_empty_document_is_not():
    assert _search([], "x", -1, 1) == "none"


def test_fresh_backward_scan_does_not_underflow():
    # last == -1 with dir == -1 must NOT compute current = -2 (the guard forces dir = 1):
    # a fresh scan always starts forward from row 0, so it still finds the first match.
    assert _search(ROWS, "beta", -1, -1) == "(0,6)"
