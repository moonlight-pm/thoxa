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

**Status: partial** (core shipped; polish active)

- No-main scripts, command sugar, bare `thoxa` REPL, dlopen steps, rc/prompt/path.
- Thin `std/` for prompts and helpers.

**Remaining (active):** Thoxa-side pipes/redirs; prelude absorption; reduce host
fast paths; optional `tcc`; grow `std/`.

Freeze: [`specs/2026-07-24-thoxa-session-repl.md`](specs/2026-07-24-thoxa-session-repl.md)

---

## Phase D — Deeper language / runtime

**Status: planned**

- Perceus / precise RC
- Sum types / richer ADTs (where oracle forces)
- File type + method mut-field write-back
- Named `ffi` root
- Algebraic effects (only when forced)

**Do not start by default** — only when CURRENT promotes a slice.

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
