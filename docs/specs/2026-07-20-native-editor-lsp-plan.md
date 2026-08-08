# Native editor support (Zed + `thoxa lsp`) — plan

**Date:** 2026-07-20  
**Status:** target (freeze)  
**Implementation:** partial  
**Gaps:** MIR lower on shared resolver; broader package intelligence; rename single-file; multi-file refs only among open buffers; typed-member incomplete for chains / no-hint receivers  


**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md) · handbook [`../handbook/editor.md`](../handbook/editor.md)

> **For agentic workers:** Implement **one PR at a time**. Gate: `cargo test -p thoxa` + `cargo make test`. Update `CURRENT.md` + `docs/capabilities.md` + `docs/decisions.md` (+ handbook when user-visible) when a PR finishes. **No Python.** Port ideas from the oracle under `apocrypha/old/ThoxaPython/editor/` and `…/thoxa/lsp/`; implement in **Rust** against the native frontend.

**Baseline (as-built):** Phase 1 (PR1–PR3) + phase 2 navigation (PR5–PR7, D76–D79) shipped. HTTP path + `Main.thx` complete. Pipeline: `.thx` → lexer → parser → AST → MIR (+ RC) → cgen → `cc` + `libthoxa_rt`.

**Goal:** Personal DX for editing `.thx` in **Zed**: thin Tree-sitter color + a real **`thoxa lsp`** that reuses the native compiler so diagnostics never drift. Phase 1 = useful (squiggles + color). Phase 2 = navigation (hover/def/refs/…) when we invest in a shared resolver.

**Oracle reference (read-only):**

| Oracle path | What to steal |
|-------------|----------------|
| `apocrypha/old/ThoxaPython/docs/specs/2026-07-12-thoxa-zed-editor-support-design.md` | Strategy A: LSP-first + thin grammar |
| `apocrypha/old/ThoxaPython/docs/specs/2026-07-13-thoxa-lsp-navigation-design.md` | Phase 2 navigation shape (resolver-first) |
| `apocrypha/old/ThoxaPython/editor/tree-sitter-thoxa/` | Lift grammar almost as-is |
| `apocrypha/old/ThoxaPython/editor/zed/` | Lift extension shell; retarget binary |
| `apocrypha/old/ThoxaPython/thoxa/lsp/` | Protocol + handler *shape* only — rewrite in Rust |

---

## 0. Current baseline (do not re-implement)

| Area | Status | Notes |
|------|--------|-------|
| Lexer / parser / MIR lower (type errors as `Error`) | ✅ | First-error-only; spans are **byte offsets** |
| `thoxa build\|run\|emit-c\|runtime` | ✅ | No `lsp` / `check` subcommands |
| Multi-file `./` + `~/` loader | ✅ | Disk-based; no open-buffer overlay |
| Conformance gate | ✅ | `cargo make test` |
| Tree-sitter / Zed / LSP | ❌ | Only under apocrypha (Python-era) |

**Known soft spots this plan addresses or documents:**

1. **CLI does not print spans** — `error: {message}` only; LSP needs span → line/col range.
2. **First error only** — acceptable for phase 1; multi-diag deferred.
3. **No parse recovery** — mid-edit → one lex/parse diagnostic (same as oracle).
4. **No resolver / binding table** — blocks phase 2 navigation until extracted.
5. **Lexer uses single `Ident(String)`** — casing is content-based (first char), not separate token kinds; semantic tokens must check casing themselves.
6. **Compiler crate has zero deps today** — LSP needs JSON; add a small dependency (see decisions).

---

## 1. Target (definition of done for this plan)

### Phase 1 — Useful DX (required)

A developer can:

1. Side-load a **dev** Zed extension from `extras/zed`.
2. Open any `.thx` and see **base highlighting** (keywords, types vs values by casing, strings, comments, numbers).
3. Run **`thoxa lsp`** (stdio) from the native binary.
4. Get **live diagnostics** (squiggles) on type/parse/lex errors from the **same** frontend as `thoxa build` (MIR lower / parse).
5. Optionally enable Zed `semantic_tokens = "combined"` and see function-vs-variable refinement from the LSP.

### Phase 2 — Navigation (optional follow-on; same plan, later PRs)

Hover, go-to-definition, references, document outline, rename — **only after** a shared lexical resolver exists. Cross-module jump/hover after that. See §7.

### Explicitly out of scope

| Item | Why |
|------|-----|
| Any Python code in the product path | User requirement; language has moved |
| Zed registry publish | Personal dev extension only |
| Thick tree-sitter grammar / full CST | Oracle strategy: thin lexer-level only |
| Completion, signature help, format-on-save | Deferred past phase 2 |
| VS Code / other editors | Same LSP can be reused later; not this plan |
| Multi-error recovery / cascading diagnostics | Phase 1 is first-error |
| Infinite-quality mid-edit multi-file typecheck | Open buffers: best-effort; see PRs |
| Kilo terminal editor (`examples/kilo`) | Unrelated |

---

## 2. Layout — where things live

### Decision: `extras/` for editor glue; LSP server inside the compiler

**`extras/`** = non-product, personal DX artifacts that are not on the compile-to-C critical path (grammars, editor extensions, probes). Not named `editor/` so it can grow (future `extras/vscode`, scripts, etc.) without implying the language runtime.

**LSP server code is not under `extras/`.** It must link the frontend (`lexer` / `parser` / `mir` / `loader`) and ship as **`thoxa lsp`**. Putting it in a detached `extras/lsp` crate that reimplements checking would reintroduce drift — the failure mode the oracle design avoided.

```text
Thoxa/
├── extras/                          # NEW — DX glue (not critical path)
│   ├── README.md                    # how to install Zed ext + rebuild grammar
│   ├── tree-sitter-thoxa/           # thin grammar (lifted + retargeted)
│   │   ├── grammar.js
│   │   ├── queries/highlights.scm
│   │   ├── src/                     # committed generated parser.c
│   │   └── test/corpus/
│   └── zed/                         # Zed dev extension
│       ├── extension.toml
│       ├── Cargo.toml               # wasm32-wasip* shim only
│       ├── src/lib.rs               # language_server_command → thoxa lsp
│       └── languages/thoxa/
│           ├── config.toml
│           └── highlights.scm
│
├── crates/compiler/                 # product
│   └── src/
│       ├── main.rs                  # + `lsp` (+ optional `check`) subcommand
│       ├── lib.rs                   # pub mod lsp;
│       ├── lsp/                     # NEW — Rust language server
│       │   ├── mod.rs               # run() entry
│       │   ├── protocol.rs          # Content-Length JSON-RPC stdio
│       │   ├── convert.rs           # Span/Error → LSP Range/Diagnostic; tokens encode
│       │   ├── analysis.rs          # check_buffer → diags; semantic_tokens from lexer
│       │   └── server.rs            # initialize, didOpen/Change/Close, handlers
│       ├── error.rs / span.rs       # span helpers for line/col (phase 1)
│       ├── lexer.rs / parser.rs
│       ├── mir.rs                   # lower_module / lower_package as check engine
│       └── loader.rs                # package load (disk); later virtual overlay
│
├── docs/specs/2026-07-20-native-editor-lsp-plan.md   # this file
└── apocrypha/old/ThoxaPython/...    # reference only — do not ship
```

**Why not `extras/lsp`?** The user asked for a logical home for “extra” stuff. Editor extensions and grammars are extras. The server is a **compiler feature** (`thoxa lsp`), same binary as `build`/`run`. If we ever need a second binary, prefer `crates/lsp` depending on `thoxa` lib — still not under `extras/`.

---

## 3. Architecture

```text
                    edit .thx in Zed
                           │
           ┌───────────────┴───────────────┐
           ▼                               ▼
  tree-sitter (extras/)              thoxa lsp (stdio)
  base highlights                    crates/compiler/src/lsp
  instant, offline                        │
                                   ┌──────┴──────┐
                                   ▼             ▼
                            parse / lower   lexer.tokenize
                            (diagnostics)   (semantic tokens)
                                   │
                                   ▼
                         same Error / Span as build
```

**Key asymmetry (from oracle, still true):** grammar is lower-value / more churn if overgrown; LSP is high-value / churn-proof if it *is* the compiler. Keep the grammar thin.

### 3.1 Check path (no codegen)

New library API used only by LSP (and optional `thoxa check`):

```text
check_source(path_hint, text) -> Vec<Diagnostic>
  1. lex text
  2. parse module
  3. if single-file / no imports: mir::lower_module
     else: best-effort package (phase 1: disk imports; see PR notes)
  4. map first Error → one Diagnostic { message, span }
  5. on Ok: empty diagnostics
```

- **Do not** run cgen or `cc`.
- **Do not** invent a second type checker. MIR lower *is* the checker for phase A.
- Full-document sync only (files are small). Debounce is editor-side; server may still recheck every `didChange`.

### 3.2 Spans

Native `Span { start, end }` is **UTF-8 byte offsets** into the buffer (good).

LSP needs 0-based `(line, character)` where character is UTF-16 code units for full LSP correctness; for ASCII-heavy `.thx` phase 1 may use UTF-8 byte index within line **or** Unicode scalar count — **lock UTF-16 for characters** if cheap (recommended), else document ASCII-only assumption for v1.

Add helpers on `span`/`convert`:

- `byte_to_line_col(src, byte) -> (line0, col0)`
- `span_to_range(src, Span) -> Range`
- Optionally improve CLI: `error: {path}:{line}:{col}: {message}` (nice; not required for LSP).

### 3.3 Transport

Hand-rolled Content-Length-framed JSON-RPC over stdin/stdout (oracle `protocol.py` ~58 lines).

- Prefer **`serde_json`** (+ `serde`) as workspace deps for the compiler crate — small, boring, correct. Zero-dep JSON for arbitrary LSP payloads is false economy.
- Log LSP internals to **stderr** only (stdout is the protocol).
- Every handler: catch panics / map errors → empty result + stderr log; never tear down the loop on a bad buffer.

### 3.4 Zed shim

`extras/zed/src/lib.rs`:

- `language_server_command` → `Command { command: thoxa_path, args: ["lsp"], env: shell_env }`
- Discovery order (native, not Python venv):
  1. `worktree.which("thoxa")` if on PATH
  2. else `{worktree}/target/rust/debug/thoxa` (dev default for this repo)
  3. else `{worktree}/target/rust/release/thoxa`
- Do **not** probe gitignored paths via `read_text_file` (oracle lesson with `.venv`).
- Grammar: `[grammars.thoxa] repository = "file://…/Thoxa"`, `path = "extras/tree-sitter-thoxa"`, pin `rev` to a commit that has generated `src/parser.c`.

### 3.5 Semantic tokens (phase 1)

Lexer-driven overlay (not AST). Emit only refinements; leave keywords/strings/comments to tree-sitter (`combined` mode):

| Pattern | LSP token type |
|---------|----------------|
| `Ident` starting with uppercase | `type` |
| `Ident` starting with lowercase + next token `LParen` | `function` |
| other lowercase `Ident` | `variable` |
| (optional) `Ident` before `Bang`+`Colon` if/when named args exist | `parameter` |

Native lexer today has no separate Upper/Lower kinds — **inspect the string**.

---

## 4. Key decisions (lock when implementing)

| ID | Decision | Choice | Rationale |
|----|----------|--------|-----------|
| E1 | Editor glue root | **`extras/`** with `tree-sitter-thoxa` + `zed` | Non-product DX; room to grow; not mixed into `crates/` |
| E2 | LSP location | **`crates/compiler/src/lsp`** + CLI **`thoxa lsp`** | Reuse frontend; one binary; no drift |
| E3 | No Python | Full Rust rewrite; oracle is reference only | Product is native |
| E4 | Strategy | LSP-first + thin tree-sitter | Oracle design still correct; Zed requires a grammar |
| E5 | Phase 1 features | Diagnostics + semantic tokens + grammar + Zed shell | High ROI; navigation is phase 2 |
| E6 | Check engine | `parse` + `mir::lower_*` (no cgen) | Lower already type-checks |
| E7 | Error model phase 1 | First diagnostic only (map `Error`) | Matches current compiler; multi-diag later |
| E8 | JSON | `serde` + `serde_json` on compiler crate | Hand-rolled JSON for LSP is not worth zero-deps |
| E9 | Grammar policy | Lexer-level only; no full CST | Churn isolation while language moves |
| E10 | Docs | Each PR updates status + decisions (D7x) when locking | Same as HTTP plan |

**Suggested decision-log IDs when PRs land:** D71 extras layout, D72 `thoxa lsp`, D73 check-without-cgen API, D74 serde_json, D75 thin grammar location, D76 semantic tokens legend.

---

## 5. Work inventory → PR mapping

| # | Item | PR | Phase |
|---|------|-----|-------|
| L1 | Span ↔ line/col helpers; improve `Error` presentation for tools | PR1 | 1 |
| L2 | `check_source` / analysis: buffer → diagnostics via lower | PR1 | 1 |
| L3 | LSP protocol + server loop + `thoxa lsp` | PR2 | 1 |
| L4 | Semantic tokens from lexer | PR2 | 1 |
| L5 | Unit tests for convert + analysis + protocol framing | PR1–PR2 | 1 |
| L6 | Lift `extras/tree-sitter-thoxa` + corpus | PR3 | 1 |
| L7 | Lift `extras/zed` + retarget shim to native `thoxa` | PR3 | 1 |
| L8 | `extras/README.md` install/smoke notes | PR3 | 1 |
| L9 | Optional `thoxa check <file>` CLI (disk, non-LSP) | PR2 or PR4 | 1 |
| L10 | Shared lexical resolver + binding table | PR5 | 2 |
| L11 | Hover / definition / references / outline / rename | PR6 | 2 |
| L12 | Cross-module + open-buffer overlay | PR7 ✅ | 2 |

---

## 6. Dependency graph (DAG)

```text
PR1 span + check_source + unit tests
  └── PR2 thoxa lsp (protocol, server, diags, semantic tokens)
        └── PR3 extras/ tree-sitter + zed (can start grammar in parallel with PR1 if careful)
              └── Phase 1 complete (manual Zed smoke)

PR5 resolver (phase 2; depends phase 1 stable)
  └── PR6 navigation handlers
        └── PR7 cross-module
```

**Levels:**

| Level | PRs | Notes |
|-------|-----|-------|
| 0 | PR1 | Compiler foundation; no Zed required to verify |
| 1 | PR2 | Fully testable without Zed (`stdio` unit/integration) |
| 1 (parallel) | PR3 grammar half | Tree-sitter only needs committed `parser.c`; independent of PR1 |
| 2 | PR3 zed wiring | Needs PR2 binary for end-to-end |
| 3+ | PR5–PR7 | Optional navigation track |

**Recommended execute order:** PR1 → PR2 → PR3 (phase 1 done). Stop unless user asks for phase 2.

---

## 7. Phase 2 sketch (do not implement until phase 1 is green)

Navigation needs a **position-queryable binding table**. Today name resolution lives inside MIR lower and is discarded. Do **not** build a second ad-hoc scope walker only for the LSP.

| Approach | Verdict |
|----------|---------|
| Extract `resolver` pass → both lower and LSP consume | **Preferred** (oracle model) |
| Side-table while lowering | OK prototype; risk of drift |
| Pure AST second walker | Reject for production |

Phase 2 features (oracle parity): hover (signature + `##` docs via source pre-scan), definition, declaration≡definition, references, documentSymbol, prepareRename/rename. Members only when receiver type is provable. Cross-module: lazy open of import path; open buffers win over disk.

Phase 2 is a **compiler architecture** investment (~2–4 weeks), not a thin LSP PR. Track under this plan’s PR5–PR7 when scheduled.

---

## 8. Global constraints (every implementer)

1. Read `AGENTS.md` + `CURRENT.md` + `docs/capabilities.md` first; this plan is active only when CURRENT points here.
2. **No Python** in product or `extras/`. Oracle is read-only reference.
3. One PR = focused diff; no drive-by MIR refactors outside the PR’s scope.
4. After compiler changes: `cargo test -p thoxa` and `cargo make test` green.
5. Do **not** commit unless the user asks.
6. Keep tree-sitter **thin** — no “full parse” grammar creep.
7. LSP stdout is protocol-only; logs on stderr.
8. Build products stay under `target/`; `cargo make` for day-to-day gates.
9. Prefer matching oracle *feature names* (semantic token types, capability flags) so mental model transfers; do not copy Python modules.

---

## 9. File map

| Path | Role |
|------|------|
| `crates/compiler/src/span.rs` | Byte↔line helpers |
| `crates/compiler/src/error.rs` | Optional richer display; `Diagnostic` type if introduced |
| `crates/compiler/src/lsp/*` | Server |
| `crates/compiler/src/main.rs` | `lsp` / optional `check` |
| `crates/compiler/src/lib.rs` | `pub mod lsp` |
| `crates/compiler/Cargo.toml` | `serde`, `serde_json` |
| `extras/tree-sitter-thoxa/*` | Grammar |
| `extras/zed/*` | Dev extension |
| `extras/README.md` | Install + smoke |
| `CURRENT.md`, `docs/capabilities.md`, `docs/decisions.md`, `AGENTS.md` | Progress |
| `crates/compiler` unit tests | `lsp` convert/protocol/analysis (no conformance `.thx` required for pure LSP wiring) |

Conformance `.thx` fixtures are **not** required for phase 1 LSP (language surface unchanged). Optional: a small integration test that runs `thoxa lsp` with a scripted initialize/didOpen exchange.

---

## PR Plan

### PR 1: Span mapping + `check_source` (compiler foundation)

- **Description:** Add byte-offset → LSP-ready line/col helpers. Introduce a small public check API that lexes/parses/lowers a source string (and optionally a path hint for error labels) and returns zero or one diagnostic. No stdio server yet. Unit tests with crafted good/bad snippets (including a known-good conformance file string).
- **Files:** `span.rs`, `error.rs` (maybe `Diagnostic` alias or keep `Error`), new `lsp/analysis.rs` or `check.rs` if preferred before full `lsp/` tree, `lib.rs`, unit tests under `crates/compiler/tests/` or `src/lsp/*_test` modules, `docs/*`
- **Dependencies:** None
- **Acceptance:**
  - [x] `span_to_range` correct on multi-line buffers (byte offsets)
  - [x] `check_source` on `fn main() -> Int64 { 0 }` → no diagnostics
  - [x] `check_source` on `fn main() -> Int64 { if 3 { } 0 }` → one diagnostic with non-empty range mentioning Bool/Int as today
  - [x] Lex/parse error yields one diagnostic with a span
  - [x] `cargo test -p thoxa` + `cargo make test` green
- **Implementation notes:**
  - Prefer `pub fn check_source(filename: &str, text: &str) -> Vec<LspDiagnostic>` even if len ≤ 1 for now (API ready for multi later).
  - Single-file: `parse` → `mir::lower_module`. For imports, phase 1 may fail with a clear diagnostic or fall back to disk `load_package` using `filename`’s parent as root — **document chosen behavior** in the PR summary; open-buffer overlay is phase 2.
  - Do not add serde yet if only analysis is in this PR.

### PR 2: `thoxa lsp` — protocol, diagnostics, semantic tokens

- **Description:** Hand-rolled stdio LSP. Capabilities: `textDocumentSync: Full`, `semanticTokensProvider` (full), publishDiagnostics on open/change, clear on close. CLI: `thoxa lsp`. Add `serde`/`serde_json`. Unit tests for framing + token encoding + initialize response shape. Optional: `thoxa check <path>` printing diagnostics for disk files (DX for non-Zed).
- **Files:** `crates/compiler/src/lsp/{mod,protocol,convert,analysis,server}.rs`, `main.rs`, `Cargo.toml`, tests, `docs/*`
- **Dependencies:** PR1
- **Acceptance:**
  - [x] `thoxa lsp` speaks initialize → initialized → does not exit
  - [x] didOpen with type error → `textDocument/publishDiagnostics` with one item
  - [x] Fix buffer via didChange → diagnostics empty
  - [x] `semanticTokens/full` returns relative-encoded data for a sample with `fn` + call
  - [x] Handler panics/errors never kill the loop (test or documented catch)
  - [x] `cargo test -p thoxa` + `cargo make test` green
- **Implementation notes:**
  - Mirror oracle method names / capability flags where possible.
  - Token legend: at least `type`, `function`, `variable` (add `parameter` only if named-arg surface exists).
  - Document in help text: `thoxa lsp` is for editors; not interactive.
  - Decision D72/D74 when locking. ✅

### PR 3: `extras/` — tree-sitter + Zed extension

- **Description:** Create `extras/`. Lift oracle grammar into `extras/tree-sitter-thoxa` (refresh corpus against current native samples if needed; regenerate `src/parser.c`). Lift Zed extension into `extras/zed`; retarget `extension.toml` paths/`rev`; rewrite shim discovery to native `thoxa` paths. `extras/README.md` with NixOS/dev install steps (Install Dev Extension → this directory). Manual smoke checklist in README.
- **Files:** `extras/**`, docs; no language semantics changes
- **Dependencies:** PR2 for full E2E; grammar portion can land after PR1
- **Acceptance:**
  - [x] Grammar builds / `tree-sitter test` if tooling available (or commit generated sources + document generate command)
  - [x] `extension.toml` points at `extras/tree-sitter-thoxa` in **this** repo
  - [x] Shim launches `thoxa lsp` (PATH or `target/rust/debug/thoxa`)
  - [x] README smoke: color + squiggle on a type error + clear on fix
  - [x] No Python paths remain in `extras/`
  - [x] `cargo make test` still green (untouched)
- **Implementation notes:**
  - Pin `rev` to the commit that adds generated grammar sources (chicken/egg: first land grammar + generated C, commit, then set `rev` to that SHA — or document “bump rev after first commit”). ✅ documented in `extras/README.md` + placeholder rev
  - Language config: `path_suffixes = ["thx"]`, line comments `#`, brackets, **do not** set unsupported keys that Zed rejects in extension config (oracle: put `semantic_tokens` in user `settings.json`).
  - Decision D71/D75. ✅

### PR 4 (optional polish): `thoxa check` + CLI span printing

- **Description:** If not done in PR2: `thoxa check <file|dir>` using `check_source` / package load; print `path:line:col: message`. Teach `main` error path to show spans when present.
- **Dependencies:** PR1
- **Acceptance:** Bad file exits non-zero with location; good file exits 0; conformance unaffected.

### PR 5–7 (phase 2)

| PR | Title | Depends | Done when |
|----|-------|---------|-----------|
| PR5 | Shared lexical resolver (+ index scaffold) | phase 1 | ✅ Resolver unit tests; lower behavior unchanged; D76. MIR still own env. |
| PR6 | Navigation LSP handlers | PR5 | ✅ Hover/def/refs/outline/rename tests; D77 |
| PR7 | Cross-module + buffer overlay | PR6 | ✅ Jump into `./`/`~/`; open buffer wins (D78–D79) |

---

## 10. Testing strategy

| Layer | How |
|-------|-----|
| Span/convert | Unit tests with multi-line strings |
| Analysis | In-memory snippets + maybe one real `tests/conformance/hello.thx` body |
| Protocol | Fake stdin/stdout or in-memory cursor frames |
| Semantic tokens | Snapshot relative encoding for a fixed snippet |
| Grammar | `tree-sitter test` corpus (when CLI available) |
| Zed | Manual smoke only (document in `extras/README.md`) |
| Regression | `cargo make test` every compiler PR |

No new conformance ladder rungs required for phase 1 (language surface unchanged).

---

## 11. Effort estimate (for scheduling)

| Slice | Estimate | Outcome |
|-------|----------|---------|
| PR1 | ~0.5–1 day | Check API + spans |
| PR2 | ~1–2 days | Working `thoxa lsp` |
| PR3 | ~0.5–1 day | Zed colors + launch |
| **Phase 1 total** | **~2–4 days** | Daily DX |
| Phase 2 (PR5–7) | ~2–4 weeks | Full nav parity |

---

## 12. Execute-session bootstrap

When starting the implementation session:

1. Read `AGENTS.md`, `CURRENT.md`, `docs/capabilities.md`, **this file**.
2. Confirm phase: default **phase 1 only** (PR1→PR2→PR3) unless user says “navigation” / “phase 2”.
3. Do not re-walk apocrypha beyond the reference table in §0–1 when stuck on wire format.
4. After each PR: update status Done/Last session; append decisions D71+ as locked; point **Next** at remaining PRs in this plan.
5. When phase 1 complete: Next can return to optional HTTP polish (PR9 Io) or phase 2 of this plan — ask user if ambiguous.

---

## 13. Smoke checklist (phase 1 complete)

1. `cargo build -p thoxa` → `target/rust/debug/thoxa`
2. `echo` / scripted LSP: initialize + didOpen bad snippet → diagnostics on stderr-driven client
3. Zed → Install Dev Extension → `extras/zed`
4. Open `tests/conformance/hello.thx` → colored
5. Break a type → squiggle; fix → clear
6. (Optional) user settings: `"languages": { "Thoxa": { "semantic_tokens": "combined" } }`
