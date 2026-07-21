# Thoxa × Kilo — Stage 4: Editing (`insertAt`/`removeAt` value splice)

- **Date:** 2026-07-17
- **Status:** Approved design (Stage 4 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 3 (the read-only viewer).
- **Resolves roadmap fork #4** (Array/`Buffer` splice — `insertAt`/`removeAt`).
- **Lands one language increment:** a **functional, copying, strict-bounds splice** on
  both `Bytes` and `Array<T>` — `insertAt(at, x) -> …` and `removeAt(at) -> …` (§2) —
  and turns the viewer into an **editor**: `examples/kilo/{Row,State,Main}.thx` gain the
  editing operations built on it (§3–§5).

## 1. Context & goal

Stage 3 made kilo a **read-only file viewer**: open a file, load its lines as `Row`s,
scroll, and draw the visible window. Stage 4 makes it an **editor**: type a character,
split a line with Enter, join/erase with Backspace and Delete, and track an unsaved-
changes flag. This is kilo through its editing milestone (`editorInsertChar`,
`editorInsertNewline`, `editorDelChar` and the row primitives `editorRowInsertChar`,
`editorRowDelChar`, `editorRowAppendString`, `editorInsertRow`, `editorDelRow`),
stopping short of **writing to disk** — the save path (`O_CREAT|O_TRUNC`, serialize,
quit-confirm) is Stage 5.

The through-line is **fork #4** — the editor cannot insert a byte into a line, nor a
whole line into the file, without a *mid-sequence splice*, which neither `Bytes` nor
`Array` provides today. kilo's eight editing functions reduce to four data-structure
operations:

| op  | what                                             | status entering Stage 4                          |
| --- | ------------------------------------------------ | ------------------------------------------------ |
| (a) | insert/remove **a byte at an index** in a line   | **new** — the byte splice                        |
| (b) | **append a byte-range** to a line (line-join)    | ✅ `Buffer.appendBytes` already                  |
| (c) | **split** a line at an index (newline)           | ✅ `Bytes.slice` already                         |
| (d) | insert/remove **a whole `Row` at an index**      | **new** — the row splice                         |

So Stage 4 adds exactly two genuinely-new primitives — a byte splice (a) and a row
splice (d); (b) and (c) fall out of primitives shipped in earlier stages.

### 1.1 How fork #4 is resolved

Fork #4 was left open as *"touches the value-semantics direction; author's call."* The
call: **functional / value splice.** `insertAt`/`removeAt` are **copying** — they
return a *new*, independently-owned value and leave the receiver untouched — with
**strict bounds** that trap on violation, exactly like `Bytes.slice` (Stage 3) and
out-of-bounds indexing. Concretely:

- **Rows stay fixed-length `Bytes`.** An edit does not mutate a line in place; it builds
  a new `Bytes` and a new `Row`. Because the `Row` constructor re-runs `expandTabs`, the
  tab-expanded `render` stays in sync **for free** — there is no stateful rebuild step.
- **The row collection stays a value.** `State.textRows` is an `Array<Row>`; inserting or
  deleting a row produces a new array (`textRows = textRows.insertAt(…)`), matching the
  existing functional `Array` methods (`.append`, `.dropLast`).

This is the most direct expression of Thoxa's **immutable-by-default** thesis: mutation
is an *implementation optimization*, not a surface concept. The copies are honest — a
future Perceus/native pass makes a copy that is provably unique a reuse-in-place, so the
surface never has to change. What is **set aside** (and why):

- **In-place `Buffer` memmove splice** (`Buffer.insertAt(i, byte) -> Void`, COW-guarded).
  It is the most literal translation of kilo's C `memmove`, but it would migrate
  `Row.chars` from `Bytes` to a growable `Buffer` — pulling the aliasing/COW machinery
  into every `Row` field, forcing `render` to become a stateful `mut`, and requiring a
  `freeze()` (or a deferred `Buffer` *view*) before `Row.window()` could `slice`. More
  moving parts, against the value model, for no observable gain in a tree-walker. It also
  would **still** need a separate `Array` splice for op (d). Deferred to the native pass
  that already owns Perceus reuse; consistent with the growable-`Buffer` design, which
  deferred *views/borrows* as the one construct that reintroduces the invalidation hazard
  the COW model exists to avoid.
- **In-place `Array` splice** (`Array.insertAt -> Void`, matching the existing in-place
  `xs[i] = v`). Rejected for the same reason: it would deepen "arrays are the mutable
  escape hatch" against immutable-by-default. (Note: within-line **element replacement** —
  swapping one existing `Row` for its edited successor without changing the array's
  length — still uses the existing in-place `textRows[cy] = newRow`; only *length-changing*
  edits use the new functional `insertAt`/`removeAt`. Verified that index-assign on a
  module-level `mut` array type-checks and runs.)

### 1.2 Where the editing logic lives

kilo splits the row primitives (`editorRow*`) from the cursor-level operations
(`editorInsertChar`/…) only because C has no encapsulation — everything touches the
global `E`. Thoxa's `State` already **owns `cx`/`cy`/`textRows` as private
module-level `mut`s, reachable only through exported functions** (the `Counter`/modstate
pattern). The editing algorithms manipulate those three together, so they live **in
`State`**, alongside the existing cursor movers — keeping that state private instead of
exposing a broad `insertRowAt`/`setCursorX`/`bumpDirty` setter API to a separate module.
This **supersedes the roadmap's provisional `Edit` module** (the roadmap module table is
updated to fold `Edit` into `State`). `Row` keeps the per-line byte work (it owns a
line's `chars`); `State` orchestrates cursor + row-collection.

## 2. Language increment — value splice on `Bytes` and `Array<T>`

### 2.1 What editing forces

- **A byte into / out of a line (op a).** `editorRowInsertChar(row, at, c)` opens a
  1-byte gap at `at` and stores `c`; `editorRowDelChar(row, at)` closes the gap. In the
  value model these are `chars.insertAt(at, c)` and `chars.removeAt(at)` → a fresh
  `Bytes` → a fresh `Row`.
- **A whole `Row` into / out of the file (op d).** `editorInsertRow(at, …)` opens a slot
  in `E.row[]`; `editorDelRow(at)` closes it. In the value model these are
  `textRows.insertAt(at, row)` and `textRows.removeAt(at)` → a fresh `Array<Row>`.

Ops (b) and (c) need nothing new: line-**join** is `Buffer.appendBytes` (§3, `Row.concat`);
line-**split** is two `Bytes.slice`s (§3, `Row.take`/`Row.drop`).

### 2.2 The decision: `insertAt` / `removeAt` — methods, copying, strict-bounds

```thoxa
Bytes.insertAt(at!: Int64, byte!: Byte) -> Bytes    # a fresh Bytes with `byte` inserted at `at`
Bytes.removeAt(at!: Int64)              -> Bytes     # a fresh Bytes with the byte at `at` removed

Array<T>.insertAt(at!: Int64, item!: T) -> Array<T> # a fresh array with `item` inserted at `at`
Array<T>.removeAt(at!: Int64)           -> Array<T> # a fresh array with element `at` removed
```

- **Methods, not subscript sugar.** Consistent with Stage 3's choice of `slice` over
  `a[i..j]`: one obvious way, no parser change.
- **Copying, never a view.** The result is a new allocation with a copy of the elements —
  it is not a borrow into the source, so it can never dangle. The receiver is unchanged
  (`let b2 = b.insertAt(…)` leaves `b` intact). This is the value-semantics guarantee the
  conformance test pins (§2.4).
- **`Int64` index, strict bounds, trap on violation** — the caller clamps, the same
  discipline as `slice` and `a[i]`. **Insert** admits `0 ≤ at ≤ count` (`at == count`
  appends, so `insertAt(count, x)` is the functional `append`); **remove** admits
  `0 ≤ at < count`. A violation traps with an out-of-bounds message, the same class of
  runtime error as a bad index — a bad splice index is a bug, not silently absorbed.
- **`Bytes.insertAt`'s `byte` is masked `& 0xFF`**, exactly like `Buffer.append`/`.set`.
- **`Array.insertAt`/`removeAt` are functional** (return a new array), matching `.append`
  and `.dropLast`; they do **not** mutate in place.

The splice never changes the FFI story: `Bytes` still crosses the FFI (a fresh `Bytes`
is as stable as any other); `Buffer` is untouched by this stage and still cannot cross
un-`freeze`d.

### 2.3 Implementation

The checker types every `Bytes`/`Buffer`/`Array` method as `Unknown` (it has no method
tables for these primitives — the same permissive path `Bytes.slice` took in Stage 3), so
**the checker needs no change**: `b.insertAt(…)`/`xs.removeAt(…)` type as `Unknown`, and
`Unknown` unifies with anything (so `textRows = textRows.insertAt(…)` re-assigning an
`Array<Row>` mut is accepted). All work is in the runtime.

| Site                        | Change                                                                                                                                                             |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `thoxa/ffi.py` (`Bytes`)    | Add host methods, each first taking `data = self._buf.raw[:self._len]` (the logical bytes, **excluding** any trailing NUL a `from_string` buffer carries): `insert_at(at, b)` → `Bytes.from_bytes(data[:at] + bytes([b & 0xFF]) + data[at:])` (bounds `0 ≤ at ≤ _len`); `remove_at(at)` → `Bytes.from_bytes(data[:at] + data[at+1:])` (bounds `0 ≤ at < _len`); each raises `IndexError` on a bad index. Copying, returns a fresh independently-owned `Bytes`. |
| `thoxa/interp.py` (`_bytes_method`) | Route `insertAt`/`removeAt` (validate int args, convert `IndexError` → a located `self.error`, mirroring `slice`).                                          |
| `thoxa/interp.py` (`Bytes` member-access gate) | Add `"insertAt"`, `"removeAt"` to the whitelist tuple `("get","set","toString","slice")` so member access hands back a `BoundBuiltin`.                   |
| `thoxa/interp.py` (`ARRAY_METHODS`) | Add `"insertAt"`, `"removeAt"` to the set so array member access dispatches them.                                                                              |
| `thoxa/interp.py` (`_array_method`) | Add branches: `insertAt` → `recv[:at] + [item] + recv[at:]` (bounds `0 ≤ at ≤ len`); `removeAt` → `recv[:at] + recv[at+1:]` (bounds `0 ≤ at < len`); both **return a new list** (functional, like `append`); trap on a bad index with a located error. |
| `spec/SPEC.md` (§9, §9.1)   | Document `Array<T>.insertAt`/`.removeAt` (§9) and `Bytes.insertAt`/`.removeAt` (§9.1) alongside the existing methods, in the same phrasing.                        |

### 2.4 Conformance & tests

- **`spec/conformance/splice.thx` (+ `.out`)** — the fork-#4 "done" gate. A single
  `export fn main() -> Int64` that exercises the happy paths and **pins value-semantics**,
  asserting only through `.count` / `.get` / `.toString` / index / `println`
  interpolation so the `.out` carries no raw control bytes:
  - `Bytes.insertAt` mid-sequence and at `count` (append); `Bytes.removeAt`; and the
    **receiver unchanged** after each (`let b2 = b.insertAt(…)`; print `b` and `b2`).
  - `Array<Int64>.insertAt`/`.removeAt` mid-sequence and at the ends; and the **source
    array unchanged** after each.
- **`tests/test_splice.py`** — the trap/behavior paths that can't be deterministic stdout:
  out-of-range `insertAt`/`removeAt` on both `Bytes` and `Array` (below 0, past the insert
  bound, `removeAt` at `count`) trap; `insertAt(count, …)` succeeds as append; the
  `byte & 0xFF` masking on `Bytes.insertAt`; value-semantics re-asserted at the Python
  layer.

## 3. `examples/kilo/Row.thx` — per-line editing methods

`Row` keeps `chars: Bytes` private and gains methods that each return a **new `Row`**
(so `render` is rebuilt by the constructor). One public read accessor, `bytes()`, is
added so `concat` can read another row's content — Thoxa forbids reading a *private
field* of another instance (`other.chars`), but a *public method* call (`other.bytes()`)
is allowed; `bytes()` returns a defensive copy (`chars.slice(0, count)`) so the internal
buffer is never handed out.

```thoxa
export type Row(chars!: Bytes) {
    let render = expandTabs(chars)

    # ... existing: charCount(), renderCount(), cxToRx(cx), window(coloff, width) ...

    # Stage 4 — editing (each returns a NEW Row; render re-derived by the ctor):
    export fn insertChar(at!: Int64, byte!: Byte) -> Row   # Row(chars.insertAt(at, byte))
    export fn deleteChar(at!: Int64)              -> Row   # Row(chars.removeAt(at))
    export fn take(count!: Int64)                 -> Row   # prefix — Row(chars.slice(0, count))
    export fn drop(count!: Int64)                 -> Row   # suffix — Row(chars.slice(count, charCount()))
    export fn concat(other!: Row)                 -> Row   # join — Buffer(chars ⧺ other.bytes()).freeze()

    # Stage 4 — read accessors:
    export fn bytes() -> Bytes                            # chars.slice(0, charCount()) — a fresh copy
    export fn text()  -> String                          # chars.toString() — for tests + the Stage-5 status bar
}
```

`take`/`drop` realize op (c) (the newline split): the current row becomes `take(cx)`, the
new row below becomes `drop(cx)`. `concat` realizes op (b) (the backspace join). Because
these operate on the receiver's own immutable `chars` and return fresh values, there is
no stale-handle hazard across a subsequent `Array` splice (kilo's C "re-fetch the row
pointer after the `memmove`" gotcha does not exist in the value model).

## 4. `examples/kilo/State.thx` — editing entry points + dirty flag

New private field + read:

```thoxa
mut dirty = false
export fn isDirty() -> Bool
```

`dirty` is set `true` by every editing op; **Stage 5** adds `markSaved()` to clear it and
the quit-confirm that consumes it. A private helper `fn emptyRow() -> Row { Row(Bytes(0)) }`
makes a blank line.

Four exported editing operations, each running kilo's algorithm with a functional
`textRows` splice and cursor fixup (validated idioms: module-`mut` index-assign, early
`return Void` guards):

```thoxa
# editorInsertChar — insert `byte` at the cursor
export fn insertChar(byte!: Byte) -> Void
    # if cy == numRows(): textRows = textRows.insertAt(numRows(), emptyRow())   # materialize the phantom EOF line
    # textRows[cy] = row(cy).insertChar(cx, byte)                              # in-place element swap (length unchanged)
    # cx = cx + 1 ; dirty = true

# editorInsertNewline — split the line at the cursor (Enter)
export fn insertNewline() -> Void
    # if cx == 0: textRows = textRows.insertAt(cy, emptyRow())                 # blank line above
    # else:  let line = row(cy)
    #        textRows = textRows.insertAt(cy + 1, line.drop(cx))               # suffix becomes the new row below
    #        textRows[cy] = line.take(cx)                                      # current row truncated to the prefix
    # cy = cy + 1 ; cx = 0 ; dirty = true

# editorDelChar — Backspace (delete the char left of the cursor; join at column 0)
export fn deleteChar() -> Void
    # if cy == numRows(): return Void                                         # nothing on the phantom line
    # if cx == 0 && cy == 0: return Void                                      # start of file — no-op
    # if cx > 0: textRows[cy] = row(cy).deleteChar(cx - 1) ; cx = cx - 1
    # else:  let prev = row(cy - 1)
    #        cx = prev.charCount()                                            # cursor lands at the old end of prev
    #        textRows[cy - 1] = prev.concat(row(cy))                          # append this line onto prev
    #        textRows = textRows.removeAt(cy)                                 # drop the merged line
    #        cy = cy - 1
    # dirty = true

# editorMoveCursor(RIGHT) + editorDelChar — the Delete key (forward delete)
export fn deleteForward() -> Void
    # moveRight() ; deleteChar()
```

`deleteForward` reuses `moveRight` + `deleteChar` exactly as kilo maps the Delete key
(`editorMoveCursor(ARROW_RIGHT); editorDelChar();`) — at end-of-line `moveRight` wraps to
the next line's start, and `deleteChar` then joins, which is the correct forward-delete.

Scrolling and `rx` need no new code: `scroll()` already recomputes `rx` from the current
row on each `Screen.refresh()`, so an edited line's cursor column stays correct.

## 5. `examples/kilo/Main.thx` — key dispatch

The Stage 3 loop is unchanged; the two live no-op cases are filled and the printable case
gains editing. `Delete` is already imported from `Keys`; Enter is matched as the byte
literal `b'\r'` (validated to lex as 13) and Backspace/ESC numerically — so no new `Key`
variant is needed.

```thoxa
Press(b) {
    if b == ctrlKey(b'q') { break }        # Ctrl-Q — quit (dirty-confirm is Stage 5)
    if b == b'\r'         { state.insertNewline() }   # Enter (13)
    else if b == 127      { state.deleteChar() }      # Backspace (DEL)
    else if b == ctrlKey(b'h') { state.deleteChar() } # Ctrl-H (8) — also backspace
    else if b == ctrlKey(b'l') { Void }               # Ctrl-L — refresh no-op
    else if b == 27       { Void }                     # ESC — no-op
    else                  { state.insertChar(b) }      # any other byte — insert (incl. Tab)
}
Delete { state.deleteForward() }           # was a no-op in Stage 3
# Up/Down/Left/Right/Home/End/PageUp/PageDown — unchanged movers
```

Ctrl-Q quits even when `dirty` — the quit-confirm ("file has unsaved changes, press
Ctrl-Q N more times") is Stage 5. `esc` and the `ctrlKey` helper already exist in `Keys`.

## 6. `examples/kilo/Screen.thx` — unchanged

`Screen` is a pure projection of `State`; it reads, never mutates. Each edited `Row`
carries a freshly-derived `render`, so `row(filerow).window(…)` reflects edits with no
change to `Screen`. Once the first character is typed into an empty buffer, `numRows()`
becomes 1 and the welcome banner naturally disappears. The status/message bars remain
Stage 5.

## 7. Verification

Per the roadmap discipline (**design → doc + `SPEC.md` → conformance test → TDD → green →
commit**), Stage 4 is "done" only when these are green:

1. **`spec/conformance/splice.thx` + `splice.out`** — happy-path `Bytes`/`Array` splice,
   control-byte-free, pinning value-semantics (receiver/source unchanged). *(the fork-#4
   gate)*
2. **`tests/test_splice.py`** — out-of-range traps (both types), `insertAt(count,…)`
   append, `& 0xFF` byte masking, value-semantics at the Python layer.
3. **`tests/test_kilo_edit.py`** — deterministic `State` editing: insert into an empty
   buffer (phantom-row materialization), type a word, `insertNewline` mid-line and at
   column 0, `deleteChar` within a line and the column-0 join, `deleteForward` at
   end-of-line — each asserting `Row.text()` content, `numRows()`, `cx`/`cy`, and
   `isDirty()`.
4. **`tests/test_kilo_keys.py`** (updated pty smoke) — over a real pty, type text, press
   Enter, Backspace and Delete, and assert the painted frames and cursor trail reflect the
   edits and that raw mode / restore still hold.
5. **`spec/SPEC.md`** §9 / §9.1 updated (§2.3).
6. **Roadmap** fork-#4 row → **✔ Resolved (Stage 4)** with this doc + `spec/conformance/splice.thx`;
   the module table's `Edit` row folded into `State` (§1.2); the build-order line marked done.
7. **`CHANGELOG.md`** `[Unreleased]` entry — added once green.

## 8. Out of scope / deferred

- **Saving to disk** — `editorRowsToString` (join rows with `\n`, the inverse of
  `FileIO.splitRows`), `open` with `O_CREAT|O_TRUNC|O_WRONLY`, `write`, and the
  **dirty quit-confirm** — **Stage 5**. Stage 4 produces its inputs: the mutated
  `textRows` and the `dirty` flag.
- **Status / message bars** (showing the filename, line count, `(modified)`, and
  keypress hints) — **Stage 5**.
- **In-place `Buffer`/`Array` splice** (COW memmove) and static **Perceus reuse** of a
  provably-unique copy — the native/backend pass (§1.1).
- **`Find`** (incremental search) — **Stage 6**; **syntax highlighting** — **Stage 7**.
- kilo's per-row `idx` field (multi-line-comment tracking) — introduced with **Stage 7**.
