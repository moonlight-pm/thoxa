# Changelog

All notable changes to Thoxa are documented here. This changelog is **honest by construction**:
a feature is only listed as done when it has a passing test under `spec/conformance/` or `tests/`.
(The prior incarnations' changelogs claimed features "100% complete" that never worked; we do not
repeat that.)

Format loosely based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Export on constructor parameters
- **`export` on type ctor params** publishes the field under the same name:
  `export type Header(export name: String, export value: String) {}` then `h.name`.
  Body `export let` / `export fn` unchanged. `export` on function parameters is a
  parse error. Prefer keyword params for self-documenting construction.
  Spec: `docs/specs/2026-07-18-export-ctor-params-design.md`. Migrated
  `examples/http/Http.thx` `Header`/`Request`/`Response`. Tests in `test_parser.py`,
  `test_interp.py`; `SPEC.md` §5.

### Kilo Stage 7 — syntax highlighting (`Bytes.equals`) — completes the port (green: 614 tests)
- **`Bytes.equals(other) -> Bool`.** Structural byte-sequence comparison — `true` iff same length
  and same bytes (length-counted, embedded-NUL safe; `Bytes ==` is identity, this is content
  equality). The Thoxa answer to `strncmp`/`strcmp`, on which keyword / `//` `/*` `*/` / extension
  matching is built. Interpreter + `ffi.py` only — **no checker change**. `spec/conformance/bytes_equals.thx`;
  `tests/test_bytes_equals.py`; `SPEC.md` §9.1.
- **`examples/kilo/Syntax.thx` — the classifier (kilo `editorUpdateSyntax` + `editorSyntaxToColor`
  + `editorSelectSyntaxHighlight`).** A closed `Hl` sum type (`Normal`/`Comment`/`Keyword1`/`Keyword2`/
  `Str`/`Number`/`Matched`) with a separate `color` match; a data-driven C `Syntax` table; and a pure
  `highlightRow(render, prevOpen, syntax) -> RowHl` that classifies each render byte (single-/multi-line
  comments, strings with escapes, numbers, keyword-1/2 at separator boundaries) and returns the row's
  ending open-comment flag. Extension-based `selectSyntax`. `tests/test_kilo_syntax.py`.
- **State caches a whole-document highlight fold** (`refreshHighlight`), recomputed on any document /
  filename change, carrying the multi-line-comment-open flag from row 0 — so cross-row comment state
  needs no kilo-style forward recursion. `Screen.drawRowBytes` interleaves ANSI SGR codes per cell
  (color-change-tracked, per-row `\x1b[39m` reset).
- **The deferred Find `HL_MATCH` (Stage 6) lands as an immutable overlay**, not kilo's
  `malloc`/`memcpy`/restore of a mutable `hl[]`: `State.rowCells` derives a fresh `Array<Hl>` for the
  frame with the match painted `Matched` (chars→render mapped via `cxToRx`); `Find` sets/clears the
  match as the cursor jumps and clears it whenever the query stops matching or empties (faithful to
  kilo's restore-every-keystroke). `examples/kilo/sample.c` exercises every class end-to-end (real-pty
  smoke asserts the colors + the blue match). **This completes the kilo port** (all 10 modules, all 5
  language-design forks resolved).

### Kilo Stage 6 — Find (incremental search) + the modal line-input prompt (`Bytes.find`) (green: 578 tests)
- **`Bytes.find(needle) -> May<Int64>`.** Substring search over a byte buffer, the Thoxa-idiomatic
  answer to C's `strstr`: `Has(i)` at the **leftmost** index where the `Bytes` `needle` occurs, else
  `Not` — **never a `-1` sentinel** (empty needle → `Has(0)`; needle longer than the buffer → `Not`;
  length-counted, so an embedded NUL never cuts the search short). Interpreter + `ffi.py` only — **no
  checker change** (`Bytes` members type as `UNKNOWN`, and `match … { Has(i){} Not{} }` on an
  `UNKNOWN` scrutinee needs no `else`). `spec/conformance/bytes_find.thx`; `tests/test_bytes_find.py`;
  `SPEC.md` §9.1.
- **`examples/kilo/Prompt.thx` — a modal line-input prompt (kilo `editorPrompt`).**
  `promptLine(label) -> May<String>` echoes into the message bar (raw mode has no terminal echo) and
  reuses the single-frame refresh; Enter with content → `Has(text)`, ESC → `Not`, Backspace/Ctrl-H
  erase, printables append. Used for the **save-as prompt** — the Stage-5 deferral, so an unnamed
  buffer can now be named and saved (`State.setFilename`).
- **`examples/kilo/Find.thx` — incremental search (kilo `editorFind`/`editorFindCallback`).** A pure,
  deterministically-tested `searchFrom(rows, query, lastMatchRow, direction) -> May<Match>` (the
  wrap-around row scan over each row's `chars`, so the match column *is* the byte cursor column — no
  `rxToCx`; leftmost-in-row and row-granular, faithful to kilo, with the `lastMatchRow == -1` →
  forward guard) and an interactive `run()`: Ctrl-F searches as you type, arrows step next/previous
  (wrapping), Enter keeps the match, ESC restores the pre-search cursor + scroll. **Match
  *highlighting* is deferred to Stage 7** (it belongs with the `hl` array) — Find jumps + scrolls to
  the match now. `tests/test_kilo_find.py`.
- **`examples/kilo/State.thx`** gains `allRows` (feeds `searchFrom`), `jumpTo`/`restoreView` (Find's
  cursor jump + ESC-cancel), and `setFilename` (the save-as path). `tests/test_kilo_state.py`.
- **`examples/kilo/Main.thx` — an I/O-free `dispatch` seam.** `dispatch(k)` changes from `-> Bool` to
  `-> Action` (`Quit | RunFind | PromptSave | Resume`) so the interactive sub-loops (Find, save-as
  prompt) run in the driver loop, never in the pure key-mapper — preserving the deterministic
  `dispatch` seam Stage 4 built. `tests/test_kilo_dispatch.py`; the `tests/test_kilo_keys.py` pty
  smoke drives find (type + step + ESC-restore, read off the status bar) and save-as (type a name,
  Enter, assert the file on disk) end to end. Deferred: syntax highlighting + the match highlight
  (Stage 7).

### Kilo Stage 5 — save + quit-confirm + status/message bars (octal literals `0o…`, `time()`) (green: 547 tests)
- **Octal integer literals `0o…` / `0O…`.** A third integer base beside `0x`/`0b` (digits `0`–`7`,
  `_` separators, sized suffixes), added for Unix file modes (`0o644`). All three based-int forms
  now route through one lexer helper (`_radix_int`), which also **hardens a pre-existing latent
  crash**: an empty digit run (`0x`, `0o`, `0b`, or only `_`) is now a clean located `ThoxaError`
  instead of an uncaught `ValueError`. `spec/conformance/octal_literals.thx`;
  `tests/test_octal_literals.py`; `SPEC.md` §1.5.
- **`examples/kilo/FileIO.thx` gains the save path (kilo `editorSave`/`editorRowsToString`).**
  `save(path, rows)` serializes the rows (`rowsToString` joins with `\n`, a trailing `\n` per row
  so it round-trips through `splitRows`) and writes them with `O_WRONLY|O_CREAT|O_TRUNC` (the LF-A
  bitwise-`|` payoff, flags written in octal) + `write` + `close`, returning the byte count; a
  **short (partial) write is reported as a failure** (matching kilo's `write(...) == len`), so a
  truncated write never clears `dirty`. Takes the rows as a parameter, so FileIO imports only
  `Row`/`ffi` (never `State` — no cycle). `tests/test_kilo_fileio.py`.
- **`examples/kilo/State.thx` consumes the `dirty` flag.** `save()` writes via FileIO, clears
  `dirty` only on a full write, and reports "N bytes written"; a **status message with a 5-second
  fade** backed by a real `time()` primitive (a private libc extern via the established `Bytes(8)`
  out-param idiom — no compiler change; the clock read is split from the freshness policy so tests
  drive the fade deterministically); a **quit-confirm** (`confirmQuit`/`resetQuitTimes` — a dirty
  buffer needs four Ctrl-Q presses, a clean one quits immediately); and `textHeight()` (the terminal
  minus the two reserved bar rows) which `scroll`/`pageUp`/`pageDown` now track (with a lower clamp
  so a 1–2-row terminal never drives the cursor negative). `tests/test_kilo_state.py`.
- **`examples/kilo/Screen.thx` draws the two bottom bars (kilo `editorDrawStatusBar`/`MessageBar`).**
  A reverse-video **status bar** (filename/`[No Name]`, line count, `(modified)`, right-aligned
  `cy+1/numRows`) and a **message bar** (`currentMessage()`), each built as a returned `Bytes`
  (`Buffer` is value-typed, so a callee cannot append into the shared frame) and appended once; the
  text region is `textHeight()` rows. No new primitive. `tests/test_kilo_screen.py`.
- **`examples/kilo/Main.thx` dispatches Ctrl-S + the quit-confirm.** `dispatch` splits into
  `isQuitKey`/`applyKey`: Ctrl-Q routes to `confirmQuit` (the only key that skips the counter reset),
  Ctrl-S saves, every other key resets the counter; a startup hint seeds the message bar.
  `tests/test_kilo_dispatch.py` pins the routing; the `tests/test_kilo_keys.py` pty smoke types,
  Ctrl-S, then asserts the file changed on disk and the status/message bars render. Deferred: the
  interactive filename prompt for an unnamed buffer (Stage 6), Find (Stage 6), syntax (Stage 7).

### Kilo Stage 4 — the editor (`insertAt`/`removeAt` value splice) (green: 518 tests)
- **`Bytes.insertAt(at, byte)` / `Bytes.removeAt(at)` and `Array<T>.insertAt(at, item)` /
  `Array<T>.removeAt(at)`.** A **functional, copying, strict-bounds** value splice: each
  returns a *fresh* value (a new `Bytes` / a new array) with the byte/element inserted or
  removed, leaving the receiver untouched; out-of-range indices trap like `slice`/indexing
  (`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove; `Bytes` masks the byte to 0–255). This
  **resolves roadmap fork #4** by committing to value semantics — in-place `Buffer`/`Array`
  memmove (and static Perceus reuse of a provably-unique copy) is left to the native pass.
  The checker is unchanged (it types these methods `Unknown`, as it does `slice`).
  `spec/conformance/splice.thx`; `tests/test_splice.py`; `SPEC.md` §9 / §9.1.
- **`examples/kilo/Row.thx` gains per-line editing.** `insertChar`/`deleteChar`/`take`/`drop`/
  `concat` each return a **new `Row`** (so `render` re-derives via the constructor — kilo's
  `editorUpdateRow`), plus a public `bytes()`/`text()` read accessor (`concat` reads
  `other.bytes()` since a private field of another instance isn't readable). `tests/test_kilo_row.py`.
- **`examples/kilo/State.thx` becomes the editor core.** New `insertChar`/`insertNewline`/
  `deleteChar`/`deleteForward` run kilo's `editorInsertChar`/`InsertNewline`/`DelChar`
  algorithms — splicing `textRows` functionally, swapping a within-line edit in place,
  materializing the phantom EOF row, joining/splitting lines, and fixing the cursor — plus a
  `dirty` flag (`isDirty()`; consumed by Stage 5's quit-confirm). Editing lives here (not a
  separate `Edit` module) to keep `cx`/`cy`/`textRows` private. `tests/test_kilo_edit.py`.
- **`examples/kilo/Main.thx` dispatches edits.** The key→action mapping is extracted from
  the driver loop into a testable `dispatch(k) -> Bool` (`true` = quit): printables insert,
  Enter splits, Backspace/Ctrl-H erase or join, Delete forward-deletes; Ctrl-L/ESC are
  no-ops; Ctrl-Q quits (quit-confirm is Stage 5). `tests/test_kilo_dispatch.py` pins the
  byte→action mapping deterministically (a mis-mapping fails fast); the `tests/test_kilo_keys.py`
  pty smoke then drives the whole editor over a real terminal and asserts the newline split
  actually rendered (the cursor reaches screen row 2). Deferred: save + status/message bars
  (Stage 5).

### Kilo Stage 3 — the read-only viewer (`Bytes.slice` + command-line `arguments`) (green: 481 tests)
- **`Bytes.slice(start, end) -> Bytes`.** A copying, strict-bounds byte slice: a fresh,
  independently owned `Bytes` over the half-open range `[start, end)`; out-of-range bounds
  (`0 ≤ start ≤ end ≤ count` violated) trap like an index. Copying, not a borrowing view —
  consistent with the growable-`Buffer` COW model, so a slice can never dangle. This **resolves
  roadmap fork #5** by committing to bytes: rows are byte-buffers end-to-end (ASCII-first), so the
  cursor's coordinates are byte offsets and the byte↔codepoint question disappears (String slicing +
  codepoints deferred, no forcing use). `spec/conformance/bytes_slice.thx`; `tests/test_bytes_slice.py`;
  `SPEC.md` §9.1.
- **Optional command-line `arguments` on `main`.** `main` may declare one `arguments: Array<String>`
  parameter; `thoxa run <module> file.txt` calls it with `[<module>, "file.txt"]` (C-style, program
  path first). Threaded through the CLI (`argparse.REMAINDER`, so option-like tokens pass through) and
  bound via `apply()`/`_bind_values`; a zero-parameter `main()` is unaffected. `tests/test_arguments.py`;
  `SPEC.md` entry section.
- **`examples/kilo/Row.thx` — kilo's `erow`.** `Row(chars!: Bytes)` computes a tab-expanded `render`
  (`KILO_TAB_STOP = 8`) once at construction; `cxToRx` maps a chars byte-index to a render column, and
  `window(coloff, width)` returns the visible `render` slice (bounds clamped so the strict slice never
  traps). `tests/test_kilo_row.py`.
- **`examples/kilo/FileIO.thx` — kilo's `editorOpen`.** `open(path)` reads the whole file (rewinding
  after the size seek — mirrors `Io.read`) and splits it into `Row`s on `\n`, dropping a trailing `\r`;
  a bundled `examples/kilo/sample.txt` drives the demo. `tests/test_kilo_fileio.py`.
- **`State` becomes a file-coordinate cursor.** The loaded file is `textRows`; the cursor cx/cy index
  the file (Int64 byte offsets), with `scroll()` (kilo `editorScroll`) tracking `rowoff`/`coloff`, and
  boundary-aware movers (left/right wrap lines, up/down snap to the shorter row, page up/down with the
  EOF clamp) — every `row(cy)` access guarded so an empty file never traps. `tests/test_kilo_state.py`.
- **`Screen` draws the scrolled file; `Main` drives the viewer.** `refresh()` scrolls to the cursor and
  paints each visible row's windowed render (welcome banner only for an empty buffer, truncated via
  `slice`; cursor placed from `rx`). `Main` loads `arguments[1]` (missing file ⇒ empty buffer) and
  dispatches keys to the movers. The pty smoke opens the sample file and asserts rendering, tab
  expansion, and vertical scroll. Deferred: editing (Stage 4), status/message bars + save (Stage 5).

### Kilo Stage 2 — the `Screen` module + string/`Char` `\xNN` escapes (green: 454 tests)
- **`\xNN` hex escapes in `Char`/`String` literals, ASCII-restricted.** Extended the byte-literal
  `\xNN` escape (Stage 1) to `'…'` and `"…"` through the shared `Lexer._escape`, denoting the
  character with codepoint `0xNN` and **restricted to `0x00`–`0x7f`** — a `Char` is a Unicode scalar
  and a `String` is UTF-8, so a lone byte `≥ 0x80` is ambiguous (Rust's rule). Byte literals keep the
  full `0x00`–`0xFF` range. This is what lets VT100 sequences be written as ordinary literals
  (`"\x1b[2J"`). `spec/conformance/string_hex_escape.thx`; `tests/test_string_hex_escape.py`;
  `SPEC.md` §1.5/§8.
- **`examples/kilo/Screen.thx` — one frame in a `Buffer`, one `write()`.** `refresh()` builds kilo's
  no-file screen — hide cursor (`\x1b[?25l`), home (`\x1b[H`), a `~` down the left column with a
  centred `Kilo editor -- version 0.0.1` banner a third of the way down, per-line erase (`\x1b[K`),
  the cursor repositioned from `State` (1-based CUP), show cursor (`\x1b[?25h`) — into a growable
  `Buffer`, `freeze()`d and flushed through a private `sysWrite` extern. `clear()` wipes the screen
  (`\x1b[2J`). Deferred to their stages: text rows/scrolling (Stage 3), status/message bars and SGR
  colours (Stage 7). `tests/test_kilo_screen.py` pins the exact frame bytes.
- **The `Main` driver now paints.** `loop { poll size; Screen.refresh(); dispatch(readKey()) }`;
  Ctrl-Q clears the screen and quits; the terminal is restored on every exit. The Stage 1 text echo
  is gone — the pty smoke (`tests/test_kilo_keys.py`) now asserts the painted frame and reads the
  cursor trail from the per-frame CUP escape.

### `print`/`println` are Thoxa, and inferred `Trial` always shows both type args (green: 338 tests)
- **`print`/`println` moved out of the host into the prelude.** They are now ordinary Thoxa
  functions in `std/Prelude.thx`, written over a private `sysWrite` extern bound to libc `write(2)`
  (fd 1) — no Python in the output path, dogfooding the FFI like `examples/ffi_hello.thx`. `println`'s
  argument is optional (`println()` prints a bare newline); both return `Void`, and any value
  interpolates (`println("{n}")`). The old host builtins and the `_BUILTINS` set are gone.
- **Real stdout, bridged for capture.** Program output now goes to real fd 1. A non-default output
  sink (tests' `run(out=…)`, the notebook) is served by an interpreter-side fd-1 capture that
  replays into the sink (`Interpreter._capture_stdout`; grep `FD1-CAPTURE`). This is a deliberate,
  clearly-marked bridge to be replaced once callers capture fd 1 directly; the `run_out` test API is
  unchanged.
- **A `Trial` shows both type args or none.** Inferred fallible returns (constructors and
  fn/methods) render the full `Trial<T, E>` only when both `T` and `E` are known; when `E` can't be
  determined the return is left undeclared (bare) rather than a partial `Trial<T>`. `May<T>` keeps
  its single arg. Reverts the earlier `Trial<Self>`/`Trial<T>` drop-E rendering.
- **Cross-module method returns feed hover inference.** During the quiet return-inference pre-pass,
  a value of a namespace-imported type resolves its members' return types (never emitting a
  diagnostic), so `examples/http/Main.thx` now hovers `listen() -> Trial<Void, String>`,
  `accept() -> Trial<Client, String>`, `receive() -> Trial<Void, Void>`, and `close() -> Void`. A
  `Fail(...)`/`Not` branch of an `if` is treated as the error channel when computing the success type.

### Inferred Trial/May threading — fallible constructors and variant-carrying functions (green: 239 tests)
- **A `?` or a variant construction infers a fallible return.** A function or method is Trial- or
  May-returning by inference when its body uses `?` **or directly constructs a variant** (`Fail`,
  `Pass`, `Not`, `Has`): no `-> Trial<…>`/`-> May<…>` annotation and no explicit wrap on the success
  tail. The tail is auto-wrapped, a `?` short-circuits, and a tail that already yields a Trial/May
  (a `Pass`/`Fail`, a `Not`/`Has`, or a call to another fallible function) is passed through, not
  double-wrapped. `?` and any `Pass`/`Fail` select the Trial family (wrap in `Pass`); a lone
  `Not`/`Has` selects May (wrap in `Has`). Construction is detected as a `TypeValue` in expression
  position, so a `match` arm (`Pass(v) { … }`) never counts as one.
- **Fallible constructors.** A `?` in a field initializer (or ctor-param default) makes the
  constructor return `Trial<Self, E>`: `Pass(instance)` on success, or the short-circuited `Fail`.
  Threaded at the call site with `?` (`let server = Server(port: 9091)?`). Constructors infer only
  from `?` — a `Fail` *stored* in a field is not a short-circuit; no-`?` constructors stay infallible.
- **`Fail()` carries Void.** `Fail`'s payload now defaults to `Void` (`Fail<E>(error!: E = Void)`),
  so `Fail()` is a failure carrying no information. Positional params may now carry defaults, which
  must be trailing (`bind_args`/`_check_signature`).
- **Opt-outs preserved.** An explicit non-Trial/May return type (e.g. `main -> Int64`) keeps the raw
  value even with a `?` in the body. So `Net.thx`'s explicitly-annotated socket methods are unchanged.
- **Scope.** Runtime binding change in `interp.py` (`construct`, `_run_body` over `call_function`/
  `call_method`/`apply`), gated by a static body scan (`_fn_trial_family`). Demonstrated in
  `spec/conformance/fallible.thx`; `examples/http` now models the connection as a `Server` + `Client`
  pair whose `receive` infers its Trial from `Fail()`, and `Socket.send` takes its payload positionally.

### Prelude result/option constructors take positional payloads (green: 222 tests)
- **`Pass`, `Fail`, `Has` are now positional.** Write `Pass(x)`, `Fail(e)`, `Has(v)` — the
  payload no longer takes a label (`Pass(value: x)` is an error). `Not` is unchanged (nullary).
  The change is just the `!` positional marker on the prelude ctor params (`Has<T>(value!: T)`,
  `Pass<T>(value!: T)`, `Fail<E>(error!: E)`); pattern destructuring (`Pass(v)`, `Fail(e)`) was
  already positional and is untouched.
- **Migration.** Every call site across the prelude, `std`, `spec/conformance`, `examples/http`,
  the notebook sample, `README`, and `SPEC.md` updated to the positional form.

### Bare-name argument punning (green: 220 tests)
- **Call-site sugar.** An unlabelled bare identifier that matches a keyword parameter is
  shorthand for `name: name`: `serve(port)` means `serve(port: port)`, `server.bind(port)`
  means `server.bind(port: port)`. Works for functions, methods, and constructors (all bind
  through one path). Only bare identifiers pun — literals and compound expressions never do.
- **Disambiguation.** Punning matches *keyword* parameters only. A bare name matching a
  positional (`!`) parameter stays positional; a bare name matching a keyword parameter is
  *always* the pun, so it cannot fill a positional slot. Ordering is unchanged (a punned arg
  counts as named and must follow any positionals).
- **Scope.** Purely a runtime binding change (`interp.py:bind_args`); the checker already
  type-checks the bare name as an ordinary variable reference, so an undefined name still
  errors. Demonstrated in `spec/conformance/params.thx`; `examples/http` now uses the sugar.

### Modules are PascalCase types, instances are camelCase; a `Main.thx` entrypoint (green: 211 tests)
- **Naming.** A module file describes a *type*, so its name is now **PascalCase** (`Http.thx`,
  `std/Math`, `std/Prelude`); importing it binds a singleton *instance*, so the namespace binding
  is **camelCase**, chosen freely by the importer (`import http from "./Http"`, or `import client
  from "./Http"`). A non-PascalCase file module and a PascalCase namespace binding are both errors.
  The old "the namespace name must equal the file" rule **and** the namespace `as` clause are
  removed — the binding is already free; selective `as` is unchanged. Named roots `std`/`ffi` stay
  lowercase (they are roots, not modules).
- **`Main.thx` entrypoint.** A runnable project is a directory containing a `Main.thx` that
  **exports** `fn main` (the runtime calls it from outside the module, so it must be public). `thoxa
  run <dir>` and `thoxa check <dir>` resolve `<dir>/Main.thx`; `thoxa run <file>` still runs a single
  file directly, calling its `main` if present. The entry's directory is the project root for `~/…`.
- **Migration.** Every bundled `std`/conformance module renamed to PascalCase; the single-file
  `examples/http_server.thx` became the multi-file **`examples/http/`** project (`Main` + `Net` +
  `Http`, run with `thoxa run examples/http`). Design:
  `docs/specs/2026-07-13-thoxa-pascalcase-modules-and-main-entrypoint-design.md`.

### M8 — Module system: cross-file imports & a real stdlib seam (green: 190 tests)
- **Every `.thx` file is a module**; the file you run is the entry (keeps its script body), and
  imported modules are declarations-only. `export` publishes a top-level `let`/`fn`/`type`/`extern fn`;
  a top-level `mut` is always private (module state reached through exported functions).
- **Logical resolution** (`thoxa/loader.py`): `./…` local (siblings/children; `..` is an error —
  reach up via the project root), `~/…` the project root, and bare **named roots** (`std/…` the
  bundled standard library, `ffi` the built-in FFI module). Shared by interpreter and checker so they
  agree on what a specifier means.
- **Import surface**: selective `import { a, b as c } from "…"` and namespace `import m [as x] from "…"`
  (a namespace is *not a value* — valid only left of a `.`; its written name must be the module's
  canonical name unless aliased). `as` aliasing in both forms; no glob import.
- **Closed prelude**: the algebraic vocabulary now lives in an auto-imported **`std/prelude.thx`**;
  `sqrt` moved to **`std/math.thx`** (a Thoxa module binding libc via `extern fn`); `abs`/`min`/`max`/`len`
  were dropped as ambient builtins. Every name is now either the closed prelude or an explicit import.
- **Two-phase load**: names register first (so `fn`/`type` mutual recursion across files works), then
  `let`/`mut` values initialize in dependency order, each module exactly once. A cycle among *value*
  initializers is a located error; module-level private `mut` is verified end-to-end.
- Conformance: `modules` (`./` + `~/`, exported fn/type/let, transitive diamond), `namespaces`
  (namespace + `as`), `cycles` (fn + type mutual recursion across files), `modstate` (private-`mut`
  counter). Full design: `docs/specs/2026-07-12-thoxa-module-system-design.md`.

### M7 — libc FFI: real syscalls from Thoxa (green: 128 tests)
- **`extern fn`** binds libc functions:
  `extern fn write(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c"`. A mandatory
  `from "<library>"` clause (v1: only `"c"`) with an optional C-symbol string for names Thoxa
  can't spell (`... from "c" "inet_pton"`). The bridge is stdlib `ctypes` (`thoxa/ffi.py`) —
  still zero runtime dependencies.
- **`Bytes`** primitive: a mutable, bounds-checked byte buffer C writes into — `Bytes(n)`,
  `.count`, `.get`/`.set`, `.toString`, `Bytes.fromString`.
- **Error channel** via `import { errno, strerror } from "ffi"`, a built-in pseudo-module (no
  ambient globals; a real module system stays deferred).
- **Static checks**: extern params must be positional and marshalable; `Bool`/`Char`/`Int`/`Float`
  and String returns are rejected with fix-it hints; only the `"ffi"` import resolves.
- **Programs, all driven over real sockets/fds**: `examples/ffi_hello.thx` (write(2) to stdout),
  `examples/tcp_echo.thx` (socket/bind/listen/accept/recv/send), and `examples/http_server.thx`
  (an HTTP/1.1 200 responder). Verified end-to-end — pipe write, a live TCP echo, and a real HTTP GET.

### Language — removed ambiguous `Int` / `Float`
- The width-ambiguous `Int` and `Float` types are gone. Integer literals default to `Int64`, floats
  to `Float64`; every annotation names a concrete width. The checker rejects surface `Int`/`Float`
  in every type position (annotations, casts, `is`/`can`) with a fix-it, and the runtime drops them
  from its primitive set so they are undefined there too.

### M6 — Reference web server via host functions *(retired in M7)*
- Superseded by the libc FFI. The `serve(port, handler)` builtin and `thoxa/web.py` are gone; the
  web server is now written in Thoxa over real sockets (`examples/web_server.thx` →
  `examples/http_server.thx`). `Interpreter.apply` and the `install` seam remain as the general
  host→Thoxa bridge for any future native host functions.

### M5 — Literate interactive notebook (green: 76 tests)
- **Local kernel over the real interpreter** (`thoxa/notebook.py`, `thoxa notebook`): a
  standard-library-only `http.server` on `127.0.0.1` serving one self-contained HTML page.
- **Bindings persist across cells** (the evcxr model) — a single persistent `Interpreter`; the last
  expression of a cell shows its value; `println` output and friendly caret errors render inline.
  Seeded with a runnable four-cell tour. Zero external dependencies; no sandboxing (runs your own code).
- `NotebookSession` kernel is unit-tested (persistence, value display, error reporting). A public
  WASM browser playground remains deferred to the host-language port.

### M4 — Conversions + static checker (green: 70 tests incl. 12 conformance programs)
- **`as` / `can` / `is`** in the interpreter: numeric conversions (Int↔Float, truncation),
  `as String` via `toString` (built-in for primitives, user-defined for types), `is` runtime type
  guards incl. **union membership** (`circle is Shape`), and `can` (is a conversion defined?).
- **Conservative static checker** (`thoxa/checker.py`, `thoxa check <file>`): flags undefined names,
  non-Bool `if`/`while` conditions, non-numeric arithmetic operands, string `+`, unknown/private
  members (HasField/HasMethod on known types), and **static `match` exhaustiveness over known
  unions** — the promised M4 headline. Designed for **zero false positives**: a test asserts every
  conformance program checks clean. Anything unresolved is `Unknown` and never rejected.
- Conformance: `conversions`. *Deferred:* full inference (generics/let-polymorphism), static
  mutability/return-type checks, and user-defined `as` conversion blocks. The interpreter remains the
  runtime source of truth; `check` is advisory.

### M3 — Named-argument dispatch (green: 45 tests incl. 11 conformance programs)
- Strict **named-by-default** calling convention: named parameters are labelled and passed in
  declaration order (**order enforced**); parameters with defaults may be omitted; `!` marks a
  **positional** parameter (no label, no default, appears first); `...name` is a **variadic**
  (last, collects trailing unlabelled args).
- Signature validation (positionals-before-named, no default on positionals, variadic-last) and
  friendly call-site errors: out-of-order, unknown, missing, duplicate, positional-after-named.
- Builtins (`println`, `print`, …) remain positional. Example/conformance programs updated to use
  `!` where a positional call reads best. Conformance: `params`.

### M2 — Sum types, match, `?` (green: 37 tests incl. 10 conformance programs)
- **Prelude** (`thoxa/prelude.py`): `May<T> = Has<T> | Not` and `Trial<T,E> = Pass<T> | Fail<E>`,
  written in Thoxa itself and loaded into every interpreter.
- **`match`**: block-only arms `Pattern { body }` with constructor destructuring (`Pass(v)`),
  literal patterns (`0`, `"x"`), binding patterns (`other`), and a final `else { }` arm. `match`
  is an expression.
- **`?` operator**: unwraps `Pass`/`Has`, early-returns `Fail`/`Not` from the enclosing function.
- Conformance: `options` (May), `results` (Trial + `?`), `shapes` (user sum type + `else`).
- *Deferred to M4:* **static** exhaustiveness checking (requires the type checker). Today a
  non-exhaustive `match` with no applicable arm and no `else` traps at runtime.

### M1 — Core language (green: 30 tests incl. 7 conformance programs)
- **Lexer** (`thoxa/lexer.py`): `#`/`##` comments, hard-enforced camelCase/PascalCase identifiers
  (underscores rejected), integer/float literals with `0x`/`0b` bases, `_` separators and sized
  suffixes, char literals, and string literals with `{expr}` interpolation, `{{`/`}}` escaping and
  multi-line auto-dedent.
- **Diagnostics** (`thoxa/errors.py`): source spans with caret-annotated rendering.
- **Parser** (`thoxa/parser.py`): hand-written recursive-descent + Pratt. Disambiguates
  angle-bracket generics from comparison via bounded speculative parsing (`Stack<Int>(…)` vs
  `a < b > c`). No statement separators.
- **Interpreter** (`thoxa/interp.py`): `let`/`mut` with immutability enforcement; `fn` with implicit
  return and early `return`; `type` with constructor params, private-by-default fields/methods,
  `export`, mutable fields, and implicit field access (no `self`); `if`/`else`, `loop`, `while`,
  `for`-in over arrays/ranges/strings; arrays (literals, indexing with bounds traps, `.count`,
  `append`/`dropLast`/…); string interpolation; builtins (`println`, `print`, …).
  Also lands the general machinery `match`/`?` build on next.
- **CLI + REPL** (`thoxa/cli.py`, `thoxa/repl.py`): `thoxa run <file>`, `thoxa repl`, `thoxa --version`;
  `main() -> Int` return becomes the process exit code.
- **Conformance harness** (`tests/test_conformance.py`) + first 7 programs (hello, arithmetic,
  counter, arrays, control_flow, strings, methods) with pinned expected output.

### M0 — Clean base
- Established the revived project: `spec/VISION.md`, `spec/SPEC.md`, `docs/ARCHITECTURE.md`,
  the design doc, an honest changelog, and an "unfrozen" README.
- Preserved the three prior implementations read-only under `old/` (git-ignored).
- Reconciled the canonical surface syntax (previous docs/grammar/examples disagreed): `#`/`##`
  comments, hard-enforced `camelCase`/`PascalCase`, no statement separators, `{expr}` interpolation
  (interpolation-only), PascalCase primitives, `let`/`mut`, `type` with default-private members and
  no `self`, sum types as separate constructors + union alias, block-only `match` arms with a
  required-unless-exhaustive `else`, named-by-default parameters (order-enforced) with `!` positional,
  angle-bracket generics, and the `as`/`can`/`is` conversion triad (roadmap).
