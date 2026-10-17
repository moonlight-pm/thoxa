# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-10-17. **thoxa.dev rewritten (D189)** at Joshua's request: a
home page that states the design rationale, features, status, and build steps
and shows nine sample programs (run by `cargo make test`), and `/manual`, the
handbook rendered as one page. The handbook was rewritten as a reference
manual for it. `containers/` is now `workloads/`; the repository moved to
`github.com/moonlight-pm/thoxa`. Fixes found writing the site: `and` / `or`
short-circuit (D183), array literals take the expected element type (D184), a
refined value's heap base is owned when read (D185, a use-after-free), `let`
binds a Void value (D186), `Float64` text in full between 1e-7 and 1e21
(D187), a session line that starts a statement is never a command (D188).
Then **D190**: every `extern fn` gets its own C prototype (asm-labelled
private name), so any linked C function can be called and `Address<T>`
fits a C struct pointer parameter. Then **D191** (conversions: `as` /
`as?`, `from` clauses, implicit encoding to `Json`, typed route body
parameters) shipped in six slices, with D192 between them.
2026-10-16: **Language pass (D173–D181):** `main`'s return is
the exit status, 0–255 (D173); unannotated methods infer their return type,
and `x?` then `Pass(value)` infers `Trial<T, E>` (D174); structural `==` on
products, unions, and arrays (D175, D177, D178); strings inside interpolation
holes (D176); user types as Map keys (D179); collection writes never vanish
(D180); `remove` keeps a Map's insertion order (D181). Editor: Tree-sitter and
LSP colours read `<` as the compiler does (`May<String> {` is not a tag) and
parse string holes as code. Earlier the same day:
**D172** the sites send security headers and
ETags (images and fonts cached a day; review finding 10). **D171** `std/http/server` answers pipelined
requests in order (a request without `Content-Length` has no body; review
finding 9). **D170** a program that is PID 1 ends on SIGTERM /
SIGINT; the site images build their own C from digest-pinned bases and run
as nobody (review findings 8 and 57). Review follow-ups: **D161** defined shifts, **D162** a
line starting `-x` is a new expression, **D163** mutating methods need a
writable receiver (and user method calls write back), **D164** `return` is
Never, **D165** HTTP host deadlines + connection cap + strict framing, **D166** a
panic in a `-->` task ends only that task (the server keeps serving), **D167**
implicit integer narrowing panics when the value does not fit
(`T(truncating: x)` keeps the low bits), **D168** resource constructors
are private to their module and resources live in module state
(`server.peerOf` for SSE), **D169** session `let` / `mut` initialize once and
keep their values (steps link against one base image per declaration set);
struct-field write-back use-after-free fixed; `Address<T>` parameters
compile; `tests/conformance/fail/`
expected-rejection tests; type / signature errors name their file;
`extras/shell` and the `examples/http` store work again; site Containerfiles
pass `-fwrapv`. Correctness pass from the 2026-10-14 project review
(committed): shadowing, method field collection write-back,
fixed-array bounds, string literal emission, **D157** wrapping Int64 + checked
`/` `%`, **D158** else-less `if` is Void, **D159** ASCII `\xHH`, **D160**
session panic/fault recovery + child signals; guarded task stacks, JSON depth
cap, Map tombstones, amortized append, `send` without SIGPIPE, EMFILE backoff,
LSP malformed messages; `cargo make test` also runs the Rust unit tests.
2026-09-04: Interactive REPL: emacs line editor + Tab complete
(**D156**); no startup banner; default prompt `§ ` (D96). **D155** resource
`drop()` on last RC (`File` / `Socket` /
`Db`). **D153** simple-RC **gate is done** (empty `lsan-skip` +
documented ASan). Alloc size wrap panics; `Socket.close` clears `fd`.
**Not Perceus.** Overwrite-drop, nested `Array`/`Map`
`HEAP_ELEMS`, heap params owned (**D154**), early-return final `drop_places`,
if/match result overwrite-drop. Language harvest **D147–D150** shipped.
**D151** LeakSanitizer is the default leak gate; **D152** match/init RC.
`workloads/landing` = thoxa.dev; `workloads/sola` = sola.computer;
`workloads/oath` = oath.wicket.cloud.
Crema Phase **E** through **E5**; **E6** WebSockets parked. Session compile
cache shipped. **`having`/`else` parked indefinitely.**

---

## Now

**Focus: the core language** (Joshua, 2026-10-16): compiler, type system,
codegen, runtime semantics. Site and hosting work is paused (see holds); the
2026-10-17 thoxa.dev rewrite was an explicit request, not a change of focus.
One language slice at a time, each with conformance.

**Typed codecs (D182) shipped**, freeze
[`docs/specs/2026-10-16-typed-codecs.md`](docs/specs/2026-10-16-typed-codecs.md):
`toJson` / `encode` on every encodable type (unions tagged by `"type"`); reads
report every issue as `Trial<T, Array<Issue>>`; `T.readForm` reads forms and
query strings into the same types; `{…}` / `[…]` literals where a `Json` is
expected, with `...` spreads; reads of unions, `Map` / `Json` fields, and
constructor defaults; a type's own `toJson` wins. Its non-goals (renaming,
generic types, top-level `Array` reads, custom decoding) stay parked.

**Conversions (D191) shipped**, freeze
[`docs/specs/2026-10-17-conversions.md`](docs/specs/2026-10-17-conversions.md):
§1 `x as T` with the built-in conversion table, §2 `x as? T`, §3 `from`
clauses (a `from(json: Json)` clause is the type's reader), §4 implicit
encoding where a `Json` is expected (and `match` takes its expected type), §5
typed body parameters in route clauses, §6 `T.from` / `T.fromJson` removed.
Also on the way: D192 (`return` / `=` convert to the expected type).

**Language gaps to pick from** (as built; see `docs/capabilities.md`):
unannotated multi-clause methods and methods of generic types are still Void
(D174); no `==` on Map; keys cannot hold Array fields (no array hash yet,
D179); `UInt64` arithmetic above `Int64` max panics (D21 / D167); `Int64` →
`Float64` rounds unchecked. Found 2026-10-17: in `where`, `lo..hi` includes `hi` (unlike a `for`
range) and the message prints `lo..hi` for `lo..=hi`; indexing a `Map`
through a field (`row.cells["name"]`) is an error (`.get` works); a method
cannot call a sibling method by bare name; `T.from` without `std/json` in the
program reports "cannot be read" instead of the missing import; a session
command inside a block is not recognized (use `shell.run`); `thoxa -c` with an `import` line runs nothing.

Session step compile cache shipped (D105 §3.5 / §11.1; base image **D169**);
release REPL ~**8 ms**/line. **D155** resource `drop` shipped. D153 simple-RC
gate is done. **Not Perceus.** Editor only when DX forces it. Crema Phase
**E** through **E5**; **E6 WebSockets stay parked.** Do **not** vendor HTMX.
Crema leftovers (product migrate, file-based routing) stay parked.

Freezes:
[`docs/specs/2026-08-05-session-libtcc.md`](docs/specs/2026-08-05-session-libtcc.md)
(compile cache);
[`docs/specs/2026-09-03-resource-drop.md`](docs/specs/2026-09-03-resource-drop.md)
(**D155**);
[`docs/specs/2026-09-02-simple-rc-correctness.md`](docs/specs/2026-09-02-simple-rc-correctness.md)
(**D153**).

**Shipped recently:** the 2026-10-16 language pass (D173–D181, above), each
with conformance and rejection gates; `cargo make test` 128 programs, 22
rejections, 274 unit tests; `cargo make test-asan` 118 clean. Before it:
session step compile cache (D105 §3.5): no per-line
scratch `.thx`; reuse prelude MIR; libtcc base image (in memory since **D169**) + ~500-line step C.
`cargo make shell` runs **release** `thoxa`. Session rc form split skips
`#` / `##` comments, so an apostrophe in `## not Thoxa's default` no longer
glues later `import` / `fn` into one statement (would fail `program requires
fn main`). Interactive TTY REPL is an emacs line editor with Tab complete
and history (**D156**). Interactive REPL starts with `§ ` (no banner).
POSIX login-shell face: `thoxa -c '<chunk>'` and
`thoxa -l` / `--login` (same REPL as bare `thoxa`); libtcc finds the
process libc via `/proc/self/maps` so NixOS/Oath session steps work
without `/usr/lib/libc.so`. **D155** `fn drop()` last-RC boxes (`file_drop`; File /
Socket / Db); `thoxa_alloc` size wrap + `Socket.close` clears `fd`
(`http_net_err`); bounded ASan pass (88 non-fiber clean on 2026-09-03, 109 on
2026-10-15; 9 `-->` programs excepted in `asan-skip`); if/match result overwrite-drop (`lsan-skip` empty);
early-return drops from the final `drop_places` set; extra-retain `jsx` /
`multi_clause` / `sqlite`; **D154** callee-owned heap params; nested Array/Map
lits; overwrite-drop; **D152** match/init RC.

**On master, not product:** none.

**Review follow-ups (open, from 2026-10-14):** Ctrl-C in a pure-Thoxa loop;
command-or-Thoxa guess (`x + 1`, `sleep 1`, `other.count`); redefining a
session `fn` is an error, not a replacement; no CI (R8, deferred). Paused with
the site work: the event loop's wake scan and blocking log writes (finding
14) and the copied site templates (finding 38). thoxa.dev runs `32ea2c2` (D189, D190; redeployed 2026-10-17); sola.computer and oath.wicket.cloud still run `e0c0bcf` (D170–D172, redeployed 2026-10-16).

### Proceed order

1. **Editor depth** — only when DX forces it. Freezes:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md)
   (recovery/nav residual);
   [`docs/specs/2026-08-09-structured-diagnostics.md`](docs/specs/2026-08-09-structured-diagnostics.md)
   (**D124** shipped — polish codes opportunistically).
2. **Deferred: remaining session / shell residual** — redir polish,
   Shell import-first, libtcc process-RT / license. Line editor shipped
   (**D156**). Compile cache + scratch `.thx` drop shipped. Freeze parked:
   [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
3. **Deferred megawork** — M4, Perceus, effects, named `ffi` root, cycle
   collector. Multi-logger (`initLog`) is trajectory under D125, not a
   CURRENT line until samples force it. Perceus is **not unparked**.
   Session/libtcc sanitizer stays parked with session (task stack guard
   pages shipped 2026-10-14). `drop` on unions / C-layout is out of D155 v0.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups; shrinking `lsan-skip` (LSan-clean a listed test, then
delete its line). Further **`std/` growth is sample-driven** (D97/D136).
Crema **domain** (pages, models, cookies policy) stays in `examples/http/`.
Public sites are **`workloads/landing`** (thoxa.dev),
**`workloads/sola`** (sola.computer), and **`workloads/oath`**
(oath.wicket.cloud) — not `examples/`, not `extras/`.

**Explicit holds:**

- **Site and hosting work** (paused 2026-10-16 at Joshua's request; the focus
  is the core language): `workloads/`, `std/http/server` tuning, deploys
- **`having` / `else` May sugar** (parked indefinitely — `match` is enough)
- **Crema domain in `std/`** (pages/models/layout — example-only)
- **Remaining session / shell residual** (redir polish, Shell import-first, libtcc process-RT / license — parked; line editor **D156** and compile cache shipped)
- Inventing shell dialect (`if`/`for`/full `$` forest)
- Tree-walk product interpreter or QBE path (phase A)
- Claiming full oracle SPEC coverage
- Windows / non-Linux session backends
- Treating “expand std/” as its own priority line
- **`"…" as Bytes`** or cast-as-encode for String→Bytes (use `Bytes.fromString`;
  D15 **rejected**)
- **Perceus** (deferred megawork — D153 gate done, not unparked)

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
| REPL step backend | **libtcc in-process only** (`TCC_OUTPUT_MEMORY`) → call `thoxa_session_step` (D105); no step `.so`. Decl-stable cache: re-lower `Shell.step` + link against the cached base image; module state in host slots, initialized once (**D169**). |
| Batch / RT C | System **`cc`** (+ `ar` for runtime) unchanged |
| Host platforms | **Linux** product track (x86_64 libtcc); Windows out of scope |
| Command sugar | Thoxa-first; flags like `ls -al` desugar; pure `1 - 2` stays Thoxa |
| `extras/shell` | Dogfood only; product face is bare `thoxa` |
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; Io/Net/Http/HttpStatic shipped (**D126**); Http helpers + **`std/http/server`** (**D136**); hosts under **`std/http/`** (**D139**); Str parse + remaining Http wrappers (**D137**); **`std/json`** (**D141**); **`std/sqlite`** (**D143**); fragments + ETag (**D145**); SSE takeover (**D146**); `ffi` deferred |
| Logging / print | **`std/logging` only** (**D125**); ambient **`println`/`print` removed**; app face `import logging, { info, … }` (**D127**); no ambient inject; multi-logger later |
| Diverging loop | Bare `loop` with no exiting `break` is **Never** (**D128**) — valid function tail without dummy value |
| Return inference | Unannotated free fns and methods: returns inferred from exits (**D129**, **D174**); explicit `->` authoritative; no Trial-by-name; recursion / bare Pass/Fail alone need annotate; `x?` then `Pass(value)` refines the success type; `drop` is Void; multi-clause and generic-type methods still annotate or default Void; hover uses inferred types |
| Match arms | Bare ctor = tag-only; `=>` single-expr body or `{ }` multi-stmt (**D130**); `->` types only |
| Multi-clause fns | Same name → one fn (match sugar); `name: Type` formals; any-type dispatch; source order; bare literal types (**D131** — **shipped**). No defaults on multi-clause v0; overlap/export mismatch are **errors**. D16 std factories still separate. |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132** — **shipped**). Receiver+code only; no lexical closures. Module store yes; fields/arrays no (D48 / D135). |
| Nowait | Token **`--> name(args)`** starts a named free Thoxa function (**D148**). `Void`; return dropped. Colorless: Net parks; `main` drains. No Future / `async` / `--> { }` / pointer `-->`. |
| Value collections | **D133 shipped** — Array/Map writes through `mut` places (Swift/Buffer COW). `let` aliases do not see later writes. Mutating methods return `Void`. An element of a `mut` Array is a place (`grid[0].append(x)`); a field from outside its type and a temporary are not, and a write on them is an error (**D180**). |
| Equality | Structural, no traits: `==` / `!=` compare products field by field, unions by arm and payload, arrays by count and elements (**D175**, **D177**, **D178**); never by RC pointer. No ordering on them; no `==` on Map. |
| Map | **D134 shipped** — builtin `Map<K, V>`; keys String/Int64/Bool; values include **Float64 (D140)** and User sums (`Json`, **D141**). `.has` / `May` get; `{k:v}` lits; `for k, v in m`. User products / unions are keys too (**D175** `==` + a generated hash, **D179**; not traits). `remove` keeps insertion order (**D181**). No `std/http` header flip. |
| Ctor fields | Int64, **Float64**, Bool, String, Bytes, Node, Array, Map, constructor-param User (including sums) (**D135** / **D140**). No C-layout, no fn types, no recursive User fields (use Array/Map). |
| HTTP request | `Request.query` kept (no leading `?`); path still stripped. `parseRequest` body empty; host `readBody` fills it (**D136**). Headers stay `Array<Header>`. `queryParam` / `formField` / `cookie` / `htmlResponse` (**D137**). `Cookie` / `withCookie` / `clearCookie` (**D142**); `htmlFragment` / `etagOf` / `cached` (**D145**). **`peerFd` + `streamed()`** (**D146**) for SSE takeover; the handler gets
its Socket from `server.peerOf(request)` (**D168**). Cookie and `Cache-Control` *policy* stay in the app. |
| SQLite | Amalgamation in `libthoxa_rt` (**D143**). Write `sql"… {value} …"` (**D144**); holes quote themselves. `exec`/`query` take `Sql`. No named `ffi` root. No product `thoxa migrate` — apps own schema files. |
| Combined import | `import ns, { a, b as c } from "…"` (**D127**) — namespace + selective in one clause |
| Module files | A `.thx` file is a **module** (scope), not a type (**D138**). Non-entry stems are kebab-case (`http.thx`, `math-utils.thx`). Types stay PascalCase; namespace bindings stay camelCase. Directory entry is `main.thx` (D68 / D138). Module ids for C mangling are path-qualified (`std/http/server` → `std_http_server`). HTTP hosts live under `std/http/` (**D139**). No user-facing `Module` type |
| Node / tag literals | Builtin **`Node`** (`Element \| Text \| Fragment`) + JSX tags (**D147**). No import. PascalCase is `Type(…).view(children:)`. `{e}` children/attrs stringify String / Int64 / Float64 / Bool; `{for}` collects `Array<Node>`. `.html()` is the markup codec — not generic `toString`. Comparison `<` needs a space (`a < b`). |
| Refinements / `Type.read` | `where` refinements are real types (**D149**). `T.from` → `Trial<T, Issue>`; `T.read` / `T.fromJson` → `Trial<T, Array<Issue>>` with every issue (**D182** §2; `Issue` in `std/json`). Literal unions <: String. Qty→base implicit. Tree `decode` stays D141. No cross-field `where`; write `May<Qty>`. |
| Route clauses | Type-body `GET "/…"` / `POST` / … on a nullary type → `route(req) -> Response` (**D150**). String wrap `ok`; miss `notFound()`; `Int64`/`String` captures. No JSON/form bind; no fallthrough. |
| String → Bytes | **`Bytes.fromString` only** — no cast sugar (D15 **rejected**) |
| Resource `drop` | Product `fn drop()` on last RC (**D155**). Heap box; copies share. `File` / `Socket` / `Db` close in `drop`. Explicit `close` stays inert. Only the defining module calls the constructor (**D168**). No unions / C-layout / multi-clause `drop`. Not Perceus. |
| Leak gate | GCC/Clang LeakSanitizer on batch `cc` (**D151**). `cargo make test` always sets `THOXA_SANITIZE=leak`. Match-bind / union-match / unused-global RC (**D152**). Heap params owned (**D154**). If/match result places overwrite-drop. `tests/conformance/lsan-skip` is empty (ratchet). Bounded ASan (**D153**, 2026-09-03): 88 non-fiber programs clean (109 on 2026-10-15); `cargo make test-asan` is optional, not the default (ucontext fibers in `asan-skip`). `thoxa_alloc` panics on size wrap; `Socket.close` clears `fd`. Resource `drop` **D155**. Not Perceus. Session/libtcc unsanitized. |
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
   + **D136** / **D137** / **D139** / **D142** / **D145** / **D146** / **D150** in [`docs/decisions.md`](docs/decisions.md)
    + Phase E protocol [`docs/ideas/2026-08-18-html-over-the-wire.md`](docs/ideas/2026-08-18-html-over-the-wire.md)
    + route freeze [`docs/specs/2026-08-23-route-clauses.md`](docs/specs/2026-08-23-route-clauses.md)
11. If JSON / `Type.read`: [`docs/decisions.md`](docs/decisions.md) (**D141** tree,
    **D149** refinements) + freeze
    [`docs/specs/2026-08-23-type-read.md`](docs/specs/2026-08-23-type-read.md)
    + **D140** (`Float64`)
12. If SQLite: [`docs/decisions.md`](docs/decisions.md) (**D143** / **D144**) —
    amalgamation in `vendor/sqlite`; `sql"… {value} …"`. Crema persist is
    example-only (`examples/http` `/tmp/crema.db`); no product migrate.
13. If RC / leaks / memory safety: freeze
    [`docs/specs/2026-09-02-simple-rc-correctness.md`](docs/specs/2026-09-02-simple-rc-correctness.md)
    + [`docs/decisions.md`](docs/decisions.md) (**D153** campaign gate done,
    **D151** LSan gate, **D152** match/init balance, **D155** resource `drop`)
    + freeze [`docs/specs/2026-09-03-resource-drop.md`](docs/specs/2026-09-03-resource-drop.md)
    + `tests/conformance/lsan-skip` + `tests/conformance/asan-skip`.

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo make test-asan
cargo test -p thoxa --lib
cargo make run examples/http
cargo make run workloads/landing
cargo make run workloads/sola
cargo make run workloads/oath
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
