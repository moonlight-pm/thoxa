"""State — file-coordinate cursor: wrap, snap, scroll, and empty-file safety."""
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
        'fn main() -> Int64 {\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "state_probe.thx"), out=buf.append)
    return "".join(buf)


def test_state_type_checks_clean():
    p = KILO / "State.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


LOAD2 = (
    'state.setScreenSize(24, 80)\n'
    'mut rows: Array<Row> = []\n'
    'rows = rows.append(Row(Bytes.fromString("hello")))\n'
    'rows = rows.append(Row(Bytes.fromString("hi")))\n'
    'state.setRows(rows, "t.txt")\n'
)


def test_move_end_and_snap_down():
    # cursor at end of line 0 ("hello", len 5), move down to shorter line 1 ("hi", 2):
    # cx snaps from 5 to 2.
    out = _probe(
        LOAD2 +
        'state.moveEnd()\n'
        'state.moveDown()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=2 cy=1\n"


def test_move_right_wraps_line():
    # at end of line 0, moveRight goes to (line1, col0)
    out = _probe(
        LOAD2 +
        'state.moveEnd()\n'
        'state.moveRight()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=0 cy=1\n"


def test_move_left_wraps_to_prev_line_end():
    # at (line1, col0), moveLeft goes to end of line 0 (col 5)
    out = _probe(
        LOAD2 +
        'state.moveDown()\n'      # cy=1, cx snapped to 0
        'state.moveLeft()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=5 cy=0\n"


def test_empty_file_moves_are_safe():
    # no rows: every mover is a guarded no-op, nothing traps
    out = _probe(
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "empty")\n'
        'state.moveEnd()\nstate.moveRight()\nstate.moveDown()\nstate.moveUp()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=0 cy=0\n"


def test_scroll_tracks_cursor_vertically():
    # a 3-text-row viewport over a 10-line file; arrow down past the viewport advances
    # rowoff. Stage 5 reserves 2 rows for the status+message bars, so a height-5 terminal
    # gives textHeight()==3 — that reduced height is what scroll() tracks against.
    body = (
        'state.setScreenSize(5, 80)\n'
        'mut rows: Array<Row> = []\n'
        'mut i = 0\n'
        'loop { if i >= 10 { break } rows = rows.append(Row(Bytes.fromString("x"))) i = i + 1 }\n'
        'state.setRows(rows, "t")\n'
        'mut k = 0\n'
        'loop { if k >= 5 { break } state.moveDown() k = k + 1 }\n'
        'state.scroll()\n'
        'println("cy={state.cursorY()} rowoff={state.rowOff()}")'
    )
    # cy=5, textHeight 3 -> rowoff = 5 - 3 + 1 = 3
    assert _probe(body) == "cy=5 rowoff=3\n"


# -- Stage 5: save / message / quit-confirm / text height ---------------------


def test_save_clears_dirty(tmp_path):
    dst = tmp_path / "s.txt"
    body = (
        'state.setRows([], "DST")\n'
        "state.insertChar(b'h')\n"          # dirties the buffer, materializes a row
        'let before = state.isDirty()\n'
        'state.save()\n'
        'let after = state.isDirty()\n'
        'println("before={before} after={after}")'
    ).replace("DST", str(dst))
    assert _probe(body) == "before=true after=false\n"
    assert dst.read_bytes() == b"h\n"


def test_status_message_fades_after_five_seconds():
    # The deterministic seam: fresh while (clock - stamp) < 5, blank at exactly 5s on.
    out = _probe(
        'state.setStatusMessageAt("saved", 100)\n'
        'println("a=[{state.messageFor(102)}]")\n'
        'println("b=[{state.messageFor(104)}]")\n'
        'println("c=[{state.messageFor(105)}]")\n'
        'println("d=[{state.messageFor(106)}]")'
    )
    assert out == "a=[saved]\nb=[saved]\nc=[]\nd=[]\n"


def test_quit_confirm_dirty_needs_four_presses():
    body = (
        'state.setRows([], "x")\n'
        "state.insertChar(b'z')\n"          # dirty
        'let a = state.confirmQuit()\n'     # warn (3->2), false
        'let b = state.confirmQuit()\n'     # warn (2->1), false
        'let c = state.confirmQuit()\n'     # warn (1->0), false
        'let d = state.confirmQuit()\n'     # quit, true
        'println("a={a} b={b} c={c} d={d}")'
    )
    assert _probe(body) == "a=false b=false c=false d=true\n"


def test_quit_confirm_clean_quits_immediately():
    body = (
        'state.setRows([], "x")\n'
        'println("q={state.confirmQuit()}")'
    )
    assert _probe(body) == "q=true\n"


def test_reset_quit_times_restores_the_counter():
    # After two warnings (3->2->1) a reset returns the counter to 3, so it now takes THREE
    # more warnings before the quit (f). This must press through to the actual quit=true:
    # a no-op reset would quit at d (quitTimes already 1), so d=false vs d=true discriminates
    # a working reset from a broken one (a same-length all-false prefix would not).
    body = (
        'state.setRows([], "x")\n'
        "state.insertChar(b'z')\n"          # dirty
        'let a = state.confirmQuit()\n'     # warn (3->2)
        'let b = state.confirmQuit()\n'     # warn (2->1)
        'state.resetQuitTimes()\n'          # back to 3
        'let c = state.confirmQuit()\n'     # warn (3->2)
        'let d = state.confirmQuit()\n'     # warn (2->1) — a no-op reset would QUIT here
        'let e = state.confirmQuit()\n'     # warn (1->0)
        'let f = state.confirmQuit()\n'     # quit (true)
        'println("a={a} b={b} c={c} d={d} e={e} f={f}")'
    )
    assert _probe(body) == "a=false b=false c=false d=false e=false f=true\n"


def test_page_down_on_a_two_row_terminal_is_safe():
    # textHeight()==0 (2 rows - 2 reserved bars). pageDown must not drive cy negative, or
    # the next scroll() would index row(-1) and trap, aborting the editor mid-render.
    body = (
        'state.setScreenSize(2, 80)\n'
        'mut rows: Array<Row> = []\n'
        'rows = rows.append(Row(Bytes.fromString("a")))\n'
        'rows = rows.append(Row(Bytes.fromString("b")))\n'
        'state.setRows(rows, "t")\n'
        'state.pageDown()\n'
        'state.scroll()\n'                  # would trap on row(-1) before the lower clamp
        'println("cy={state.cursorY()}")'
    )
    assert _probe(body) == "cy=0\n"


def test_text_height_reserves_two_rows():
    body = (
        'state.setScreenSize(24, 80)\n'
        'let big = state.textHeight()\n'
        'state.setScreenSize(2, 80)\n'
        'let tiny = state.textHeight()\n'
        'state.setScreenSize(1, 80)\n'
        'let one = state.textHeight()\n'
        'println("big={big} tiny={tiny} one={one}")'
    )
    assert _probe(body) == "big=22 tiny=0 one=0\n"


# -- Stage 6: Find/save-as support (allRows / jumpTo / restoreView / setFilename) ---


def test_all_rows_returns_the_loaded_document():
    out = _probe(
        LOAD2 +
        'let rs = state.allRows()\n'
        'println("n={rs.count} r0={rs[0].text()} r1={rs[1].text()}")'
    )
    assert out == "n=2 r0=hello r1=hi\n"


def test_jump_to_sets_the_cursor():
    out = _probe(
        LOAD2 +
        'state.jumpTo(3, 1)\n'                # Find lands the cursor on a match
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=3 cy=1\n"


def test_restore_view_sets_all_four_coordinates():
    # Find's ESC-cancel restores cursor AND scroll — all four, not just cx/cy.
    out = _probe(
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "t")\n'
        'state.restoreView(7, 2, 1, 4)\n'
        'println("cx={state.cursorX()} cy={state.cursorY()} '
        'coloff={state.colOff()} rowoff={state.rowOff()}")'
    )
    assert out == "cx=7 cy=2 coloff=1 rowoff=4\n"


def test_set_filename_names_an_unnamed_buffer_then_saves(tmp_path):
    # The save-as path: an unnamed buffer ("") gets a name, and a subsequent save() writes
    # it and clears dirty — the half of Stage 5's save() the deferred prompt now completes.
    dst = tmp_path / "named.txt"
    body = (
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "")\n'
        'let before = state.currentFilename()\n'
        "state.insertChar(b'k')\n"                  # dirties the buffer
        'state.setFilename("DST")\n'
        'let after = state.currentFilename()\n'
        'state.save()\n'
        'println("before=[{before}] after=[{after}] dirty={state.isDirty()}")'
    ).replace("DST", str(dst))
    assert _probe(body) == f"before=[] after=[{dst}] dirty=false\n"
    assert dst.read_bytes() == b"k\n"


# -- Stage 7: the derived syntax-highlight cache + the Find match overlay ------

def _syntax_probe(body: str) -> str:
    src = (
        'import state from "./State"\n'
        'import { Row } from "./Row"\n'
        'import { Hl, Normal, Comment, Keyword2, Number, Matched } from "./Syntax"\n'
        "fn letter(h!: Hl) -> String {\n"
        '  match h { Normal { "N" } Comment { "C" } Keyword2 { "2" } Number { "#" } '
        'Matched { "M" } else { "?" } }\n'
        "}\n"
        "fn cells(filerow!: Int64) -> String {\n"
        "  let cs = state.rowCells(filerow)\n"
        '  mut out = ""\n'
        "  mut i = 0\n"
        "  loop { if i >= cs.count { break }  out = \"{out}{letter(cs[i])}\"  i = i + 1 }\n"
        "  out\n"
        "}\n"
        "fn main() -> Int64 {\n" + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "state_syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_set_rows_with_c_extension_highlights():
    # "int x = 1;" under a .c name: int -> Keyword2 (222), 1 -> Number (#).
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int x = 1;")))\n'
        '  state.setRows(rs, "a.c")\n'
        '  println(cells(0))'
    )
    assert out == "222NNNNN#N"


def test_plain_text_name_is_all_normal():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int x = 1;")))\n'
        '  state.setRows(rs, "a.txt")\n'
        '  println(cells(0))'
    )
    assert out == "NNNNNNNNNN"


def test_edit_updates_the_highlight():
    # Opening a block comment on row 0 must recolor row 1 (cross-row fold, via refreshHighlight).
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("x")))\n'
        '  rs = rs.append(Row(Bytes.fromString("y")))\n'
        '  state.setRows(rs, "a.c")\n'
        '  println(cells(1))\n'                          # before: "N"
        '  state.jumpTo(1, 0)\n'                          # cursor to end of row 0
        "  state.insertChar(b'/')\n"
        "  state.insertChar(b'*')\n"                      # row 0 now "x/*" -> opens a comment
        '  println(cells(1))'                             # after: row 1 is comment -> "C"
    )
    assert out.splitlines() == ["N", "C"]


def test_set_filename_enables_highlighting():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int a;")))\n'
        '  state.setRows(rs, "")\n'
        '  println(cells(0))\n'                           # unnamed: all normal
        '  state.setFilename("a.c")\n'
        '  println(cells(0))'                             # named .c: int -> 222
    )
    assert out.splitlines() == ["NNNNNN", "222NNN"]


def test_set_and_clear_match_overlay():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("hello world")))\n'
        '  state.setRows(rs, "a.txt")\n'                  # plain text: normally all Normal
        '  state.setMatch(0, 6, 5)\n'                     # match "world" at chars col 6, len 5
        '  println(cells(0))\n'                           # cols 6..11 -> Matched
        '  state.clearMatch()\n'
        '  println(cells(0))'
    )
    assert out.splitlines() == ["NNNNNNMMMMM", "NNNNNNNNNNN"]


def test_delete_char_line_join_updates_highlight():
    # A line-join Backspace (deleteChar at col 0) grows the previous row; the highlight cache
    # must refresh so rowCells stays parallel to it. Guards the deleteChar refreshHighlight()
    # site — whose removal leaves rowHls short and crashes Screen mid-render on a .c buffer.
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int")))\n'   # a keyword on its own line -> "222"
        '  rs = rs.append(Row(Bytes.fromString("x")))\n'
        '  state.setRows(rs, "a.c")\n'
        '  state.jumpTo(0, 1)\n'                              # cursor row 1, col 0
        '  state.deleteChar()\n'                              # join -> row 0 = "intx" (1 row)
        '  println("{state.numRows()} [{cells(0)}]")'
    )
    # "intx" has no keyword boundary after "int", so all Normal (4 cells); a stale cache would
    # report the old "int" -> "222" (3 cells).
    assert out == "1 [NNNN]"


def test_match_overlay_maps_chars_to_render_across_a_tab():
    # A tab-indented row: render expands the tab to spaces, so the match's chars column must be
    # mapped through cxToRx to land on the right render cells. Guards overlayMatch's cxToRx — a
    # stub overlaying raw char columns would misplace the highlight only on a tab row.
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("a\\tworld")))\n'  # render = "a" + 7 spaces + "world"
        '  state.setRows(rs, "a.txt")\n'
        '  state.setMatch(0, 2, 5)\n'                              # "world" at chars col 2, len 5
        '  println(cells(0))'
    )
    # cxToRx(2)=8, cxToRx(7)=13 -> Matched over render cols 8..12 (the 5 "world" cells).
    assert out == "NNNNNNNNMMMMM"
