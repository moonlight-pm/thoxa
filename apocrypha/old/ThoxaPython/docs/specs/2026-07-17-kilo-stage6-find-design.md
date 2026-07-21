# Thoxa × Kilo — Stage 6: Find (incremental search) + the modal line-input prompt

- **Date:** 2026-07-17
- **Status:** Approved design (Stage 6 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 5 (save + quit-confirm + bars).
- **Lands one language increment:**
  1. **`Bytes.find(needle) -> May<Int64>`** (§2) — a byte-buffer substring search, the
     Thoxa-idiomatic answer to kilo's `strstr`: `Has(index)` / `Not`, **never** a C `-1`
     sentinel. Interpreter + `ffi.py` only — **no checker change**.
- **Builds the modal line-input machinery** the roadmap parked in Stage 5: a generic
  `Prompt.promptLine` (which finally lets an unnamed buffer be **saved-as**) and, on top of
  the same idea, `Find`'s incremental search. New modules `examples/kilo/{Prompt,Find}.thx`;
  `examples/kilo/{State,Main}.thx` gain the small surface to drive them.
- **Match highlighting is deferred to Stage 7** (it belongs with the `hl` array that Syntax
  introduces). Stage-6 Find is fully functional without colour: it jumps the cursor to the
  match and scrolls it on-screen.

## 1. Context & goal

Stage 5 made kilo an editor that **persists**. Stage 6 makes it one you can **navigate and
name** — the two capabilities that both hang off one piece of machinery kilo calls
`editorPrompt`: a modal line-input loop that takes over the keyboard, echoes into the
message bar, and ends on Enter (confirm) or ESC (cancel).

- **Find** — kilo's `editorFind` + `editorFindCallback`: **Ctrl-F** opens a "Search:"
  prompt; as you type, the editor searches the file and jumps the cursor to the next match,
  incrementally; **arrows** step to the next/previous match (wrapping); **Enter** leaves the
  cursor at the match; **ESC** restores the pre-search cursor and scroll.
- **Save-as** — the Stage-5 deferral: **Ctrl-S** on an unnamed buffer (`filename == ""`)
  opens a "Save as:" prompt, and a confirmed name is set and saved. This is the *simple*
  use of the prompt (no per-keystroke callback), so it falls out of the same module for free.

kilo implements both with **one** `editorPrompt(prompt, callback)` — a line-input loop that
invokes a callback on every keystroke; `editorFind` passes the incremental-search callback,
save-as passes none. **Thoxa has no first-class functions** (the SPEC lists a `Fn(...)`
*type* but nothing constructs a function value, and the port has never needed one), so the
callback cannot be ported directly. Instead the two split cleanly:

- a **generic** `promptLine(label) -> May<String>` for the callback-free case (save-as), and
- **`Find`** inlining its own read/refresh loop, because the per-keystroke search *is* the
  callback we cannot pass — so it lives in the loop body rather than behind an interface.

This is the module split chosen up front (`Prompt` + `Find`, over folding both into one
`Find`): the prompt is genuinely reusable and callback-free; Find's loop is search-specific.

Almost everything reuses primitives already in the language — the prompt echoes through the
existing message bar (raw mode has no terminal echo), the query is an ordinary `Bytes` built
with `insertAt`/`slice`, cursor moves reuse the Stage-3 scroll. **One** genuinely-new thing
is forced: searching a byte buffer for a substring.

## 2. Language increment — `Bytes.find(needle: Bytes) -> May<Int64>`

### 2.1 What forces it

kilo's search is `strstr(row->render, query)` — find a substring within a line. Rows are
byte buffers (fork #5), the typed query is a `Bytes`, so this is **substring search over
`Bytes`**, and `Bytes` has no such method today (`get`/`set`/`toString`/`slice`/`insertAt`/
`removeAt` only). A byte buffer that can't be searched is an odd gap for a systems string
type — the same "grow the real primitive" reasoning as the earlier stages
(`thoxa-demo-is-means`), not a kilo-only convenience.

It *could* be written in pure Thoxa (a naïve `O(n·m)` double loop over `.get`), keeping
Stage 6 a zero-compiler-change authoring stage like Stage 0. We chose instead to **add the
primitive**, and — the real design call — to give it a **`May<Int64>` return, not a C-style
`-1`**. Thoxa's whole identity is "`May`/`Trial` instead of sentinels"; the search primitive
is exactly where that identity should show. `Has(i)` / `Not` cannot be confused with a valid
index the way `-1` silently can.

### 2.2 The decision

```thoxa
let hay = Bytes.fromString("hello world")
match hay.find(Bytes.fromString("wor")) {
    Has(i) { … }   # i == 6, the first (leftmost) index
    Not    { … }   # substring absent
}
```

- Returns `Has(i)` where `i` is the **leftmost** index at which `needle` occurs, else `Not`.
- **Empty needle → `Has(0)`** (Python/`strstr` semantics: the empty string occurs at the
  start). Find's own loop treats an empty query as "no search" separately (§4.2), so this
  edge never moves the cursor; it is pinned only so the primitive is totally defined.
- `needle` longer than the receiver → `Not`. A single-byte needle works (`find` of a
  `Bytes` of count 1). The argument is a `Bytes` (not a `Byte`), uniformly.

### 2.3 Implementation — interpreter + `ffi.py`, no checker change

**`thoxa/ffi.py`** — one method on `Bytes`, mirroring `slice`/`insert_at`:

```python
def find(self, needle: "Bytes") -> int:
    """First index where `needle` occurs, or -1 (bytes.find semantics)."""
    hay = self._buf.raw[: self._len]
    ndl = needle._buf.raw[: needle._len]
    return hay.find(ndl)
```

**`thoxa/interp.py`** — a `find` branch in `_bytes_method`, building the prelude `May`
variants exactly as the `?`/auto-wrap path does (`_wrap_has`, and `Instance(base.get("Not"))`
for the nullary `Not`), plus `"find"` in the `Bytes` member allow-list so `bytes.find`
resolves to a `BoundBuiltin`:

```python
if name == "find":
    if len(values) != 1 or not isinstance(values[0], Bytes):
        self.error("Bytes.find(needle) expects a Bytes buffer", span)
    idx = buf.find(values[0])
    return self._wrap_has(idx) if idx >= 0 else Instance(self.base.get("Not"))
# … and in _member: name in ("get","set","toString","slice","insertAt","removeAt","find")
```

**No checker change.** The checker types every `Bytes` member as `UNKNOWN` (it is the
*interpreter* that validates member names), so `bytes.find(q)` is `UNKNOWN` like `slice`.
Crucially, `match` on an `UNKNOWN` scrutinee is permissive: exhaustiveness is only enforced
when the scrutinee is a known `("union", …)` type, and `_bind_pattern` binds a
`ConstructorPattern`'s fields from the **pattern's own** constructor (`Has`'s `value`), not
from the scrutinee type — so `match hay.find(q) { Has(i) { … } Not { … } }` type-checks with
**no `else`** and `i` usable as an integer. Typing `find` precisely as `May<Int64>` (for
inferred hover) is a deferred nicety, consistent with the other `Bytes` methods being
`UNKNOWN`.

### 2.4 Conformance & tests

- **`spec/conformance/bytes_find.thx` (+ `.out`)** — the "done" gate: present substring →
  index; absent → a "not found" branch; leftmost of overlapping occurrences; needle at the
  start; needle at the end; needle longer than the haystack → not found; single-byte needle;
  empty needle → `0`. Printed as plain integers / words (control-byte-free stdout).
- **`tests/test_bytes_find.py`** — the same at the Python layer plus the argument-type guard
  (`find` of a non-`Bytes` traps cleanly) and embedded-NUL correctness (search survives a
  `\x00` in the haystack, since `Bytes` is length-counted, not NUL-terminated).
- **`spec/SPEC.md` §9.1** gains `.find(needle) -> May<Int64>` in the `Bytes` method list.

## 3. `examples/kilo/Prompt.thx` — the generic modal line-input

A new module: the callback-free line prompt. It reuses the message bar as its echo (raw mode
has no terminal echo) and the existing single-frame refresh; the query is an ordinary
`Bytes`. It imports `Keys` (read a key), `Screen` (redraw), and `State` (set the message) —
no cycle (nothing Screen/State/Keys depend on imports `Prompt`).

```thoxa
import { readKey, Key, Press, Up, Down, Left, Right, Home, End, PageUp, PageDown, Delete } from "./Keys"
import screen from "./Screen"
import state from "./State"

## Modal line-input (kilo editorPrompt, callback-free). Draws "{label}{typed}" in the
## message bar each keystroke; returns Has(text) on Enter with content, Not on ESC.
## Backspace erases; other control/navigation keys are ignored (a plain line editor).
export fn promptLine(label!: String) -> May<String> {
    mut query = Bytes(0)
    loop {
        state.setStatusMessage("{label}{query.toString()}")
        match screen.refresh() {
            Fail(e) { return Not() }     # can't draw the prompt → abort the prompt
            Pass { Void }
        }
        match readKey() {
            Fail(e) { return Not() }
            Pass(k) {
                match k {
                    Press(b) {
                        if b == 27 {                 # ESC → cancel
                            state.setStatusMessage("")
                            return Not()
                        } else if b == b'\r' {       # Enter → confirm iff non-empty
                            if query.count > 0 {
                                state.setStatusMessage("")
                                return Has(query.toString())
                            }
                        } else if b == 127 {         # Backspace / DEL
                            if query.count > 0 { query = query.slice(0, query.count - 1) }
                        } else if b == ctrlKeyH(b) {  # Ctrl-H also erases (see note)
                            if query.count > 0 { query = query.slice(0, query.count - 1) }
                        } else if isPrintable(b) {
                            query = query.insertAt(query.count, b)
                        }
                    }
                    else { Void }                    # navigation keys: ignored in a plain prompt
                }
            }
        }
    }
}
```

Two small helpers keep the `Press` arm readable and are independently testable:

```thoxa
## A byte is echoable text if it is not a C0 control byte and is ASCII (kilo: !iscntrl && <128).
fn isPrintable(b!: Byte) -> Bool {
    if b < 32 { false } else { b < 127 }
}
```

Ctrl-H handling: kilo folds `CTRL_KEY('h')` (8) into backspace. Rather than import `ctrlKey`
just to compare, the prompt tests `b == 8` directly (`ctrlKeyH` in the sketch is shorthand
for the literal `8`); the final code uses the literal with a comment, matching how `Main`
already compares control bytes. `\r` (13) and DEL (127) are the byte literals already used in
`Main`'s editing routing.

**Fade interaction.** `setStatusMessage` restamps `now()` every keystroke, and the loop
redraws once per key *before* blocking in `readKey`, so an actively-typed prompt never fades.
A pause > 5 s blanks the echoed text until the next key — the identical quirk kilo has
(its message bar checks `time(NULL) - statusmsg_time < 5` in the same loop); we match it
rather than special-casing.

## 4. `examples/kilo/Find.thx` — incremental search

### 4.1 The pure, deterministic core

The search algorithm is extracted as a free function of explicit arguments — the same
testability seam as Stage 5's `messageFor(clock)` — so it is unit-tested with no terminal,
no singleton, no keys:

```thoxa
import { Row } from "./Row"

## A search hit: file row + byte column. The immutable-record pattern (Stage-0 WindowSize):
## ctor params are private, projected into public `export let` reads, one-call construction.
export type Match(r!: Int64, c!: Int64) {
    export let row = r
    export let col = c
}

## kilo editorFindCallback's scan: from `lastMatchRow` step `direction` (+1/-1) through the
## rows, wrapping at both ends, and return the first row whose `chars` contain `query`
## (leftmost occurrence). `lastMatchRow == -1` starts a fresh scan at row 0. Row-granular
## and always leftmost-in-row — faithful to kilo (which likewise advances by whole rows).
export fn searchFrom(rows!: Array<Row>, query!: Bytes, lastMatchRow!: Int64,
                     direction!: Int64) -> May<Match> {
    if rows.count == 0 { return Not() }
    ## A fresh scan (no prior match) always steps forward from row 0 — otherwise a
    ## backward fresh scan would compute current = -2 and the single-step wrap misses it
    ## (kilo's `if (last_match == -1) direction = 1`).
    mut dir = direction
    if lastMatchRow == -1 { dir = 1 }
    mut current = lastMatchRow
    mut i = 0
    loop {
        if i >= rows.count { break }
        current = current + dir
        if current == -1 { current = rows.count - 1 }        # wrap to the last row
        if current == rows.count { current = 0 }             # wrap to the first row
        match rows[current].bytes().find(query) {
            Has(col) { return Has(Match(current, col)) }
            Not { Void }
        }
        i = i + 1
    }
    Not()
}
```

Searching each row's **`chars`** (not the tab-expanded `render`) means the match column *is*
the cursor's byte column — no `render`-to-`chars` conversion (kilo needs `editorRowRxToCx`
because it searches `render`; our byte model avoids it). `rows[current].bytes()` is `Row`'s
existing public byte accessor.

### 4.2 The interactive loop

```thoxa
import { readKey, Key, Press, Up, Down, Left, Right, Home, End, PageUp, PageDown, Delete } from "./Keys"
import screen from "./Screen"
import state from "./State"

let hint = " (ESC cancel | Arrows next/prev | Enter confirm)"

## Ctrl-F incremental search (kilo editorFind). Saves the cursor + scroll, runs a modal
## search loop that jumps to matches as the query changes, and on ESC restores the saved
## view (Enter keeps the current match). Match *highlighting* lands with Stage 7's hl array.
export fn run() -> Void {
    let savedCx = state.cursorX()
    let savedCy = state.cursorY()
    let savedColoff = state.colOff()
    let savedRowoff = state.rowOff()

    mut query = Bytes(0)
    mut lastMatch = -1              # -1 = no match yet (a fresh scan starts at row 0)
    mut direction = 1
    loop {
        state.setStatusMessage("Search: {query.toString()}{hint}")
        match screen.refresh() { Fail(e) { break } Pass { Void } }
        match readKey() {
            Fail(e) { break }
            Pass(k) {
                match k {
                    Press(b) {
                        if b == 27 { state.restoreView(savedCx, savedCy, savedColoff, savedRowoff)  break }  # ESC
                        else if b == b'\r' { break }                                                          # Enter
                        else if b == 127 {
                            if query.count > 0 { query = query.slice(0, query.count - 1) }
                            lastMatch = -1  direction = 1
                        } else if isPrintable(b) {
                            query = query.insertAt(query.count, b)
                            lastMatch = -1  direction = 1
                        }
                    }
                    Left  { direction = -1 }
                    Up    { direction = -1 }
                    Right { direction = 1 }
                    Down  { direction = 1 }
                    else  { Void }         # Home/End/PageUp/PageDown/Delete: ignored during search
                }
                ## Re-search on every key (kilo runs the callback each keystroke). An empty
                ## query never moves the cursor.
                if query.count > 0 {
                    match searchFrom(state.allRows(), query, lastMatch, direction) {
                        Has(m) { lastMatch = m.row  state.jumpTo(m.col, m.row) }
                        Not { Void }
                    }
                }
            }
        }
    }
    state.setStatusMessage("")
}
```

Notes:
- **ESC restores** all four saved coordinates via one `State.restoreView`; **Enter** leaves
  the cursor wherever the last match put it (kilo's behaviour).
- After the cursor jumps, the **next `screen.refresh()` scrolls** the match on-screen — the
  Stage-3 `scroll()` already recenters on `cy`, so `jumpTo` only sets `cx`/`cy`.
- A key that changes the query resets `lastMatch = -1` / `direction = 1` (kilo: a text edit
  restarts the scan forward from the top); an arrow only sets `direction` and re-scans from
  the current match.
- `isPrintable` is Find's own copy of the §3 helper (both modules filter typed bytes the
  same way). It is 3 lines; a shared `Prompt.isPrintable` import would couple Find to Prompt
  for no real reuse, so each keeps its own (both unit-tested).

## 5. `examples/kilo/State.thx` — the small surface Find/Prompt drive

Four exported additions, all plain State mutations/reads — no language change:

```thoxa
## The whole document, for Find's pure searchFrom (named around the `rows` screen-height
## field, which owns the `rows` name).
export fn allRows() -> Array<Row> { textRows }

## Move the cursor to a search hit; the next refresh's scroll() brings it on-screen.
export fn jumpTo(col!: Int64, row!: Int64) -> Void { cx = col  cy = row  Void }

## Restore a saved cursor + scroll (Find's ESC-cancel).
export fn restoreView(cxv!: Int64, cyv!: Int64, coloffv!: Int64, rowoffv!: Int64) -> Void {
    cx = cxv  cy = cyv  coloff = coloffv  rowoff = rowoffv
    Void
}

## Name an unnamed buffer (the save-as prompt), then save() writes it.
export fn setFilename(name!: String) -> Void { filename = name  Void }
```

`allRows()` returns the module's `textRows` (Rows are immutable, so read-only sharing is
safe). `jumpTo` deliberately sets only `cx`/`cy`; `restoreView` sets all four because a
cancel must undo horizontal/vertical scroll too. `setFilename` is the missing half of
Stage 5's `save()` — with it, an unnamed buffer is no longer a dead end.

## 6. `examples/kilo/Main.thx` — the intent seam, and running the sub-loops

The Stage-4/5 `dispatch(k) -> Bool` is a **pure key→action mapper that does no terminal
I/O** — the property that makes it unit-testable (`test_kilo_dispatch.py`). Find and the
save-as prompt *are* terminal I/O (their own read/refresh loops), so they must not run
*inside* `dispatch`. `dispatch` instead returns an **intent** the driver loop interprets:

```thoxa
export type Quit {}
export type RunFind {}
export type PromptSave {}
export type Resume {}
export type Action = Quit | RunFind | PromptSave | Resume

## Map a key to a driver action. Pure of terminal I/O (State mutations only) → unit-testable.
export fn dispatch(k!: Key) -> Action {
    if isQuitKey(k) {
        return if state.confirmQuit() { Quit() } else { Resume() }
    }
    ## Ctrl-F and an unnamed-buffer Ctrl-S need a modal sub-loop → hand back an intent.
    match findOrSaveIntent(k) {
        Has(a) { state.resetQuitTimes()  return a }
        Not { Void }
    }
    applyKey(k)
    state.resetQuitTimes()
    Resume()
}

## Ctrl-F → RunFind; Ctrl-S on an unnamed buffer → PromptSave (a named buffer saves inline
## in applyKey, no sub-loop). Nothing else needs the driver.
fn findOrSaveIntent(k!: Key) -> May<Action> {
    match k {
        Press(b) {
            if b == ctrlKey(b'f') { Has(RunFind()) }
            else if b == ctrlKey(b's') {
                if state.currentFilename().count == 0 { Has(PromptSave()) } else { Not() }
            } else { Not() }
        }
        else { Not() }
    }
}
```

`applyKey` keeps the Stage-5 routing; its `Ctrl-S` arm now only runs for a **named** buffer
(the unnamed case is intercepted above as `PromptSave`), so it still calls `state.save()`
directly. The driver loop interprets the intent — the only place the sub-loops run:

```thoxa
match dispatch(k) {
    Quit       { break }
    Resume     { Void }
    RunFind    { find.run() }
    PromptSave {
        match prompt.promptLine("Save as (ESC to cancel): ") {
            Has(name) { state.setFilename(name)  state.save() }
            Not       { state.setStatusMessage("Save aborted") }
        }
    }
}
```

The startup hint gains Find: `"HELP: Ctrl-S = save | Ctrl-F = find | Ctrl-Q = quit"`.

**Why an intent, not a direct call:** keeping `dispatch` I/O-free preserves the deterministic
seam Stage 4 built and Stage 5 kept — the dispatch tests stay pure key→`Action` assertions,
and the interactive sub-loops are covered by the pty smoke. It is the same "extract the
testable core" move applied to the driver rather than the search.

## 7. Verification

Per the roadmap discipline (**design → doc + `SPEC.md` → conformance test → TDD → green →
commit**), Stage 6 is "done" only when these are green:

1. **`spec/conformance/bytes_find.thx` + `.out`** — the `Bytes.find` gate (§2.4).
2. **`tests/test_bytes_find.py`** — `find` at the Python layer: present/absent/leftmost/
   at-start/at-end/needle-too-long/single-byte/empty-needle, the non-`Bytes` argument trap,
   embedded-NUL correctness.
3. **`tests/test_kilo_find.py`** — the `Find` and `Prompt` modules type-check, and
   deterministic `searchFrom` (no terminal): a match in the first row, a later row, absent →
   `Not`, **forward wrap** (match before the start row is found by wrapping), **backward
   wrap** (`direction = -1`), leftmost-in-row, empty document → `Not`, and the fresh-backward
   `-1`/`-1` no-underflow guard.
4. **The interactive loops** (`Find.run`, `Prompt.promptLine`) are terminal-driven, so they
   are covered end to end by the pty smoke (item 7), not a separate unit file — this keeps
   the pty harness single-sourced in `test_kilo_keys.py`.
5. **`tests/test_kilo_dispatch.py`** (updated) — the new `Action` return: Ctrl-F → `RunFind`;
   Ctrl-S unnamed → `PromptSave`; Ctrl-S named → `Resume` (saved inline); Ctrl-Q → `Quit`/
   `Resume`; a printable/mover → `Resume` and resets the quit counter.
6. **`tests/test_kilo_state.py`** (extended) — `allRows` returns the loaded rows; `jumpTo`
   sets `cx`/`cy`; `restoreView` restores all four; `setFilename` names an unnamed buffer so
   a subsequent `save()` writes it (real temp file).
7. **`tests/test_kilo_keys.py`** (updated pty smoke) — the end-to-end driver holds with the
   `Action` dispatch, and three new flows over a real pty: **find** (Ctrl-F, type `five`,
   Enter → the match scrolls into a 3-row viewport), **find-step-and-restore** (Ctrl-F, type
   `line`, **Down** steps forward to the next match, **ESC** restores the pre-search cursor —
   read off the status bar's `line/total` indicator, which discriminates a swapped/ignored
   arrow arm *and* a broken ESC restore), and **save-as** (a new unnamed buffer, type a byte,
   Ctrl-S → the save-as prompt, type an absolute path, Enter → the file is written and the
   "bytes written" message renders) — exercising `Prompt.promptLine` end to end. The existing
   Stage-0..5 flows (raw mode, edit, inline save) still hold under the new dispatch.
8. **`spec/SPEC.md`** §9.1 updated (§2.3); **roadmap** Stage-6 line marked done with this doc
   + the conformance test; **`CHANGELOG.md`** `[Unreleased]` entry — added once green.

## 8. Out of scope / deferred

- **Match highlighting** (kilo `HL_MATCH`) — needs the per-render-cell `hl` array that
  **Stage 7 (Syntax)** introduces; Find jumps + scrolls to the match now, colours it then.
- **Next-occurrence-within-a-row** search — kilo is row-granular (always the leftmost match
  in a row); we match it. Sub-row stepping would need a `fromCol` in `searchFrom`; deferred
  as it diverges from kilo for no demo need.
- **Precise `May<Int64>` typing of `Bytes.find`** in the checker (for inferred hover) — the
  method stays `UNKNOWN` like the other `Bytes` methods; a deferred hover nicety.
- **A shared prompt/Find key-edit core** — the ~3-line printable/backspace filter is
  duplicated in `Prompt` and `Find` (each unit-tested) rather than abstracted; a shared
  `editStep` is possible if a third caller appears.
- **First-class functions / a real `editorPrompt(callback)`** — the callback split (§1) is a
  workaround for their absence, not a request for them; unrelated to this stage.
