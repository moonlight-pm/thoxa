# Thoxa — Decision Log

Living log of load-bearing choices. Newest first within each section when updated.

**Doc layout** (full model: [`progress-model.md`](progress-model.md); map: [`README.md`](README.md)):

| Path | Role |
|------|------|
| `AGENTS.md` | Session bootstrap — keep short |
| `CURRENT.md` | **Only** living focus handoff (priority + locks) |
| `docs/capabilities.md` | As-built capability maturity + gaps |
| `docs/decisions.md` | Locked choices + deferred analyses (this file) |
| `docs/specs/` | Target freezes (desired shape; not changelogs) |
| `docs/handbook/` | User manual — **shipped** behavior only |
| `docs/ideas/` | Parking lot — **not** decisions |
| `.grok/config.toml` | Project permission allows |

No knowledge graph (`docs/graph/`, node databases, etc.) until cross-links and “where did we say X?” pain are real. Prefer prose + IDs here over premature graph tooling. Handoff is file-based (`AGENTS.md` + `CURRENT.md` + `capabilities.md`), not a separate memory server.

---

## 2026-08-23 — Refinements + `Type.read`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D149 | Validation is types as inhabitation | **`where`** refinements are real types (`Qty` is not `Int64`). Literal inhabitation at compile time; `T.from` / `T.read` at runtime → `Trial<T, Issue>`. `Issue` in **`std/json`**. Tree `decode`/`encode` stay (**D141**). Closed slugs are literal unions <: String. Qty→base implicit; reverse is `from`. Compiler-attached methods (no traits). Fail-fast. | **Shipped.** Freeze [`specs/2026-08-23-type-read.md`](specs/2026-08-23-type-read.md). Gates: `json_read`, `where_refine`, `sums_literal`. Amends D141 (`Issue` + typed `read`). Punted: collect-all, `May<Int64 where …>`, cross-field `where`. Dogfood: Crema `CreateOrder`. |

## 2026-08-22 — `-->` nowait

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D148 | Product nowait is token `--> name(args)` | Begin a **named free Thoxa function**; do not wait. Expression type **`Void`**; callee return dropped. Token only (not a word). One OS thread; stackful fibers; Net parks; `main` / session step **drain until idle**. No Future / `async` / `run`. No `--> { }`. No `-->` of a pointer, method, extern, or type. | **Shipped.** Freeze [`specs/2026-08-22-start-tasks.md`](specs/2026-08-22-start-tasks.md). Gates: `start_tasks`, `start_nested`, `start_net`, `start_keepalive`, `http_host`. Amends D132 (named-fn `-->` is product; pointer `-->` still out). Dogfood: `std/http/server` `--> serveOne`. |

## 2026-08-22 — Builtin Node + tag literals

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D147 | Product HTML trees are builtin `Node` + tag literals | Ambient **`Node`** (`Element \| Text \| Fragment`, same `ThoxaHtml*` tree). Tags `<div>` / `<>` (no space after `<`); comparison is `a < b`. PascalCase is eager `Type(…).view(children:)`. `{e}` children: String / Int64 / Float64 / Bool / Node / `Array<Node>`; `{for x in xs { child }}` collects `Array<Node>`. Attrs stringify the same scalars; bare `disabled` is boolean. HTML `{...Array<Attr>}` appends; component `{...product}` copies ctor fields. `.html() -> String` (escaped; void tags `br hr img input meta link`). No import. Keep `.html()` — do not fold into generic `toString`. | **Shipped.** Freeze [`specs/2026-08-22-jsx-node.md`](specs/2026-08-22-jsx-node.md). Gate: `jsx`. Amends D140 (JSX stringify includes Float64). Field storage stays **D135**. `htmlResponse` / `htmlFragment` stay **D137** / **D145**. Punted: interpolation of `Node`, client VDOM, Tree-sitter `<>` polish, dedicated `.html()` hover. |

## 2026-08-19 — SSE / socket takeover

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D146 | Handler can own the peer for SSE | **`Request.peerFd`** (Int64, default 0) is host-filled by `std/http/server`. **`streamed()`** is status `0`; `serveOne` skips `writeResponse`, drains until close, then closes. **`beginEventStream` / `sendEvent`** write `text/event-stream`. Not chunked encoding, not WebSockets, not `fn` fields on Response (D48). | **Shipped.** Sample-driven (Crema Phase **E5**). Gate: `http_helpers` (`streamed=0`); live path is example-only (`examples/http` `/events`). |

## 2026-08-19 — HTML fragments + cache validators

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D145 | Fragment HTML + content-hash ETag | **`std/http`:** `htmlFragment(root)` is `root.html()` as `text/html` (no doctype). `etagOf(body)` is a strong quoted 8-digit hex FNV-1a-32. `ifNoneMatch` / `etagFresh` (`*` or listed token). `notModified` is 304 empty. `withCache` appends `ETag` + `Cache-Control`. `cached(req, res, cacheControl)` 304s when `If-None-Match` matches. Not weak ETags, not `Last-Modified`, not `Vary`. | **Shipped.** Sample-driven (D97/D136): Crema Phase **E2** fragment GETs. Gate: `http_helpers`. Cache *policy* (`private, no-cache` vs `max-age`) stays at the call site. |

## 2026-08-18 — `sql"…"` tagged interpolator

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D144 | `sql"…"` is a tagged interpolator | Ident **immediately** followed by a string (`sql"…"`; no space, like JSX `<tag`) is a tag. **v0 tag is `sql` only**; other tags error. Holes: String → quoted literal; Int64/Float64 → digits; Bool → `0`/`1`; `Sql` fragment → spliced raw (`ident` / `null` / `lit`). Result type is **`Sql`**. `Db.exec` / `Db.query` take `Sql`, not String. Not general JS tagged templates (no rest-args / `tag(parts, …values)`). | **Shipped.** Amends **D143** call surface. Gate: `sqlite`. `sql "…"` (space) is not a tag. Nested quotes inside `{…}` still can't live in the same lexer string (bind a local). |

## 2026-08-18 — Embedded SQLite

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D143 | Product SQLite is amalgamation + interpolated literals | **Amalgamation** in `vendor/sqlite` (3.53.4) compiled into `libthoxa_rt.a` (`SQLITE_THREADSAFE=0`, no load_extension). Thin RT `thoxa_sql_*` (Int64 handles; no stored `Address`). **`std/sqlite`:** `open` / `Db.exec` / `Db.query` / `close`; rows are `Row(cells: Map<String, SqlValue>)` with `SqlNull \| SqlInt \| SqlFloat \| SqlText`. **Injection (amended D144):** write `sql"… {value} …"`; holes are embedded as SQL. `lit` / `ident` remain as fragment helpers. Not `?` + a bind list. No named `ffi` root. | **Shipped.** Sample-driven (D97). Gate: `sqlite`. **Call surface → D144.** No blobs / prepared binds / transactions API. **No product `thoxa migrate`** — apps own schema. Crema persist is example-only (`examples/http` `/tmp/crema.db`). |

## 2026-08-18 — `Set-Cookie` builder

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D142 | Product `Set-Cookie` is a `Cookie` value | **`std/Http`:** `SameSite = "Strict" \| "Lax" \| "None"`; `Cookie(name, value, path="/", domain="", maxAge=Not(), httpOnly=false, secure=false, sameSite="Lax")` with `.header()`; `withCookie(res, cookie)` appends `Set-Cookie`; `clearCookie(name, path="/", domain="")` is `Max-Age=0` + empty value. No `Expires`, no cookie-octet escaping. Read path stays `cookie(req, name)` / `parseCookies`. | **Shipped.** Sample-driven (D97/D136): Crema remember/forget were literal `Set-Cookie` strings. **Amends D137** — wire format is std; cookie *policy* (name `customer`, Max-Age 30d, HttpOnly) stays in `examples/http`. Gate: `http_helpers`. |

## 2026-08-16 — `std/json`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D141 | Product JSON codec is a dynamic `Json` tree | `std/json`: `Json = JNull \| JBool \| JInt \| JFloat \| JStr \| JArr \| JObj`. **`decode(s) -> Trial<Json, String>`**; **`encode(v) -> String`**. Integers without `.` / exponent are `JInt` (out-of-range Int64 fails). Every other JSON number is `JFloat` via `strtod` (non-finite fails). Arrays / objects recurse through `Array<Json>` / `Map<String, Json>`. No pretty-print, no comments / trailing commas / NaN tokens, no HTTP helper in this module. | **Shipped.** Sample-driven (D97); charter [`ideas/2026-08-15-json-codec.md`](ideas/2026-08-15-json-codec.md). Face is encode/decode (codec pair), not JS parse/stringify. Enables union-member fields that are `Float64` / `Array` / `Map` / User (was a stale scalar-only check). Array/Map of **sums** now RC the live arm. `Map<K,V>` seeds `May<V>` so `.get` works for User values. RT: `thoxa_f64_from_string` / `thoxa_f64_is_finite`. Gate: `json`. Crema dogfood: `POST /api/orders`. **Typed `T.read` / `Issue` → D149.** |

## 2026-08-15 — Float64

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D140 | Product `Float64` | IEEE-754 binary64 (`double`). Literals `1.5`, `1e2`, `1.5e-1`, `1f64`, `3.14f64`. A `.` starts a fraction only when the next char is a digit, so `1..5` stays a range. No `Float32` (`f32` suffix is an error). Arith `+ - * /` and compares; no `%`; no bitwise. Int widths coerce to `Float64` at assign / call / field / return / mixed binop; never `Float64`→int implicit. Lives in ctor fields, `Array` elems, Map **values**, C-layout ABI scalars, and `extern` as `double`. Interp / JSX stringify is shortest round-trip (integral values print with `.0`). | **Shipped.** Amends **D28** (new scalar width) and **D48** (ctor fields). Not a Map key. No float literal types or match arms. Gate: `floats`. JSON codec followed as **D141**. |

## 2026-08-15 — HTTP host modules under `std/http/`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D139 | HTTP hosts live under `std/http/` | Protocol stays **`std/http.thx`** (syscall-free). Host modules are **`std/http/server`** and **`std/http/static`**. File-beside-folder: `"std/http"` is still the protocol file; `"std/http/server"` is the nested file. No directory-import (`index.thx` / `main.thx` as barrel). No re-exports from `http`. | **Shipped.** Amends D126/D136 import paths (was `std/http-server`, `std/http-static`). Nested specifiers already resolve (`format_thx` appends `.thx`). C id `std/http/server` → `std_http_server`. Hyphenated stems remain legal (D138). Gates: `http_host`, `router`. |

## 2026-08-15 — kebab-case module files

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D138 | Module files are kebab-case scopes | A `.thx` file is a **module** (a compilation unit / scope), not a type. Non-entry library stems are **kebab-case**: `[a-z][a-z0-9]*(-[a-z0-9]+)*` (`http.thx`, `http-server.thx`). Types remain PascalCase; values and namespace bindings remain camelCase. Directory entry is **`main.thx`** (amends **D68**). Import specifiers match the file (`"std/http"`, `"std/http/server"`, `"./math-utils"`). Namespace import is still not a value (**D33**). No user-facing `type Module`. | **Shipped.** Drops the oracle/loader “file describes a type / import is an instance” story (never surfaced). Amends **D35**: library module ids are path-qualified and sanitized (`std/http/server` → `std_http_server`; `/` and `-` → `_`). Do not use `__` (MIR monomorphs). Amends **D68**: entry file is `main.thx`. Duplicate ids after sanitize are an error. Lone-file entries (conformance `hello.thx`) keep any stem. Gates: `modules`, `namespaces`, `stdlib`. File-based routing stays parked: [`ideas/2026-08-15-file-based-routing.md`](ideas/2026-08-15-file-based-routing.md). |

## 2026-08-15 — HTTP leftover helpers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D137 | Public Str parse + remaining Http wrappers | **`std/Str`:** `parseUInt` → `May<Int64>`; `splitOnce` → `May<Split>` (`before` / `after`; empty sep or no match → `Not`). **`std/Http`:** `queryParam(req, key)`, `formField(body, key)`, `cookie(req, name)` (empty value → `Not`), `htmlResponse(root: Node)` (doctype + `.html()` + `text/html; charset=utf-8`). | **Shipped.** Finishes D136 optional list. Crema uses the wrappers; cookie policy (`customer` / Max-Age) stays in the example. **Set-Cookie builder → D142.** Gates: `stdlib`, `http_helpers`. |

## 2026-08-15 — HTTP host / wire into std

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D136 | Product `Request.query` + HTTP helpers + `std/HttpServer` | **`Request.query: String`** (no leading `?`); `parseRequest` keeps it and still strips path. Body stays empty at parse. **`std/Http`** (still syscall-free) gains `headerGet`, `contentLength`, `urlDecode`, `parseQuery` / `parseForm` / `parseCookies` / `queryMap`, `seeOther`, `withHeader`, `wantsKeepAlive`, `finalize`. **`std/HttpServer`** (Net syscalls): `bindListen`, `readBody`, `readRequest`, `writeResponse(peer, …)`, `serveOne`, `serve(port, handle)`. HEAD is rewritten to GET for the handle; `finalize` still sees the real method. | **Shipped.** Amends D69; extends D126. Crema-dogfooded; example `framework/` removed. Not promoted: `Incoming` header packing, `X-Thoxa-Query`. Headers stay `Array<Header>` (D134). Gates: `http_parse`, `http_helpers`, `http_host`. Leftover wrappers → **D137**. **Import path amended by D139:** `std/http/server`. |

## 2026-08-15 — Nested constructor fields

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D135 | Node + constructor-param User as ctor fields | `type Wrap(n: Node)` and `type Card(d: Drink)` are legal. User includes sums (`Drink.roast: Roast`). Stored inline (same RC field walk as today). Recursive / mutual User fields are an **error** (infinite C struct); `Array` / `Map` of the same type is fine. C-layout body-field types and function types stay out (D24 / D132). | **Shipped.** Amends D48. Gate: `nested_fields`. Dogfood: Crema `Roast` + `DrinkCard(d: Drink)`. |

## 2026-08-14 — Map and value collections

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D133 | Value collections (Swift / Buffer COW) | `Array` (and `Map`) writes go through a **`mut` place**: unique → in-place; shared → copy, update that place only. `let` aliases do not observe later writes. Mutating methods return **`Void`** (no `xs = xs.append(x)`). Temporaries allowed. | **Shipped.** Amends D54. Not Rust moves. **Bytes** splice stays D55. Fixed `Array<T,N>` unchanged. Freeze: [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md). Gates: `arrays`. |
| D134 | Builtin `Map<K, V>` | Heap RC `ThoxaMap*`. Keys v0: String / Int64 / Bool. Values: same as Array elems + nested Map (User values yes). Empty `Map()`. Read `m[k]` / `.get` → `May<V>`; `.has(k) -> Bool`; `.count`. Write `.set` / `.remove` / `m[k] =` write-back. Literals `{ k: v }` (ident key = string name; `{}` still empty block). `for k, v in m` insertion order; `.keys()` / `.values()` snapshots. Ctor fields yes (D48/D69 grow). | **Shipped.** Not `std/`. **User keys deferred:** wait on structural `==` / private hash of constructor-param products — **not** Eq/Hash traits; not pointer identity. No `==` on Array/Map; no Http header migration. Gates: `maps`, `maps_user`. |

## 2026-08-13 — Function pointers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D132 | Function pointers + bound methods (no closures) | Function type is `(a: T) -> R` (not oracle `Fn`). Values: free fn name / `module.fn`; unbound `Type.method` (type qualifier, extra `self` arg); bound `instance.method` (receiver + code; call ≡ `instance.method(args)`). Unboxed `{receiver, code}` pair; bound copy dups the instance. **Live in** args, returns, locals, **module `let`/`mut`**. **Not** ctor/body fields, arrays, or generic product fields (**D48 unchanged** — type error, not a lint). Call through a pointer requires all args (no defaults). No `==`, no interp, no JSX embed, no `-->` of a pointer, no nested/lexical fns, no multi-clause/`extern` as values. | **Shipped.** Freeze: [`specs/2026-08-13-function-pointers.md`](specs/2026-08-13-function-pointers.md). Gate: `fn_pointers`. Full FCF + ORC stay parked. Traits are a separate surface. Dogfood: Crema `serve(port, handle)`. |

## 2026-08-11 — Multi-clause functions

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D131 | Multi-clause functions as type-driven match sugar | Same name in one scope (module free fn **or** type method) → **one** function; formals stay `name: Type`; **any** types may discriminate; **source order** = match order; sugar only (one runtime entry). Bare **singleton literal types** + member-set `\|` in general type positions (formals included). Equal arity; **no defaults** on multi-clause v0; non-adjacent clauses OK. Domain = specific join of clause domains (no forced widen). Unreachable / duplicate domain / export mismatch → **errors**. Returns: **one** merged return via match/D129-style join; per-clause `-> T` is local expect on that arm. If any clause `export`, all must be. | **Not** C++ overload rank; **not** pattern formals (later optional). Cross-arity, composition order, `extern` multi-clause, fn values — out of v0. **D16** unchanged (std factories separate). Freeze: [`specs/2026-08-11-multi-clause-functions.md`](specs/2026-08-11-multi-clause-functions.md). Gate: `multi_clause`. Sample sketch: `scratch/sample.thx`. |

## 2026-08-10 — Match arm sugar

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D130 | Tag-only bare ctors + `=>` arm bodies | (1) Bare constructor pattern (`Pass`, `Fail`, `Has`, user tags, …) matches **by tag only** — no payload destructure (same as all-`_` binds; heap payload stays on the scrutinee). Payload binds still use `Name(x)` / `Name(_)`. (2) Arm body may be **`=> expr`** (one expression; sugar for a single-expr block) or **`{ … }`** (multi-stmt; newlines). Guards work on both: `Pat if g => e` / `Pat if g { … }`. No commas between arms. **`->` stays type-level**; `=>` is match-arm only. | Supersedes D66 “no bare Fail/Pass without parens (deferred)”. Gate: `match_arm_sugar`. Dogfood: `examples/http` `main` (`Pass => 0` / block `Fail`). |

## 2026-08-10 — Return-type inference

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D129 | Body-inferred return types | Unannotated non-`main` free-function return types are **inferred** from exit types when possible; explicit `-> T` is authoritative and checked. Missing `->` is **no longer** default `Void` for free fns. Unannotated `main` still **`Int64`**. | Residual `?`: structural collapse of success type-args to `Void`, keep residual extract; sum with value exits. Bare `Pass`/`Fail` alone cannot pick a union → annotate. Self/mutual recursion unannotated → annotate (no fixed-point v0). No special-casing the name `Trial`. **LSP hover** shows inferred/`->` returns from last successful lower (local + remote). Freeze: [`specs/2026-08-10-return-type-inference.md`](specs/2026-08-10-return-type-inference.md). Gate: `return_infer`. Dogfood: `examples/http` `run()`. v0 gap: unannotated **methods** still default `Void` in MIR. |

## 2026-08-10 — Diverging loop (no break)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D128 | `loop` without `break` diverges | A bare `loop { … }` that contains no `break` targeting that loop is typed as bottom (**Never**): it unifies with any expected type, so it may be the last expression of a function returning `Trial` / `Int64` / … without a dummy tail (`Pass(Void)`, `0`, …). Nested loops' `break`s do not count for the outer. `while` / `for` still yield `Void` (they can end). | Forced by `examples/http` `run()`. Gate: `loop_diverge`. |

## 2026-08-10 — Combined namespace + selective import

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D127 | Combined import clause | `import ns, { a, b as c } from "…"` binds a camelCase **namespace** and **selective** names from the same module in one statement | Order is namespace first, then brace list (not `{…}, ns`). Same binding/export rules as two separate imports. Extends D32/D33. Gate: `import_combined`. Dogfood: `examples/http` Logging/Net. |

## 2026-08-10 — std Io / Net / Http (sample-driven)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D126 | Product `std/Io` + `std/Net` + `std/Http` + `std/HttpStatic` | Promote the mature sample/conformance I/O and HTTP surface into product **`std/`**. **`std/Io`:** `File`, `openFile`/`readFile`/`writeFile`/`appendFile`, OpenMode, seek. **`std/Net`:** TCP `Socket` + `receiveRequest`/`sendAll`. **`std/Http`:** parse/write Request/Response + status helpers. **`std/HttpStatic`:** static-file `Router` (GET/HEAD). Fail payloads remain **String**. Linux product track. | **Shipped:** modules under `std/`; conformance gates retargeted (removed `tests/conformance/{iolib,httplib}/`). Dogfood: `examples/http` Crema-lite. Extends D56/D69/D70/D97/D109+. **Grown by D136** (`Request.query`, helpers, `std/HttpServer`). **Import path amended by D139:** `std/http/static`. Named `ffi` root still deferred. |

## 2026-08-10 — std/Logging + remove ambient println

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D125 | `std/Logging` + no ambient print | Product logging is **`std/Logging`** (not `Log` — reserve **`log`** for logger *instances* later). v0 = **default logger**: five levels (Trace…Error), default **Info** + **stderr** + full format; emit void (no `?`); generic emit named **`emit`** (not `log`); **`toFile` / path `setSink` → `Trial`**; emit write failures best-effort. Multi named loggers / `initLog` **later**. Ambient **`println`/`print` removed**. **App import face:** `import logging, { info, warn, … } from "std/logging"` (**D127** combined; two-line dual import still valid). Golden/data: `toStdout` + `formatPlain` + level fn. Import required. | **Shipped:** `std/Logging.thx` + RT `thoxa_write_line_fd`; session Shell inject loads Logging (+ deps); gates `logging_levels`, `logging_sink` + full conformance dogfood; `examples/http` uses combined import. Freeze: [`specs/2026-08-10-std-log-and-println-removal.md`](specs/2026-08-10-std-log-and-println-removal.md). |

## 2026-08-09 — Structured diagnostics (foundation)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D124 | Structured diagnostics | One **diagnostic IR** (code, severity, message, span, labels, help, expected/actual) drives **pretty `thoxa check`**, **`thoxa check --json`**, and **LSP** (`code` + `relatedInformation`). Editor/human clarity is the ship criterion; agent-only surfaces deferred | **Shipped:** `crates/compiler/src/diagnostic.rs` + enriched `Error`; codes THX001–THX010 (classifier + explicit emit; **THX010** unused-import **warning**); pretty CLI; JSON report with `compiler.version`; LSP code/related/help-in-message + warning severity. Exit/`ok` ignore warnings alone. Freeze: [`specs/2026-08-09-structured-diagnostics.md`](specs/2026-08-09-structured-diagnostics.md). Parked agent ideas: [`ideas/2026-08-09-agent-dx-from-zerolang.md`](ideas/2026-08-09-agent-dx-from-zerolang.md). |

## 2026-08-09 — Fallible import (no ambient prelude)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D123 | May/Trial not ambient | Programs **must** `import { … } from "std/fallible"`; compiler no longer injects Fallible into every module | Refines D118 ambient cutover. Session Shell inject still loads `std/Fallible` into the package (Shell imports Has/Not/May). Types remain ordinary generic unions. Gates: `mays`, `trials*`, `bytes`, `http_*`, `stdlib`. |

## 2026-08-09 — Examples / argv / module-global RC

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D121 | Program argv | `fn main(arguments: Array<String>)` receives process argv; `thoxa run path args…` forwards trailing tokens | Bare `fn main()` still zero-arg. `arguments[0]` is the binary path. Forced by `examples/kilo`. |
| D122 | Module-global end-of-fn drop | **Never** schedule module `mut`/`let` heap places (`g_*`) for function-exit drops | Assign into a global still drops the **old** value and dups the RHS; the global itself outlives the function. Bug: registering globals in `note_drop_place` double-freed `textRows` after `insertChar` (kilo empty-buffer crash). Locals unchanged. |

## 2026-08-08 — Union match guards

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D120 | Union-match guards | `Ctor if bool` / `Ctor(x) if bool` on tagged/literal union arms; binds visible to the guard | Not on catch-all (`else` / bare whole-sum bind). Guarded arms do **not** exhaust a tag (same tag may repeat). Guard fail falls through to later arms. Applies to user unions and May/Trial. Conformance: `sums_match_guard`. Extends D111 + D118. |

## 2026-08-08 — Union identity (nominal atoms · transparent aliases)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D119 | Union identity | **Nominal atoms** (`type Foo` / products always unique by decl) + **transparent union aliases** (`type Outcome = Done \| Quit` ≡ member set `Done \| Quit`) | Equality by **member set** (order irrelevant). Inference yields the set; does **not** search for a matching alias name. True type stays the atom; assignability into a union is membership. Optional opaque/newtype wrapper later. Extends D118. **Shipped:** member-set eq + alias remap + expect on `if` + unannotated if lub (`sums_alias`, `sums_if_lub`). Freeze §2.3: [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md). |

## 2026-08-07 — Sum types / fallibility (target freeze)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D118 | Sum types + general `?` | TS-leaning unions; arm marks `!`/`?`; May/Trial as std generics | **Shipped:** unions + marks + generics + May/Trial as `std/Fallible` unions; cross-module residual rewrap. **Import → D123** (no ambient inject). Gates: `sums_*`, `mays`, `trials*`, `http_*`. **Identity → D119.** Freeze: [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md). |

## 2026-08-07 — Fixed Array locals

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D117 | First-class fixed `Array<T,N>` locals | Locals with `Array<T,N>` annotation + array lit of length N; element get/set; ABI scalar elems only | Extends field-only **D27**. **Whole-array assign/bind** via element-wise copy in cgen (C arrays not first-class). Lit elems coerce to `T`. Gates: `fixed_array_local`, `fixed_array_assign`. |

## 2026-08-07 — Default-arg defining-module scope

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D116 | Default-arg name scope | Free names in default exprs resolve in the **defining** module (not the call site) | Defaults still **lower** at the call site (D98/D106 codegen), but `name_to_*` maps swap to the definer for those exprs. Applies to free fns, methods, and ctor field defaults. Conformance: `defaults_mod`. |

## 2026-08-07 — File seek/position

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D115 | File seek / position | `File.seek(offset, whence = seekSet)` and `File.position()` → `Trial<Int64, String>`; export `seekSet`/`seekCur`/`seekEnd` | Was `iolib/Io.thx`; product home **`std/Io`** (**D126**). Closed file → Fail. Conformance: `file_seek`. |

## 2026-08-07 — Nested Trial as payload

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D114 | Nested Trial as payload | Allow **`Trial<…>`** as Trial ok/err and May inner types | Completes D113. cgen interleaves May/Trial monomorph typedefs by nest depth (`May<Trial>` and `Trial<May>` both work). Address still not a payload. Conformance: `trials_nested_trial`. |

## 2026-08-07 — Nested Trial/May payloads

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D113 | Nested Array/May payloads | Allow **dynamic `Array<T>`** and **`May<U>`** as Trial ok/err and May inner types | Nested **Trial → D114**. Lexer `>>` in type args is split into two `>` closers. cgen monomorph typedefs by nest depth. `Not()` outside `-> May` defaults to `May<Int64>` (so `Pass(Not())` works when ok is that monomorph). Conformance: `trials_nested`. |

## 2026-08-07 — File write/create

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D112 | File write/create | Free `writeFile` / `appendFile`; `File.write`; open flags `oCreat`/`oTrunc`/`oAppend` + `sysOpenMode` (3-arg open) | Was iolib; product **`std/Io`** (**D126**). `openFile` optional `mode` (default 0644). Int32 fd → `File(fd: Int64)` (**D21**). Conformance: `file_write`. |

## 2026-08-06 — Language: match guards/binds/or, File, write-back, defaults

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D111 | Value-match guards | `pattern if bool_expr { … }` on literal/or arms; cond = pattern match **and** guard | Not on catch-all (`else`/bare bind). Not on Trial/May. Same lit may repeat when guards differ. Conformance: `match_guard`. Extends D87. |
| D110 | Value-match bare bind | Final catch-all arm may be a bare name `n { … }` binding the scrutinee (instead of `else`) | `_` as bare pattern is discard catch-all (same as `else`). Not for Trial/May. Conformance: `match_bind`. Extends D87. |
| D109 | File type (product Io) | `export type File(fd: Int64)` + `openFile` / `readAll` / `close` / `isOpen`; `readFile` reuses File | Product **`std/Io`** (**D126**; was conformance iolib). `fd` Int64; close write-backs `fd = -1` (**D107**). Conformance: `file_type`. **Write → D112**. |
| D108 | Value-match or-patterns | `0 \| 1 \| 2 { … }` of string/int/bool literals; required `else` still | Lowers to `||` of equality tests. Not for Trial/May arms. Duplicate leaf lits rejected across arms. Conformance: `match_or`. Extends D87. |
| D107 | Method mut-field write-back | In methods, bare assign to a ctor-param field (or C-layout `mut` field) updates the **receiver** | C ABI: User method receivers are pointers (`Type *`). Name receivers pass the binding place (not a `needs_drop` temp). Heap FieldAssign drops old field + dups new. Unlocks File-type style `fd = …` in methods. Conformance: `method_field_writeback`. |
| D106 | Defaults on methods and ctors | Same rules as free-function defaults (D98): `name: T = expr`; omit at call; lowered at **call site** | Constructor params and methods. Ordering: no required formal after a defaulted one. **`!` / extern defaults allowed** (call-site fill; C still receives all args). **Name scope → D116**. Gates: `param_defaults`, `param_bang_defaults`. |

## 2026-08-05 — Session libtcc step backend (target)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D105 | REPL / session-step C backend | **In-process libtcc only** (`TCC_OUTPUT_MEMORY`); no compiler spawn, no step `.so` | Vendored TinyCC under `vendor/tinycc` (Linux x86_64). Batch + `libthoxa_rt` stay system `cc`. Legacy `cc`+dlopen escape **removed**. Freeze: `docs/specs/2026-08-05-session-libtcc.md`. |

## 2026-08-05 — Session pipes, redirs, pure-step, std Shell

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D101 | Session OS pipelines | Command sugar `a \| b` → `shell = shell.pipeline(blob)`; RT `thoxa_pipeline` (pipe/fork/execvp/wait); status = last stage | Blob: NUL-terminated argv words per stage, empty field (double-NUL) between stages. Helpers: `sessionEncodeArgv` + `sessionPipelineBytes`. Pure int BitOr (`1 \| 2`) stays Thoxa. Conformance: `session_pipe`. |
| D102 | Session file redirections | Command sugar `>` / `>>` / `<` → `shell = shell.runIo(argv, in, out, append)`; RT `thoxa_spawn_wait_io` | Empty path strings inherit. Redir ops as separate words. Pure int comparison (`1 > 0`) stays Thoxa. No `2>`; no redir on pipeline stages yet. Conformance: `session_redir`. |
| D103 | REPL pure step only | Remove host fast paths for `status` / `cd` / `\|`; all executable chunks use session step path | In-process `chdir` still updates host cwd. Unit: `repl::pure_step_*`. Step **backend** target → **D105**. |
| D104 | Session Shell in product std | Default `Shell` + helpers live in `std/Shell.thx`; session inject prefers on-disk std, compile-time `include_str` fallback | Replaces compiler-only `session_prelude.thx`. Still auto-injected for session scripts (not required `import`). Shell imports Fallible; inject loads `std/Fallible` into the package (**D123**). |

## 2026-07-25 — Session `path()` hook

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D100 | Session PATH hook | Optional `fn path(parent: Array<String>) -> Array<String>` in rc/decls; host runs after rc and on decl change via **session step .so** (stdout capture), not a child binary | `parent` = current process PATH split on `:`. Return components **replace** process PATH (join `:`); empty `[]` clears PATH. No merge unless body appends `parent`. Host captures pre-hook PATH as `THOXA_TOOLCHAIN_PATH` / `capture_toolchain_path` so `cc`/`ar` still resolve when session PATH is empty/minimal. Unit: `repl::tests`. |

## 2026-07-25 — Parameter defaults + std color hex

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D98 | Parameter defaults (M3 subset) | `name: T = expr`; omit at call (named or trailing positional); defaults lowered at **call site** | Originally non-`!` only; **now includes `!` and `extern`** (expand before C call). Non-default formals may not follow a defaulted one. Methods/ctors → **D106**; free-name scope → **D116**. Gates: `param_defaults`, `param_bang_defaults`. |
| D99 | Hex color → SGR | `std/Ansi.parseRgba` / `fgHex` / `bgHex`; primary `#RRGGBB`, also `#RRGGBBAA` (optional `#`); alpha optional, ignored for SGR (`a=255` if omitted) | Invalid hex → black SGR. `defaultPrompt(signature: String = "#3B9DC2")` uses `fgHex` for user@host. |

## 2026-07-24 — Session / REPL (Thoxa is the session language)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D88 | Session product shape | Bare `thoxa` / `thoxa repl` = session REPL; `thoxa path` runs program (`main`/`main.thx`) or **session script** (no `main`); subcommands unchanged | Plan: `docs/specs/2026-07-24-thoxa-session-repl.md` |
| D89 | Session value | `type Shell(status: Int64)` + methods (`echo`/`pwd`/`cd`/`run`/`runIo`/`pipeline`/…) | Real Thoxa; status field (not bash `$?` core). Product file: `std/Shell.thx` (**D104**). |
| D90 | Session code placement | Decls stay module-level; top-level stmts → `Shell.step` body; synthetic `main` returns `step().status` | `Item::SessionStmt`; `session::prepare` |
| D91 | In-function import/type | Not required; host lifts / top-level decls only | Deferred general nested modules |
| D92 | Command sugar | Thoxa-first; bare word/string sequences desugar to `shell = shell.*`; **pipes/redirs before Thoxa-first** so `|`/`>` are not kept as BitOr/Gt | `command.rs`; quotes from thoxash v3. Command-shaped parse trees include path Div, **flag Sub/Unary** (`ls -al`, `--all`); pure literal arith (`1 - 2`), pure int BitOr (`1 \| 2`), pure int Gt (`1 > 0`) stay Thoxa. Pipes → **D101**; redirs → **D102**. |
| D93 | REPL execution | Host loop: **decls accumulate**; host-held status threaded into `Shell.step`; entry remains `thoxa_session_step(ThoxaShellState*)` | Step backend → **D105** libtcc only. Was v1 compile+exec child; was cc+dlopen. Fast paths removed → **D103**. |
| D94 | Script execution | Splice + synthetic `main` + normal executable | No dlopen |
| D95 | extras/shell | Dogfood prototype of Shell API; product face is bare `thoxa` | `cargo make shell` → bare `thoxa` |
| D96 | Session rc + prompt | Interactive REPL loads `$HOME/.config/thoxa/shell.thx` (missing = no-op). Decls accumulate; stmts run once. Optional `fn prompt() -> String` — host rebuilds prompt **step .so** when decls change, **dlopen-calls** it every primary prompt (stdout captured). Host sets `THOXA_STATUS`. Default `$ `; continuation `> ` fixed. Overrides: `THOXA_SHELL_RC`, `THOXA_NORC` | Real Thoxa (not a second grammar). Piped REPL / session scripts do not load rc. Optional `fn path` → **D100**. |
| D97 | Product `std/` root | Import `"std/Module"` resolves under product std tree (`THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to `thoxa` binary). Modules: `Shell`, `Str`, `Path`, `Ansi`, `Env`, `Git`, `Prompt` (`defaultPrompt`). `ffi` still deferred. **Growth is sample-driven** — not a standalone “expand std” todo | Partial unlock of D30 named roots for session prompts + shared helpers. Conformance: `stdlib`. Session Shell → **D104**. |

## 2026-07-21 — Value match (string / int / bool + else)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D87 | Value `match` patterns | String / Int64 / Bool **literal** arms + required final `else` | SPEC §7.4 subset. Lowers to `if scrut == lit` cascade (`string_eq` for String). Duplicate lits rejected; arms after `else` rejected. Trial/May still Pass/Fail or Has/Not only (exactly two arms). No bare binding pattern / or-patterns / guards yet. Conformance: `match_value`. Dogfood: thoxash `evalLine`. |

## 2026-07-21 — thoxash spawn helper

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D86 | External process spawn (dogfood) | RT `thoxa_spawn_wait(path, argv_blob, argv_len)` — fork/execv/waitpid | Array not FFI-marshalable; argv is consecutive NUL-terminated strings in `Bytes`. Child exec fail → exit 127. Parent returns exit status / 128+sig / -1 on fork fail. Used by `extras/shell` only for now; not a language surface. |

## 2026-07-20 — May / Has / Not + Bytes.find

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D85 | `May<T>` | Surface `Has(x)` / `Not()`; match / `?` | **Refined by D118 + D123:** `std/Fallible` union `May<T> = Has<T!> \| Not?` (import required; not a builtin layout). Nested Array/May → D113. No May↔Trial `?` mixing. |
| D46′ | `Bytes.find` | `find(needle[, from]) -> May<Int64>` | **Supersedes D46** (`-1` sentinel). Runtime still returns -1; MIR wraps to `Has(i)` / `Not`. Caller needs `May` in scope (import `std/Fallible`). Empty needle → `Has(from)` (in range); missing → `Not`. Conformance: `mays`, `bytes`. |

## 2026-07-20 — Optional empty type body

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D84 | Type body braces | `{}` optional when a type has no body fields and no methods | `type Header(name: String, value: String)` and bare `type Marker` are valid; empty `{}` still accepted. Body-field (R7) and method-bearing types still need `{ … }`. |

## 2026-07-20 — LSP: library modules need no main

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D83 | Check `require_main` | Buffer defines `fn main` → require it; else library check (`LowerOptions.require_main = false`) | Fixes false “program requires `fn main`” on library modules (was iolib/httplib; now `std/` libs + open buffers). Build/run still require main via `lower_package`. |

## 2026-07-20 — post-R9 PR9: Io OpenMode + errno polish

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D80 | errno / strerror without `ffi` root | `thoxa_errno()` in `thoxa_rt` + `extern fn … from "c" "strerror"`; Fail via `fileErr` / **`netErr`** (`"{what}: {strerror(errno)}"`) | No named `import { errno, strerror } from "ffi"` (D30 still deferred). Path included in open/seek/read tags. Net socket Fail tags: `http_net_err`. Conformance expects English ENOENT / EBADF text. |
| D81 | OpenMode (native) | Was Int32 lets only | **Refined:** nullary union `OpenMode = Rdonly \| Wronly \| Rdwr` + `openModeBits` in **`std/Io`** (**D126**); Int32 `oRdonly`/`oWronly`/`oRdwr` for bitwise OR with creat/trunc/append. Gate: `file_openmode`. |
| D82 | Extern String return | C `char*` → `thoxa_string_from_cstr(…)` in cgen | Completes D20 marshal for String *returns* (args already used `thoxa_string_cstr`). |

## 2026-07-20 — Editor PR7: cross-module + open-buffer overlay

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D78 | Cross-module hover/def | Lazy `lsp/crossmodule::resolve_remote` at hover/definition time | Selective imports (incl. `as`), namespace name (`module <spec>` + leading `##`), namespace member (`ns.x`); **typed members on values from namespace + `?`** (`server.reuseAddr` → remote `Socket` method). Jump URI = defining file; open buffer wins over disk (path-matched). Named roots (`std/`) covered for go-to-def. Failures → local fallback (never crash). |
| D79 | Open-buffer package check | `check_source_with_overlays` re-parses any package module whose path is open | Entry buffer always overlaid; other open docs replace disk AST for `lower_package`. Parse fail on overlay → keep disk AST. Untitled/non-path still cannot check imports. |

## 2026-07-20 — Editor PR6: navigation handlers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D77 | LSP navigation (single-file) | `hover` / `definition`≡`declaration` / `references` / `documentSymbol` / `prepareRename`+`rename` via `lsp/index` + resolver | Hover: source-line signature + contiguous `##` docs; **call-arg formals** (`listen(16)` → `backlog: Int64`); `?` unwraps Trial/May for typed members. Rename: binding-id keyed; rejects builtins; PascalCase types / camelCase values. Cross-module jump: **D78**. |

## 2026-07-20 — Editor PR5: shared lexical resolver

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D76 | Lexical resolver | Standalone `resolver::resolve_module` / `resolve_source` → `Resolution` (bindings + references) | Type-independent scopes; member refs **pending** until type-driven link. Kinds: fn/extern/type/field/method/param/let/mut/forvar/pattern/import/builtin. Single-file: imports bind locally (jump-to-import). `lsp/index::build_index` for navigation. **MIR lower still uses its own env** this PR (conformance unchanged); full types-by-binding lower migration deferred. Position API: UTF-8 spans + `span_covers` / LSP 0-based line+UTF-16 char. |

## 2026-07-20 — Editor PR3: extras/ (grammar + Zed)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D71 | DX glue root | **`extras/`** with `tree-sitter-thoxa` + `zed` | Non-product personal DX; not mixed into `crates/`. Room for future `extras/vscode` etc. |
| D75 | Thin grammar | Lexer-level tree-sitter only (keywords, casing, literals, comments, operators); no full CST | Churn isolation. Semantic tokens from `thoxa lsp` refine function/variable. Committed `src/parser.c`; Zed pins `[grammars.thoxa].rev` to a commit that contains it (`file://` repo path). Highlighting islands (not a CST): JSX tags; glued `sql"…"` with SQL injection over `sql_text` (`injections.scm`). |

## 2026-07-20 — Editor PR2: `thoxa lsp`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D72 | LSP CLI | `thoxa lsp` — Language Server Protocol over **stdio** (Content-Length JSON-RPC); not interactive | Phase 1 caps: `textDocumentSync: Full` (1), `semanticTokensProvider` (full + legend). `publishDiagnostics` on didOpen/didChange; clear on didClose. Handlers catch panics/errors → stderr + empty/error result; loop stays up. Also `thoxa check <file\|dir>` for disk DX (exit 1 on diags). Navigation deferred (phase 2). |
| D74 | JSON deps | `serde` + `serde_json` on `thoxa` compiler crate | Required for LSP payloads; zero-dep JSON rejected (E8). |

## 2026-07-20 — Editor PR1: check without cgen

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D73 | Check API | `lsp::check_source(filename, text) -> Vec<CheckDiagnostic>` — lex → parse → MIR lower only (no cgen/`cc`) | First diagnostic only (compiler `Error`); `Vec` ready for multi later. Spans: UTF-8 byte offsets → LSP `Range` via `span::{byte_to_position,span_to_range}` with **UTF-16** character units. **Imports:** no `import` → single-file lower; imports + path → `load_package` + open-module overlay; open-buffer deps via **D79** `check_source_with_overlays`. **Main:** buffer with `fn main` → `require_main`; library modules without main typecheck freely (**D83**). |

## 2026-07-20 — Native editor / LSP plan

Plan: [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md). Phase 1 complete (D71–D75). Phase 2 complete (PR5–PR7, D76–D79).

| Planned | Choice | Status |
|---------|--------|--------|
| DX glue root | `extras/{tree-sitter-thoxa,zed}` | **D71** |
| Thin grammar | lexer-level only | **D75** |
| LSP server | `crates/compiler/src/lsp` + CLI `thoxa lsp` | **D72** |
| No Python | Oracle editor/LSP is reference only | locked |
| Phase 1 | diagnostics + semantic tokens + thin grammar + Zed | **complete** |
| Check engine | parse + MIR lower (no cgen) | **D73** |
| Lexical resolver | `resolver` + `lsp/index` | **D76** |
| Phase 2 nav | hover/def/refs/outline/rename | **D77** |
| Cross-module | open-buffer overlay | **D78–D79** |
| JSON | `serde` + `serde_json` on compiler crate | **D74** |

## 2026-07-20 — Directory entry `main.thx` (PR8)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D68 | Directory entry | `thoxa run\|build\|emit-c <dir>` resolves `<dir>/main.thx`; project root for `~/` is that directory (entry file’s parent — same as D31) | **Amended by D138:** file is `main.thx` (was `Main.thx`). Missing `main.thx` → clear error. File path mode unchanged. Default binary stem for a dir is the directory name (not `main`). Conformance harness also discovers top-level package dirs with `main.thx` + sibling `<name>.out`. Conformance: `pkgmain`. Oracle `export fn main` contract not enforced yet (entry still any `fn main`). |

## 2026-07-20 — Bounded multi-request static server (PR7)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D67 | Bounded server conformance | Single-process loop: listen on ephemeral `bind(0)`; for each of N fixed paths, client `connect(localPort)` then server `accept` → `receive` → `parseRequest` → `Router.handle` → `writeResponse` → `send` → close peer+client; then close listen socket and exit | No threads; no infinite accept loop (CI-safe). N≥2 with ≥1×200 and ≥1×404. Deterministic stdout: `"{path} {status-line}"` + `server-ok`. Integrates Net + Http + Router over `tests/conformance/io_data/web/`. Conformance: `http_server`. Closes post-R9 HTTP critical path; oracle infinite `Main` accept loop remains product-only. |

## 2026-07-20 — Router type (PR6)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D70 | Router type | `export type Router(root: String) { export fn handle(req: Request) -> Response }` | Replaces free `handle(root, path) -> Bytes` (D61). Segments + `..`/`.` reject + index.html + Content-Type. Absolute `/` path. Product home **`std/HttpStatic`** (**D126**; was httplib). Conformance: `router`. MIME subset `.html`/`.css`/`.txt`/octet-stream. |

## 2026-07-20 — Request / Response / Header (PR5)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D69 | HTTP protocol types | `Header(name,value)`, `Request(method,path,version,headers,body)`, `Response(status,reason,headers,body)`; `parseRequest(raw) -> Trial<Request,String>`; `writeResponse(res) -> Bytes`; `ok`/`notFound`/`badRequest` → `Response` | Constructor-param fields may be `Array<T>` (heap RC via existing User field walk); `user.arrField[i]` loads dynamic Array then indexes (FixedArray peel unchanged). Headers: simple `Name: value` until blank; body always empty at parse. `Bytes.find` → `May<Int64>` (**D85**). **`summarizeRequest` is intentionally an adapter over `parseRequest`** (requires `\r\n\r\n`; path is query-stripped) — not the pre-PR5 request-line-only scan. Empty method/path/version tokens not rejected (oracle parity; tighten later if server forces). Phase-A RC may over-dup on `Trial<Request>` / `Array<Header>` (LSan noise; no UAF; not a conformance gate). Conformance: `http_parse`. **Amended by D136:** `Request.query` field; helpers + `std/HttpServer`. |

## 2026-07-19 — Post-R9 HTTP plan (PR1)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D63 | Early `return` | `return` / `return expr` as expression (like `break`); type-checked against enclosing `fn`/`method` return type | Bare `return` only for `Void`; MIR `Stmt::Return { value: Option<Place> }`; drop live `needs_drop` **Assign** locals before early return / `?` Fail (same set as end-of-fn `collect_drop_places` — **not params**; caller still owns args); exits function from inside loops. **No ASI:** optional value is parsed whenever the next token can start an expression (newline does not end a bare `return`); write `return` then `}` / non-expr, or put the following statement after a non-expr-start token — intentional `return expr` may span lines. |

## 2026-07-20 — Array&lt;User&gt; elements (PR3)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D65 | `Array<User>` storage | `ThoxaArray { len, cap, flags, elem_size, data[] }`; User elems **inline** (`elem_size = sizeof(C struct)`) | Int64/String keep `elem_size=8` + HEAP_ELEMS; User uses cgen `ThoxaArrayElemFn` field RC (dup/drop helpers). Constructor-param User only (Int64/Bool/String/Bytes fields). Conformance: `array_user` |

## 2026-07-20 — User+heap field RC (PR2)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D64 | User+heap RC | `needs_drop` (not `is_heap`) for Trial Pass/Fail/`?`/match extract; method field prolog Dups heap fields; match binds use fresh places | cgen `emit_rc` already field-walks User; MIR must insert balanced Dup/Drop. Unblocks `Trial<User+String>` / File-shaped APIs. Conformance: `user_rc` |

## 2026-07-20 — Match discard binding

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D66 | Match discard `_` | `Pass(_)` / `Fail(_)` — pattern `_` does not introduce a binding | Standalone `_` lexes as ordinary `Ident("_")` (usable as a normal name in `let`/params/etc.); **only match patterns special-case it** (`bind: None`). A `_` in the arm body is ordinary name lookup (error if unbound; resolves an outer `let _` if present — discard does not shadow). Payload still extracted + RC-owned for end-of-fn drop; **bare tag without parens → D130** (was deferred grammar churn); `_foo` still rejected (not an ident start) |

## 2026-07-19 — R9+ file I/O + Router

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D56 | File surface | Free `readFile(path) -> Trial<Bytes, String>` | Subset of oracle `Io.thx`; whole-file via lseek size; **OpenMode constants shipped (D81)**; **File type → D109**; **write/create → D112** |
| D58 | File errors | Was simple `Fail("open"\|"read"\|"seek")` | **Superseded by D80** — path + strerror via RT errno + libc extern |
| D59 | User zero-init | Constructor-param User with String/Bytes fields zero as NULL heap | Match out-places; **field-RC hardened in D64** |
| D60 | String equality | `==` / `!=` via `thoxa_string_eq` (length + bytes) | Ordering compares not supported |
| D61 | Static Router | `handle(root, path) -> Bytes` HTTP response | Segments + `..` reject + index.html + Content-Type; **superseded by D70** (`Router` type + `Response`) |
| D62 | for-in RC | Drop previous loop-var each iter; last value dropped at fn end | Fixes double-drop of last heap element (was drop-per-iter + fn-end drop) |

## 2026-07-19 — R9+ dynamic Array

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D53 | Dynamic `Array<T>` | Heap RC `ThoxaArray*` (slots as `int64_t`; heap elems bitcast) | Distinct from fixed field `Array<T,N>` (D27); elems: Int64, Bool, String, Bytes; **User via D65** |
| D54 | Array mutation model | Functional `append`/`dropLast` return new arrays; index set mutates shared storage | **Amended by D133** (Swift/Buffer write-back; `let` aliases do not see writes). Historical oracle-list note no longer product. |
| D55 | Array / Bytes splice surface | Literals, empty `[]` + type ann, `.count`, `[i]`, append, dropLast, **`insertAt`/`removeAt`**, `for x in arr`, display; Bytes **`insertAt`/`removeAt`** + **`equals`** | Functional copy (source unchanged); insert `0 ≤ at ≤ count`, remove `0 ≤ at < count` (else panic); byte masked 0–255. Gates: `arrays`, `array_user`, `splice`. |

## 2026-07-19 — R9+ Buffer

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D50 | Growable Buffer | Heap RC `ThoxaBuffer*` with COW mutate | Geometric cap 0→8→double; `append`/`appendBytes` return unique ptr (MIR write-back on name recv); not FFI-marshalable |
| D51 | Buffer surface | `Buffer()` / `Buffer(cap)`, `.count`/`.capacity`, append, appendBytes, freeze→Bytes | `freeze` copies live prefix (consume-on-freeze deferred); `reserve` deferred |
| D52 | Buffer vs Bytes | Buffer builds; freeze → fixed Bytes for FFI/send | Matches D17; no Buffer at extern boundary |

## 2026-07-19 — R9+ control flow + String fields

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D47 | Loops (subset) | `while`, `loop`, `for x in lo..hi` / `lo..=hi`, `break`, `continue` | Loops yield `Void`; **`for x in arr` shipped with D55**; `continue` in range-for hits C for-step |
| D48 | Constructor-param field types | Int64, Bool, **String**, **Bytes** | C-layout body fields remain ABI-only. **Amended:** dynamic `Array` (D69), `Map` (D134), **Node + nested User / user sums (D135)**, **Float64 (D140)**. |
| D49 | RC on user values | `needs_drop(User)` always; cgen walks heap fields | Dup after `StructLit` and heap field loads; method `_self` is a param (not auto-dropped) |

## 2026-07-19 — R9+ Trial monomorphs + wire I/O

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D40 | Trial representation | Was `Ty::Trial` monomorph C structs | **Refined by D118 + D123:** `std/Fallible` `Trial<P,F> = Pass<P!> \| Fail<F?>` (tagged user union monomorphs; import required) |
| D41 | Trial payloads | Void, ints, Bool, String, Bytes, User | **Array + May → D113**; **nested Trial → D114**; Address still deferred |
| D42 | Pass/Fail typing | Prefer enclosing `-> Trial<…>` annotation | Unannotated Pass/Fail(Int) still infers `Trial<Int64,Int64>` when that mono exists |
| D43 | `?` across monomorphs | Same residual payload; rewrap into outer Trial/May | Cross-module Fail monomorphs rebuild by arm name (D118) |
| D44 | Trial RC | Dup live heap side on copy; drop live side on drop | Sum-aware `emit_rc`; `assign_no_drop` for sum-payload temps |
| D45 | Wire conformance | One-shot listen/connect/accept/recv/send | Ephemeral port via `getsockname`; no infinite accept loop |
| D46 | `Bytes.find` | Was Int64 / `-1` if missing | **Superseded by D46′ / D85** — now `May<Int64>` |

## 2026-07-19 — R9 HTTP subset

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D36 | R9 scope | Subset, not full oracle server | Multi-module Net + Http; offline parse/response; socket lifecycle (create→reuse→bind→listen→close). No accept loop, file router, Buffer, dynamic Array |
| D18′ | `Bytes(n)` | Ship zero-filled fixed buffer | Forced by sockaddr / setsockopt packing; locks D18 “when forced” |
| D37 | `Bytes.slice(start, end)` | Half-open copy → new Bytes | Enough for request-line parse without `find` / Array; OOB panics |
| D38 | Socket errors (R9) | Was `Trial<Int64,Int64>` codes | **Superseded by D40–D45** — String / User Trial + wire I/O |
| D39 | Net surface | Private `extern` + `export type Socket` / `createSocket` | Linux sockaddr_in pack by hand; port `0` = ephemeral for CI |

### Notes

- Oracle `examples/http` remains the north star; R9 proves modules + FFI + Bytes integration without inventing stdlib roots (`ffi` / `std/`).
- errno/`strerror`: **shipped for Io and Net (D80)** without named `ffi` root.
- Constructor-param types were Int64/Bool only at R9 → `Socket(fd: Int64)`; **String/Bytes fields shipped later (D48–D49)**.

## 2026-07-19 — R8 modules / multi-file

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D30 | Import origins (R8) | `./…` and `~/…` only at ship; **D97** adds `std/…` | `ffi` still deferred |
| D31 | Resolution | No `..` segments; `~/` = package root | Package root is the nearest ancestor `main.thx` when present, else the entry file’s directory. Matches oracle “never climb”. Nested-file `thoxa check` / LSP use the same root so `app/Pages.thx` can `~/framework/…`. |
| D32 | Export surface | `export` on top-level `fn` / `type` / `let` / `extern`; `mut` never exported | Selective + namespace imports |
| D33 | Namespace import | `import ns from "…"` camelCase binding; members via `ns.x` | Not a first-class value. Combined form → **D127** |
| D34 | Module-level values | Static C globals + `thoxa_module_init()` before main | Dep-order init; private mut state |
| D35 | Name mangling | Non-entry: `{Module}_{name}` types/fns; globals `g_{Module}_{name}` | Entry keeps unprefixed names; `main` special |

## 2026-07-19 — R7 C-layout struct / packing

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D23 | Struct layout default | Declaration-order, natural C alignment (markerless) | Matches oracle LF-B; no `repr(C)` |
| D24 | Body-field types | `type T { [mut\|let] f: Ty }` with ABI scalars / `Array<T,N>` | Separate from R3 constructor-param types |
| D25 | Construction | `T()` zero-inits C-layout types | No field args; R3 still uses named constructor args |
| D26 | `Address<T>` | Extern params only; implicit `&` on mut User places | Explicit `address()`, storing Address deferred |
| D27 | Fixed arrays | Field-only `Array<T, N>`; element get/set via `[i]` | Locals → **D117**. Whole-array assign → D117 element copy; no slice |
| D28 | New scalar widths | UInt32, Byte (=UInt8); bitwise on integers in Int64 then truncate on field store | Hex `0x…` literals. **Amended by D140:** `Float64`. |
| D29 | Struct value-copy / `sizeof` | Deferred | Oracle LF-B §9; kilo re-reads via tcgetattr |

### Notes

- Field write is a typed width boundary (truncate). Bitwise ops promote to Int64 for the op.
- Passing a non-`mut` binding where `Address<T>` is required is a compile error.
- C codegen emits real C structs (`uint32_t`, `uint8_t f_cc[4]`, …) so padding matches the platform ABI (Regs = 16 bytes on x86-64).

## 2026-07-19 — R6 extern libc (subset)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D19 | Extern libraries (R6) | `"c"` only | Headers (`unistd.h`, `stdlib.h`, `string.h`) supply prototypes |
| D20 | Extern types (R6) | Int32, Int64, UInt64, String, Bytes, Void | R7 adds UInt32, Byte, `Address<T>` |
| D21 | Int width at boundary | Coerce Int64↔Int32/UInt64 at args; widen ints for `==`/`<` etc. | Literals stay Int64; R7 extends to UInt32/Byte |
| D22 | C symbol rename | `from "c" "symbol"` | When Thoxa name ≠ C name |

---

## 2026-07-19 — Bytes construction sugar (R5 follow-up)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D14 | String → Bytes surface (now) | Keep `Bytes.fromString(s)` | Matches `apocrypha/old/ThoxaPython` SPEC §9.1; R5 ship shape |
| D15 | `"…" as Bytes` | **Rejected** (not deferred) | Encode/copy is not a cast. Named `fromString` is the product surface forever for String→UTF-8 Bytes. Do not reintroduce as sugar under M4 either. |
| D16 | Overloaded `Bytes.from(…)` | Reject (for now) | No ad-hoc overloading story; prefer explicit names if more factories appear. **D131** is the deliberate multi-clause language feature — does **not** by itself reopen std `from` overloads; revisit factories only in a separate decision after D131 ships. |
| D17 | Multi-byte / dynamic build | Prefer `Buffer` path later | `append` / `freeze` → `Bytes`; not `from([1,2,3])` as the main story |
| D18 | Extra factories | Only when forced | e.g. `Bytes(n)` zero buffer for FFI (R6 pressure); maybe `fromArray` when arrays exist |

### Analysis (why)

**`mut b = "abc" as Bytes` — rejected**

- String→Bytes is **encode/copy** (UTF-8 text → raw buffer), not a cast or rebrand.
- Rust-style “cast” is the wrong model; named construction (`fromString`) keeps encoding explicit.
- A general `as` conversion system (if ever) still should not pretend text↔bytes is free or total both ways; Bytes→String needs UTF-8 policy.

**`Bytes.from("abc")` with overloads (`from([1,2,3])`, …)**

- Friendly, but SPEC already names `fromString`. Renaming reopens the oracle.
- Overload sets need either ad-hoc resolution or a `From`-like trait; Thoxa has neither.
- Bare `from` hides source/encoding (UTF-8 text? raw ints? size?).
- Array form needs array literals + element rules — later rungs.
- If multiple sources appear, prefer **distinct names** (`fromString`, `fromArray`, `Bytes(n)`), same spirit as Buffer’s `append` vs `appendBytes`.

**Rule of thumb**

- Domain construction from a specific source → **named static method** (`fromString`).
- Multiple input shapes → **multiple names**, not silent overload, until overloading is a deliberate language feature.
- Do **not** use cast sugar for String→Bytes (**D15 rejected**).

Revisit D17 with Buffer / growable-buffer design; revisit D18 when more factories are forced.

---

## 2026-07-18 — Kernel kickoff

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D1 | Growth model | Example ladder toward HTTP | Small kernel first |
| D2 | Algebraic effects | Deferred | Revisit with async; may later interact with general `?` (D118) |
| D3 | Error model | `May`/`Trial` + force-to-deal | Not effect-`exn`. **D118 + D123:** May/Trial as `std/Fallible` unions (import required) + general `?` via type marks — freeze `2026-08-07-sum-types-and-fallible` |
| D4 | Value model | Hybrid (A) | Unboxed scalars; inline small sums; RC heap; C-layout FFI structs |
| D5 | Phase A backend | Compile-to-C | No QBE yet |
| D6 | Compiler host | Rust under `crates/compiler` | |
| D7 | Runtime | C `thoxa_rt` | Headered RC objects |
| D8 | RC ambition day one | Simple correct RC | Perceus later |
| D9 | Docs path | `docs/specs/`, `docs/decisions.md` | No `docs/superpowers`; no graph until needed |
| D10 | Build automation | `crates/make` xtask, `cargo make …` | No project `.sh` scripts |
| D11 | Build products | Single `target/` tree: `rust/`, `c/`, `thoxa/` | No separate `targets/`; Cargo uses `target/rust` via `.cargo/config.toml` |
| D12 | Language oracle | `apocrypha/old/ThoxaPython` SPEC + conformance | Reopen syntax only deliberately |
| D13 | Old QBE tree | Reference only | Do not revive megapass codegen |
```