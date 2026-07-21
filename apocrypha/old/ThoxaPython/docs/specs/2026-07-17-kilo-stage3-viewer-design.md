# Thoxa × Kilo — Stage 3: the read-only viewer (`Bytes.slice` + command-line `arguments`)

- **Date:** 2026-07-17
- **Status:** Approved design (Stage 3 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows Stage 2 (`Screen` skeleton).
- **Resolves roadmap fork #5** (string/byte slicing + byte↔codepoint coordinate).
- **Lands two language increments:** a copying `Bytes.slice(start, end)` (§2) and an
  optional command-line `arguments` parameter on `main` (§3).

## 1. Context & goal

Stage 2 painted a fixed skeleton (tilde column + welcome banner) driven only by the
cursor and screen size. Stage 3 makes it a **read-only file viewer**: open a file
named on the command line, load its lines as rows, and draw the portion of them that
fits — with **vertical scrolling** (`rowoff`), **horizontal scrolling** (`coloff`),
and **tab expansion** — while the cursor navigates the *file*, not just the screen
box. This is kilo through its "viewer" milestone (`editorOpen`, `editorScroll`,
`editorDrawRows`, `editorRowCxToRx`, tab-expanded `render`), stopping short of
editing.

It is the largest stage so far: two language increments plus two new modules (`Row`,
`FileIO`) and reworked `State`/`Screen`/`Main`. The through-line is **fork #5** — the
editor cannot draw a horizontal window of a line without slicing, and cannot split a
file into lines without slicing, so a slice primitive is forced.

### 1.1 How fork #5 is resolved

Fork #5 bundled two questions: *(a)* how to express a slice, and *(b)* the
byte↔codepoint coordinate problem (a cursor column is a byte index into `chars`, but
a UTF-8 string indexes by codepoint). Both are settled by **committing to bytes**:

- **Rows are byte-buffers end-to-end, ASCII-first.** A `Row`'s `chars` and `render`
  are `Bytes`; the cursor `cx`/`rx` are byte indices. This is exactly kilo's model
  (it is a byte-oriented editor) and it mirrors the FFI design's rule that `Char` (a
  Unicode scalar) is kept out of the byte layer. So (b) **disappears** — there is no
  codepoint coordinate in the viewer, only byte offsets.
- The slice therefore operates on **`Bytes`**, not `String`. **String slicing and
  codepoint coordinates are deferred** — nothing in kilo forces them, and adding
  String slicing now would *reopen* (b) (a byte range can split a UTF-8 scalar). When
  a stage forces multibyte text, String slicing gets its own fork with the codepoint
  question faced head-on.

## 2. Language increment A — `Bytes.slice(start, end) -> Bytes`

### 2.1 What the viewer forces

Two operations, both pure byte work:

- **Draw a row's horizontal window.** kilo writes `&row->render[coloff]` for
  `len = clamp(rsize - coloff, 0, screencols)` bytes. In Thoxa that is a sub-`Bytes`
  of `render`.
- **Split the file into lines.** kilo uses `getline`; we read the whole file into one
  `Bytes` and cut it on `\n` — each line is a sub-`Bytes`.

`Bytes` today exposes `.count`, `.get(i)`, `.set(i, b)`, `Bytes.fromString(s)` — an
element/length surface, but no sub-range. `Bytes.slice` adds it.

### 2.2 The decision: a copying method with strict bounds

`Bytes.slice(start!: Int64, end!: Int64) -> Bytes` returns a **fresh, independently
owned `Bytes`** holding the half-open range `[start, end)`.

- **Copying, not a view.** A slice is a new allocation with a copy of the bytes — it
  is *not* a borrow into the source. This is deliberate and consistent with the
  growable-`Buffer` design, which explicitly deferred "slices / views into a buffer (a
  borrow with a stable pointer)" as the one construct that reintroduces the
  invalidation hazard the COW model exists to avoid. A copying slice can never dangle;
  the source and the slice are fully independent afterwards. (The native backend will
  make a slice of a *unique* source zero-copy where it can prove safety; the prototype
  always copies — conservative, never unsafe.)
- **Method, not index-range sugar.** `render.slice(a, b)` reads as an explicit
  operation and matches the existing `Bytes` method surface (`.get`, `.set`,
  `.toString`, and the `.count` property). (The `a[i..j]` subscript form was considered
  and set aside; `.slice` keeps one obvious way and needs no parser change to
  distinguish element-index from range-index.)
- **`(start, end)`, end-exclusive.** `slice(a, b)` has length `b - a`, so
  `slice(0, count)` is a full copy and `slice(i, i)` is empty. This matches the range
  half-open convention already in the language (`0..10`).
- **Strict bounds — the caller clamps.** `slice` requires `0 ≤ start ≤ end ≤ count`;
  any violation **traps** with `slice [start..end] out of bounds (length N)`, the
  same class of runtime error as an out-of-bounds `a[i]`. A systems language should
  treat a bad index as a bug, not silently absorb it. The viewer never trips this
  because every caller computes a clamped range first — exactly as kilo clamps the
  visible length before it reads `render`. The clamp logic lives in one place
  (`Row.window`, §4), so `Screen` never hand-rolls a bound.

### 2.3 Implementation

| Site | change |
| --- | --- |
| `thoxa/ffi.py` `Bytes` | add `slice(self, start, end)`: bounds-check `0 ≤ start ≤ end ≤ count`, else raise `IndexError`; return a fresh `Bytes` (via `from_bytes` over the copied `[start, end)` range). |
| `thoxa/interp.py` — **two** sites | (1) the `_member` gate (`~line 1453`): add `"slice"` to the tuple of allowed `Bytes` members, or the call never dispatches. (2) the `_bytes_method` handler (`~1121–1141`): add a `slice` branch (require two ints; catch the `IndexError` and re-raise as the interpreter's span-carrying error `slice [start..end] out of bounds (length N)`). |
| `thoxa/checker.py` | **no change.** The checker is permissive for `Bytes` members — it has no `Bytes` method table and types them as unknown — so `Bytes.slice` type-checks with zero work. (Real return-type checking for `Bytes` methods would be net-new infrastructure, out of scope for this stage.) |
| `spec/SPEC.md` | document `Bytes.slice` in the collections/`Bytes` section. |

(Dispatch shape verified against the code: `Bytes`'s methods are `get`/`set`/`toString`, `.count` is a bare property, and `.append`/`.appendBytes` belong to `Buffer` — `slice` slots alongside `get`/`set`/`toString`.)

### 2.4 Conformance & tests

- **`spec/conformance/bytes_slice.thx` + `.out`** — the "done" gate, written so the
  `.out` carries no raw control bytes (assert via `.count`, `.get`, and printable
  comparisons). Pins: `slice(0, count)` equals the source content; a sub-range has the
  right length and bytes; `slice(i, i)` is empty; the slice is **independent** (a
  `.set` on a `Buffer` built from the source does not change the slice, and vice
  versa); a file-split example (split `"ab\ncd"` on `\n` into `"ab"`, `"cd"`).
- **`tests/test_bytes_slice.py`** — the error paths: `start > end`, `end > count`,
  negative `start` each trap with the bounds message; source is unmodified by a slice.

## 3. Language increment B — command-line `arguments`

### 3.1 What the viewer forces

A viewer must be told what to view. kilo takes the path from `argv[1]`. Thoxa's `main`
is called today with no arguments (`call_main` passes `[]`), so there is no way in.

### 3.2 The decision: an optional `Array<String>` parameter, C-style indexing

`main` may **optionally** declare one parameter:

```thoxa
export fn main(arguments: Array<String>) -> Int64
```

- **Optional & backward compatible.** A `main()` with no parameter still runs (the
  `http` example and every prior kilo stage keep working). `call_main` inspects
  `main`'s declared parameters: one parameter ⇒ pass the arguments array; zero ⇒ call
  with none.
- **C-style layout.** `arguments[0]` is the **program** — the module path exactly as
  typed on the command line — and `arguments[1..]` are the trailing tokens. For
  `thoxa run examples/kilo file.txt`, `arguments == ["examples/kilo", "file.txt"]`, so
  the viewer reads `arguments[1]` and guards on `arguments.count > 1`. This mirrors C's
  `argv`, which the port is transliterated from.
- **Full word.** The parameter is `arguments`, not `argv`/`args` — Thoxa spells names
  out.

### 3.3 Implementation — threading the tokens

| Site | change |
| --- | --- |
| `thoxa/cli.py` `run` subparser | collect trailing tokens after the module: `p_run.add_argument("arguments", nargs=argparse.REMAINDER)`. |
| `thoxa/cli.py` `cmd_run(path, arguments)` | pass `[path] + arguments` (path = the module as typed) down to `run`. |
| `thoxa/interp.py` `run(..., arguments=())` | forward to `call_main(list(arguments))`. |
| `thoxa/interp.py` `call_main(arguments)` | if `main.defn.params` has one entry, `self.apply(main, [arguments], span)`; else `self.apply(main, [], span)`. **Must be `apply`, not `call_function`** — `call_function`/`bind_args` operate on AST argument nodes, whereas `apply`/`_bind_values` bind already-evaluated runtime values by position (so the parameter stays *named*, no `!`). The arguments array is a Python `list[str]`, which *is* an `Array<String>` at runtime (`.count` and element access work). |
| `thoxa/checker.py` | accept a `main` with either arity; a one-parameter `main` must take `Array<String>`. |
| `spec/SPEC.md` | note the `main(arguments: Array<String>)` entrypoint form in §4 / the entrypoint section. |

### 3.4 Conformance & tests

- **`tests/test_arguments.py`** — a CLI-level test (arguments arrive from the process
  boundary, so this is a Python test, not a stdout-conformance slice): running a
  module that prints `arguments.count` / `arguments[1]` with trailing tokens sees them;
  with none, `arguments == [module]` (count 1); a zero-parameter `main` still runs
  unchanged. (The deterministic-stdout conformance harness does not pass process
  arguments, so it cannot host this; `Bytes.slice` carries the conformance slice for
  Stage 3's feature gate.)

## 4. `examples/kilo/Row.thx` — kilo's `erow`

A new module owning the row type. Read-only, so `render` is built **once at
construction** and never rebuilt (the rebuild-on-mutation path is Stage 4).

```thoxa
let tabStop = 8   # KILO_TAB_STOP

## One text line. `chars` is the raw bytes; `render` is the display expansion
## (tabs → spaces to the next tab stop). Both are Bytes — the viewer is byte-
## oriented (fork #5: rows are byte-buffers, ASCII-first).
export type Row(chars!: Bytes) {
    let render = expandTabs(chars)   # computed from the ctor param at construction

    ## Rendered-line length (bytes). `chars.count` is the raw length.
    export fn renderCount() -> Int64 { render.count }
    export fn charCount()   -> Int64 { chars.count }

    ## chars-index → render-column, accounting for tabs (kilo editorRowCxToRx).
    export fn cxToRx(cx!: Int64) -> Int64 { … }

    ## The visible byte window of `render`: render[coloff .. coloff+width], with the
    ## bounds clamped into [0, renderCount] so the strict `Bytes.slice` never traps.
    ## Empty when scrolled past the line's end.
    export fn window(coloff!: Int64, width!: Int64) -> Bytes {
        let begin = if coloff > render.count { render.count } else { coloff }
        let stop  = if coloff + width > render.count { render.count } else { coloff + width }
        render.slice(begin, stop)
    }
}
```

- **`expandTabs(chars!: Bytes) -> Bytes`** (module-private): walk `chars` in a
  `Buffer`; on a tab byte (`b'\t'`) append `tabStop - (column % tabStop)` spaces (always
  ≥ 1); else append the byte; `freeze()`. The parameter is **positional** (`chars!:`) so
  the bare call `expandTabs(chars)` in the field initializer binds (Thoxa is
  named-by-default; a keyword param would only bind by-name, and the bare identifier
  puns only when the keyword's name is literally `chars`). `render` being a field
  initializer that references the constructor parameter `chars` works because a
  constructor binds its params as instance fields before evaluating field initializers
  (verified against `Interpreter.construct`).
- **`cxToRx`** mirrors kilo: `rx = 0; for i in 0..cx { if chars.get(i) == tab {
  rx += (tabStop - 1) - (rx % tabStop) } rx += 1 } rx`.
- No `hl`/highlight array (Stage 7); no `insertChar`/rebuild (Stage 4).

## 5. `examples/kilo/FileIO.thx` — kilo's `editorOpen`

Mirrors the `examples/http/Io.thx` house style: private `sys`-prefixed externs behind
a `Trial`-returning API. It opens read-only, reads the whole file into one `Bytes`,
and splits it into `Row`s.

```thoxa
extern fn sysOpen(path!: String, flags!: Int32) -> Int32 from "c" "open"
extern fn sysRead(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c" "read"
extern fn sysLseek(fd!: Int32, offset!: Int64, whence!: Int32) -> Int64 from "c" "lseek"
extern fn sysClose(fd!: Int32) -> Int32 from "c" "close"

## Read `path` and split it into rows. Fail on any open/read error (the caller
## decides what a failure means — Stage 3's Main treats it as an empty new buffer).
export fn open(path!: String) -> Trial<Array<Row>, String>
```

- **Read the whole file:** `open(O_RDONLY)`; get the size with `lseek(fd, 0, SEEK_END)`,
  then **`lseek(fd, 0, SEEK_SET)` to rewind before reading** (mirrors `Io.read`, which
  seeks back — omitting the rewind leaves the offset at EOF, so `read` returns 0 and
  every file loads empty); `read` into a `Bytes(size)`, **looping until the buffer is
  full or `read` returns 0** (a single `read(2)` may short-read on a larger file). Close
  before returning.
- **Split on `\n`:** walk the file `Bytes` with an index; at each `\n` (`b'\n'`), take
  `file.slice(lineStart, i)`, strip a trailing `\r` (if the slice's last byte is
  `b'\r'`, slice one shorter) → `Row(lineBytes)`; advance `lineStart = i + 1`. A final
  line with no trailing newline is emitted too. An empty file yields zero rows.
- **Errors** are `Fail("open <path>: <errno text>")` etc., via the `Io`-style
  `fileErr` helper.

## 6. `State` — file-coordinate cursor + scroll

State grows from "cursor in a screen box" to "cursor in a file, projected onto a
screen." The loaded file lives in a new module-private `mut` **`textRows`** — named to
avoid colliding with the *existing* screen-height field `rows` (State.thx:12,
`mut rows = 0`). There is no separate `numrows`: `numRows()` returns `textRows.count`,
so length can never desync.

```
mut textRows: Array<Row> = []   # the loaded file (kilo's E.row[])
mut rowoff = 0                  # first visible file row (vertical scroll)
mut coloff = 0                  # first visible render column (horizontal scroll)
mut rx = 0                      # cursor's render column (from cxToRx)
mut filename = ""               # for a future save (Stage 5) + the bar (Stage 5)
```

`cy` now indexes **file rows** and may sit **one past the last line** (`cy` in
`0 .. numRows()` *inclusive* — kilo's appendable empty line); `cx` indexes into the
current row's `chars` (`0 .. charCount` inclusive).

**Integer widths (the one place two coordinate spaces meet).** A file coordinate is a
byte offset, and `Bytes.count`/`Row.charCount`/`Row.cxToRx`/`Row.window` are all
`Int64`, so `cx`, `cy`, `rx`, `rowoff`, `coloff` become **`Int64`**, and the accessors
`cursorX()`/`cursorY()`/`renderX()`/`rowOff()`/`colOff()` return `Int64` (Stage 1's
`cursorX()`/`cursorY()` were `Int32` — they widen). Screen dimensions stay **`Int32`**
(they come from `Terminal.windowSize()`); an `Int32` screen dim widens implicitly to
`Int64` where the two meet (e.g. `rx >= coloff + screenCols()`), and an `Int64` file
coordinate is **never** stored back into an `Int32` field (no implicit narrowing).

New/changed exports:

- `setRows(rows!: Array<Row>, name!: String)` — install a loaded file (sets `textRows`
  and `filename`).
- `numRows()` (= `textRows.count`), `row(index!: Int64) -> Row`, `rowOff()`,
  `colOff()`, `renderX()` — reads for `Screen`.
- **`scroll()`** — kilo `editorScroll`, called at the top of each `refresh()`:
  `rx = if cy < numRows() { row(cy).cxToRx(cx) } else { 0 }`; then
  `if rx < coloff { coloff = rx }`, `if rx >= coloff + screenCols() { coloff = rx - screenCols() + 1 }`;
  and the same pair for `cy`/`rowoff` against `screenRows()`.
- **Movement** — boundary-aware discrete functions (replacing Stage 1's
  `moveCursor(dx, dy)` delta trick, which can express neither line-wrap nor the snap).
  **Every access to the current row is guarded by `cy < numRows()`**, so an empty file
  or the one-past-the-end cursor never indexes `row(cy)` out of bounds:
  - `moveLeft`: `if cx > 0 { cx -= 1 } else if cy > 0 { cy -= 1; cx = row(cy).charCount() }`.
  - `moveRight`: `if cy < numRows() { let n = row(cy).charCount(); if cx < n { cx += 1 } else { cy += 1; cx = 0 } }` (no-op past EOF).
  - `moveUp`: `if cy > 0 { cy -= 1 }`. `moveDown`: `if cy < numRows() { cy += 1 }`. Then
    **snap** in both: `let n = if cy < numRows() { row(cy).charCount() } else { 0 }; if cx > n { cx = n }`.
  - `moveHome`: `cx = 0`. `moveEnd`: `if cy < numRows() { cx = row(cy).charCount() }`.
  - `pageUp`: `cy = rowoff`, then a screenful of `moveUp`. `pageDown`:
    `cy = rowoff + screenRows() - 1; if cy > numRows() { cy = numRows() }`, then a
    screenful of `moveDown` (kilo's PAGE_UP/DOWN; the clamp keeps `cy` from seating past
    EOF).

## 7. `Screen` — draw rows, scroll, position by `rx`

`refresh()` keeps its no-argument signature and reads everything it draws from `State`
(the filename is stored now but only *displayed* at Stage 5 with the status bar). The
welcome branch narrows to the empty-buffer case:

1. `state.scroll()` first (so `rowoff`/`coloff`/`rx` are current).
2. For each screen row `y` in `0..screenRows`: `filerow = y + rowOff()`.
   - `filerow ≥ numRows()` → the **no-content** line: a `~`, and — **only when
     `numRows() == 0`** — the centered welcome banner on the welcome row. The banner is
     now truncated with the new slice, clamped with an `if`-expression (there is no
     `min` primitive — same shape as `Row.window` and Stage 2's `Screen`; `screenCols()`'s
     `Int32` widens to the `Int64` slice bound):
     `banner.slice(0, if banner.count < screenCols() { banner.count } else { screenCols() })`
     (Stage 2 fell back to a bare `~` here because slicing didn't exist).
   - else → the row's visible window: `state.row(filerow).window(colOff(), screenCols)`
     appended to the frame.
   - then `\x1b[K`, and `\r\n` on all but the last row (the Stage 2 guard stays; the
     status bar that will remove it is Stage 5).
3. Cursor: `\x1b[{(cy - rowOff()) + 1};{(renderX() - colOff()) + 1}H` (screen-space,
   1-based).

`clear()` is unchanged.

## 8. `Main` — load the file, then loop

```thoxa
export fn main(arguments: Array<String>) -> Int64 {
    # window size seed + RawMode.enable() as before …

    ## arguments[0] is the program (module path); arguments[1] is the file to view.
    if arguments.count > 1 {
        match fileio.open(arguments[1]) {
            Pass(rows) { state.setRows(rows, arguments[1]) }
            ## Missing/unreadable ⇒ empty new buffer (record the name for a future
            ## save); no error, no exit — kilo shows the welcome screen for a new file.
            Fail(e) { state.setRows([], arguments[1]) }
        }
    }

    # screen.clear() → loop { poll size; screen.refresh(); dispatch(readKey()) } …
}
```

The dispatch `match` stays **exhaustive** over all nine `Key` variants plus `Press`:
the eight navigation keys map to their movers (`Up → state.moveUp()`,
`Home → state.moveHome()`, `PageUp → state.pageUp()`, …); `Delete { Void }` is a no-op
(editing is Stage 4); and `Press(b) { if b == ctrlKey(b'q') { break } }` — other
printables are no-ops (no text entry yet). The Stage-2 `Keys` imports (`Up`…`Delete`)
stay. `restore()` runs on every exit path. A bundled
`examples/kilo/sample.txt` (a few lines, including a tab) gives the demo and the pty
smoke something to render:

```
thoxa run examples/kilo examples/kilo/sample.txt
```

## 9. Verification

Stage-0/1/2 gate: a conformance slice for the *language feature*, deterministic module
tests + a pty smoke for the *editor*.

1. **`spec/conformance/bytes_slice.thx` + `.out`** — the `Bytes.slice` done-gate
   (§2.4). Rides the existing `test_conformance` / `test_checker` / `test_lsp` gates.
2. **`tests/test_bytes_slice.py`** — slice error paths + source non-mutation (§2.4).
3. **`tests/test_arguments.py`** — CLI forwards trailing tokens to `main(arguments)`;
   `arguments[0]` is the module; a zero-parameter `main` is unaffected (§3.4).
4. **`tests/test_kilo_row.py`** — `expandTabs` (a `"a\tb"` renders `a` + spaces to
   column 8 + `b`), `cxToRx` across a tab, `window` clamps at both ends (past-EOF →
   empty, no trap).
5. **`tests/test_kilo_fileio.py`** — split on `\n`; trailing `\r` stripped;
   no-final-newline line emitted; empty file → zero rows; missing file → `Fail`.
6. **`tests/test_kilo_screen.py`** (updated) — with a 2-row `State`, a `rowoff`/`coloff`
   set, assert the exact drawn window bytes for each visible row and that the welcome
   banner appears **only** when `numRows() == 0`; cursor CUP reflects `rx`/`coloff`.
7. **`tests/test_kilo_keys.py`** pty smoke (updated) — open `sample.txt`, `wait_for`
   the first line to render; arrow-down past the last visible row scrolls (`rowoff`
   advances, a lower line appears); a long line + arrow-right scrolls horizontally;
   `Ctrl-Q` quits and the terminal restores. The deterministic `State` movement tests
   (wrap + snap + clamp) run without a pty.
8. **`spec/SPEC.md`** — `Bytes.slice`; the `main(arguments: Array<String>)` entrypoint.
9. **`CHANGELOG.md`** — a Stage 3 entry under `[Unreleased]`, added once green.

## 10. Out of scope / deferred

- **Editing** (`insertChar`/`delChar`, rebuild-on-mutation) + the **array/`Buffer`
  splice** it needs — Stage 4, fork #4.
- **Status & message bars** (`\x1b[7m`, filename/line-count/position) and the
  last-row-newline change they bring — Stage 5. `filename` is stored now but only
  displayed then.
- **Save** (`O_CREAT|O_TRUNC`, `ftruncate`, serialize rows) — Stage 5; this is what
  makes the "missing file ⇒ empty new buffer" choice payoff. Distinguishing ENOENT
  from a real open error (permission, is-a-directory) is deferred with it — Stage 3
  treats *any* open failure as an empty buffer, which is honest only because there is
  nothing to lose yet.
- **String slicing + codepoint coordinates** — no forcing use; bytes chosen (§1.1).
- **`a[i..j]` subscript-slice sugar** — set aside for the `.slice` method; can be
  added later as sugar if a stage wants it.
- **SGR syntax colouring** — Stage 7.
