# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-17 (**D141** `std/json` `decode`/`encode` + Crema
`POST /api/orders`;
**D140** `Float64`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case module files **shipped**;
**D137** Str `parseUInt`/`splitOnce` + Http
`queryParam`/`formField`/`cookie`/`htmlResponse`; **D136**
`Request.query` + `std/http/server`; **D135** Node + nested User ctor fields;
JSX `Node` is `Element | Text | Fragment` + `{...}` spreads **on master**;
D133/D134 Map; D132 fn pointers; `-->` nowait **on master**, not freeze /
not handbook; Crema-lite Phase A–D1 dogfoods product `http` / `http/server` /
`json`)

---

## Now

**Session / shell remains deferred.**

**Shipped recently:** **`std/json`** (**D141** — `decode` / `encode`;
Crema `POST /api/orders`);
**`Float64`** (**D140**);
**HTTP hosts under `std/http/`** (**D139**);
**kebab-case modules** (**D138**);
**HTTP leftover helpers** (**D137**);
**HTTP host / wire** (**D136** — `Request.query`,
`std/http` helpers, `std/http/server`); **Node + nested User ctor fields**
(**D135**); **Map + value collections** (**D133/D134**); **function
pointers** (**D132**); **`-->` nowait**
(colorless wait; Net parks; `HttpServer` `--> serveOne` + keep-alive);
**multi-clause functions** (**D131**);
Crema-lite **Phase A–D1**; **match arm sugar** (**D130**); **return-type
inference** (**D129** + LSP hover); **diverging `loop`** (**D128**);
**combined import** (**D127**); **`std/io`+`Net`+`Http`** (**D126**);
**`std/logging`** (**D125**); LSP nav; **D124** diagnostics.

**On master, not product:**

- Colorless wait + **`--> name(args)`** (capability `start-tasks` **partial**).
  No Future / `async` / `run`. Token only (not a word). `--> { }` later.
  No D-id / handbook. Crema dogfoods it. Review: `scratch/async.thx`
  (gitignored).
- Type-body `GET "/…"` desugar + LSP (idea
  [`docs/ideas/2026-08-12-router-clause-syntax-experiment.md`](docs/ideas/2026-08-12-router-clause-syntax-experiment.md);
  capability `route-clauses` **partial**). No D-id / handbook. Crema dogfoods
  the clauses (example-only).
- Tag literals + builtin **`Node`** (idea
  [`docs/ideas/2026-08-13-jsx-node-syntax-experiment.md`](docs/ideas/2026-08-13-jsx-node-syntax-experiment.md);
  capability `jsx-node` **partial**). No import; `.html()`; `{e}` children
  are String / Int64 / Float64 / Bool / Node / Array<Node>; `{e}` attrs stringify
  Int64 / Float64 / Bool the same way; `<tag` (no space) is JSX
  including after another expr, `a < b` (spaces) is comparison; `Node` is
  `Element | Text | Fragment` (same tree); `{...attrs}` / `{...props}`
  spreads; gate `jsx`. No D-id / handbook. Crema pages dogfood it
  (`app/layout.thx` `Page` + `app/pages.thx`).

**In progress / next:**

1. **Crema-lite** — remaining D+ (htmx / persistence / sessions) only if
   useful. `POST /api/orders` already dogfoods **`std/json`**. App/domain
   stays example-only. Host/wire is product **`std/http` + `std/http/server`**
   (**D136**/**D139**). Query/form/cookies are builtin `Map` (headers stay
   `Array<Header>`). File-based routing is **parked**
   ([`docs/ideas/2026-08-15-file-based-routing.md`](docs/ideas/2026-08-15-file-based-routing.md)).
2. Editor only when DX forces it.

### Proceed order

1. **Crema-lite experiment** — remaining checklist in
   [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)
   (app under `examples/http/`; host/wire already in `std/`; **JSON
   `POST /api/orders` shipped**). Further D+ only if useful.  
2. **Editor depth** — only when DX forces it. Freezes:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md)
   (recovery/nav residual);
   [`docs/specs/2026-08-09-structured-diagnostics.md`](docs/specs/2026-08-09-structured-diagnostics.md)
   (**D124** shipped — polish codes opportunistically).  
3. **Deferred: all session / shell residual** — redir polish, line editor,
   Shell import-first, libtcc cache/license, etc. Freeze parked:
   [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
4. **Deferred megawork** — M4, Perceus, effects, named `ffi` root — not default
   until CURRENT says so. Multi-logger (`initLog`) is trajectory under D125, not
   a CURRENT line until samples force it.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups. Further **`std/` growth is sample-driven** (D97/D136).
Crema **domain** (pages, models, cookies policy) stays in `examples/http/`.

**Explicit holds:**

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
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; Io/Net/Http/HttpStatic shipped (**D126**); Http helpers + **`std/http/server`** (**D136**); hosts under **`std/http/`** (**D139**); Str parse + remaining Http wrappers (**D137**); **`std/json`** (**D141**); `ffi` deferred |
| Logging / print | **`std/logging` only** (**D125**); ambient **`println`/`print` removed**; app face `import logging, { info, … }` (**D127**); no ambient inject; multi-logger later |
| Diverging loop | Bare `loop` with no exiting `break` is **Never** (**D128**) — valid function tail without dummy value |
| Return inference | Unannotated free fns: returns inferred from exits (**D129**); explicit `->` authoritative; no Trial-by-name; recursion / bare Pass/Fail alone need annotate; methods still annotate or default Void; hover uses inferred types |
| Match arms | Bare ctor = tag-only; `=>` single-expr body or `{ }` multi-stmt (**D130**); `->` types only |
| Multi-clause fns | Same name → one fn (match sugar); `name: Type` formals; any-type dispatch; source order; bare literal types (**D131** — **shipped**). No defaults on multi-clause v0; overlap/export mismatch are **errors**. D16 std factories still separate. |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132** — **shipped**). Receiver+code only; no lexical closures. Module store yes; fields/arrays no (D48 / D135). |
| Value collections | **D133 shipped** — Array/Map writes through `mut` places (Swift/Buffer COW). `let` aliases do not see later writes. Mutating methods return `Void`. |
| Map | **D134 shipped** — builtin `Map<K, V>`; keys String/Int64/Bool; values include **Float64 (D140)** and User sums (`Json`, **D141**). `.has` / `May` get; `{k:v}` lits; `for k, v in m`. User types are values, not keys. User keys later via structural product `==` (not traits). No `std/http` header flip. |
| Ctor fields | Int64, **Float64**, Bool, String, Bytes, Node, Array, Map, constructor-param User (including sums) (**D135** / **D140**). No C-layout, no fn types, no recursive User fields (use Array/Map). |
| HTTP request | `Request.query` kept (no leading `?`); path still stripped. `parseRequest` body empty; host `readBody` fills it (**D136**). Headers stay `Array<Header>`. `queryParam` / `formField` / `cookie` / `htmlResponse` (**D137**). |
| Combined import | `import ns, { a, b as c } from "…"` (**D127**) — namespace + selective in one clause |
| Module files | A `.thx` file is a **module** (scope), not a type (**D138**). Non-entry stems are kebab-case (`http.thx`, `math-utils.thx`). Types stay PascalCase; namespace bindings stay camelCase. Directory entry is `main.thx` (D68 / D138). Module ids for C mangling are path-qualified (`std/http/server` → `std_http_server`). HTTP hosts live under `std/http/` (**D139**). No user-facing `Module` type |
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
6. If D132: [`docs/specs/2026-08-13-function-pointers.md`](docs/specs/2026-08-13-function-pointers.md)  
7. If D133/D134: [`docs/specs/2026-08-14-map-and-value-collections.md`](docs/specs/2026-08-14-map-and-value-collections.md)  
8. If D135 / JSX `Node`: [`docs/decisions.md`](docs/decisions.md) (D135) +
   [`docs/ideas/2026-08-13-jsx-node-syntax-experiment.md`](docs/ideas/2026-08-13-jsx-node-syntax-experiment.md)  
9. If D138 / module files: [`docs/decisions.md`](docs/decisions.md) (D138) — kebab-case stems, path-qualified ids; file-based routing parked in [`docs/ideas/2026-08-15-file-based-routing.md`](docs/ideas/2026-08-15-file-based-routing.md)
10. If Crema / HTTP std: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)
   + **D136** / **D137** / **D139** in [`docs/decisions.md`](docs/decisions.md)
11. If JSON: [`docs/decisions.md`](docs/decisions.md) (**D141**) +
    [`docs/ideas/2026-08-15-json-codec.md`](docs/ideas/2026-08-15-json-codec.md)
    + **D140** (`Float64`)

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo test -p thoxa --lib
cargo make run examples/http
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
