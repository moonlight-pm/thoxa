# Thoxa — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**Session/REPL shipped (PR0–PR7 + polish).** Optional follow-ons:

1. **Session** — in-process dlopen `step` ABI (D93 v2); Thoxa-side pipes/redirs (not only host); prelude out of compiler embed; thread `Shell.status` into chunks (host `status` is a stopgap).
2. Deferred megawork (M4, Perceus, effects, full `std/`/`ffi`, typed-member nav, multi-diag, File type / method mut-field write-back, Net errno strings, …)

HTTP + editor + Io complete. Plan: [`docs/specs/2026-07-24-thoxa-session-repl.md`](specs/2026-07-24-thoxa-session-repl.md).

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
| R9+ | Io OpenMode constants + errno/strerror Fail (D80–D82) | `file_io` (updated) |
| R9+ | String `==`/`!=`; static file Router (free `handle`) | `router` (+ `httplib/Router`) |
| R9+ | Early `return` / `return expr` (D63) | `early_return` |
| R9+ | User+heap field RC (Trial/match/`?`/method) | `user_rc` |
| R9+ | Dynamic `Array<User>` (inline elems, D65) | `array_user` |
| R9+ | Match discard `Pass(_)`/`Fail(_)` (D66) | `match_discard` |
| R9+ | Request/Response/Header + parseRequest/writeResponse (D69) | `http_parse` (+ `http`, `http_wire`) |
| R9+ | Router type `handle(Request) -> Response` (D70) | `router` (via `writeResponse`) |
| R9+ | Bounded multi-request static HTTP server (D67) | `http_server` |
| R9+ | Directory entry `Main.thx` (D68) | `pkgmain` |
| R9+ | `May<T>` / `Has` / `Not`; `Bytes.find` → `May<Int64>` (D85) | `mays`, `bytes` (+ Http) |
| Session | no-main scripts + command sugar + bare `thoxa` REPL (D88–D95) | `session_script`, `session_step`, `session_cmd` |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Session dlopen step ABI | session plan D93 | **Partial** — host compile-per-chunk + `ThoxaShellState`/`thoxa_pipe` placeholders |
| Postfix `having` / `else` for May | [`docs/ideas/2026-07-20-having-else-may.md`](ideas/2026-07-20-having-else-may.md) | **Sleeping** |
| Immutable values / structural sharing | [`docs/ideas/2026-07-21-immutable-values.md`](ideas/2026-07-21-immutable-values.md) | **Sleeping** |
| Webapp SSR / components | [`docs/ideas/2026-07-21-webapp-ssr-components.md`](ideas/2026-07-21-webapp-ssr-components.md) | **Sleeping** |

## Last session

- **2026-07-24 (session PR0–PR7):** Full session ladder. PR0 Shell dogfood; PR1 session scripts (`SessionStmt` → `Shell.step`); PR2 command sugar + `session_prelude.thx`; PR3 RT `ThoxaShellState` + `thoxa_pipe`; PR4 bare `thoxa` REPL; PR5 decl accumulation + multiline; PR6 host `\|` pipelines; PR7 decisions D88–D95 + docs. `cargo make shell` → bare `thoxa`.
- **2026-07-24 (session plan):** Approach 3 locked in specs.
- **2026-07-21 (D87 value match + thoxash v0–v3):** prior shell dogfood.

## Verify quickly

```bash
cargo make test
# REPL
printf 'echo hi\nexit\n' | cargo run -p thoxa --
# session script
cargo run -p thoxa -- tests/conformance/session_cmd.thx
```
