# Roadmap

**This file is the program horizon** — coarse phase status over months.  
Day-to-day maturity of product surfaces lives in
[`capabilities.md`](capabilities.md). Session priority lives in root
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

Update phase status when a **phase-level** flip happens. Prefer updating
capability rows for feature-level progress.

---

## Phase A — Native kernel ladder

**Status: done**

- Compile-to-C pipeline, hybrid value model, simple RC.
- R0–R9 + post-R9 through bounded HTTP server, modules, extern, C-layout.
- Conformance suite under `tests/conformance/`.

**Remaining:** optional polish only; do not re-open as default work.

Freeze: [`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md)  
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

**Status: partial** (core shipped; **polish parked**)

- No-main scripts, command sugar, bare `thoxa` REPL, **libtcc** session steps
  (D105), rc/prompt/path, pipes/redirs, pure-step builtins, `std/Shell.thx`.
- Thin `std/` sample-driven: prompts/helpers plus **Io/Net/Http/HttpStatic**
  (**D126**) and Logging (**D125**).

**Parked residual:** redir polish (`2>`, redir-on-pipe); Shell import-first;
crude line editor. Resume only when CURRENT says so.

**Later polish (not active):** libtcc base-object cache; TinyCC license packaging
notice — libtcc freeze §11.

Freezes:

- [`specs/2026-07-24-thoxa-session-repl.md`](specs/2026-07-24-thoxa-session-repl.md)
  (session product)
- [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md)
  (step C backend — shipped)

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
- ~~Combined import `import ns, { a } from "…"`~~ (**D127**)
- ~~Diverging `loop` (Never / no dummy tail)~~ (**D128**)

**Sample dogfood (not a phase flip):** `examples/http` Crema-lite **experiment**
(`server.serve(routes.app())`; `framework/` + `app/` under `examples/http/`
only; no product `std/` growth for the experiment; charter
[`ideas/2026-08-10-examples-http-crema-experiment.md`](ideas/2026-08-10-examples-http-crema-experiment.md);
accept loop uses **D128**), `examples/kilo` shipped and exercised; continue
matrix gaps when samples force them.

**Horizon / deferred until CURRENT says so:**

- Perceus / precise RC
- Named `ffi` root
- Algebraic effects (only when forced; may later interact with `?`)

---

## Phase E — Product packaging / ecosystem

**Status: unplanned**

- Public handbook site beyond the repo
- Broader editor ecosystem
- Distribution story beyond workspace `cargo make`

---

## Parked ideas (not phases)

See [`ideas/`](ideas/) — having/else May sugar, immutable values / structural
sharing, webapp SSR components. Promote to a freeze + D-id before scheduling.
