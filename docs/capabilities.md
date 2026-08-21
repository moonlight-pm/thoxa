# Capabilities — as-built progress

**Purpose:** Single engineering view of **what exists** vs **what remains** for
each product capability. Target design lives in freezes under
[`specs/`](specs/). Session priority lives in root
[`CURRENT.md`](../CURRENT.md). User product docs live in
[`handbook/`](handbook/) (shipped only).

**Update this file in the same change** whenever capability status or gaps
change. See [`progress-model.md`](progress-model.md).

**Status vocabulary:** `shipped` · `partial` · `spec’d` · `planned` · `idea`

**As of:** 2026-08-21 (`containers/landing` thoxa.dev live;
`containers/sola` sola.computer teaser live; static JPEG/font MIME;
**D146** SSE `peerFd` / `streamed()`;
Crema **E4–E5**; **D145** `htmlFragment` / `cached`;
Crema **E2** fragment GETs;
**built-in validation** on master, not product;
`Type.read`;
**D144** `sql"…"`;
**D143** `std/sqlite`;
**D142** `Cookie` / `withCookie` / `clearCookie`;
**D141** `std/json` `decode`/`encode`;
**D140** `Float64`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case module files + `main.thx` entry **shipped**;
**D137** Str parse + Http wrappers;
**D136** `Request.query` + `std/http/server`; **D135** Node + nested User ctor fields;
JSX `Node` match + `Attr` + `{...}` spreads + `{for}` lists **on master**; D133/D134 Map;
D132 fn pointers; Crema-lite **A–D2 complete** (`POST /api/orders` +
`/tmp/crema.db`) + `Roast` / `DrinkCard`; Phase **E** through **E5** (E6 parked);
type-body
route clauses + JSX/`Node` + **`-->` nowait** **partial** on master — not
freeze)

---

## Language kernel

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| driver-cgen | Compile-to-C driver, hello, C emit | shipped | `hello` | kernel | `run`/`build` reuse `target/thoxa/bin/*` when package / `std/` / runtime / compiler are unchanged (`THOXA_FORCE_BUILD=1` to skip). No incremental `cc`. | yes |
| lets-arith-if | `let`, arith, `if` | shipped | `arithmetic`, `lets_if` | R1 | Integer arith. Float → **floats** / **D140**. | yes |
| floats | `Float64` type + literals + arith | shipped | `floats` | **D140** | No `Float32`. No `%`. No float match arms / literal types. No float Map keys. Int→`Float64` coerce only. Interp/JSX stringify shortest round-trip. | yes |
| multi-clause | Same-name multi-clause fn/method = match sugar on formal types | shipped | `multi_clause` | **D131**; [`specs/2026-08-11-multi-clause-functions.md`](specs/2026-08-11-multi-clause-functions.md) | Pattern formals / cross-arity / composition / `extern` multi-clause out of v0. D16 std factories still separate. Bare singleton lit types + inline `\|` in formals shipped as typespace substrate. | yes |
| fn-pointers | Function types `(a: T) -> R`; free + unbound + bound methods | shipped | `fn_pointers` | **D132**; [`specs/2026-08-13-function-pointers.md`](specs/2026-08-13-function-pointers.md) | Bound = receiver+code (not lexical closure). Module `let`/`mut` allowed; fields/arrays **no** (D48 / D135). No `Fn` word; no closures; no multi-clause/`extern` values; no `-->` of a pointer. | yes |
| trial | `Trial` + `match` + `?` via `std/fallible` | shipped | `trials`, `trials_mono`, `trials_nested`, `trials_nested_trial`, `http_*` | D40–D45, D113–D114, D118, D123 | none for v0 — import `Pass`/`Fail`/`Trial` from `std/fallible`; cross-module residual rewrap | yes |
| may | `May` / `Has` / `Not` + `?` via `std/fallible` | shipped | `mays`, `trials_nested`, `bytes` (find) | D85, D113–D114, D118, D123 | none for v0 — import `Has`/`Not`/`May` from `std/fallible`; `Bytes.find` needs `May` in scope | yes |
| sum-types | User unions, literals, tagged layout, type `!`/`?` marks | shipped | `sums_*`, `sums_match_guard`, `sums_generic*`, `json` | D118–D120, **D141**; [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md) | Union-member product fields include **Float64**, **Array**, **Map**, User (**D141**). Array/Map of sums RC the live arm. | yes |
| value-match | Literals + or + bind/`else` + guards + `=>` | shipped | `match_value`, `match_or`, `match_bind`, `match_guard`, `match_arm_sugar` | D87, D108, D110, D111, **D130** | — | yes |
| match-discard | `Pass(_)` / `Fail(_)` + bare tag-only | shipped | `match_discard`, `match_arm_sugar` | D66, **D130** | — | yes |
| user-types | User `type` + methods (scalars) | shipped | `types` | R3 | — | yes |
| string | String, interpolation, `.count`, simple RC | shipped | `strings`, `strings_multiline`, `sqlite` | R4, **D144** | `sql"…"` tagged interpolator (sql only). Newlines kept in `"…"` / `sql"…"` (no dedent). Perceus later; `\xHH` string escapes | yes |
| string-eq | String `==` / `!=` | shipped | `router` (uses) | D60 | — | yes |
| bytes | Bytes API + slice + find→May + insertAt/removeAt + equals | shipped | `bytes`, `splice` | D14–D18, D36–D37, D46′/D85, D55 | String→Bytes is `fromString` only; **no** `"…" as Bytes` (D15 rejected) | yes |
| buffer | Growable Buffer COW → freeze | shipped | `buffer` | D50–D52 | — | yes |
| mut-assign | `mut` + assign + method field write-back | shipped | `bytes`, `method_field_writeback` | R5, D107 | — | yes |
| control-flow | while / loop / for / break / continue | shipped | `control_flow`, `loops`, `loop_diverge` | D47, D55, **D128** | `loop` without `break` diverges (Never); `while`/`for` still Void | yes |
| early-return | `return` / `return expr` | shipped | `early_return` | D63 | — | yes |
| start-tasks | `--> name(args)` — begin a call; do not wait | partial | `start_tasks`, `start_nested`, `start_net`, `start_keepalive`, `http_host` | on master, not freeze | No Future / `async` / `run`. Token `-->` only (not a word). Free Thoxa fns only; `--> { }` later. Net parks. `std/http/server` `--> serveOne` + keep-alive (**D136**). No handbook / D-id | no |
| return-infer | Unannotated free-fn return from body exits | shipped | `return_infer` | **D129**; [`specs/2026-08-10-return-type-inference.md`](specs/2026-08-10-return-type-inference.md) | Unannotated **methods** still default `Void` (not body-inferred yet); recursive/mutual unannotated free fns → annotate; bare Pass/Fail alone → annotate; LSP hover shows free-fn inferred types after lower | yes |
| array-dyn | Dynamic `Array<T>` (incl. User) + insertAt/removeAt | shipped | `arrays`, `array_user`, `array_heap_elems`, `splice` | D53–D55, D62, D65; **D133** | Write-back `Void` on `mut` places (Swift/Buffer COW). `let` aliases do not see writes. | yes |
| map | Builtin `Map<K, V>` + `{k:v}` + `for k, v` | shipped | `maps`, `maps_user`, `json` | **D134**; D133; **D141**; [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md) | Keys v0 String/Int64/Bool (User types are **values**, not keys). Values include **Float64 (D140)** and User sums (`Json`, **D141**). `.has` / `.get`→May / write-back `.set`; ident keys; insertion order. `Map<K,V>` seeds `May<V>` so `.get` works without a nested annotation. Crema dogfoods query/form/cookies as `Map<String, String>`. **Gaps:** User keys wait on structural product `==` + private field hash (not Eq/Hash traits; not pointer identity). No `==` on Map. No `std/http` header flip. | yes |
| array-fixed | Fixed `Array<T,N>` fields **and locals** | shipped | `cstruct_ffi`, `fixed_array_local`, `fixed_array_assign` | D27, D117 | ABI scalar elems only (no User fixed-array elems) | yes |
| user-rc | User+heap field RC in Trial/match/method | shipped | `user_rc` | D64 | — | yes |
| string-fields | String/Bytes/Array/Map/Node/User/Float64 ctor fields | shipped | `string_fields`, `maps_user`, `nested_fields`, `floats` | D48–D49, D69, D134, **D135**, **D140** | Nested User and Node shipped (`nested_fields`), including sums as fields. Recursive User fields are an error (use Array/Map). C-layout and fn types still out. | yes |
| param-defaults | Formal defaults M3 (+ methods/ctors/`!`/extern) | shipped | `param_defaults`, `defaults_mod`, `param_bang_defaults` | D98, D106, D116 | `!` and extern formals may default (call-site fill); non-default cannot follow defaulted; free names use defining module | yes |
| modules | import/export, `./` `~/`, module let/mut | shipped | `modules`, `namespaces`, `modstate`, `import_combined` | D30–D35, D122, **D127**, **D138** | Combined `import ns, { a } from "…"` shipped. Library files are kebab-case modules, not types (**D138**); ids path-qualified. `~/` is nearest ancestor `main.thx` (else entry dir) so nested-file check/LSP match directory packages. Nested import/type in functions deferred (D91); module mut heap (Array/String/Map) must not end-of-fn drop (D122) | yes |
| pkg-main | Dir entry `main.thx` | shipped | `pkgmain` | D68, **D138** | Filename is kebab-case `main.thx` (D138) | yes |
| route-clauses | Type-body `GET "/…"` → `route(req) -> Response` | partial | unit (`route_desugar`, LSP); Crema dogfood | idea [`ideas/2026-08-12-router-clause-syntax-experiment.md`](ideas/2026-08-12-router-clause-syntax-experiment.md) | **On master, not freeze / no D-id.** String wrap + `Response` pass-through; `Int64`/`String` captures; multi-segment; optional `req`. **Gaps:** no conformance `.thx`; no handbook; no JSON/form; miss is `notFound()` (no fallthrough). Crema dogfoods via `examples/http` `type Routes`. | no |
| jsx-node | Tag literals + builtin `Node` / `.html()` | partial | `jsx` | idea [`ideas/2026-08-13-jsx-node-syntax-experiment.md`](ideas/2026-08-13-jsx-node-syntax-experiment.md) | **On master, not freeze / no D-id.** Eager PascalCase → `Type(…).view(children:)`; `{e}` children are String / Int64 / Float64 / Bool / Node / Array&lt;Node&gt;; `{for x in xs { child }}` collects Array&lt;Node&gt;; `{e}` attrs stringify String / Int64 / Float64 / Bool; void tags; bare boolean attrs render as `disabled`; empty `<>` is Node; no import. `<tag` (no space) is JSX, including after another expr; `a < b` (spaces) is comparison; glued `a<b` is an error. `Node` matches as `Element \| Text \| Fragment` (same `ThoxaHtml*` tree). Ambient `Attr` / `Element` / `Text` / `Fragment`. HTML `{...Array<Attr>}` and component `{...product}` spreads. Crema pages dogfood (`examples/http` `Page`). Bare `#` is a line comment; **`\#` in JSX text is a literal `#`**. **Gaps:** no handbook; `.html()` method hover thin. | no |
| type-read | `Type.read` / `where` / `.encode` | partial | `json_read`, `where_refine`, `sums_literal` | idea [`ideas/2026-08-17-builtin-validation.md`](ideas/2026-08-17-builtin-validation.md) | **On master, not freeze / no D-id.** `type Qty = Int64 where >= 1`; `&` / `\|` / parens; `Qty.from`; literal inhabitation; Qty→base coerce; `{value}`/`{expected}`/`{path}` templates; field-level `where` on `read`; `.toJson` / `.encode`; `Issue = Missing \| Mismatch \| Rejected`; `May<T>` fields (missing/null → `Not`); nested `read` after a failed `read` is RC-clean (Map.get of User is a slot borrow + retain). Closed slugs are literal unions (`type DrinkSlug = "espresso" \| "mocha"`) — a string-literal union is a subtype of `String` (same for `0\|1` <: Int64, `true\|false` <: Bool); `T.from` / `T.read` on the union. **Gaps:** no handbook; no collect-all; no `May<Int64 where …>` (write `May<Qty>`). | no |

## FFI and C layout

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| extern-c | `extern fn … from "c"` [+ symbol] | shipped | `extern_libc` | D19–D22, D26–D28, D82, **D140** | String returns via `thoxa_string_from_cstr`. `Float64` marshals as `double`. | yes |
| c-layout | Body fields, `T()`, Address, fixed Array, bitwise | shipped | `cstruct_ffi` | D23–D29, **D140** | ABI scalars include `Float64`. | yes |
| ffi-root | Named `import … from "ffi"` | planned | — | D30 deferred | No product `ffi/` root | no |

## I/O and HTTP

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| file-io | `readFile` / `writeFile` / `appendFile` + OpenMode + **File** (seek/position) | shipped | `file_io`, `file_type`, `file_write`, `file_seek`, `file_openmode` | D56, D80–D82, D109, D112, D115, **D126** | Product **`std/io`**; OpenMode + `openModeBits`; Int32 creat/trunc/append flags; Fail is String | yes |
| net-http | Net sockets + parse/write Request/Response | shipped | `http`, `http_wire`, `http_parse`, `http_net_err`, `http_helpers` | D69, R9, **D126**, **D136**, **D137**, **D142**, **D145**, **D146** | Product **`std/net`** + **`std/http`**; `receiveRequest` / `sendAll`; `Request.query`; header/query/form/cookie helpers + `queryParam`/`formField`/`cookie`/`htmlResponse`; `Cookie` / `withCookie` / `clearCookie`; `htmlFragment` / `etagOf` / `cached` (**D145**); `peerFd` / `streamed()` (**D146**); 303/403/304/405; Fail is String | yes |
| router | Router type `handle(Request) -> Response` | shipped | `router` | D70, **D126**, **D139** | Product **`std/http/static`** (`Router`); GET+HEAD; index.html; 403/404/405 | yes |
| http-server | Bounded multi-request static server | shipped | `http_server` | D67 | Uses `std/net` + `std/http` + `std/http/static` | yes |
| examples-http | Runnable HTTP example (Crema-lite experiment) | partial | manual (`cargo make run examples/http`) | examples | Port 9091; **`std/http/server.serve(routes.port, routes.handle)`** (**D132**/**D136**/**D139**); type-body `GET`/`POST` on `Routes` + free `handle` static fallback; **Phase A–D2 complete:** pages (JSX `Node`), forms/cookies, `?q=` search, **D1** `POST /api/orders` (`CreateOrder.read`, `qty: Qty`, `slug: DrinkSlug`) + **D2** `/tmp/crema.db` (`app/store.thx`). JSON API is the codec demo; **E1** HTML `POST /orders` + `GET /orders/:id` ticket (PRG). **E2** `GET /menu/items?q=` + `GET /orders/:id/ticket` fragments (`htmlFragment` + content-hash ETag / 304). **E3** `web/crema.js`; **E4** in-memory session + flash + CSRF; **E5** `/live` + `/events` SSE (**D146**). **Gaps:** no WebSocket (parked); session is process-local Map (not sqlite); drink-page slug is still `String` (JSON / HTML-order path uses `DrinkSlug`); no product migrate. Phase **E** through **E5** (E6 parked): [`ideas/2026-08-18-html-over-the-wire.md`](ideas/2026-08-18-html-over-the-wire.md). Charter: [`ideas/2026-08-10-examples-http-crema-experiment.md`](ideas/2026-08-10-examples-http-crema-experiment.md) | limited |
| examples-kilo | Terminal text editor (kilo port) | shipped | manual (real TTY); user-tested | examples | **Ctrl-Q** quit (3× if dirty). Needs TTY. Termios via `Bytes` buffers. Highlight recomputed (no Array of User-with-Array fields). Empty-buffer edit needs D122. | yes |
| main-args | `fn main(arguments: Array<String>)` + CLI forward | shipped | unit / examples | D121 | `thoxa run path.thx arg…` forwards args; argv[0] is binary path | yes |

## Session / shell

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| session-script | No-main scripts → `Shell.step` + synthetic main | shipped | `session_script`, `session_step` | D88–D90, D94 | — | yes |
| command-sugar | Command skin + flag trees (`ls -al`) | shipped | `session_cmd`; `command::` | D92 | Shell control-flow sugar rejected | yes |
| session-repl | Bare `thoxa` REPL, decl accumulate, multiline | shipped | `repl::` | D88, D93, D95 | Crude line editor only | yes |
| session-dlopen | Legacy step `.so` + `cc -shared` | removed | — | D93 v2; D105 | Superseded by libtcc-only steps (escape hatch deleted) | no |
| session-rc | `~/.config/thoxa/shell.thx` + `prompt()` | shipped | `repl::` | D96 | Piped REPL does not load rc | yes |
| session-path | `fn path(parent) -> Array<String>` | shipped | `repl::` | D100 | — | yes |
| session-pipeline | Thoxa-side `\|` → `Shell.pipeline` + RT | shipped | `session_pipe`; `command::` | D101 | Nested Array stages N/A (Bytes blob) | yes |
| session-redir | File `>` / `>>` / `<` → `Shell.runIo` | shipped | `session_redir`; `command::` | D102 | No `2>`; redir+pipe combo; ops need whitespace | yes |
| session-host-pipe | Host OS `\|` fast path in REPL | shipped (removed) | `repl::pure_step_*` | D103 | Replaced by Thoxa-side pipeline (D101) | yes |
| session-prelude | Shell API in product `std/shell.thx` | shipped | `session::`; session scripts | D104, D123 | Still auto-injected (not `import` by default); compile-time embed fallback; inject loads `std/fallible` for Shell | yes |
| session-host-builtins | Drop host `cd`/`status`/`\|` for pure step | shipped | `repl::pure_step_*` | D103 | All session commands via step image (libtcc) | yes |
| session-libtcc | In-process libtcc for REPL steps (no spawn / no step `.so`) | shipped | `tcc::`; `repl::pure_step_*` | D105; [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md) | Linux x86_64 only; later polish (base cache / license notice) not CURRENT | no |
| extras-shell | Pure-Thoxa dogfood Shell | shipped | `cargo make run extras/shell` | D95 | **Not** product face | no |

## Standard library

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| std-root | Named `std/…` resolution | shipped | `stdlib` | D97 | — | yes |
| std-fallible | `May`/`Trial`/`Has`/`Not`/`Pass`/`Fail` in `std/fallible` | shipped | `mays`, `trials*`, `bytes`, `stdlib` | D118, D123 | none for v0 — **import required** for programs (not ambient); session Shell inject loads Fallible | yes |
| std-str | Str helpers | partial | `stdlib` | D97, **D137** | +`parseUInt` / `splitOnce` (`Split`); +remove/truncate/reverse/isAscii/ignore-case contains; still sample-driven thin | yes |
| std-path | Path helpers | partial | `stdlib` | D97 | +`joinAll`/`stripTrailingSlash`/`ensureTrailingSlash`; sample-driven | yes |
| std-ansi | 16 colors + hex SGR | shipped | `stdlib` | D99 | — | yes |
| std-env | getenv / host identity helpers | partial | `stdlib` | D97 | +`pathVar`/`pwd`/`shell`; sample-driven | yes |
| std-git | branch helpers | partial | `stdlib` | D97 | +`headLine`/`isDetached`/`repoRoot`; sample-driven | yes |
| std-prompt | `defaultPrompt` | shipped | `stdlib` | D97, D99 | — | yes |
| std-logging | Leveled `std/logging` + no ambient `println` | shipped | `logging_levels`, `logging_sink`, full suite | **D125**; [`specs/2026-08-10-std-log-and-println-removal.md`](specs/2026-08-10-std-log-and-println-removal.md) | App face: `import logging, { info, … }` (**D127**). Multi named loggers / `initLog` later; structured/JSON; colors/TTY; source location | yes |
| std-io | File I/O in `std/io` | shipped | `file_io`, `file_type`, `file_write`, `file_seek`, `file_openmode`, `logging_sink` | **D126** | Linux `extern` open/read/write/lseek; Fail String; no Windows | yes |
| std-net | TCP sockets in `std/net` | shipped | `http`, `http_wire`, `http_net_err` | **D126** | IPv4 stream; `receiveRequest` / `sendAll`; Fail String; Linux sockaddr pack | yes |
| std-http | HTTP/1.1 protocol in `std/http` | shipped | `http_parse`, `http`, `http_wire`, `http_helpers` | **D126**, **D136**, **D137**, **D142**, **D145**, **D146** | `Request.query` kept; body empty at parse (host fills). Helpers: `headerGet`, query/form/cookie Maps + `queryParam`/`formField`/`cookie`, `htmlResponse` / `htmlResponseStatus` / `htmlFragment` (`Node`), `etagOf` / `cached` / `notModified`, `Cookie` / `withCookie` / `clearCookie`, `seeOther`, `finalize` / keep-alive, `streamed()` + `peerFd` (**D146**). Still no syscalls. No `Expires` / cookie-octet escape / weak ETags / `Last-Modified` / WebSockets | yes |
| std-http-static | Static-file `Router` in `std/http/static` | shipped | `router`, `http_server` | **D126**, **D139** | GET+HEAD; MIME `.html` / `.css` / `.js` / `.txt` plus jpeg/png/gif/svg/webp/ico/woff/woff2 (`router` + `containers/landing`); else octet-stream. Depends on Io+Http+Logging | yes |
| std-http-server | HTTP/1.1 host loop in `std/http/server` | shipped | `http_host` | **D136**, **D139**, **D146** | `serve(port, handle)` + `serveOne` keep-alive; `readBody` (64 KiB); `bindListen`; `beginEventStream` / `sendEvent`; status 0 skips write. Depends on Net+Http+Logging. Infinite `serve` is product face (gate uses `serveOne`) | yes |
| std-json | Dynamic `Json` tree in `std/json` | shipped | `json`, `json_read` | **D141**; idea [`ideas/2026-08-15-json-codec.md`](ideas/2026-08-15-json-codec.md) | `decode` / `encode`. `JInt` vs `JFloat` as charter. Fail String. This branch also: `Issue`, `readJson`, `asObj` / `*Field`, `errorObject`. No derive, no pretty, no comments / trailing commas / NaN tokens, no `jsonResponse` (Crema keeps that in `examples/http`) | yes |
| std-sqlite | Embedded SQLite in `std/sqlite` | shipped | `sqlite` | **D143**, **D144** | Amalgamation in `vendor/sqlite` → `libthoxa_rt`. `open` / `exec` / `query` / `close`. Write `sql"… {value} …"` (tagged interpolator; `exec`/`query` take `Sql`). Rows `Row` + `SqlValue`. Crema dogfoods file-backed open at `/tmp/crema.db` (example-only). **Gaps:** no blobs, no prepare/bind API, no explicit transactions, no product migrate, no general tagged templates | yes |

## Editor / LSP

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| check-source | Typecheck API + overlays | shipped | unit / LSP | D71–D79 | Soft-recovered entry AST + `load_package_with_entry_ast` so syntax errors do not hide later undefined-name diags | yes |
| lsp-diag | `thoxa lsp` diagnostics | shipped | manual + unit | phase 1; **D124** | Multi-diag + structured codes/help/`relatedInformation` from shared IR; residual mid-expression recovery still limited | yes |
| diag-structured | Structured diagnostic IR + pretty/`--json`/rich LSP | shipped | unit | **D124**; freeze `2026-08-09-structured-diagnostics` | Shared IR (`diagnostic.rs`); pretty `thoxa check`; `check --json` + version stamp; LSP `code` + help-in-message + `relatedInformation`. Codes THX001–THX010; **warnings** (unused import THX010) exit 0 / `ok: true`. Residual free-form sites still get codes when message patterns match. No `explain`/code actions yet | yes |
| lint-unused-import | Unused selective/namespace import warning | shipped | unit | THX010 | Open-buffer only (not whole package). Combined imports lint each binding. `May`/`Trial` stay live when `Has`/`Not` or `Pass`/`Fail` are referenced. No unused-local lint yet | yes |
| cli-version | `thoxa version` / LSP identity stamp | shipped | unit / CLI | build stamp | Git short hash (+`.dirty`) + binary path; LSP stderr + `window/logMessage` + `serverInfo.version` (no toast) | yes |
| lsp-nav | hover / def / refs / outline / rename | partial | unit | D77–D79 | Typed-member stack + package-tree refs/rename; **`?` unwraps Trial/May**; **call-arg hover**; hover shows **D129** inferred/`->` returns; **`let`/`mut` hover prefers nav `ty_hint`** (`let a: Response`). Residual rare untyped exprs | partial |
| lsp-workspace | `workspace/symbol` package search | shipped | unit | editor plan | Open buffers + package-tree disk (main.thx/`.git` root); query substring; cap 100 | yes |
| lsp-xmodule | Cross-module jump (`./` `~/` / `std/`) | partial | unit | D78 | `std/` go-to-def covered; open buffer wins over disk; **`~/` from nested files uses nearest `main.thx`**; **typed members after namespace + `?`** (e.g. `net.createSocket()?; server.reuseAddr`) | partial |
| tree-sitter-zed | Grammar + Zed extension extras | shipped | `cargo make zed` | editor plan, **D75** | Thin lexer grammar. JSX island; glued `sql"…"` island with SQL injection (`injections.scm`). Not a full CST / IDE | yes |
| mir-on-resolver | MIR lower on shared resolver | planned | — | editor plan | **Deferred architecture** (PR5 resolver ships for LSP; MIR still own env — not polish residual) | no |

## Runtime / heap

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| simple-rc | Single-threaded RC String/Bytes/Array/Map | partial | strings/bytes/arrays/maps | kernel D4-ish; **D122** | Module mut heap not end-of-fn dropped (D122). Perceus precision later | partial |
| spawn-wait | `thoxa_spawn_wait` for session | shipped | session / extras | D86 | Not a general language spawn surface | no |
| perceus | Precise RC / Perceus | idea | — | kernel | Deferred megawork | no |
| effects | Algebraic effects | idea | — | kernel | Out until forced | no |

## Meta / docs

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| progress-docs | CURRENT + capabilities + handbook model | shipped | this tree | progress-model | Generators / CI truth deferred | n/a |
| handbook | User manual shipped-only | partial | handbook pages | progress-model | Grow with surfaces; keep honest | yes |

---

## Historical ladder (R0–R9+)

The example ladder through bounded HTTP is **complete**. Track day-to-day work
in the matrices above, not by re-checking R0–R9.

| Rung | Summary | Conformance |
|------|---------|-------------|
| R0 | hello / driver / C emit | `hello` |
| R1 | let, arith, if | `arithmetic`, `lets_if` |
| R2 | Trial, match, `?` | `trials` |
| R3 | user `type` + methods | `types` |
| R4 | String, interpolation, RC | `strings` |
| R5 | mut + Bytes | `bytes` |
| R6 | extern from `"c"` | `extern_libc` |
| R7 | C-layout / Address / fixed Array | `cstruct_ffi` |
| R8 | modules | `modules`, `namespaces`, `modstate` |
| R9 | HTTP subset | `http` |
| R9+ | monomorphs, control flow, Buffer, Array, Io, Router, server, May, main.thx, … | see matrix |

Kernel freeze ladder table:
[`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md) §6.
