"""Stage 1 of the kilo port: the `Keys` and `State` modules, and the movable-cursor
driver (examples/kilo/{Keys,State,Main}.thx).

Deterministic tests pin that the modules type-check and that `State`'s cursor
clamping is correct. A real pseudo-terminal then drives the whole editor to prove,
end to end, that: raw mode engages (ICANON off → a single arrow key moves the cursor
with no Enter, ISIG off → Ctrl-C is a byte, not a signal, so the process survives),
`Keys.readKey()` decodes VT100 escape sequences (arrows, End, PageDown, Home —
including the multi-byte `[6~` form), and the cursor moves and clamps to the pty's
real size — observable in Stage 2's painted output as the per-frame CUP escape
(`\x1b[<row>;<col>H`). Ctrl-Q quits and the screen is cleared on exit. This is also
the raw-mode-engagement smoke for the whole port (the Stage 0/1 drivers it used to
live beside were replaced by the Stage 2 painting driver).
"""

from __future__ import annotations

import os
import pathlib
import re
import sys
import time

import pytest

from thoxa.checker import check

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


# -- the modules type-check --------------------------------------------------

@pytest.mark.parametrize("name", ["Keys", "State", "Main"])
def test_module_type_checks_clean(name):
    path = KILO / f"{name}.thx"
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


# State cursor movement (wrap / snap / scroll) now lives in tests/test_kilo_state.py.


# -- the whole editor, through a real pseudo-terminal ------------------------

def _drive_kilo_pty(keys, winsize=(3, 40), wait_for=b"Kilo, ported", timeout=8.0,
                    extra_args=("examples/kilo/sample.txt",)):
    import fcntl
    import pty
    import select
    import struct
    import subprocess
    import termios

    thoxa = pathlib.Path(sys.executable).parent / "thoxa"
    if not thoxa.exists():
        pytest.skip("thoxa CLI not found next to the interpreter")

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", winsize[0], winsize[1], 0, 0))
    proc = subprocess.Popen(
        [str(thoxa), "run", "examples/kilo", *extra_args],
        cwd=str(REPO), stdin=slave, stdout=slave, stderr=slave,
        close_fds=True, start_new_session=True,
    )
    os.close(slave)

    def drain(t=0.4):
        out = b""
        while True:
            r, _, _ = select.select([master], [], [], t)
            if not r:
                break
            try:
                chunk = os.read(master, 4096)
            except OSError:
                break
            if not chunk:
                break
            out += chunk
        return out

    buf = b""
    deadline = time.time() + timeout
    while wait_for not in buf and time.time() < deadline:
        buf += drain(0.2)
    for key in keys:
        os.write(master, key)
        time.sleep(0.08)
        buf += drain(0.4)
    buf += drain(0.6)
    try:
        code = proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        code = None
    os.close(master)
    return buf.decode(errors="replace"), code


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_viewer_renders_file_and_scrolls_over_a_pty():
    # Open the bundled sample file on a 5-row x 40-col screen: Stage 5 reserves 2 rows for
    # the status+message bars, so textHeight()==3 text rows. Down x4 walks the cursor to
    # the last line (cy=4), advancing rowoff to 2 so "line five" comes into view; Ctrl-C
    # proves ISIG is off (a byte, not a signal — and, being an ordinary byte, it inserts,
    # dirtying the buffer). Quitting a dirty buffer now takes the quit-confirm's four
    # Ctrl-Q presses. The pty's real size is read via ioctl(TIOCGWINSZ).
    quit_confirm = [b"\x11", b"\x11", b"\x11", b"\x11"]
    keys = [b"\x1b[B", b"\x1b[B", b"\x1b[B", b"\x1b[B", b"\x03", *quit_confirm]
    text, code = _drive_kilo_pty(keys, winsize=(5, 40), wait_for=b"Kilo, ported")

    # A file is loaded, so the welcome banner is NOT drawn.
    assert "Kilo editor -- version 0.0.1" not in text, text
    # The first file line rendered (raw mode entered, file read + split into rows).
    assert "Kilo, ported to Thoxa." in text, text
    # Tab expansion end to end: the leading tab became spaces to the next stop (col 8).
    assert "        an indented line" in text, text
    # The long third line is windowed to the screen width, drawn from column 0.
    assert "The quick brown fox" in text, text
    # ICANON off: single arrow keys (no Enter) scrolled the view down until the last
    # line, "line five", came on screen (off-screen in the initial 3-text-row frame).
    assert "line five" in text, text
    # Stage 5: the status bar renders in reverse video, and the quit-confirm warning fired
    # (the buffer is dirty from the inserted Ctrl-C byte).
    assert "\x1b[7m" in text, text
    assert "Press Ctrl-Q" in text, text
    # The screen was cleared on entry and on exit.
    assert "\x1b[2J" in text, text
    # ISIG off: Ctrl-C survived; after the four quit-confirm presses the editor exits clean.
    assert code == 0


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_types_newline_and_deletes_over_a_pty():
    # New empty buffer (no file arg): the welcome banner shows first. Type "hi",
    # Enter, "yo", then Backspace and Delete (the design's key set). The end-to-end
    # raw-mode proof: typed text is painted, and Enter *actually split the line* —
    # asserted by the cursor reaching screen row 2 (CUP "\x1b[2;<col>H"), which is
    # impossible with a single-row buffer, so a mis-mapped Enter (insertChar) would
    # fail here too. Byte-exact edit correctness lives in tests/test_kilo_dispatch.py.
    # Typing dirties the buffer, so the final quit takes the quit-confirm's four Ctrl-Q.
    keys = [b"h", b"i", b"\r", b"y", b"o", b"\x7f", b"\x1b[3~",
            b"\x11", b"\x11", b"\x11", b"\x11"]
    text, code = _drive_kilo_pty(
        keys, winsize=(5, 40), wait_for=b"Kilo editor", extra_args=()
    )
    assert "hi" in text, text          # first line typed and painted
    assert "yo" in text, text          # second line, painted after Enter split it off
    assert "\x1b[2;" in text, text     # cursor reached screen row 2 -> the line really split
    assert "(modified)" in text, text  # Stage 5: the status bar flags unsaved changes
    assert "Press Ctrl-Q" in text, text  # the dirty quit-confirm warning rendered
    assert "\x1b[2J" in text, text     # screen cleared on entry/exit
    assert code == 0                   # Backspace + Delete decoded, quit-confirm honored, clean exit


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_saves_to_disk_over_a_pty(tmp_path):
    # Open a real file, insert a byte at the top, Ctrl-S to save, then Ctrl-Q. Because the
    # save clears `dirty`, a single Ctrl-Q quits cleanly (no confirm). End-to-end proof of
    # the Stage 5 save path: the file on disk changed, the "bytes written" message and the
    # status bar rendered.
    target = tmp_path / "edit.txt"
    target.write_text("old\n", encoding="utf-8")
    keys = [b"Z", b"\x13", b"\x11"]    # 'Z' (insert -> dirty), Ctrl-S (save), Ctrl-Q (clean quit)
    text, code = _drive_kilo_pty(
        keys, winsize=(6, 40), wait_for=b"HELP", extra_args=(str(target),)
    )
    assert code == 0
    assert target.read_bytes() == b"Zold\n"        # the edit was written to disk
    assert "bytes written to disk" in text, text   # the save result message rendered
    assert "\x1b[7m" in text, text                  # the status bar's reverse video rendered


# -- Stage 6: Find + save-as, through the real pty ---------------------------


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_finds_and_scrolls_to_match_over_a_pty():
    # Open the sample on a 5-row (=3 text rows) screen: "line five" is off-screen initially.
    # Ctrl-F, type "five", Enter: the incremental search jumps the cursor to row 4 and the
    # next refresh scrolls it into view. No edit happened, so a single Ctrl-Q quits clean.
    # This is the end-to-end proof of Find.run + searchFrom + jumpTo driving the real editor.
    keys = [b"\x06", b"f", b"i", b"v", b"e", b"\r", b"\x11"]   # Ctrl-F "five" Enter Ctrl-Q
    text, code = _drive_kilo_pty(keys, winsize=(5, 40), wait_for=b"Kilo, ported")
    assert "Search:" in text, text          # the incremental-search prompt rendered
    assert "line five" in text, text        # the match scrolled into the 3-row viewport
    assert code == 0                         # ESC/Enter loop exited, clean quit


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_find_steps_with_arrows_and_escape_restores_over_a_pty():
    # The discriminating interactive-search test, read off the status bar's "line/total"
    # indicator (deterministic text, unlike a bare cursor CUP). sample.txt has "line" on
    # rows 1, 3, 4. Ctrl-F, type "line" -> first match is row 1 (status "2/5"); Down steps
    # FORWARD to the next match row 3 ("4/5"); ESC restores the pre-search cursor (row 0,
    # "1/5"). This kills two stubs at once: a swapped/ignored Down arm would step BACKWARD
    # to row 4 ("5/5") instead, and a broken ESC restore would leave the cursor at "4/5".
    keys = [b"\x06", b"l", b"i", b"n", b"e", b"\x1b[B", b"\x1b", b"\x11"]  # Ctrl-F "line" Down ESC Ctrl-Q
    text, code = _drive_kilo_pty(keys, winsize=(6, 40), wait_for=b"Kilo, ported")
    assert "Search:" in text, text          # the incremental-search prompt rendered
    assert "2/5" in text, text              # typing "line" jumped the cursor to the first match (row 1)
    assert "4/5" in text, text              # Down stepped FORWARD to the next match (row 3)
    assert "5/5" not in text, text          # a swapped/broken Down would have stepped BACKWARD to row 4
    positions = re.findall(r"(\d+)/5", text)
    assert positions[-1] == "1", text       # ESC restored the pre-search cursor (row 0) — not left at "4"
    assert code == 0                         # the ESC branch exited the loop; clean quit


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_save_as_prompt_writes_a_new_file_over_a_pty(tmp_path):
    # New unnamed buffer: type "Z" (dirties), Ctrl-S opens the save-as prompt (an unnamed
    # buffer can't save inline). Type an absolute path, Enter to confirm -> setFilename +
    # save writes it and clears dirty, so a single Ctrl-Q quits clean. End-to-end proof of
    # dispatch(PromptSave) + Prompt.promptLine + State.setFilename + save.
    dst = tmp_path / "s.txt"
    typed = [bytes([c]) for c in str(dst).encode()]
    keys = [b"Z", b"\x13", *typed, b"\r", b"\x11"]   # 'Z' Ctrl-S <path> Enter Ctrl-Q
    text, code = _drive_kilo_pty(
        keys, winsize=(6, 40), wait_for=b"HELP", extra_args=(), timeout=10.0
    )
    assert "Save as" in text, text                   # the save-as prompt rendered
    assert code == 0
    assert dst.read_bytes() == b"Z\n"                # the buffer was written under the typed name
    assert "bytes written to disk" in text, text


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_highlights_c_syntax_over_a_pty():
    # Open the C sample in a tall window so several highlight classes are visible in one frame.
    out, _ = _drive_kilo_pty(
        [b"\x11"],                       # Ctrl-Q to quit (clean buffer quits immediately)
        winsize=(14, 80),
        wait_for=b"kilo sample",         # the first comment line's text (a cyan run)
        extra_args=("examples/kilo/sample.c",),
    )
    assert "\x1b[36m" in out, out        # comment cyan
    assert "\x1b[31m" in out, out        # the number 42 red
    assert "\x1b[32m" in out, out        # int/char kw2 green
    assert "\x1b[35m" in out, out        # the "world" string magenta


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_find_match_highlights_blue_over_a_pty():
    # Ctrl-F, type "count" (a plain identifier -> normally uncolored) so the blue match overlay
    # is unambiguous; Enter keeps the match. The blue frame is captured during the search, before
    # Enter clears the highlight.
    out, _ = _drive_kilo_pty(
        [b"\x06", b"count", b"\r"],
        winsize=(14, 80),
        wait_for=b"kilo sample",
        extra_args=("examples/kilo/sample.c",),
    )
    assert "\x1b[34m" in out, out        # HL_MATCH blue


@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_find_clears_stale_match_over_a_pty():
    # After a match, a keystroke that stops matching must remove the blue HL_MATCH (kilo restores
    # the row highlight every keystroke). Type Ctrl-F "count" (blue appears), then "z" so the
    # query "countz" matches nothing -> the frame whose prompt reads "Search: countz" must carry
    # no match-blue. \x1b[34m is used ONLY by the match overlay, so its absence there is decisive.
    out, _ = _drive_kilo_pty(
        [b"\x06", b"count", b"z", b"\x1b", b"\x11"],   # Ctrl-F count z ESC Ctrl-Q
        winsize=(14, 80),
        wait_for=b"kilo sample",
        extra_args=("examples/kilo/sample.c",),
    )
    frames = out.split("\x1b[?25l")                    # each refresh begins by hiding the cursor
    countz = [f for f in frames if "Search: countz" in f]
    assert countz, "no 'countz' frame was captured"
    assert all("\x1b[34m" not in f for f in countz), countz[-1]     # the save result message rendered
