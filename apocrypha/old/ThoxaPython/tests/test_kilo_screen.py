"""Stage 3 of the kilo port: the `Screen` module (examples/kilo/Screen.thx).

`Screen` builds one terminal frame in a `Buffer` and flushes it with a single
`write()` to fd 1. It draws the loaded file's visible window — each file row's
tab-expanded `render`, horizontally windowed by `coloff` and vertically offset by
`rowoff` — with a `~` past end-of-file and the centred welcome banner only for an
empty buffer. The cursor is positioned from the render coordinate `rx`.

These deterministic tests drive `refresh()` through the fd-1 capture bridge (no tty
needed — `Screen` only writes) and assert the exact emitted byte stream. The
raw-mode paint over a real pseudo-terminal is exercised end to end in
tests/test_kilo_keys.py.
"""

from __future__ import annotations

import pathlib

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"

WELCOME = "Kilo editor -- version 0.0.1"


def _run_screen(setup: str) -> str:
    """Run a probe that imports ./Screen and ./State, applies `setup` (State
    mutations), calls refresh(), and returns the bytes Screen wrote to fd 1."""
    src = (
        'import state from "./State"\n'
        'import screen from "./Screen"\n'
        'import { Row } from "./Row"\n'
        "fn main() -> Int64 {\n"
        f"{setup}"
        '    match screen.refresh() { Pass { Void } Fail(e) { println("FAIL {e}") } }\n'
        "    0\n"
        "}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "screen_probe.thx"), out=buf.append)
    return "".join(buf)


# -- the module type-checks --------------------------------------------------

def test_screen_module_type_checks_clean():
    path = KILO / "Screen.thx"
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


# -- empty buffer: the welcome screen ----------------------------------------

def test_frame_exact_bytes_small():
    # 5 rows x 30 cols, empty buffer, cursor at (0,0). Stage 5 reserves the bottom 2 rows
    # for the status+message bars, so textHeight()==3. Welcome fits (28 <= 30): padding
    # (30-28)/2 == 1, so one leading '~' and no spaces. Welcome row is 3/3 == 1. Every
    # text row now ends \r\n (a bar follows below); the message bar is the last line.
    out = _run_screen("    state.setScreenSize(5, 30)\n")
    status = "\x1b[7m" + "[No Name] - 0 lines" + " " * 8 + "1/0" + "\x1b[m"
    assert out == (
        "\x1b[?25l"          # hide cursor
        "\x1b[H"             # cursor home
        "~\x1b[K\r\n"        # row 0: tilde, erase-to-EOL, newline
        f"~{WELCOME}\x1b[K\r\n"  # row 1: welcome (pad 1 -> one '~'), erase, newline
        "~\x1b[K\r\n"        # row 2: tilde (every text row ends \r\n now)
        + status + "\r\n"    # status bar: reverse video, [No Name], right-aligned 1/0
        + "\x1b[K"           # message bar: empty message -> just erase-to-EOL (last line)
        + "\x1b[1;1H"        # reposition cursor to (0,0) -> 1;1
        "\x1b[?25h"          # show cursor
    )


def test_welcome_truncates_when_too_narrow():
    # 5 rows x 10 cols: welcome (28) wider than the terminal -> truncated to 10 bytes via
    # Bytes.slice. textHeight()==3, welcome centred on row 1. The status bar's left text is
    # likewise truncated to the width (leaving no room for the right position group).
    out = _run_screen("    state.setScreenSize(5, 10)\n")
    assert out == (
        "\x1b[?25l\x1b[H"
        "~\x1b[K\r\n"
        "Kilo edito\x1b[K\r\n"        # welcome row: banner.slice(0, 10)
        "~\x1b[K\r\n"
        "\x1b[7m[No Name] \x1b[m\r\n"  # status bar: left truncated to width 10, no right
        "\x1b[K"                       # message bar (empty)
        "\x1b[1;1H\x1b[?25h"
    )


# -- a loaded file: rows, scroll, cursor -------------------------------------

def _with_rows(lines: list[str], rows: int, cols: int, moves: str = "", name: str = "t") -> str:
    setup = f"    state.setScreenSize({rows}, {cols})\n    mut rs: Array<Row> = []\n"
    for line in lines:
        setup += f'    rs = rs.append(Row(Bytes.fromString("{line}")))\n'
    setup += f'    state.setRows(rs, "{name}")\n' + moves
    return _run_screen(setup)


def test_draws_file_rows_no_welcome():
    # 4 rows x 10 (textHeight 2), one text line "abcdef": row0 shows the line, row1 a
    # tilde, NO welcome; the status bar follows in reverse video.
    out = _with_rows(["abcdef"], 4, 10)
    assert WELCOME not in out
    assert out.startswith("\x1b[?25l\x1b[H")
    assert "abcdef\x1b[39m\x1b[K\r\n" in out   # row 0 = the file line (+ per-row SGR reset)
    assert "~\x1b[K\r\n" in out        # row 1 = past EOF -> tilde (no reset; not a drawn row)
    assert "\x1b[7m" in out            # the status bar renders in reverse video
    assert out.endswith("\x1b[?25h")


def test_tab_expanded_row_is_drawn():
    # "a\tb" renders as 'a' + spaces to the next tab stop (col 8) + 'b' = "a       b".
    # 3 rows -> textHeight 1, so the single text row is this line.
    out = _with_rows(["a\\tb"], 3, 20)
    assert "a       b\x1b[39m\x1b[K" in out


def test_horizontal_scroll_windows_the_row():
    # a line wider than the screen, cursor driven right past the edge -> coloff advances.
    # 3 rows -> textHeight 1 (the horizontal window is unaffected by the reserved rows).
    moves = "    mut k = 0\n    loop { if k >= 6 { break } state.moveRight() k = k + 1 }\n"
    out = _with_rows(["0123456789"], 3, 4, moves)
    # cursor at cx=6 -> coloff makes a 4-wide window ending at col 6 -> "3456"
    assert "3456\x1b[39m\x1b[K" in out
    assert "\x1b[1;4H" in out           # cursor pinned at the right edge (rx-coloff+1 = 4)


def test_cursor_row_follows_file_cursor():
    # 3 lines on a 5-row screen; one Down moves the cursor to file row 1 -> CUP row 2.
    out = _with_rows(["aaa", "bbb", "ccc"], 5, 20, "    state.moveDown()\n")
    assert WELCOME not in out
    assert "\x1b[2;1H" in out           # (cy=1 - rowoff=0)+1 = 2 ; (rx=0 - coloff=0)+1 = 1


# -- clear() -----------------------------------------------------------------

def test_clear_wipes_and_homes():
    src = (
        'import screen from "./Screen"\n'
        "fn main() -> Int64 {\n"
        '    match screen.clear() { Pass { Void } Fail(e) { println("FAIL {e}") } }\n'
        "    0\n"
        "}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "screen_probe.thx"), out=buf.append)
    assert "".join(buf) == "\x1b[2J\x1b[H"


# -- Stage 7: syntax-colored draw ---------------------------------------------

def test_syntax_number_is_red():
    # a .c file: the number 42 is painted red (\x1b[31m), "int" kw2 green (\x1b[32m).
    frame = _with_rows(["int x = 42;"], 6, 40, name="a.c")
    assert "\x1b[31m" in frame       # HL_NUMBER red
    assert "\x1b[32m" in frame       # kw2 green


def test_all_normal_row_emits_no_color():
    frame = _with_rows(["plain text here"], 6, 40, name="a.txt")
    for code in ("\x1b[31m", "\x1b[32m", "\x1b[33m", "\x1b[35m", "\x1b[36m", "\x1b[34m"):
        assert code not in frame


def test_set_match_paints_blue():
    frame = _with_rows(
        ["hello world"], 6, 40,
        moves="    state.setMatch(0, 6, 5)\n", name="a.txt",
    )
    assert "\x1b[34m" in frame        # HL_MATCH blue over "world"
