# Thoxa — agent handoff

**Read this first.** Then open only what the task needs. Do **not** re-walk the
whole tree, re-derive the design ladder, or re-read every compiler module at
session start.

Living focus: **[CURRENT.md](CURRENT.md)**  
As-built maturity: **[docs/capabilities.md](docs/capabilities.md)**  
How docs work: **[docs/progress-model.md](docs/progress-model.md)**

---

## What this is

Native Thoxa: **Rust compiler → C** + **C runtime** (`thoxa_rt`). Python oracle
lives at **`apocrypha/old/ThoxaPython`** (SPEC + conformance) — reference only,
not the product. A feature is done only when a checked-in `.thx` compiles, runs,
and matches `.out` (plus unit tests where noted).

## Session bootstrap

1. This file.
2. [`CURRENT.md`](CURRENT.md) — **Now** + locks.
3. [`docs/capabilities.md`](docs/capabilities.md) — rows for the slice.
4. One freeze under [`docs/specs/`](docs/specs/) if implementing that domain.
5. [`docs/decisions.md`](docs/decisions.md) only when reopening a locked surface.
6. User-facing surface: [`docs/handbook/`](docs/handbook/) (shipped only).

User phrases like **“next item” / “next rung”** mean: implement the **Now**
line in `CURRENT.md` (one slice), not redesign the world.

**Skills (Grok):** `.grok/skills/` —

- `thoxa-session-start` — boot order above  
- `thoxa-progress-docs` — **mandatory** end-of-slice doc updates  

## Progress documentation is first-class

Incomplete meta work means incomplete product work.

| Kind | Home | Role |
|------|------|------|
| Focus | Root `CURRENT.md` | Priority, next moves, locks |
| As-built progress | `docs/capabilities.md` | Capability status + gaps |
| As-built map | `docs/architecture.md` | Pipeline, crates, runtime |
| Target design | `docs/specs/*` | Freezes (desired shape) |
| Horizon | `docs/roadmap.md` | Phase-level program status |
| Product docs | `docs/handbook/` | **Shipped** user truth only |
| Decisions | `docs/decisions.md` | D-ids |

**End of every real product slice (same change as code):**

1. Update capability row(s) (status and/or gaps).  
2. Update `CURRENT.md` if priority changed.  
3. Update handbook if user-visible **shipped** behavior changed.  
4. Update `architecture.md` if the system map changed.  
5. Flip `roadmap.md` phase status only when phase-level status changes.  
6. Follow [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).

Do **not** invent `docs/status.md` / `STATUS.md` / `HANDOFF.md` / session
diaries. Authority order and rules:
[`docs/progress-model.md`](docs/progress-model.md).

## Commands

```bash
cargo make test              # conformance + LeakSanitizer + Rust unit tests (preferred day-to-day gate)
cargo make test-asan         # optional ASan on non-fiber conformance (not default)
cargo make run path.thx
cargo make shell             # product session REPL (release thoxa)
cargo make shell --login     # clean login-like env
cargo make emit-c path.thx
cargo make runtime           # libthoxa_rt.a + compile_commands.json
cargo make zed               # sync extras → local Zed
cargo test -p thoxa          # Rust unit tests
cargo run -p thoxa --                 # session REPL
cargo run -p thoxa -- path.thx        # program or session script
cargo run -p thoxa -- run path.thx
cargo run -p thoxa -- check path.thx
cargo run -p thoxa -- check --json path.thx
cargo run -p thoxa -- lsp
```

All build products under **`target/{rust,c,thoxa}/`**. Prefer `cargo make …`
over ad-hoc scripts (D10).

## Layout (where to edit)

| Path | Role |
|------|------|
| `crates/compiler/src/` | Lexer → parser → AST → MIR → cgen; `diagnostic`/`error`; session/command/repl; lsp/ |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` |
| `crates/make/` | xtask (`cargo make`) |
| `std/` | Product standard library (`std/…`; Io/Net/Http/HttpStatic **D126**; `http/server` **D136**/**D139**; `json` **D141**; `sqlite` **D143**/**D144**; fragments/ETag **D145**; SSE **D146**) |
| `tests/conformance/` | `*.thx` + `*.out` |
| `extras/` | Non-product DX; dogfood shell (product REPL is bare `thoxa`) |
| `examples/` | Runnable demos (not conformance); `examples/http`, `examples/kilo` |
| `containers/` | Deployed Thoxa programs; `landing` = thoxa.dev; `sola` = sola.computer; `oath` = oath.wicket.cloud |
| `vendor/` | Vendored upstream (TinyCC for libtcc session steps) |
| `CURRENT.md` | Living focus |
| `docs/` | Canonical docs (see `docs/README.md`) |

Compiler pipeline: `.thx` → lexer → parser → AST → MIR (+ simple RC) → cgen →
`cc` + `libthoxa_rt`. Map: [`docs/architecture.md`](docs/architecture.md).

## Working rules

- **One slice at a time.** Match oracle surface; don’t invent syntax without
  checking SPEC / decisions.
- **Conformance is the definition of done** for a feature.
- **No QBE / no tree-walk product interpreter** in phase A.
- **Heap today:** simple single-threaded RC; Perceus later.
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.
- **Full-word names.** No single-letter locals (`d`, `e`, `q`, `n`, `o`, `v`).
  Prefer `drink`, `error`, `query`, `count`, `order`, `value`. Abbreviations
  like `req`/`res` → `request`/`response`. Especially in `examples/http`.
- Do **not** commit unless the user asks.
- Never mix “is” and “will be” in docs without labels (progress model).

## Do not re-research unless needed

As-built inventory lives in **[`docs/capabilities.md`](docs/capabilities.md)**.
Locks and “what next” live in **[`CURRENT.md`](CURRENT.md)**. Decisions live in
**[`docs/decisions.md`](docs/decisions.md)**. Do not re-derive the R0–R9 ladder
or paste long “already known” essays into this file.

## Optional Grok memory

Committed files above beat experimental memory for “where are we.” If memory is
enabled, still treat **`CURRENT.md` + `docs/capabilities.md` + this file** as
source of truth after a reset.
