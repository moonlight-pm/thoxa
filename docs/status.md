# Thoxa — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**Session/REPL + thin `std/` (D96–D100).** Optional follow-ons:

1. **Session polish** — Thoxa-side pipes/redirs (not only host); prelude out of compiler embed; drop host `status`/`cd` when pure step path is enough; optional faster REPL C (`tcc`).
2. Grow `std/` (more Str/Path/Io) and eventually `ffi`; deferred megawork (M4, Perceus, effects, typed-member nav, multi-diag, File type / method mut-field write-back, Net errno strings, …)

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
| Session | `~/.config/thoxa/shell.thx` rc + `fn prompt() -> String` (D96) | unit: `repl::tests` |
| Session | `fn path(parent) -> Array<String>` replaces PATH; empty `[]` ok (D100) | unit: `repl::tests` |
| Session | D93 v2: dlopen `thoxa_session_step` + preamble object cache; host-held status | unit: `repl::tests`; interactive smoke |
| Session | `prompt()` / `path()` via same step .so + stdout capture (no child exec) | unit: `repl::tests` |
| Session | Command flags (`ls -al`, `--all`) desugar, not Thoxa Sub (D92 polish) | unit: `command::` |
| std | Named root `std/` + Str/Path/Ansi/Env/Git/Prompt (D97) | `stdlib` |
| Lang | Param defaults M3 subset (D98); empty `[]` from `-> Array<T>` | `param_defaults` |
| std | Ansi 16 colors + `fgHex`/`parseRgba`; prompt signature hex (D99) | `stdlib` |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Session dlopen step ABI | session plan D93 | **Shipped** — `thoxa_session_step` + cached preamble `.o`; `prompt`/`path` same path; host `cd` / `\|` / `status` remain fast paths |
| Command skin vs Thoxa | D92 | Path-like atoms + **flag trees** (`ls -al`, `--all`) desugar as commands; pure literal arith (`1 - 2`) stays Thoxa |
| Postfix `having` / `else` for May | [`docs/ideas/2026-07-20-having-else-may.md`](ideas/2026-07-20-having-else-may.md) | **Sleeping** |
| Immutable values / structural sharing | [`docs/ideas/2026-07-21-immutable-values.md`](ideas/2026-07-21-immutable-values.md) | **Sleeping** |
| Webapp SSR / components | [`docs/ideas/2026-07-21-webapp-ssr-components.md`](ideas/2026-07-21-webapp-ssr-components.md) | **Sleeping** |

## Last session

- **2026-07-25 (session REPL speed + polish):**
  - **D93 v2:** in-process `dlopen` `thoxa_session_step(ThoxaShellState*)`; `Shell.step` seeds from receiver status; preamble `.o` cached while decls stable; runtime `-fPIC`; workspace root pinned across host `cd`.
  - **`prompt()` / `path()`:** same step `.so` path + stdout capture (no child exec); prompt lib kept mapped; path uses one-line `for` + `sessionEmptyStrings()`.
  - **D92 flags:** `ls -al` / `--all` no longer mis-kept as Thoxa Sub/Unary; pure `1 - 2` still Thoxa.
  - Parallel-safe `SessionBuildCache` ids; unit: `repl::`, `command::`, `session::`; conformance green.
- **2026-07-25 (D98–D100 + login shell):** Session readiness for default-shell experiments.
- **2026-07-25 (D96–D97 ship):** Session rc + custom prompt + product `std/`.

## Verify quickly

```bash
cargo make test
cargo test -p thoxa --lib repl::
cargo test -p thoxa --lib command::
# REPL (interactive loads ~/.config/thoxa/shell.thx; optional path/prompt)
printf 'echo hi\nls -al\nexit\n' | cargo run -p thoxa --
# Login-like env (no parent cargo/dev PATH)
cargo make shell --login
# std + defaults
cargo run -p thoxa -- tests/conformance/stdlib.thx
cargo run -p thoxa -- tests/conformance/param_defaults.thx
```
