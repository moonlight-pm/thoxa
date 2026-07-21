"""Main.thx key dispatch — the byte→action mapping (kilo editorProcessKeypress).

Deterministic: feed decoded `Key`s to the extracted `dispatch(k)` and observe either the
returned `Action` (via `act(k)` → a label) or the shared `State` singleton. This covers the
exact byte→action wiring the pty smoke's substring checks cannot distinguish (a mis-mapped
Enter/Backspace/Delete, or a Ctrl-F that fails to request Find, would ship green through the
pty test but fails here). Stage 6 changed dispatch's return from `Bool` to `Action`
(Quit | RunFind | PromptSave | Resume) so the interactive sub-loops stay out of the pure
mapper — those loops are covered by the pty smoke in tests/test_kilo_keys.py."""
from __future__ import annotations

import pathlib

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = (
        'import state from "./State"\n'
        'import { dispatch, Quit, RunFind, PromptSave, Resume } from "./Main"\n'
        'import { Key, Press, Up, Delete, ctrlKey } from "./Keys"\n'
        "fn act(k!: Key) -> String {\n"
        "  match dispatch(k) {\n"
        '    Quit { "quit" }\n'
        '    RunFind { "find" }\n'
        '    PromptSave { "saveas" }\n'
        '    Resume { "resume" }\n'
        "  }\n"
        "}\n"
        "fn main() -> Int64 {\n"
        "state.setScreenSize(24, 80)\n"
        'state.setRows([], "t")\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "dispatch_probe.thx"), out=buf.append)
    return "".join(buf)


def test_main_type_checks_clean():
    p = KILO / "Main.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_printable_byte_inserts():
    out = _probe(
        "dispatch(Press(b'h'))\n"
        "dispatch(Press(b'i'))\n"
        'println("r0={state.row(0).text()} cx={state.cursorX()}")'
    )
    assert out == "r0=hi cx=2\n"


def test_enter_splits_the_line_not_inserts_a_byte():
    # THE anti-tautology test: a broken Enter (insertChar(b'\r')) leaves one row.
    out = _probe(
        "dispatch(Press(b'a'))\n"
        "dispatch(Press(b'b'))\n"                  # row0 "ab", cx=2
        'state.moveHome()\nstate.moveRight()\n'    # cx=1
        "dispatch(Press(b'\\r'))\n"                # Enter -> split
        'println("n={state.numRows()} r0={state.row(0).text()} r1={state.row(1).text()} '
        'cy={state.cursorY()}")'
    )
    assert out == "n=2 r0=a r1=b cy=1\n"


def test_backspace_byte_deletes():
    out = _probe(
        "dispatch(Press(b'a'))\ndispatch(Press(b'b'))\n"  # "ab"
        "dispatch(Press(127))\n"                          # Backspace (DEL)
        'println("r0={state.row(0).text()} cx={state.cursorX()}")'
    )
    assert out == "r0=a cx=1\n"


def test_ctrl_h_also_deletes():
    out = _probe(
        "dispatch(Press(b'a'))\ndispatch(Press(b'b'))\n"
        "dispatch(Press(ctrlKey(b'h')))\n"                # Ctrl-H == Backspace
        'println("r0={state.row(0).text()}")'
    )
    assert out == "r0=a\n"


def test_delete_key_forward_deletes_and_joins():
    out = _probe(
        "dispatch(Press(b'a'))\n"                  # row0 "a"
        "dispatch(Press(b'\\r'))\n"                # split -> row0 "a", row1 "", cy=1
        "dispatch(Press(b'b'))\n"                  # row1 "b"
        'state.moveUp()\nstate.moveEnd()\n'        # end of row0
        'dispatch(Delete())\n'                     # forward delete joins row1 -> "ab"
        'println("n={state.numRows()} r0={state.row(0).text()}")'
    )
    assert out == "n=1 r0=ab\n"


def test_ctrl_q_signals_quit_without_editing():
    out = _probe(
        'let a = act(Press(ctrlKey(b\'q\')))\n'
        'println("a={a} n={state.numRows()}")'
    )
    assert out == "a=quit n=0\n"


def test_esc_and_ctrl_l_are_noops():
    out = _probe(
        'let e = act(Press(27))\n'                         # ESC
        'let l = act(Press(ctrlKey(b\'l\')))\n'            # Ctrl-L
        'println("e={e} l={l} n={state.numRows()}")'
    )
    assert out == "e=resume l=resume n=0\n"


def test_arrow_moves_not_inserts():
    out = _probe(
        "dispatch(Press(b'x'))\n"                  # row0 "x", cx=1
        'dispatch(Up())\n'                         # movement, not an insert
        'println("n={state.numRows()} r0={state.row(0).text()}")'
    )
    assert out == "n=1 r0=x\n"


# -- Stage 6: Ctrl-F / save-as intents ---------------------------------------


def test_ctrl_f_requests_find():
    # Ctrl-F is a driver intent (RunFind), never an edit.
    out = _probe(
        'let a = act(Press(ctrlKey(b\'f\')))\n'
        'println("a={a} n={state.numRows()}")'
    )
    assert out == "a=find n=0\n"


def test_ctrl_s_on_unnamed_buffer_requests_saveas():
    # An unnamed buffer ("") cannot save inline; Ctrl-S requests the save-as prompt.
    out = _probe(
        'state.setRows([], "")\n'
        'let a = act(Press(ctrlKey(b\'s\')))\n'
        'println("a={a}")'
    )
    assert out == "a=saveas\n"


def test_ctrl_s_on_named_buffer_saves_inline_and_resumes(tmp_path):
    # A named buffer saves inline (no sub-loop) and resumes; dirty clears, file is written.
    dst = tmp_path / "d.txt"
    body = (
        'state.setRows([], "DST")\n'
        "dispatch(Press(b'h'))\n"
        "dispatch(Press(b'i'))\n"
        'let a = act(Press(ctrlKey(b\'s\')))\n'     # Ctrl-S on a named buffer -> inline save
        'println("a={a} dirty={state.isDirty()}")'
    ).replace("DST", str(dst))
    assert _probe(body) == "a=resume dirty=false\n"
    assert dst.read_bytes() == b"hi\n"


# -- Stage 5: quit-confirm (now via the Action label) ------------------------


def test_ctrl_q_on_dirty_warns_then_quits_on_the_fourth():
    # On a dirty buffer Ctrl-Q warns (resume) three times, then quits on the fourth.
    out = _probe(
        "dispatch(Press(b'x'))\n"                  # dirties the buffer
        'let q1 = act(Press(ctrlKey(b\'q\')))\n'
        'let q2 = act(Press(ctrlKey(b\'q\')))\n'
        'let q3 = act(Press(ctrlKey(b\'q\')))\n'
        'let q4 = act(Press(ctrlKey(b\'q\')))\n'
        'println("q1={q1} q2={q2} q3={q3} q4={q4}")'
    )
    assert out == "q1=resume q2=resume q3=resume q4=quit\n"


def test_non_quit_key_resets_the_quit_counter():
    # Two warnings, then a movement key resets the counter, so the next run of Ctrl-Q
    # starts the countdown over (q4, not q2, is the one that quits).
    out = _probe(
        "dispatch(Press(b'x'))\n"                  # dirty
        "dispatch(Press(ctrlKey(b'q')))\n"         # warn (3->2)
        "dispatch(Press(ctrlKey(b'q')))\n"         # warn (2->1)
        'dispatch(Up())\n'                         # a non-quit key -> reset to 3
        'let q1 = act(Press(ctrlKey(b\'q\')))\n'
        'let q2 = act(Press(ctrlKey(b\'q\')))\n'
        'let q3 = act(Press(ctrlKey(b\'q\')))\n'
        'let q4 = act(Press(ctrlKey(b\'q\')))\n'
        'println("q1={q1} q2={q2} q3={q3} q4={q4}")'
    )
    assert out == "q1=resume q2=resume q3=resume q4=quit\n"


def test_ctrl_f_resets_the_quit_counter():
    # Ctrl-F is not Ctrl-Q, so it resets the countdown (kilo resets on every non-quit key).
    out = _probe(
        "dispatch(Press(b'x'))\n"                  # dirty
        "dispatch(Press(ctrlKey(b'q')))\n"         # warn (3->2)
        "dispatch(Press(ctrlKey(b'q')))\n"         # warn (2->1)
        'act(Press(ctrlKey(b\'f\')))\n'            # Ctrl-F (RunFind) -> resets to 3
        'let q1 = act(Press(ctrlKey(b\'q\')))\n'
        'let q2 = act(Press(ctrlKey(b\'q\')))\n'
        'let q3 = act(Press(ctrlKey(b\'q\')))\n'
        'let q4 = act(Press(ctrlKey(b\'q\')))\n'
        'println("q1={q1} q2={q2} q3={q3} q4={q4}")'
    )
    assert out == "q1=resume q2=resume q3=resume q4=quit\n"
