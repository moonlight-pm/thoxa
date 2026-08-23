# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-22 (**D148** `-->` nowait **shipped**; **D147** builtin
`Node` + tag literals **shipped**;
`containers/landing` = thoxa.dev news sign-up, live on
aulos as workload `thoxa` / `https://thoxa.dev/`;
`containers/sola` = sola.computer Paper teaser + ISO notify, live on aulos
as workload `sola` / `https://sola.computer/`;
static JPEG/font MIME; Crema pages under `app/pages/` + `ui/page.thx`;
JSX `{for}` lists + bare `disabled`; **`having`/`else` parked indefinitely**;
Crema Phase **E** complete through **E5**; **E6** WebSockets **parked**;
**D146** SSE `peerFd` / `streamed()`;
**E4** sessions + flash + CSRF; **E3** `crema.js`; **E2** + **D145**;
Crema **A–D2 complete**; HTML-over-the-wire — not vendored HTMX;
**built-in validation** on master, not product — types as inhabitation;
syntax open;
Crema **D2** persist `/tmp/crema.db`;
**D144** `sql"…"` tagged interpolator;
**D143** `std/sqlite` amalgamation;
**D142** `Cookie` / `withCookie` / `clearCookie`;
**D141** `std/json` `decode`/`encode` + Crema
`POST /api/orders`;
**D140** `Float64`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case module files **shipped**;
**D137** Str `parseUInt`/`splitOnce` + Http
`queryParam`/`formField`/`cookie`/`htmlResponse`; **D136**
`Request.query` + `std/http/server`; **D135** Node + nested User ctor fields;
D133/D134 Map; D132 fn pointers; **D148** `-->` nowait)

---

## Now

**Session / shell remains deferred.**

**Shipped recently:** **D148** `--> name(args)` nowait (freeze + handbook;
gates `start_*` / `http_host`); **D147** builtin `Node` + tag literals (freeze +
handbook; gate `jsx`); Crema page split (`app/pages/` + `ui/`); JSX `{for}` +
bare boolean attrs; **`having`/`else` parked**;
Crema **E4–E5** (sessions/flash/CSRF + SSE board);
**D146** `Request.peerFd` / `streamed()` / `beginEventStream`;
Crema **E3** `web/crema.js` (`thx-get` / `thx-post`);
**D145** `htmlFragment` / `etagOf` / `cached`;
Crema **E2** `GET /menu/items` + `GET /orders/:id/ticket`;
multiline `"…"` / `sql"…"` gated (`strings_multiline`);
Zed SQL injection on `sql"…"`;
Crema persist (`/tmp/crema.db`);
**`sql"…"`** (**D144**);
**`std/sqlite`** (**D143**);
**`Cookie` / `withCookie`** (**D142**);
**`std/json`** (**D141** — `decode` / `encode`;
Crema `POST /api/orders`);
**`Float64`** (**D140**);
**HTTP hosts under `std/http/`** (**D139**);
**kebab-case modules** (**D138**);
**HTTP leftover helpers** (**D137**);
**HTTP host / wire** (**D136** — `Request.query`,
`std/http` helpers, `std/http/server`); **Node + nested User ctor fields**
(**D135**); **Map + value collections** (**D133/D134**); **function
pointers** (**D132**); **`-->` nowait** (**D148**;
colorless wait; Net parks; `HttpServer` `--> serveOne` + keep-alive);
**multi-clause functions** (**D131**);
Crema-lite **Phase A–D2 complete**; **match arm sugar** (**D130**); **return-type**
inference** (**D129** + LSP hover); **diverging `loop`** (**D128**);
**combined import** (**D127**); **`std/io`+`Net`+`Http`** (**D126**);
**`std/logging`** (**D125**); LSP nav; **D124** diagnostics.

**On master, not product:**

- Type-body `GET "/…"` desugar + LSP (idea)
  [`docs/ideas/2026-08-12-router-clause-syntax-experiment.md`](docs/ideas/2026-08-12-router-clause-syntax-experiment.md);
  capability `route-clauses` **partial**). No D-id / handbook. Crema dogfoods
  the clauses (example-only).
- Built-in validation (idea
  [`docs/ideas/2026-08-17-builtin-validation.md`](docs/ideas/2026-08-17-builtin-validation.md);
  capability `type-read` **partial**). `Type.read` / `where` / `T.from` /
  `.encode`; `Issue = Missing | Mismatch | Rejected`; `May<T>` fields;
  closed slugs are literal unions and subtype `String`. No D-id / handbook.
  Crema `CreateOrder` dogfoods it.

**In progress / next:**

1. Harvest remaining **on master, not product** surfaces (same order):
   **`Type.read` / `where`**, then **route clauses**.
2. **Editor only when DX forces it.** Crema Phase **E** is complete through
   **E5** (HTML-over-the-wire cafe: orders, fragments, `crema.js`, sessions,
   SSE). **E6 WebSockets stay parked.** Do **not** vendor HTMX.
3. Crema leftovers (product migrate, file-based routing) stay parked.

### Proceed order

1. **`Type.read` / `where`** — lock the idea (syntax still open) + handbook.
2. **Route clauses** — needs a conformance `.thx` first (still HTTP-shaped).
3. **Editor depth** — only when DX forces it. Freezes:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md)
   (recovery/nav residual);
   [`docs/specs/2026-08-09-structured-diagnostics.md`](docs/specs/2026-08-09-structured-diagnostics.md)
   (**D124** shipped — polish codes opportunistically).
4. **Deferred: all session / shell residual** — redir polish, line editor,
   Shell import-first, libtcc cache/license, etc. Freeze parked:
   [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
5. **Deferred megawork** — M4, Perceus, effects, named `ffi` root — not default
   until CURRENT says so. Multi-logger (`initLog`) is trajectory under D125, not
   a CURRENT line until samples force it.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups. Further **`std/` growth is sample-driven** (D97/D136).
Crema **domain** (pages, models, cookies policy) stays in `examples/http/`.
Public sites are **`containers/landing`** (thoxa.dev) and
**`containers/sola`** (sola.computer) — not `examples/`, not `extras/`.

**Explicit holds:**

- **`having` / `else` May sugar** (parked indefinitely — `match` is enough)
- **Crema domain in `std/`** (pages/models/layout — example-only)
- **All session / shell residual** (parked by user direction)
- Inventing shell dialect (`if`/`for`/full `$` forest)
- Tree-walk product interpreter or QBE path (phase A)
- Claiming full oracle SPEC coverage
- Windows / non-Linux session backends
- Treating “expand std/” as its own priority line
- **`"…" as Bytes`** or cast-as-encode for String→Bytes (use `Bytes.fromString`;
  D15 **rejected**)

Horizon: [`docs/roadmap.md`](docs/roadmap.md). Matrix:
[`docs/capabilities.md`](docs/capabilities.md).

---

## Locked product models (do not re-litigate)

| Topic | Rule |
|-------|------|
| Backend (phase A) | **Compile-to-C** + `thoxa_rt`; no QBE product path |
| Value model | Hybrid A; simple single-threaded RC; Perceus later |
| Errors | `May` / `Trial` from `std/fallible` (import) + `?` / `match`; no May↔Trial mix via `?` |
| Session product face | Bare `thoxa` / `thoxa repl`; no-`main` = session script |
| Session value | `type Shell(status: Int64)` + methods; real Thoxa; `std/shell.thx` |
| REPL step backend | **libtcc in-process only** (`TCC_OUTPUT_MEMORY`) → call `thoxa_session_step` (D105); no step `.so` |
| Batch / RT C | System **`cc`** (+ `ar` for runtime) unchanged |
| Host platforms | **Linux** product track (x86_64 libtcc); Windows out of scope |
| Command sugar | Thoxa-first; flags like `ls -al` desugar; pure `1 - 2` stays Thoxa |
| `extras/shell` | Dogfood only; product face is bare `thoxa` |
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; Io/Net/Http/HttpStatic shipped (**D126**); Http helpers + **`std/http/server`** (**D136**); hosts under **`std/http/`** (**D139**); Str parse + remaining Http wrappers (**D137**); **`std/json`** (**D141**); **`std/sqlite`** (**D143**); fragments + ETag (**D145**); SSE takeover (**D146**); `ffi` deferred |
| Logging / print | **`std/logging` only** (**D125**); ambient **`println`/`print` removed**; app face `import logging, { info, … }` (**D127**); no ambient inject; multi-logger later |
| Diverging loop | Bare `loop` with no exiting `break` is **Never** (**D128**) — valid function tail without dummy value |
| Return inference | Unannotated free fns: returns inferred from exits (**D129**); explicit `->` authoritative; no Trial-by-name; recursion / bare Pass/Fail alone need annotate; methods still annotate or default Void; hover uses inferred types |
| Match arms | Bare ctor = tag-only; `=>` single-expr body or `{ }` multi-stmt (**D130**); `->` types only |
| Multi-clause fns | Same name → one fn (match sugar); `name: Type` formals; any-type dispatch; source order; bare literal types (**D131** — **shipped**). No defaults on multi-clause v0; overlap/export mismatch are **errors**. D16 std factories still separate. |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132** — **shipped**). Receiver+code only; no lexical closures. Module store yes; fields/arrays no (D48 / D135). |
| Nowait | Token **`--> name(args)`** starts a named free Thoxa function (**D148**). `Void`; return dropped. Colorless: Net parks; `main` drains. No Future / `async` / `--> { }` / pointer `-->`. |
| Value collections | **D133 shipped** — Array/Map writes through `mut` places (Swift/Buffer COW). `let` aliases do not see later writes. Mutating methods return `Void`. |
| Map | **D134 shipped** — builtin `Map<K, V>`; keys String/Int64/Bool; values include **Float64 (D140)** and User sums (`Json`, **D141**). `.has` / `May` get; `{k:v}` lits; `for k, v in m`. User types are values, not keys. User keys later via structural product `==` (not traits). No `std/http` header flip. |
| Ctor fields | Int64, **Float64**, Bool, String, Bytes, Node, Array, Map, constructor-param User (including sums) (**D135** / **D140**). No C-layout, no fn types, no recursive User fields (use Array/Map). |
| HTTP request | `Request.query` kept (no leading `?`); path still stripped. `parseRequest` body empty; host `readBody` fills it (**D136**). Headers stay `Array<Header>`. `queryParam` / `formField` / `cookie` / `htmlResponse` (**D137**). `Cookie` / `withCookie` / `clearCookie` (**D142**); `htmlFragment` / `etagOf` / `cached` (**D145**). **`peerFd` + `streamed()`** (**D146**) for SSE takeover. Cookie and `Cache-Control` *policy* stay in the app. |
| SQLite | Amalgamation in `libthoxa_rt` (**D143**). Write `sql"… {value} …"` (**D144**); holes quote themselves. `exec`/`query` take `Sql`. No named `ffi` root. No product `thoxa migrate` — apps own schema files. |
| Combined import | `import ns, { a, b as c } from "…"` (**D127**) — namespace + selective in one clause |
| Module files | A `.thx` file is a **module** (scope), not a type (**D138**). Non-entry stems are kebab-case (`http.thx`, `math-utils.thx`). Types stay PascalCase; namespace bindings stay camelCase. Directory entry is `main.thx` (D68 / D138). Module ids for C mangling are path-qualified (`std/http/server` → `std_http_server`). HTTP hosts live under `std/http/` (**D139**). No user-facing `Module` type |
| Node / tag literals | Builtin **`Node`** (`Element \| Text \| Fragment`) + JSX tags (**D147**). No import. PascalCase is `Type(…).view(children:)`. `{e}` children/attrs stringify String / Int64 / Float64 / Bool; `{for}` collects `Array<Node>`. `.html()` is the markup codec — not generic `toString`. Comparison `<` needs a space (`a < b`). |
| String → Bytes | **`Bytes.fromString` only** — no cast sugar (D15 **rejected**) |
| Done means | Conformance `.thx` + `.out` under `cargo make test` |
| Progress docs | Meta first-class; capabilities + CURRENT (+ handbook when shipped) |
| Diagnostics | Structured IR → pretty check + `--json` + rich LSP (**D124** shipped); agent-only extras parked |
| Oracle | `apocrypha/old/ThoxaPython` reference only — not the product |

More D-ids: [`docs/decisions.md`](docs/decisions.md).

---

## Session start checklist

1. [`AGENTS.md`](AGENTS.md)  
2. **This file** (Now + locks)  
3. [`docs/capabilities.md`](docs/capabilities.md) — rows for the slice  
4. One freeze if building that domain  
5. [`docs/decisions.md`](docs/decisions.md) only when reopening surface  
6. If D132 / D148: [`docs/specs/2026-08-13-function-pointers.md`](docs/specs/2026-08-13-function-pointers.md)
   + [`docs/specs/2026-08-22-start-tasks.md`](docs/specs/2026-08-22-start-tasks.md) (`-->`)  
7. If D133/D134: [`docs/specs/2026-08-14-map-and-value-collections.md`](docs/specs/2026-08-14-map-and-value-collections.md)  
8. If D135 / JSX `Node`: [`docs/decisions.md`](docs/decisions.md) (**D135**
   fields, **D147** tags) + freeze
   [`docs/specs/2026-08-22-jsx-node.md`](docs/specs/2026-08-22-jsx-node.md)  
9. If D138 / module files: [`docs/decisions.md`](docs/decisions.md) (D138) — kebab-case stems, path-qualified ids; file-based routing parked in [`docs/ideas/2026-08-15-file-based-routing.md`](docs/ideas/2026-08-15-file-based-routing.md)
10. If Crema / HTTP std: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)
   + **D136** / **D137** / **D139** / **D142** / **D145** / **D146** in [`docs/decisions.md`](docs/decisions.md)
    + Phase E protocol [`docs/ideas/2026-08-18-html-over-the-wire.md`](docs/ideas/2026-08-18-html-over-the-wire.md)
11. If JSON: [`docs/decisions.md`](docs/decisions.md) (**D141**) +
    [`docs/ideas/2026-08-15-json-codec.md`](docs/ideas/2026-08-15-json-codec.md)
    + **D140** (`Float64`)
12. If built-in validation: [`docs/ideas/2026-08-17-builtin-validation.md`](docs/ideas/2026-08-17-builtin-validation.md)
    (on master, not product; syntax open)
13. If SQLite: [`docs/decisions.md`](docs/decisions.md) (**D143** / **D144**) —
    amalgamation in `vendor/sqlite`; `sql"… {value} …"`. Crema persist is
    example-only (`examples/http` `/tmp/crema.db`); no product migrate.

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo test -p thoxa --lib
cargo make run examples/http
cargo make run containers/landing
cargo make run containers/sola
cargo run -p thoxa -- tests/conformance/param_defaults.thx
printf 'echo hi\nexit\n' | cargo run -p thoxa --
```

---

## Agent notes

- “Next item” / “next rung” → first open item under **Now**, one slice.  
- Do not re-walk the tree or re-derive the ladder at session start.  
- **After each finished checklist / product slice:** update progress docs,
  then **commit and push** (user standing order).  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
- **Full-word names** — no single-letter locals; `req`/`res` → `request`/`response`. Especially `examples/http`.  
