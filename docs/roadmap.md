# Roadmap

**This file is the program horizon** — coarse phase status over months.  
Day-to-day maturity of product surfaces lives in
[`capabilities.md`](capabilities.md). Priority lives in root
[`CURRENT.md`](../CURRENT.md). How the layers fit:
[`progress-model.md`](progress-model.md).

Status legend (phases):

| Status | Meaning |
|--------|---------|
| **done** | Good enough to build on; polish ok |
| **partial** | Scaffold or subset shipped; important gaps remain |
| **active** | Current focus (root `CURRENT.md`) |
| **next** | Queued after active |
| **planned** | Intended; not started |
| **unplanned** | Not scheduled; capture only |
| **removed** | Shipped, then taken out; the phase names the decision and the revision to recover it from |

Update phase status when a **phase-level** flip happens. Prefer updating
capability rows for feature-level progress.

---

## Phase A — Native kernel ladder

**Status: done**

- Compile-to-C pipeline, hybrid value model, simple RC.
- R0–R9 + post-R9 through bounded HTTP server, modules, extern, C-layout.
- Conformance suite under `tests/conformance/`.

**Remaining:** simple-RC **D153** gate is done (empty `lsan-skip`, bounded
ASan 2026-09-03). Resource `drop` **D155** shipped. Ladder itself stays
done; do not re-open R0–R9. Perceus stays horizon (not unparked). Since
2026-10-16 the focus is the core language (`CURRENT.md`): slices taken from
capability gaps (D173–D181 so far), not a new ladder.

Freeze: [`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md)  
RC campaign: [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md)  
HTTP plan (historical): [`specs/2026-07-19-post-r9-http-completion-plan.md`](specs/2026-07-19-post-r9-http-completion-plan.md)

---

## Phase B — Editor DX (Zed + LSP)

**Status: partial** (phase 1–2 + structured diagnostics shipped)

- Tree-sitter + Zed extras; `thoxa lsp` diagnostics (multi-diag on bodies,
  type-reg, lex, mid-block parse soft-recover; multi undefined names; check
  continues after recoverable syntax errors).
- **Structured diagnostics (D124):** shared IR → pretty `thoxa check`,
  `check --json` (compiler identity), LSP `code` / help / `relatedInformation`;
  codes THX001–THX010 (incl. unused-import **warning**).
- Navigation: hover (incl. **call-arg formals**), definition, references,
  outline, rename; cross-module jump (namespace + **typed members after `?`**);
  package-tree disk refs/rename; **`workspace/symbol`** package search.
- **`thoxa version`** / LSP identity (git hash; no toasts).

**Remaining:** MIR on shared resolver (**deferred architecture**); rare typed-member
untyped-expr edges; multi-root workspace folders if forced; mid-expression
parse recovery if forced; opportunistic code-catalog growth; agent-only extras
parked under [`ideas/2026-08-09-agent-dx-from-zerolang.md`](ideas/2026-08-09-agent-dx-from-zerolang.md).

Freezes:

- [`specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md)
  (editor / multi-diag recovery / nav)
- [`specs/2026-08-09-structured-diagnostics.md`](specs/2026-08-09-structured-diagnostics.md)
  (**D124** — shipped)

---

## Phase C — Session language (shell / REPL)

**Status: removed** (**D227**, 2026-10-18: "drop the shell feature entirely. This is scope creep.")

- What shipped here (no-main scripts, command sugar, the bare `thoxa` REPL,
  libtcc session steps, rc / prompt / path hooks, pipes and redirections,
  `std/shell.thx`, the line editor) is gone; recover it from `eefdac4`,
  along with its freezes (`docs/specs/2026-07-24-thoxa-session-repl.md`,
  `docs/specs/2026-08-05-session-libtcc.md`). A future shell would be a
  Thoxa program, not a compiler feature.
- The `std/` that grew alongside it stays: **Io/Net/Http/HttpStatic**
  (**D126**), **HttpServer** (**D136**; path **`std/http/server`** **D139**),
  Logging (**D125**), **Json** (**D141**), **Cookie** (**D142**),
  **Sqlite** (**D143**), **`sql"…"`** (**D144**), and the path / env / ansi /
  git helpers.

---

## Phase D — Deeper language / runtime

**Status: active** (see root `CURRENT.md`)

**Near-term (capability gaps, not megawork):**

- ~~Method / ctor parameter defaults~~ (**D106** shipped)
- ~~Method mut-field write-back~~ (**D107** shipped)
- ~~Value-match or-patterns~~ (**D108** shipped)
- ~~File type~~ (**D109** shipped)
- ~~Value-match bare binds~~ (**D110** shipped)
- ~~Match guards~~ (**D111** shipped)
- ~~File write/create~~ (**D112** shipped)
- ~~Nested Array/May Trial payloads~~ (**D113** shipped)
- ~~Nested Trial as payload~~ (**D114** shipped)
- ~~File seek/position~~ (**D115** shipped)
- ~~Default-arg defining-module scope~~ (**D116** shipped)
- ~~First-class fixed Array locals~~ (**D117** shipped)
- ~~Array/Bytes `insertAt`/`removeAt`~~ (**D55** splice shipped)
- ~~Program argv + CLI forward~~ (**D121** shipped; `examples/kilo`)
- ~~Module-global RC end-of-fn drop~~ (**D122** shipped; kilo empty-buffer fix)

- ~~Sum types + general fallibility (D118–D119)~~ — user unions, generics +
  methods, pure-lit scalar, **May/Trial as `std/Fallible` (import, D123)**, dead
  `Ty::May`/`Trial` removed (freeze
  [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md))
- ~~`std/Logging` + no ambient println~~ (**D125**)
- ~~Product `std/Io` + `Net` + `Http` + `HttpStatic`~~ (**D126**; sample-driven)
- ~~`Request.query` + HTTP helpers + `std/HttpServer`~~ (**D136**)
- ~~Str `parseUInt`/`splitOnce` + Http `queryParam`/`formField`/`cookie`/`htmlResponse`~~ (**D137**)
- ~~Http `Cookie` / `withCookie` / `clearCookie`~~ (**D142**)
- ~~Embedded SQLite + interpolated literals~~ (**D143**)
- ~~`sql"…"` tagged interpolator~~ (**D144**)
- ~~Combined import `import ns, { a } from "…"`~~ (**D127**)
- ~~Diverging `loop` (Never / no dummy tail)~~ (**D128**)

**Sample dogfood (not a phase flip):** `examples/http` Crema-lite **experiment**
— **initial charter A–D2 complete**; Phase **E** through **E5** (HTML-over-the-wire
cafe, not vendored HTMX; **E6** WebSockets parked; idea
[`ideas/2026-08-18-html-over-the-wire.md`](ideas/2026-08-18-html-over-the-wire.md)).
Host is `std/http/server.serve(routes.port, routes.handle)`
(**D132**/**D136**/**D139**); type-body `GET`/`POST` (**D150**); JSX `Node` pages (**D147**);
`CreateOrder.read` (**D149**);
`/tmp/crema.db` via **`std/sqlite`**; app/domain under `examples/http/`;
charter
[`ideas/2026-08-10-examples-http-crema-experiment.md`](ideas/2026-08-10-examples-http-crema-experiment.md);
accept loop uses **D128** + **`--> serveOne`** (**D148**) + keep-alive. `examples/kilo`
shipped and exercised; continue matrix gaps when samples force them.

~~**Traits (D228)**~~ (**done** 2026-10-19: twelve slices and the gaps
after them, D232–D240): declared conformance, traits with shared code,
properties, generic functions, `mixed` values of any conforming type,
associated types and `for` over user types, maps, and strings, conformances
of generic and built-in types and unions with members, methods with type
parameters, explicit type arguments, same-type constraints, writes through
`self`, defaults and clauses on members;
[`specs/2026-10-18-traits.md`](specs/2026-10-18-traits.md). What is left is
by design ([`capabilities.md`](capabilities.md), `traits` row).

**Horizon / deferred until CURRENT says so:**

- Perceus / precise RC (**D153** gate done; still deferred megawork, not unparked)
- Named `ffi` root
- Algebraic effects (only when forced; may later interact with `?`)

---

## Phase E — Product packaging / ecosystem

**Status: unplanned**

- A handbook site beyond the site's `/manual` (which renders `docs/handbook`
  publicly today, D189; at thoxa.moonlight.pm since D246)
- Broader editor ecosystem
- Distribution story beyond workspace `cargo make`

---

## Parked ideas (not phases)

See [`ideas/`](ideas/) — immutable values / structural sharing, webapp SSR
components. **`having` / `else` May sugar is parked indefinitely.** Promote
other sketches to a freeze + D-id before scheduling.
