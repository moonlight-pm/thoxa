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

**Status: partial** (phase 1–2 shipped)

- Tree-sitter + Zed extras; `thoxa lsp` diagnostics.
- Navigation: hover, definition, references, outline, rename; cross-module jump.

**Remaining:** MIR on shared resolver; multi-diag depth; typed-member nav;
broader package intelligence.

Freeze: [`specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md)

---

## Phase C — Session language (shell / REPL)

**Status: partial** (core shipped; **polish parked**)

- No-main scripts, command sugar, bare `thoxa` REPL, **libtcc** session steps
  (D105), rc/prompt/path, pipes/redirs, pure-step builtins, `std/Shell.thx`.
- Thin `std/` for prompts and helpers (sample-driven only).

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

**Horizon / deferred until CURRENT says so:**

- Perceus / precise RC
- Sum types / richer ADTs (where oracle forces)
- Named `ffi` root
- Algebraic effects (only when forced)

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
