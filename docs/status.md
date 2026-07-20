# Thoxa — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**Execute:** [`docs/specs/2026-07-19-post-r9-http-completion-plan.md`](specs/2026-07-19-post-r9-http-completion-plan.md) — remaining queue after PR1 (early `return` ✅).

Baseline done: Trial monomorphs; wire I/O; loops; String fields; Buffer; Array; file I/O; String `==`/`!=`; static Router; **User+heap field RC (D64)**.

Critical path: early `return` → ~~User+heap RC~~ → `Array<User>` → Request/Response → Router type → bounded server. Optional: `Main.thx` dir entry, Io polish. Deferred outside that plan: M4, Perceus, effects, full `std/`/`ffi` roots, Buffer consume, insertAt/removeAt.
Critical path: User+heap RC → `Array<User>` → Request/Response → Router type → bounded server. Optional: `Main.thx` dir entry, Io polish. Deferred outside that plan: M4, Perceus, effects, full `std/`/`ffi` roots, Buffer consume, insertAt/removeAt.

## Done (ladder)

| Rung | Summary | Conformance |
|------|---------|-------------|
| R0 | hello / driver / C emit | `hello` |
| R1 | let, arith, if | `arithmetic`, `lets_if` |
| R2 | Trial, match, `?` (`Trial<Int64,Int64>`) | `trials` |
| R3 | user `type` + methods (Int64/Bool fields) | `types` |
| R4 | String, interpolation, simple RC | `strings` |
| R5 | `mut` + assign; `Bytes` (fromString/count/get/set/toString) | `bytes` |
| R6 | `extern fn` from `"c"` (+ optional C symbol); Int32/UInt64; marshal String/Bytes | `extern_libc` |
| R7 | C-layout struct packing; `Address<T>`; `Array<T,N>`; UInt32/Byte; bitwise | `cstruct_ffi` |
| R8 | modules / multi-file; `import`/`export`; module-level `let`/`mut` | `modules`, `namespaces`, `modstate` |
| R9 | HTTP subset: Net sockets + Http protocol; `Bytes(n)` / `.slice` | `http` (+ `bytes` slice) |
| R9+ | Trial monomorphs; accept/recv/send/connect; one-shot wire HTTP | `trials_mono`, `http_wire` |
| R9+ | Control flow: `while` / `loop` / `for` ranges / `break` / `continue` | `control_flow`, `loops` |
| R9+ | String/Bytes constructor-param fields (RC-in-struct) | `string_fields` |
| R9+ | Growable `Buffer` (COW, geometric cap, freeze→Bytes) | `buffer` |
| R9+ | Dynamic `Array<T>` (lit, index, append/dropLast, for-in) | `arrays` |
| R9+ | File I/O (`readFile` free fn) | `file_io` (+ `iolib/`) |
| R9+ | String `==`/`!=`; static file Router | `router` (+ `httplib/Router`) |
| R9+ | Early `return` / `return expr` (D63) | `early_return` |
| R9+ | User+heap field RC (Trial/match/`?`/method) | `user_rc` |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `Bytes(n)` zero buffer shipped (D18/D36); `as Bytes` with M4 later |
| Buffer / COW | SPEC §9.1 + growable-buffer design | **Shipped** (D50): RC+COW; freeze copies (consume deferred) |
| Dynamic Array | SPEC §9 + D53 | **Shipped** (D53–D55): `Array<T>` heap RC; functional append; for-in |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |
| FFI libraries beyond `"c"` | R6 subset | Only `"c"` (libc via headers) for now |
| Explicit `address()`, struct copy, `sizeof` | D23–D25 | Deferred (oracle LF-B §9) |
| Named roots `std/` / `ffi`, dir `Main.thx` | D30–D32 | Deferred; R8 is `./` + `~/` only |
| Full oracle HTTP | D36–D38 | Files + Router shipped; accept-loop server still post-R9 |

## Last session

- **2026-07-20 (PR1 early return):** Parse/lower `return` / `return expr`; type-check vs enclosing ret; drop live heap locals before early return; C `return` / bare `return;`; conformance `early_return` (if/for/while/loop/match/method/Void). Decision D63.
- **2026-07-20 (PR2 User+heap RC):** MIR uses `needs_drop` for Trial Pass/Fail/`?`/match extract (User field-walk via cgen `emit_rc`); method prolog Dups heap fields; match binds get fresh places (no cross-match overwrite leak); conformance `user_rc`; ASan clean. Unblocks `Trial<Box(s:String),…>` / File-shaped APIs (D64).
- **2026-07-19 (R9+ file I/O + Router):** `iolib/Io.readFile` (open/lseek/read/close; Fail open|seek|read); cgen `fcntl.h`; String `==`/`!=`; `httplib/Router.handle(root, path)` segments/traversal/index.html/Content-Type; for-in heap-elem RC fix (drop previous, not double-drop); avoid `Trial<User+String>` until field-RC hardened; conformance `file_io`, `router`.
- **2026-07-19 (R9+ Array):** Dynamic `Array<T>` (Int64/Bool/String/Bytes); literals `[…]`, empty `[]` with annotation; `.count`, `[i]` get/set, functional `append`/`dropLast`, `for x in arr`; display in interpolation; conformance `arrays`.
- **2026-07-19 (R9+ Buffer):** `Buffer()` / `Buffer(cap)`; `.count`/`.capacity`; COW `append`/`appendBytes` (write-back unique ptr); `freeze()` → Bytes (copy); geometric 0→8→16→32; conformance `buffer`.
- **2026-07-19 (R9+ loops + string fields):** `while` / `loop` / `for i in a..b` / `a..=b` / `break` / `continue`; constructor-param `String`/`Bytes` fields with RC walk on User dup/drop; conformance `control_flow`, `loops`, `string_fields`.
- **2026-07-19 (R9+ cont.):** `Bytes.find(needle[, from])` → index or -1; Http uses find instead of recursive scan.
- **2026-07-19 (R9+):** Generalized `Trial<Ok,Err>` (Void/String/User/Int64 monomorphs, heap RC, `?` rewrap); Net: accept/recv/send/connect/getsockname + String errors; `http_wire` one-shot client↔server; `trials_mono`.
- **2026-07-19 (R9):** HTTP subset — `httplib/Net` (socket/bind/listen/close), `httplib/Http` (request-line parse + response build), entry `http.thx`; `Bytes(n)` zero-fill, `Bytes.slice`; socket headers in cgen; Pass/Fail int widen.
- **2026-07-19 (R8):** Multi-file modules — `import {…} from` / namespace import, `export` on fn/type/let, `./` + `~/` loader, module-level let/mut statics + init, name mangling; `modules`, `namespaces`, `modstate` conformance.
- **2026-07-19 (R7):** C-layout structs (`type Regs { mut a: UInt32 … }`), zero-init `Regs()`, field/index assign, `Array<Byte,N>`, `Address<T>` implicit on mut places, bitwise, hex lits, UInt32/Byte; `cstruct_ffi`.
- **2026-07-19 (later):** R6 `extern fn` libc.
- **2026-07-19:** R5 (`mut`, `Bytes`), D14–D18, agent handoff.

## Verify quickly

```bash
cargo make test
```
