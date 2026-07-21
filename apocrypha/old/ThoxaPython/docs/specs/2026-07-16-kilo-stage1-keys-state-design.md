# Thoxa × Kilo — Stage 1: `Keys` + `State` (and fork #3)

- **Date:** 2026-07-16
- **Status:** Approved design (Stage 1 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 0 (`Terminal`).
- **Resolves language-design fork #3** (`char`↔`int` code + ordered char compare).

## 1. Context & goal

Stage 1 delivers the input half of the editor: `examples/kilo/Keys.thx`
(`readKey()` — raw byte reads + VT100 escape decoding → a `Key` union) and
`examples/kilo/State.thx` (the editor singleton as module-level `mut`, reached
only through exported functions). `Main.thx` grows from Stage 0's "echo raw
codes" into a **movable cursor**: arrow keys move a cursor held in `State`,
Ctrl-Q quits, other keys echo. There is no screen rendering yet (Stage 2) — the
printed cursor position is the visible proof that `Keys` + `State` work.

Along the way it resolves **fork #3**, which turns out to need *no* char↔int
machinery at all — see §2.

## 2. Fork #3 — resolved by byte literals, not char conversion

### 2.1 The reframing

The roadmap framed fork #3 as "`char`↔`int` code + ordered char compare," with a
leading sketch of `Char.fromCode` / `.code` / `as`. Investigating the language as
built changes the answer:

- **A Thoxa `Char` is a Unicode scalar, deliberately *not* a byte or number.**
  This is an existing, documented invariant (`2026-07-12-thoxa-libc-ffi-design.md`
  §: *"a Thoxa `Char` is a Unicode scalar, not a C `char` … use `Byte`/`Bytes`
  for C character data"*). `Char` is kept out of the FFI on purpose.
- The kilo port is **byte-oriented end to end**: terminal reads are bytes, rows
  will be byte buffers (fork #5's leaning), output is bytes. It works in `Byte`,
  exactly as the FFI design prescribes — never in `Char`.
- **No `.thx` in the tree uses a char literal at all** (verified) — so the char
  world is unconstrained by existing code.

So the real need was never "convert a char to an int." It was: *when working in
`Byte`, how do you write a readable byte constant* so the editor is not a wall of
`0x5b` for `[`? (Stage 0's `Main` already hit this: `if code == 113` where kilo
writes `if c == 'q'`.)

### 2.2 The decision: Rust-style byte literals `b'…'`

Add a **byte literal** `b'c'` that denotes a `Byte` (0–255). This is the C/Rust
model where a character constant *is* its code:

```thoxa
if c == b'\x1b' { … }          # ESC (27)
if c == b'[' { … }
if c == ctrlKey(b'q') { quit } # CTRL_KEY(k) = k & 0x1f  ->  b'q' & 0x1f == 17
if b'0' <= c and c <= b'9' { … }   # classification (Stage 7) — code-range check
```

Consequences — the fork collapses:

- **No `.code` / `Char.fromCode` / `as` extension.** A byte literal is just an
  integer literal written in character form; its value is a `Byte`.
- **"Ordered char compare" needs nothing.** A `Byte` is already a numeric integer,
  so `<`/`<=`/`>=`/`>` and `&`/`|`/`^`/`<<`/`>>` and `==`/`!=` all work on byte
  literals with zero operator or checker changes. Classification (`b'0' <= c &&
  c <= b'9'`) is an ordered *int* comparison, which already type-checks.
- **`Char` stays exactly as-is** — a Unicode scalar for the eventual string model.
  The "chars aren't bytes/numbers" invariant is *honored*, not reversed: `b'q'`
  genuinely is a `Byte`.
- The `Int == Char` footgun (type-checks but is always `false` at runtime, since
  `Char` ≡ `String` ≡ Python `str`) is sidestepped — the idiom is `Byte == Byte`.

### 2.3 Grammar & escape rules (the Rust rule — unambiguous)

- `b'…'` yields a **`Byte`**. Value range 0x00–0xFF.
- A **bare (unescaped) character must be ASCII (0x00–0x7f)**. A non-ASCII source
  character in `b'…'` is a lex error: *"byte literals must be ASCII; use \xNN for
  bytes ≥ 0x80"* — because `'é'` is genuinely ambiguous between Latin-1 `0xE9`
  and UTF-8 `0xC3 0xA9`.
- **The full 0x00–0xFF range is reachable only via a new `\xNN` escape** (exactly
  two hex digits): `b'\x1b'`, `b'\x7f'`, `b'\xff'`.
- The existing escapes are also accepted in `b'…'`: `\n \t \r \\ \' \" \0`
  (each denotes its ASCII byte, e.g. `b'\r'` == 13).
- **`\xNN` is scoped to byte literals this stage.** String/`Char` literals do not
  gain `\x` yet. Strings *will* want it (VT100 output like `"\x1b[2J"`), but that
  is forced by **Stage 2 (Screen)**, where string `\x` (ASCII-restricted, per
  UTF-8) rides in. Keeping it out of `char`/`string` now keeps `Stage 1` tight and
  avoids the "what does `\xNN` mean in a UTF-8 string" question until it is forced.

### 2.4 Implementation — `T.BYTE` mirrors `T.CHAR` at every site

A byte literal is a scalar literal exactly like a char literal, so the change is a
mechanical mirror of the existing `CHAR` handling:

| Site | `CHAR` today | add for `BYTE` |
| --- | --- | --- |
| `tokens.py` | `T.CHAR` | `T.BYTE` |
| `lexer.py` | `char()` + `'` dispatch | `byte()` + `b'` dispatch (before `isalpha`); `_byte_escape()` with `\xNN`; `_HEXDIGITS` |
| `ast.py` | `CharLit(value:str, span)` | `ByteLit(value:int, span)` |
| `parser.py` | `primary()`; pattern parsing | `primary()` → `ByteLit`; pattern parsing → byte match patterns |
| `checker.py` | `_infer_CharLit → "Char"` | `_infer_ByteLit → _INT_TAG` (Int64 — all int widths unify) |
| `interp.py` | `eval_CharLit → node.value` | `eval_ByteLit → node.value` (an int) |
| `lsp/index.py` | `_literal_str` CharLit branch | `_literal_str` ByteLit branch |

Lexer dispatch: `b` is `isalpha`, so the `b'` branch must precede the `name`
branch in `tokenize`. `b` immediately followed by `'` is unambiguous — no valid
program has an identifier directly abutting a `'`.

**Not in scope:** byte literals as *valued-type* intrinsics (`type X b'q'`) — the
valued-type site (`parser.py` `type_decl`) is left unchanged; no stage forces
byte-valued types. Deferred, one token + one test if ever wanted.

## 3. `examples/kilo/Keys.thx`

Owns stdin reading — the throwaway `sysRead`/`readByte` that Stage 0 parked in
`Main` moves here and becomes real. Depends only on `ffi` (it is independent of
`Terminal`, refining the roadmap DAG's "Keys → Terminal").

```thoxa
import { errno, strerror } from "ffi"

## A decoded keypress: an ordinary byte, or a VT100-decoded navigation key.
type Key = Press(byte!: Byte)
         | Up | Down | Left | Right
         | Home | End | PageUp | PageDown
         | Delete

## CTRL_KEY(k) = k & 0x1f — the control-combo byte. Reused by dispatch and Find.
export fn ctrlKey(k!: Byte) -> Byte { k & 0x1f }

## Read + decode one keypress. Blocks (retrying the VMIN=0/VTIME=1 idle timeout)
## until a key arrives; Fail only on a real read() error.
export fn readKey() -> Trial<Key, String> { … }
```

**`readKey` decoding** (faithful to kilo's `editorReadKey`):

- `readByte()` — the blocking read: retry on `n == 0` (idle timeout, *not* EOF —
  the Stage 0 bug), `Fail` on `n < 0`, else the byte.
- `readByteOnce()` — a single-shot read for escape-sequence bytes: the byte, or
  `-1` if none arrived (timeout or error). A lone ESC (no follow-on within the
  100 ms VTIME window) decodes to `Press(b'\x1b')`, matching kilo.
- Decode table (only `\x1b[`/`\x1bO` sequences become named keys; everything
  else, incl. Enter `\r`, Backspace `\x7f`, Ctrl-keys, printables, is `Press`):

  | Input | Key |
  | --- | --- |
  | `\x1b [ A` / `B` / `C` / `D` | Up / Down / Right / Left |
  | `\x1b [ H` / `F`, `\x1b O H` / `O F` | Home / End |
  | `\x1b [ 1~` / `7~` / `H`… | Home |
  | `\x1b [ 4~` / `8~` / `F` | End |
  | `\x1b [ 3~` | Delete |
  | `\x1b [ 5~` / `6~` | PageUp / PageDown |
  | lone/unrecognized `\x1b` | `Press(b'\x1b')` |
  | any other byte `n` | `Press(n)` |

`readKey` returns `Trial<Key,String>` (idiomatic — reads can fail, matching
`Socket`/`File`/`RawMode`); the `?` operator threads the read error.

## 4. `examples/kilo/State.thx`

The editor singleton, following the conformance-proven `modstate/Counter.thx`
pattern: private top-level `mut`, reached only through exported functions. Depends
on nothing — it takes cursor *deltas*, never `Key`s, so it stays Keys-independent.

```thoxa
mut cx = 0   mut cy = 0            # cursor column / row
mut rows = 0   mut cols = 0        # screen size in cells

export fn setScreenSize(r!: Int32, c!: Int32) -> Void   # store dimensions
export fn moveCursor(dx!: Int32, dy!: Int32) -> Void     # add deltas, clamp to bounds
export fn cursorX() -> Int32     export fn cursorY() -> Int32
export fn screenRows() -> Int32  export fn screenCols() -> Int32
```

`moveCursor` clamps to `[0, cols-1] × [0, rows-1]` (a 0-sized screen clamps to 0).
Deliberately minimal: `filename` / `dirty` / `rows[]` / `syntax` are added by the
stages that force them (YAGNI — no Stage-1 code reads them).

## 5. `examples/kilo/Main.thx`

The Stage 1 deliverable — a cursor you can drive with the arrow keys:

1. `windowSize()` → seed `State.setScreenSize` (fallback to a default on `Fail`).
2. `RawMode().enable()`; on `Fail`, report and exit 1.
3. Loop: `match readKey()`:
   - `Fail(e)` → print, break.
   - `Pass(Press(b))` with `b == ctrlKey(b'q')` → break (quit).
   - `Pass(Up/Down/Left/Right)` → `moveCursor(0,-1)` / `(0,1)` / `(-1,0)` / `(1,0)`.
   - `Pass(Home)` → move to column 0; `End` → last column; `PageUp/PageDown` →
     top/bottom row; `Delete` → (no-op stub this stage).
   - `Pass(Press(b))` → echo `key: byte {b}`.
   - After a move, print `cursor: {cursorX()},{cursorY()}` — the visible proof.
4. `restore()` on every exit path (`\r\n` line endings — OPOST is off).

## 6. Verification (Stage 0's gate: deterministic slice + automated pty smoke)

1. **`spec/conformance/byte_literals.thx` + `.out`** — the fork-#3 "done" gate:
   value checks (`b'A'`==65, `b'\x1b'`==27, `b'\x7f'`==127, `b'\xff'`==255,
   `b'\n'`==10, `b'\''`==39), a `ctrlKey`-style mask (`b'q' & 0x1f`==17), ordered
   range checks (`b'0' <= b'5' and b'5' <= b'9'`), and a **byte-literal match
   pattern** (`match 65 { b'A' { … } else { … } }`). Rides the existing
   `test_conformance` (stdout==`.out`), `test_checker` (clean), and `test_lsp`
   (no diagnostics) gates.
2. **`tests/test_kilo_keys.py`**:
   - Type-check `Keys.thx` / `State.thx` / `Main.thx` clean.
   - A **pty smoke** driving the real `Main` through a pseudo-terminal, feeding
     `a`, Right, Down, Ctrl-C (the byte-3 / ISIG-off proof), End, PageDown, Home,
     then Ctrl-Q, asserting the decoded echoes, the cursor trail with clamping to
     the pty's real 40×100 size (End→x=99, PageDown→y=39), a clean quit, and restore.
   - A **deterministic `State` test** in the `modstate` style: a small importing
     `.thx` drives `moveCursor`/clamping and prints results.
   - **Lexer error-path tests**: a non-ASCII bare char in `b'…'` and a malformed
     `\x` escape are rejected with the documented messages.
3. **`SPEC.md`** — add the byte-literal production + `\xNN` escape to the lexical
   section, beside the char-literal example.

## 7. Out of scope / deferred

- **String/`Char` `\x` escapes** — forced by Stage 2 (Screen)'s VT100 output.
- **Byte literals as valued-type intrinsics** (`type X b'q'`) — no forcing use.
- **`byte` string literals `b"…"`** — not needed; `Bytes.fromString` covers it.
- **Screen rendering** — Stage 2. Stage 1 prints cursor state as text.
- **`filename`/`dirty`/`rows[]`/`syntax` in `State`** — added by their stages.
- **tree-sitter grammar** update for `b'…'` — the lexical grammar can gain the
  token later; the LSP (real lexer) already colors/diagnoses it. Optional
  follow-up, not a Stage 1 blocker (per the editor-support "LSP-first" stance).
```
