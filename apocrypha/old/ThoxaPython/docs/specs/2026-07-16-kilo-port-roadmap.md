# Thoxa × Kilo Port — Roadmap

- **Date:** 2026-07-16
- **Status:** Living roadmap (umbrella for the port and the language features it forces) — **LF-A (bitwise operators), LF-B (typed C-struct FFI), Stage 0 (Terminal raw mode), Stage 1 (Keys + State, byte literals `b'…'` resolving fork #3), and Stage 2 (Screen skeleton, with string/`Char` `\xNN` escapes) all shipped 2026-07-16; Stage 3 (Read-only viewer — `Bytes.slice` + command-line `arguments`, resolving fork #5) shipped 2026-07-17. Stage 4 (Editing — `Bytes`/`Array` `insertAt`/`removeAt` value splice, resolving fork #4) shipped 2026-07-17. Stage 5 (Save + quit-confirm + status/message bars — adding octal integer literals `0o…` and a real `time()` primitive) shipped 2026-07-17. Stage 6 (Find — incremental search + the modal line-input prompt/save-as, adding `Bytes.find -> May<Int64>`) shipped 2026-07-17. Stage 7 (Syntax highlighting — per-cell `hl`
  classification + ANSI SGR draw + cross-row multi-line-comment state + the deferred Find `HL_MATCH`
  overlay, adding `Bytes.equals -> Bool`) shipped 2026-07-17. **The kilo port is COMPLETE** — all 10
  modules and all 5 language-design forks resolved.**

## Goal

Port antirez/kilo (~1000 LOC single-file C terminal text editor) to Thoxa as `examples/kilo/`,
split into multiple focused modules. The port is a **forcing function to grow real language
primitives** — never a place for shortcuts (see memory `thoxa-demo-is-means`). Each capability kilo
needs is weighed as a *language increment*, and the language author makes every syntax/semantics call
(memory `thoxa-design-collaboration`).

## Module decomposition

Mirrors the `examples/http` shape (private raw externs → semantic `Trial`-returning wrappers → a
`Main` driver). Clean dependency DAG.

| Module | Responsibility | Depends on |
| --- | --- | --- |
| **Terminal** | `termios`/`ioctl` externs → `RawMode.enable()/restore()` + `windowSize()`, `Trial`-returning | ffi |
| **Keys** | `readKey() -> Key`: raw byte reads + VT100 escape decoding → a `Key` union | Terminal |
| **State** | the editor `E` singleton (rows, cursor, scroll, size, filename, dirty, syntax) as a module-level `mut`, reached through exported fns | — |
| **Row** | the `erow` type: raw byte `chars`, tab-expanded `render`, parallel `hl` array; rebuild on each mutation | — |
| ~~**Edit**~~ | *folded into `State` (Stage 4): the editing ops own `cx`/`cy`/`textRows`, kept private — no separate module* | — |
| **Screen** | build one frame in a `Buffer` (rows + SGR colors + status/message bars + cursor), single `write()` | Row, Syntax, Terminal, State |
| **FileIO** | open → rows (ENOENT ⇒ new buffer); serialize rows → `open`(O_WRONLY\|O_CREAT\|O_TRUNC)+`write`+`close` | Row, ffi |
| **Find** | modal incremental search: own read/refresh loop, temp match highlight, save/restore on ESC | Row, Screen, Keys, State |
| **Syntax** | classify render chars (numbers/strings/comments/keywords), HLDB by extension, cross-row comment state, `HL_*`→ANSI | Row |
| **Main** | driver: init → enable raw → `loop { Screen.refresh(); dispatch(Keys.readKey()) }`, restore on every exit | all |

## Language-design forks & decisions

| # | Fork | Decision |
| --- | --- | --- |
| 1 | Raw-mode `termios`/`ioctl` struct-pointer FFI | **✔ Implemented (LF-B):** markerless C-ABI structs (`ctypes.Structure`-backed), `Array<T, N>` fixed inline fields, `Address<T>` pointer type with implicit addressing, native-endian width-truncating field writes. Design `2026-07-16-thoxa-cstruct-ffi-design.md` + plan `docs/plans/2026-07-16-thoxa-cstruct-ffi.md`; commits `fdeb053..5682e19`, `spec/conformance/cstruct_ffi.thx`. Deferred to a later pass: explicit `address()`, const/read-only pointers, `mut`-at-boundary enforcement, struct copy, pointer returns, `sizeof`. |
| 2 | Bitwise operators | **✔ Implemented (LF-A):** `& \| ^ ~ << >>`, C-style symbols, bitwise tighter than comparison. Design `2026-07-16-thoxa-bitwise-operators-design.md` + plan `docs/plans/2026-07-16-thoxa-bitwise-operators.md`; commits `976cdac..af652f4`, `spec/conformance/bitwise.thx`. |
| 3 | `char`↔`int` code + ordered char compare | **✔ Resolved (Stage 1):** *not* a char↔int problem — the port is byte-oriented, so it works in `Byte` (as the FFI design already prescribes: `Char` is a Unicode scalar kept out of the FFI). Added **Rust-style byte literals `b'…'`** (a `Byte`; bare chars ASCII-only, `\xNN` for the full range) — no `.code`/`Char.fromCode`/`as` machinery; ordered compare + CTRL masking fall out because a `Byte` is a numeric int. Design `2026-07-16-kilo-stage1-keys-state-design.md`; `spec/conformance/byte_literals.thx`. |
| 4 | Array/`Buffer` splice (`insertAt`/`removeAt`) | **✔ Resolved (Stage 4):** committed to a **functional, copying, strict-bounds value splice** — `Bytes.insertAt/removeAt` (a byte in a line) and `Array<T>.insertAt/removeAt` (a row in the file), each returning a fresh value with the source untouched; in-place `Buffer`/`Array` memmove set aside for the Perceus/native pass. Rows stay fixed-length `Bytes`; an edit builds a new `Row` (render re-derived). Design `2026-07-17-kilo-stage4-editing-design.md`; `spec/conformance/splice.thx`. |
| 5 | String/byte slicing + byte↔codepoint coordinate | **✔ Resolved (Stage 3):** committed to **bytes** — rows are byte-buffers end-to-end (ASCII-first), so cursor coordinates are byte offsets and the byte↔codepoint question disappears. Added a **copying, strict-bounds `Bytes.slice(start, end)`** (a method, chosen over `s[a..b]` sugar); String slicing + codepoints deferred (no forcing use). Design `2026-07-17-kilo-stage3-viewer-design.md`; `spec/conformance/bytes_slice.thx`. |

Window size is **not** a fork — `ioctl(TIOCGWINSZ, Bytes(8))` (or the cursor-probe fallback) works
today. `SIGWINCH`/`atexit` are sidestepped (poll size each refresh; restore the terminal explicitly on
every exit path — no C→Thoxa callback FFI needed).

## Build order

```
LF-A  Bitwise operators          ✔ DONE (976cdac..af652f4; spec/conformance/bitwise.thx)
LF-B  Typed C-struct FFI (Address<T>, Array<T,N>)  ✔ DONE (fdeb053..5682e19; spec/conformance/cstruct_ffi.thx)
──────────────────────────────────────────────────────────────────
Stage 0  Terminal   (real termios raw mode + window size)   ✔ DONE (examples/kilo/{Terminal,Main}.thx; spec/conformance/kilo_terminal.thx; tests/test_kilo_terminal.py)
Stage 1  Keys + State            → resolves fork #3          ✔ DONE (byte literals b'…' + examples/kilo/{Keys,State}.thx + movable-cursor Main; spec/conformance/byte_literals.thx; tests/test_kilo_keys.py incl. the pty smoke)
Stage 2  Screen skeleton         ✔ DONE (string/Char \xNN escapes + examples/kilo/Screen.thx + painting Main; spec/conformance/string_hex_escape.thx; tests/test_kilo_screen.py + updated pty smoke)
Stage 3  Read-only viewer        → resolves fork #5          ✔ DONE (Bytes.slice + command-line `arguments`; examples/kilo/{Row,FileIO}.thx + file-coordinate State + row-drawing Screen + Main; spec/conformance/bytes_slice.thx; tests/test_{bytes_slice,arguments,kilo_row,kilo_fileio,kilo_state}.py + updated pty smoke)
Stage 4  Editing                 → resolves fork #4          ✔ DONE (Bytes/Array insertAt/removeAt + examples/kilo/{Row,State,Main}.thx editing; spec/conformance/splice.thx; tests/test_{splice,kilo_edit}.py + updated pty smoke)
Stage 5  Save + quit-confirm     → adds octal literals 0o… + a time() primitive  ✔ DONE (O_WRONLY|O_CREAT|O_TRUNC save + status/message bars + dirty quit-confirm; examples/kilo/{FileIO,State,Screen,Main}.thx; spec/conformance/octal_literals.thx; tests/test_octal_literals.py + updated test_kilo_{fileio,state,dispatch,screen,keys}.py)
Stage 6  Find                    → adds Bytes.find -> May<Int64>  ✔ DONE (incremental search + modal line-input prompt/save-as; examples/kilo/{Prompt,Find}.thx + State/Main; spec/conformance/bytes_find.thx; tests/test_{bytes_find,kilo_find}.py + updated test_kilo_{dispatch,state,keys}.py; design 2026-07-17-kilo-stage6-find-design.md)
Stage 7  Syntax highlighting     → adds Bytes.equals -> Bool  ✔ DONE (per-cell Hl classify + ANSI SGR draw + cross-row comment state + HL_MATCH overlay; examples/kilo/{Syntax,State,Screen,Find}.thx + sample.c; spec/conformance/bytes_equals.thx; tests/test_{bytes_equals,kilo_syntax}.py + updated test_kilo_{state,screen,keys}.py; design 2026-07-17-kilo-stage7-syntax-design.md)
```

**The kilo port is complete.** All 10 modules (Terminal/Keys/State/Row/Screen/FileIO/Find/Prompt/Syntax/Main)
and all 5 language-design forks are resolved; every stage has a passing conformance test and/or unit + pty smoke.

## Discipline

Every feature and stage runs its own cycle: **design → `docs/specs/` doc (+ `SPEC.md` update) →
a `spec/conformance/` test → TDD implementation → verify green → commit to master** (memory
`work-on-master`). A feature is "done" only when its conformance test passes.
