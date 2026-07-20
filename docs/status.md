# Thoxa — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**Editor plan complete** (phase 1 + phase 2, D71–D79). Optional follow-ons:

1. post-R9 **PR9** Io OpenMode/errno polish
2. Deferred megawork (M4, Perceus, effects, full `std/`/`ffi`, typed-member nav, multi-diag, …)
3. Manual: after committing `extras/`, set `extras/zed/extension.toml` `[grammars.thoxa].rev`, Install Dev Extension → `extras/zed`

HTTP ladder + `Main.thx` complete. Editor: D71–D79.

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
| R9+ | String `==`/`!=`; static file Router (free `handle`) | `router` (+ `httplib/Router`) |
| R9+ | Early `return` / `return expr` (D63) | `early_return` |
| R9+ | User+heap field RC (Trial/match/`?`/method) | `user_rc` |
| R9+ | Dynamic `Array<User>` (inline elems, D65) | `array_user` |
| R9+ | Match discard `Pass(_)`/`Fail(_)` (D66) | `match_discard` |
| R9+ | Request/Response/Header + parseRequest/writeResponse (D69) | `http_parse` (+ `http`, `http_wire`) |
| R9+ | Router type `handle(Request) -> Response` (D70) | `router` (via `writeResponse`) |
| R9+ | Bounded multi-request static HTTP server (D67) | `http_server` |
| R9+ | Directory entry `Main.thx` (D68) | `pkgmain` |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `Bytes(n)` zero buffer shipped (D18/D36); `as Bytes` with M4 later |
| Buffer / COW | SPEC §9.1 + growable-buffer design | **Shipped** (D50): RC+COW; freeze copies (consume deferred) |
| Dynamic Array | SPEC §9 + D53 | **Shipped** (D53–D55, D65): `Array<T>` heap RC; User inline elems + field RC; functional append; for-in |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |
| FFI libraries beyond `"c"` | R6 subset | Only `"c"` (libc via headers) for now |
| Explicit `address()`, struct copy, `sizeof` | D23–D25 | Deferred (oracle LF-B §9) |
| Named roots `std/` / `ffi` | D30–D32 | Deferred; R8 is `./` + `~/` only; **dir `Main.thx` shipped (D68)** |
| Full oracle HTTP | D36–D38 | Request/Response (D69) + Router (D70) + **bounded server (D67)** shipped; infinite accept-loop product binary deferred |

## Last session

- **2026-07-20 (editor PR7: cross-module):** `lsp/crossmodule` lazy resolve for hover/def: selective + alias, namespace name/member, `./`+`~/`. Open buffers win over disk (path-matched). `Binding.import_origin`; index stores AST. `check_source_with_overlays` for package check. Unit + handler tests. Decisions D78/D79. Editor phase 2 complete.
- **2026-07-20 (editor PR6: navigation):** LSP handlers: hover, definition≡declaration, references, documentSymbol, prepareRename/rename. Caps advertised on initialize. Signatures from source line + `##` doc_above; rename casing (PascalCase types / camelCase values). Single-file only (imports → import site). Unit tests def/hover/refs/outline/rename. Decision D77.
- **2026-07-20 (editor PR5: resolver):** `resolver.rs` + `lsp/index`; D76. MIR lower still own env.
- **2026-07-20 (editor PR3: extras/):** `extras/tree-sitter-thoxa` + `extras/zed`. D71/D75. Phase 1 complete.
- **2026-07-20 (editor PR2: thoxa lsp):** `thoxa lsp` stdio JSON-RPC (`Content-Length`); capabilities full sync + semantic tokens; `publishDiagnostics` on didOpen/Change, clear on Close; lexer semantic tokens (type/function/variable); `serde`/`serde_json` (D74). Optional `thoxa check <path>` prints `path:line:col: message`. Unit tests: framing, initialize, diags, tokens, shutdown. Decisions D72/D74.
- **2026-07-20 (editor PR1: check + spans):** `span::{Position,Range,byte_to_position,span_to_range}` (UTF-8 bytes → 0-based line + UTF-16 character). `lsp::check_source(filename, text) -> Vec<CheckDiagnostic>` — lex/parse/MIR lower, no cgen; first-error only. No imports → `lower_module`; imports + real path → disk package with entry AST overlay; imports without path → clear diagnostic. Unit tests (good/type/lex/parse/import). Decision D73.
- **2026-07-20 (plan: native editor + LSP):** Wrote [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md). Layout: `extras/{tree-sitter-thoxa,zed}` for DX glue; LSP server in `crates/compiler/src/lsp` as **`thoxa lsp`** (Rust, reuses MIR lower). Phase 1 = PR1–PR3 (check API → lsp → Zed). No Python. Ready to execute next session.
- **2026-07-20 (PR1 early return):** Parse/lower `return` / `return expr`; type-check vs enclosing ret; drop live heap locals before early return; C `return` / bare `return;`; conformance `early_return` (if/for/while/loop/match/method/Void). Decision D63.
- **2026-07-20 (PR3 Array&lt;User&gt;):** D65 `ThoxaArray` + `elem_size` + inline User structs; cgen field-RC callbacks (`ThoxaArrayElemFn`); get/set/append/dropLast/for-in; conformance `array_user`. Int64/String arrays unchanged.
- **2026-07-20 (PR2 User+heap RC):** MIR uses `needs_drop` for Trial Pass/Fail/`?`/match extract (User field-walk via cgen `emit_rc`); method prolog Dups heap fields; match binds get fresh places (no cross-match overwrite leak); conformance `user_rc`; ASan clean. Unblocks `Trial<Box(s:String),…>` / File-shaped APIs (D64).
- **2026-07-20 (PR4 match discard):** `Pass(_)` / `Fail(_)` discard binding (D66); `_` does not define a name; discarded heap payloads still RC-dropped; conformance `match_discard`.
- **2026-07-20 (PR8 Main.thx dir entry):** `thoxa run|build|emit-c <dir>` → `<dir>/Main.thx`; `~/` project root = that dir (D31/D68); harness discovers package dirs + sibling `<name>.out`; conformance `pkgmain` (Helper via `./` + `~/`).
- **2026-07-20 (PR7 bounded server):** `http_server` binds ephemeral port, N=3 sequential client reconnects (GET `/` 200, `/style.css` 200, `/nope.html` 404); accept/parseRequest/Router.handle/writeResponse/send/close per connection; Net+Http+Router; exits (D67). HTTP ladder complete for this plan.
- **2026-07-20 (PR6 Router type):** `type Router(root: String) { handle(req: Request) -> Response }`; early return; `ok`/`notFound` + Content-Type; free `handle(root,path)->Bytes` removed; conformance `router` asserts via `writeResponse` status lines (D70).
- **2026-07-20 (PR5 Request/Response):** `Header`/`Request`/`Response`; `parseRequest`/`writeResponse`/`ok`/`notFound`/`badRequest`; constructor-param `Array` fields (D69); `user.arrField[i]` index; conformance `http_parse`; `http`/`http_wire` use typed API + `writeResponse`.
- **2026-07-19 (R9+ file I/O + Router):** `iolib/Io.readFile` (open/lseek/read/close; Fail open|seek|read); cgen `fcntl.h`; String `==`/`!=`; static Router free `handle(root, path)` (later D70 type); for-in heap-elem RC fix; conformance `file_io`, `router`.
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
