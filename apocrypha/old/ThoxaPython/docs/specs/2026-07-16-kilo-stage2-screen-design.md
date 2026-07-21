# Thoxa × Kilo — Stage 2: the `Screen` module (and string/`Char` `\xNN` escapes)

- **Date:** 2026-07-16
- **Status:** Approved design (Stage 2 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 1 (`Keys` + `State`).
- **Lands the language increment deferred from Stage 1**: `\xNN` in `Char`/`String`
  literals (ASCII-restricted, `0x00`–`0x7f`).

## 1. Context & goal

Stage 2 delivers the output half of the editor: `examples/kilo/Screen.thx` builds
one terminal frame in a `Buffer` and flushes it with a **single `write()`**. The
*skeleton* draws exactly what kilo draws before a file is open — a `~` down the
left column, a centered welcome banner a third of the way down, and the cursor
positioned from `State` — with kilo's flicker-free refresh discipline (hide the
cursor, home it, erase each line as it is drawn, reposition, show the cursor, one
write). `Main.thx` grows from Stage 1's text echo into
`loop { screen.refresh(); dispatch(readKey()) }`.

There are no text rows yet (that is Stage 3) — the drawn tildes, the welcome
banner, and a cursor you can drive with the arrow keys are the visible proof that
`Screen` works. The Stage 1 driver *printed* the cursor position as text; Stage 2
*paints* it.

Along the way it lands the one language increment the roadmap parked for this
stage: string/`Char` `\xNN` escapes (§2), because every VT100 sequence begins with
ESC (`\x1b`), which no `"…"` literal can express today.

## 2. String/`Char` `\xNN` escapes — the language increment

### 2.1 What Screen forces

The whole frame is escape sequences: `"\x1b[?25l"` (hide cursor), `"\x1b[H"`
(home), `"\x1b[K"` (erase line), `"\x1b[2J"` (clear), `"\x1b[{row};{col}H"`
(position cursor), `"\x1b[?25h"` (show cursor). Each begins with the ESC byte
`0x1b`. But Thoxa string literals accept only `\n \t \r \\ \' \" \0` today —
`\xNN` was **scoped to byte literals** in Stage 1 (see that doc's §2.3 and its
deferred list), deliberately leaving "what does `\xNN` mean in a UTF-8 string"
until a stage forced it. Stage 2 forces it: `"\x1b[2J"` must lex.

### 2.2 The decision: extend `\xNN` to `Char`/`String`, ASCII-restricted

Add the `\xNN` escape (exactly two hex digits) to `'…'` and `"…"` literals,
denoting the character whose codepoint is `0xNN`, **restricted to `0x00`–`0x7f`
(ASCII)**. A value `≥ 0x80` is a lex error.

The restriction is the point of difference from the byte literal:

- **A byte literal is a byte** — `b'\xff'` is the number 255, and the full
  `0x00`–`0xFF` range is meaningful.
- **A `Char` is a Unicode scalar and a `String` is UTF-8** — a lone byte `≥ 0x80`
  is neither a valid UTF-8 code unit nor an unambiguous scalar (`\xE9` could mean
  Latin-1 `é` or the first byte of a UTF-8 sequence). So in a `Char`/`String`,
  `\xNN` is only well-defined for ASCII, where codepoint ≡ byte and UTF-8 is a
  single byte. This is exactly Rust's rule (`\x` in a non-byte string must be
  `≤ 0x7F`; `\u{…}` reaches higher) and mirrors the byte literal's *bare-char*
  ASCII rule from Stage 1 §2.3.

Every VT100 sequence kilo emits is pure ASCII, so the restriction costs the port
nothing. Non-ASCII escapes (`\u{…}`) are deferred — no stage forces them yet.

### 2.3 Grammar & escape rules

- `\xNN` (**exactly two hex digits**, case-insensitive) is now accepted in `'…'`
  and `"…"`, in addition to the existing `\n \t \r \\ \' \" \0`.
- In a `Char`/`String` it denotes the character with codepoint `0xNN` and is
  **restricted to `0x00`–`0x7f`**. `\x80`+ is a lex error:
  *"`\xNN` escape in a string/char must be ASCII (0x00–0x7f)"*.
- Malformed `\x` (fewer than two hex digits, non-hex) is the existing byte-literal
  error: *"`\x` escape requires two hex digits"*.
- **Byte literals are unchanged** — `b'\xNN'` keeps the full `0x00`–`0xFF` range
  (a byte is not a scalar).

### 2.4 Implementation — one shared `_escape` site

`char()` and `string()` both resolve backslash escapes through the *same*
`Lexer._escape()` helper (`thoxa/lexer.py`). Adding `\xNN` there extends it to
`Char` **and** `String` in one place — a mechanical mirror of the existing
`_byte_escape()`, with the ASCII guard added and returning a `chr(value)` string
instead of an int.

| Site | today | change |
| --- | --- | --- |
| `lexer.py` `_escape` (shared by `char()` + `string()`) | `_ESCAPES` table only | add a `c == "x"` branch: read two `_HEXDIGITS`, require `value <= 0x7F`, return `chr(value)` |
| `lexer.py` `byte()` / `_byte_escape` | `\xNN` over `0x00`–`0xFF` | **unchanged** |
| `checker.py` / `interp.py` | `CharLit → "Char"`; string parts are `str` | **unchanged** — an ASCII `\xNN` is an ordinary scalar; the escape is fully resolved in the lexer |
| `lsp/*` | uses the real lexer | **unchanged** — `"\x1b…"` now lexes clean, so no diagnostic |
| `SPEC.md` | §1.5 "`\xNN` is not yet accepted in `Char`/`String`" | relax to "accepted, ASCII-restricted"; note in §8 |

The reuse of `_HEXDIGITS` and the two-hex-digit shape from Stage 1's
`_byte_escape` (lexer.py) makes this a near-copy; the only new logic is the
`value <= 0x7F` guard.

## 3. `examples/kilo/Screen.thx`

A new sibling module, `Screen`, following the Terminal/Keys/State house style:
private `sys`-prefixed FFI hidden behind `Trial`-returning semantic functions,
`export` for the public surface only. Screen is **stateless** — a frame derives
entirely from `State` (cursor + size), so it exposes plain functions (like
Terminal's `windowSize()`), not a stateful type. It depends only on `State` (it
owns its own `write` extern, so it needs nothing from `Terminal`).

### 3.1 Private FFI layer (own `sysWrite`, mirrors `Keys`/`Net`/`Io`)

```thoxa
import state from "./State"
import { errno, strerror } from "ffi"

let stdoutFileno = 1

## write(2) — the single-syscall frame flush. Private, per the established
## per-module extern convention (Keys re-declares its own `sysRead`).
extern fn sysWrite(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c" "write"

fn screenErr(what!: String) -> String { "{what}: {strerror(errno())}" }
```

### 3.2 Public semantic API — `refresh()` / `clear()`

```thoxa
## Paint one full frame: tilde column + centered welcome + cursor, built in a
## Buffer and flushed with a single write(). Fail on a write error.
export fn refresh() -> Trial<Void, String>

## Wipe the screen (\x1b[2J) and home the cursor. Used for the initial paint and
## before quitting, so the shell prompt returns to a clean screen.
export fn clear() -> Trial<Void, String>
```

Both build a `Buffer`, `freeze()` it to a `Bytes`, and write once:

```thoxa
fn flush(frame!: Bytes) -> Trial<Void, String> {
    if sysWrite(stdoutFileno, frame, frame.count) < 0 { return Fail(screenErr("write")) }
    Pass(Void)
}
```

### 3.3 The frame algorithm (kilo's `numrows == 0` branch)

`refresh()` builds the frame inline on a local `mut buf = Buffer()` — **not** via a
mutating helper, because `Buffer` has copy-on-write value semantics: passing it to
a free function shares it and the callee's append copies-on-write, so the caller
would never see the write. Appends are therefore direct (`buf.appendBytes(…)`), in
emission order:

1. `\x1b[?25l` — hide the cursor (avoids flicker during the redraw).
2. `\x1b[H` — cursor home (top-left); no full `\x1b[2J` per frame.
3. For `y` in `0 .. screenRows()`:
   - if `y == screenRows() / 3`: the **welcome row** — a `~`, then centred padding
     spaces, then `"Kilo editor -- version 0.0.1"`. If the banner is wider than the
     terminal, fall back to a bare `~` (full truncation needs string slicing, which
     is Stage 3 / fork #5).
   - else: a single `~`.
   - then `\x1b[K` — erase from the cursor to end of line (the flicker-free
     per-line clear that lets kilo skip a full-screen clear each frame).
   - then `\r\n` **only if `y < screenRows() - 1`** — the last-row-newline guard
     (a bare final `\r\n` would scroll the top row off). This guard is what the
     bars would remove later; deferring the bars keeps it.
4. `\x1b[{row};{col}H` — reposition the cursor to `(cursorY()+1, cursorX()+1)`
   (CUP is 1-based). Built with interpolation; `row`/`col` are computed into
   `let`s first for clarity.
5. `\x1b[?25h` — show the cursor.
6. `freeze()` + one `sysWrite`.

Centering follows kilo: `pad = (cols - len) / 2`; emit `~`, then `pad-1` spaces,
then the banner. The `pad` spaces are a small `loop` over `buf.append(b' ')` (no
`fill`/`repeat` primitive yet — flagged, not forced).

## 4. `examples/kilo/Main.thx` — the render loop

The Stage 1 echo loop becomes a paint loop:

```thoxa
import screen from "./Screen"           # added

# ... windowSize seed + RawMode.enable() unchanged ...
match screen.clear() { Fail(e) { … } Pass { Void } }   # one-time clean screen
loop {
    ## Poll the size every frame — the roadmap's SIGWINCH sidestep.
    match terminal.windowSize() { Pass(sz) { state.setScreenSize(sz.rows, sz.cols) } Fail(e) { Void } }
    match screen.refresh() { Fail(e) { … break } Pass { Void } }
    match readKey() {
        Fail(e) { break }
        Pass(k) {
            match k {
                Press(b) { if b == ctrlKey(b'q') { break } }   # printables are no-ops (no text yet)
                Up { … } Down { … } Left { … } Right { … }
                Home { … } End { … } PageUp { … } PageDown { … } Delete { Void }
            }
        }
    }
}
match screen.clear() { … }              # leave a clean screen behind
match term.restore() { … }
0
```

The Stage 1 debug prints (`print("key: byte …")`, `print("cursor: …")`) are
**removed** — the paint replaces them. Ctrl-Q clears the screen, then quits;
`restore()` still runs on every exit path.

## 5. Verification

Stage 0/1's gate: a deterministic conformance slice for the *language feature*, a
deterministic module test + a pty smoke for the *editor*.

1. **`spec/conformance/string_hex_escape.thx` + `.out`** — the `\xNN`-in-strings
   "done" gate. Pins that `"\x1b"` is one byte `27`, `"\x41" == "A"`, a `Char`
   `'\x1b'` round-trips, a full sequence `"\x1b[2J"` is 4 bytes, and interpolation
   composes with `\x`. Written so the `.out` carries **no raw control bytes** —
   it asserts via `Bytes.fromString("\x1b[H").count`/`.get(0)` and printable
   comparisons, so the `.out` stays reviewable. Rides the existing
   `test_conformance` (stdout == `.out`), `test_checker` (clean), and `test_lsp`
   (no diagnostics) gates.
2. **Lexer error-path tests** (`tests/test_string_hex_escape.py`): `"\x80"` (and
   `'\x80'`) rejected with the ASCII message; `"\x1"` / `"\xZZ"` rejected as
   malformed; byte literals `b'\xff'` still accept `≥ 0x80`.
3. **`tests/test_kilo_screen.py`** — `Screen.thx` type-checks clean; a
   deterministic frame test sets a fixed size (24×80) and cursor (0,0) through
   `State`, calls `refresh()`, captures fd 1 via the `out=` bridge, and asserts
   the exact byte stream: begins `\x1b[?25l\x1b[H`, 24 tilde rows each ending
   `\x1b[K` (with `\r\n` on all but the last), the welcome centred on row 8
   (`24/3`), and ends `\x1b[1;1H\x1b[?25h`.
4. **`tests/test_kilo_keys.py`** — the pty smoke is **updated** from the removed
   text echo to the painted output: `wait_for` the welcome banner, assert the
   cursor CUP escape reflects each move (Right → `\x1b[1;2H`), raw mode still
   engages (a byte arrives un-cooked; Ctrl-C is byte 3, not a signal), Ctrl-Q
   quits, terminal restores. The deterministic `State` clamping test is unchanged.
5. **`SPEC.md`** — §1.5 relaxes the "`\xNN` is not yet accepted in `Char`/`String`"
   line to "accepted, ASCII-restricted `0x00`–`0x7f`"; §8 notes the string `\xNN`
   escape.
6. **`CHANGELOG.md`** — a `### Stage 2 — Screen skeleton + string `\xNN` escapes
   (green: N tests)` entry under `[Unreleased]`, added only once the suite is green.

## 6. Out of scope / deferred

- **Text rows & scrolling** (`rowoff`/`coloff`, the `Row`/`erow` type, `rx`
  tab-expansion, and welcome truncation via string slicing) — Stage 3, fork #5.
- **Status & message bars** (`\x1b[7m` inverse video, filename/line-count/position,
  the transient message line) — deferred; they land with the state they display
  (`filename`/`dirty` at Stage 5, `statusmsg` with editing). The last-row-newline
  guard is the only trace they leave in the skeleton.
- **SGR syntax colouring** — Stage 7.
- **`\u{…}` non-ASCII escapes** — no forcing use; ASCII `\xNN` covers all VT100.
- **A `Buffer.fill`/`appendString` ergonomic primitive** — flagged (the `pad`
  spaces loop and the `Bytes.fromString(…)` two-step are minor noise), not forced;
  a mutating free-function helper is blocked by `Buffer`'s value semantics, so if
  factoring the frame builder later becomes worth it, a `Frame` *type* with
  receiver-mutating methods is the shape (deferred until bars/rows make it pay).
