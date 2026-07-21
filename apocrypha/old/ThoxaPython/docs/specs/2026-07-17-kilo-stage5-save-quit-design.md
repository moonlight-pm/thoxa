# Thoxa × Kilo — Stage 5: Save + quit-confirm + status/message bars

- **Date:** 2026-07-17
- **Status:** Approved design (Stage 5 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 4 (editing).
- **Resolves no roadmap fork** — Stage 5 has none. It is the **LF-A (`|`) payoff**
  (`O_WRONLY|O_CREAT|O_TRUNC`) plus two small increments.
- **Lands two language increments:**
  1. **Octal integer literals `0o…`** (§2) — a lexer increment, the only compiler
     change, mirroring the existing `0x`/`0b` bases. It expresses the file-permission
     mode `0o644` idiomatically.
  2. **A real `time()` primitive** (§3) — the message bar's 5-second fade. Delivered
     with **no compiler change**: a private libc extern called through the established
     `Bytes(n)` out-parameter idiom, wrapped semantically (memory `thoxa-ffi-wrapping`).
- **Turns the editor into one that persists:** `examples/kilo/{FileIO,State,Screen,Main}.thx`
  gain the save path, the dirty-consuming quit-confirm, and the two bottom bars.

## 1. Context & goal

Stage 4 made kilo an in-memory **editor**: type, split, join, delete, and a `dirty`
flag that nothing yet consumes. Stage 5 makes edits **durable and safe to abandon**:

- **Save** to disk — kilo's `editorSave` + `editorRowsToString`: serialize the rows
  (join with `\n`) and write them with `O_WRONLY|O_CREAT|O_TRUNC` + `write` + `close`,
  clearing `dirty`. Bound to **Ctrl-S**.
- **Quit-confirm** — kilo's `KILO_QUIT_TIMES` dance: Ctrl-Q on a `dirty` buffer warns
  and must be pressed several more times; any other key resets the counter. This is
  what finally **consumes** the Stage-4 `dirty` flag.
- **Status bar + message bar** — kilo's `editorDrawStatusBar` / `editorDrawMessageBar`:
  the two reserved bottom rows showing the filename, line count, `(modified)`, the
  cursor's line position, and transient messages (save results, the quit warning, a
  startup hint). The message bar auto-clears after 5 seconds.

The roadmap calls Stage 5 "the LF-A payoff" because the headline capability — `open`
with `O_CREAT|O_TRUNC` — is a **bitwise OR of flag constants**, which LF-A already
shipped. Almost everything else reuses primitives already in the language: inverted
video is a Stage-2 `\xNN` string escape, `editorRowsToString` is a `Buffer` join, the
`dirty` flag exists. Only two genuinely-new things are forced, and both are small.

## 2. Language increment — octal integer literals `0o…`

### 2.1 What forces it

Creating a file needs a permission mode: `open(path, O_WRONLY|O_CREAT|O_TRUNC, 0644)`.
Unix file modes are **octal by convention** (`0644` = `rw-r--r--`); the three-octal-digit
grouping *is* the owner/group/other structure. The lexer already has `0x` (hex) and `0b`
(binary) bases but no octal — an odd gap for a systems language, and file permissions are
the canonical octal case. Writing the mode as decimal `420` or hex `0x1A4` would obscure
exactly the structure octal makes legible.

### 2.2 The decision

Add **`0o…` / `0O…` octal integer literals**, structurally identical to `0x`/`0b`:

```thoxa
0o644     # -> 420   (rw-r--r--)
0o777     # -> 511
0o1_000   # -> 512   (underscore separators, as elsewhere)
0o17u8    # -> 15u8  (sized suffix, as elsewhere)
```

- Digits `0`–`7` and `_` separators; a sized suffix (`i32`/`u8`/…) is accepted exactly
  as for the other bases (via the shared `_emit_int`/`_num_suffix` path).
- Value = `int(digits, 8)`. An octal literal is a plain `Int64` (or its suffixed width) —
  nothing about the *base* survives lexing; `0o644` and `420` are the same token value.

### 2.3 Implementation — one lexer change, made uniform

`Lexer.number` currently open-codes the `0x` and `0b` branches. Stage 5 factors the three
based-integer branches through a shared helper and adds octal:

```python
def number(self, start):
    if self.peek() == "0" and self.peek(1) in ("x", "X"):
        self._radix_int(start, "x", 16, "0123456789abcdefABCDEF_"); return
    if self.peek() == "0" and self.peek(1) in ("o", "O"):
        self._radix_int(start, "o", 8, "01234567_"); return
    if self.peek() == "0" and self.peek(1) in ("b", "B"):
        self._radix_int(start, "b", 2, "01_"); return
    # ... decimal / float path unchanged ...

def _radix_int(self, start, base_char, base, digit_chars):
    self.advance(); self.advance()          # consume the "0x"/"0o"/"0b" prefix
    digits = self.take(lambda c: c in digit_chars).replace("_", "")
    if not digits:
        self.error(f"0{base_char} literal has no digits", start)
    self._emit_int(int(digits, base), start)
```

This also **hardens a pre-existing latent crash**: today `0x`/`0b` with no following
digits (`0x`, `0b`, or `0x_`) reaches `int("", 16)` → an uncaught `ValueError`. Routing
all three through `_radix_int` turns that into a clean, located `ThoxaError` for octal
**and** for hex/binary, at zero behavioral cost for valid literals (the same token value
is emitted). Greedy digit-taking is preserved (`0o8` → the octal part is empty → the
"no digits" error; `0o18` → `0o1` then a separate `8`, exactly as `0x1G` lexes today).

`SPEC.md §1.5` gains `0o` to the base list and an example line.

### 2.4 Conformance & tests

- **`spec/conformance/octal_literals.thx` (+ `.out`)** — the octal "done" gate: a
  `main` that prints `0o0`, `0o644`, `0o777`, an underscore-separated `0o1_000`, a
  suffixed `0o17u8`, and octal inside a bitwise expression (`0o1 | 0o40 | 0o200` — the
  actual save flags written in octal). Stdout is plain integers (control-byte-free).
- **`tests/test_octal_literals.py`** — the lexer edges a stdout test can't pin: `0o`
  with no digits errors cleanly (and, now, `0x`/`0b` too), uppercase `0O644`, greedy
  stop at `8`/`9`, and value/suffix correctness at the token layer.

## 3. The `time()` primitive — a wall clock with no compiler change

### 3.1 What forces it

kilo's message bar shows a message for 5 seconds, then blanks it
(`editorDrawMessageBar`: `if (msglen && time(NULL) - E.statusmsg_time < 5)`). That is the
one place the editor asks the wall clock what time it is. A faithful port needs a real
`time()` — chosen deliberately over faking the fade, per `thoxa-demo-is-means`: grow the
real primitive the systems language will want anyway.

### 3.2 The mechanism — the `Bytes(n)` out-parameter idiom, not `Address<Int64>`

C's `time_t time(time_t *tloc)` takes a pointer and *also returns* the value; `time(NULL)`
passes a null pointer and uses the return. Thoxa has **no null-pointer literal**, and the
LF-B `Address<T>` type marshals **only ABI structs** (`ffi._to_c` requires a `CStruct`),
so `Address<Int64>` cannot carry a scalar and cannot be null. The established alternative
is the one `windowSize` already uses for `ioctl(TIOCGWINSZ, Bytes(8))`: **a `Bytes` value
marshals as a pointer to its buffer**. So:

```thoxa
extern fn sysTime(tloc!: Bytes) -> Int64 from "c" "time"   # private, in State

fn now() -> Int64 {
    sysTime(Bytes(8))     # time_t is 8 bytes; time() writes it AND returns it —
                          # we use the return, exactly as C's time(NULL) does
}
```

`Bytes(8)` is a valid, exactly-8-byte writable region (`ctypes.create_string_buffer(8)`),
so `time` writing `sizeof(time_t)` bytes into it cannot overflow; the throwaway buffer is
never read back. **No FFI change, no new type, no checker change** — `Bytes` is already a
marshalable parameter. (An ABI `TimeT` struct passed by `Address` would also work and is
arguably more "typed", but it is more machinery for an out-param we ignore in favour of
the return value; the `Bytes(8)` idiom is the minimal, already-proven path.)

### 3.3 Where it lives — entirely in `State`

`State` owns the status message and its timestamp; `Screen` is a pure projection that
must not grow its own clock. So the time comparison lives in `State`, and `Screen` only
asks "what's the current message?" (§5, §6). `now()` and `sysTime` stay **private** to
`State`.

## 4. `examples/kilo/FileIO.thx` — the save path

`FileIO` gains the write side to match its existing read side, keeping the private-`sys`-
extern → `Trial`-returning-wrapper shape. It takes the rows to write as a **parameter**,
so `FileIO` still imports only `Row` + `ffi` (never `State`) — no import cycle when
`State` calls it (§5).

```thoxa
extern fn sysOpen3(path!: String, flags!: Int32, mode!: Int32) -> Int32 from "c" "open"
extern fn sysWrite(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c" "write"
# sysClose already declared for open()

let oWrOnly = 0o1
let oCreat  = 0o100     # 64
let oTrunc  = 0o1000    # 512

## kilo editorRowsToString: join every row's bytes with '\n', a trailing '\n' after
## each line (so the file ends with a newline and round-trips through splitRows).
fn rowsToString(rows!: Array<Row>) -> Bytes {
    mut buf = Buffer()
    mut i = 0
    loop {
        if i >= rows.count { break }
        buf.appendBytes(rows[i].bytes())
        buf.append(b'\n')
        i = i + 1
    }
    buf.freeze()
}

## kilo editorSave: serialize the rows and write them with O_WRONLY|O_CREAT|O_TRUNC,
## mode 0644. Returns the number of bytes written; Fail on any open/write error.
export fn save(path!: String, rows!: Array<Row>) -> Trial<Int64, String> {
    let data = rowsToString(rows)
    let flags = oWrOnly | oCreat | oTrunc
    let fd = sysOpen3(path, flags, 0o644)
    if fd < 0 { return Fail(fileErr("open {path}")) }
    let n = sysWrite(fd, data, data.count)
    sysClose(fd)
    if n < 0 { return Fail(fileErr("write {path}")) }
    Pass(n)
}
```

**Flag values** are Linux `asm-generic` (`O_WRONLY=0o1`, `O_CREAT=0o100`, `O_TRUNC=0o1000`)
— the same platform-specific-constant discipline as `Terminal`'s termios flags. Writing
them in octal is a second, incidental payoff of §2. Using `O_TRUNC` (not a separate
`ftruncate`) keeps the path to one `open`.

## 5. `examples/kilo/State.thx` — consume `dirty`; message; quit-confirm

### 5.1 Status message (with a determinism seam)

```thoxa
mut statusMessage = ""
mut statusTime = 0        # the now() stamp when the message was set

extern fn sysTime(tloc!: Bytes) -> Int64 from "c" "time"   # private
fn now() -> Int64 { sysTime(Bytes(8)) }

## The deterministic core (tests drive these with fixed clocks):
export fn setStatusMessageAt(msg!: String, when!: Int64) -> Void {
    statusMessage = msg
    statusTime = when
    Void
}
export fn messageFor(clock!: Int64) -> String {
    if clock - statusTime < 5 { statusMessage } else { "" }
}

## Live wrappers (real use reads the real clock):
export fn setStatusMessage(msg!: String) -> Void { setStatusMessageAt(msg, now()) }
export fn currentMessage() -> String { messageFor(now()) }
```

Splitting the clock read from the freshness policy is the same testability move as
Stage 4's extraction of `dispatch`: `Screen` calls `currentMessage()`; the tests call
`setStatusMessageAt(msg, 100)` then `messageFor(102)` (fresh) / `messageFor(106)`
(expired) — deterministic without sleeping or mocking the clock.

### 5.2 Save

```thoxa
import fileio from "./FileIO"

export fn save() -> Void {
    if filename.count == 0 {                          # String has only .count; no `== ""`
        setStatusMessage("Can't save: no filename")   # kilo prompts here; prompt deferred
        return Void
    }
    match fileio.save(filename, textRows) {
        Pass(n) { dirty = false; setStatusMessage("{n} bytes written to disk") }
        Fail(e) { setStatusMessage("Can't save! {e}") }
    }
    Void
}

# named currentFilename(), not filename(), because a `fn filename` would collide with the
# module-level `mut filename` (one namespace) and shadow it (a runtime not-callable trap):
export fn currentFilename() -> String { filename }
```

A successful save clears `dirty` — the first consumer of the Stage-4 flag. `State`
importing `FileIO` (which imports only `Row`/`ffi`) forms no cycle. kilo's interactive
**filename prompt** for an unnamed buffer is **deferred** (it is modal line-input, the
same machinery `Find` needs — Stage 6); an unnamed buffer reports it can't save. The demo
always launches with `arguments[1]`, so `filename` is set in practice.

### 5.3 Quit-confirm

```thoxa
mut quitTimes = 3        # KILO_QUIT_TIMES

## Ctrl-Q: returns true iff the editor should quit now. A dirty buffer warns and
## decrements a counter first; a clean buffer quits immediately.
export fn confirmQuit() -> Bool {
    if dirty {
        if quitTimes > 0 {
            setStatusMessage("WARNING! Unsaved changes. Press Ctrl-Q {quitTimes} more times to quit.")
            quitTimes = quitTimes - 1
            return false
        }
    }
    true
}

## Any non-quit keypress resets the counter (kilo resets at the end of every keypress
## that isn't the early-returning Ctrl-Q).
export fn resetQuitTimes() -> Void { quitTimes = 3  Void }
```

This mirrors kilo exactly: on a dirty buffer it takes four Ctrl-Q presses (three warnings
counting down `3 → 2 → 1`, then the quit); a clean buffer quits on the first. The counter
+ message live in `State` (with `dirty`, which they read/write) rather than as an input-
handler local, keeping `Main` a thin router — consistent with Stage 4 folding editing into
`State`.

## 6. `examples/kilo/Screen.thx` — two bottom bars

### 6.1 The text region shrinks by two rows

kilo reserves the bottom two rows for the bars (`E.screenrows -= 2`). `State` keeps the
**full** terminal size (so `Screen` knows where the bars go), and a derived text height is
what the viewport math uses:

```thoxa
# in State:
export fn textHeight() -> Int64 { if rows > 2 { rows - 2 } else { 0 } }
```

`scroll()`, `pageUp()`, and `pageDown()` switch from `rows` to `textHeight()` (the vertical
extent the cursor may occupy); the `Screen` draw loop runs `y` over `textHeight()`. This is
the only change rippling out of the reserved rows — the cursor can never land on a bar
because scrolling clamps `cy` into `[rowoff, rowoff + textHeight())`.

### 6.2 Drawing (kilo `editorDrawStatusBar` / `editorDrawMessageBar`)

The frame is built in the existing single `Buffer`. After the text rows (each now ending
`\r\n`, since a bar follows), two lines are appended:

- **Status bar** — inverted video `\x1b[7m … \x1b[m`. Left: the filename (or `[No Name]`)
  and `" - {numRows} lines"` and `"(modified)"` when `dirty`. Right: `"{cy+1}/{numRows}"`.
  The left is truncated to the width; spaces pad the middle so the right group sits
  flush-right (kilo's `while (len < cols) { if cols-len == rlen … else space }`), tracked
  with a running column counter as bytes are appended. Then `\r\n`.
- **Message bar** — `\x1b[K` then `currentMessage()` (truncated to the width). It is the
  **last** line, so no trailing `\r\n` (the old "skip the newline after the final text
  row" rule moves here).

Everything is bytes appended to the `Buffer` over existing primitives (`\xNN` escapes,
`Bytes.fromString`, `Bytes.slice` for truncation, `append(b' ')` for padding) — **no new
primitive**. `Screen` reads two new `State` accessors (`currentFilename()`, `currentMessage()`)
plus the existing `numRows()`/`cursorY()`/`isDirty()`.

The cursor reposition is unchanged (`cy - rowoff + 1`, `rx - coloff + 1`) — it already
lands within the text region, which is now `textHeight()` rows tall.

## 7. `examples/kilo/Main.thx` — Ctrl-S, quit-confirm, startup hint

`dispatch` gains two routes and the counter reset; the loop is otherwise unchanged.

```thoxa
export fn dispatch(k!: Key) -> Bool {
    if isQuitKey(k) { return state.confirmQuit() }   # the only key that skips the reset
    applyKey(k)                                       # all the Stage-4 routing, + Ctrl-S
    state.resetQuitTimes()
    false
}
```

- `isQuitKey(k)` matches `Press(ctrlKey(b'q'))`; `applyKey(k)` is the Stage-4 `match`
  body (movers + editing), with a new `else if b == ctrlKey(b's') { state.save() }` route.
  Splitting the quit check out keeps the "reset on every other key" a single statement
  instead of sprinkling it through every arm.
- Before the loop: `state.setStatusMessage("HELP: Ctrl-S = save | Ctrl-Q = quit")`
  (kilo's startup hint), visible until it fades or is replaced.

## 8. Verification

Per the roadmap discipline (**design → doc + `SPEC.md` → conformance test → TDD → green →
commit**), Stage 5 is "done" only when these are green:

1. **`spec/conformance/octal_literals.thx` + `.out`** — the octal-literal gate (§2.4).
2. **`tests/test_octal_literals.py`** — octal lexer edges + the hardened empty-`0x`/`0b`.
3. **`tests/test_kilo_fileio.py`** (extended) — `save` round-trips (save rows to a temp
   file, reopen with `FileIO.open`, assert the same rows), the returned byte count, the
   `oWrOnly|oCreat|oTrunc` flag value, `rowsToString`'s trailing-newline shape, and `Fail`
   on an unwritable path (a directory).
4. **`tests/test_kilo_state.py`** (extended) — `save()` clears `dirty` (real temp file);
   the message seam's 5-second expiry via `setStatusMessageAt`/`messageFor`; the
   quit-confirm counter/`dirty` interplay (dirty ⇒ four `confirmQuit()`s to quit, clean ⇒
   one; `resetQuitTimes` restores the count); `textHeight()` clamping.
5. **`tests/test_kilo_dispatch.py`** (extended) — Ctrl-S routes to save, Ctrl-Q routes to
   the confirm, a non-quit key resets the counter.
6. **`tests/test_kilo_keys.py`** (updated pty smoke) — over a real pty: type text, Ctrl-S,
   then assert the file on disk changed; assert the status/message bars render (inverted
   status line present; the "bytes written" message shows) and raw mode / restore hold.
7. **`spec/SPEC.md`** §1.5 updated (§2.3).
8. **Roadmap** — the Stage-5 build-order line marked done, with this doc + the conformance
   test. **`CHANGELOG.md`** `[Unreleased]` entry — added once green.

## 9. Out of scope / deferred

- **Interactive filename prompt** for an unnamed buffer (kilo `editorPrompt`) — modal
  line-input, the same machinery `Find` needs; **Stage 6**. An unnamed buffer reports it
  can't save.
- **`Find`** (incremental search) — **Stage 6**; **syntax highlighting** — **Stage 7**.
- **In-place `Buffer`/`Array` splice** and static **Perceus reuse** — the native pass.
- A `TimeT` ABI struct / explicit `address()` / null-pointer literal — not needed; the
  `Bytes(8)` out-param idiom (§3.2) suffices, and the FFI's `Address<T>`/null story stays
  a deferred LF-B follow-up.
