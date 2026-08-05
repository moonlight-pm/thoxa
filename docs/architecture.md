# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Session priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-08-05

---

## One sentence

Native Thoxa is a **Rust compiler that emits C**, linked with a **C runtime**
(`thoxa_rt`). Programs and session steps share that pipeline; the interactive
shell is a long-lived Rust host that `dlopen`s per-chunk step libraries.

## Pipeline

```text
.thx → lexer → parser → AST → MIR (+ simple RC) → cgen → cc + libthoxa_rt
```

| Stage | Location | Notes |
|-------|----------|--------|
| CLI / driver | `crates/compiler/src/main.rs`, `driver.rs` | `run` / `build` / `check` / `emit-c` / `lsp` / bare REPL |
| Frontend | `lexer`, `parser`, `ast`, `resolver` | Shared resolver used by LSP |
| MIR | `mir.rs` | Simple single-threaded RC (dup/drop) |
| Cgen | `cgen.rs` | Emits C under `target/thoxa/c/` |
| Session | `session.rs`, `command.rs`, `session_prelude.thx` | Splice + command sugar + embedded Shell prelude |
| REPL host | `repl.rs` | Decl accumulate; `cc -shared` → dlopen step |
| LSP | `lsp/` | stdio server; check overlays; navigation |

## Crates and trees

| Path | Role |
|------|------|
| `crates/compiler` | `thoxa` binary + compiler library |
| `crates/make` | xtask (`cargo make …`) |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` |
| `std/` | Product standard library (`std/…` import root) |
| `tests/conformance/` | `.thx` + `.out` gate |
| `extras/` | Non-product: tree-sitter, Zed extension, dogfood shell |
| `apocrypha/` | Historical + Python oracle |

## Session execution (as-built)

```text
┌─────────────────────────────────────────┐
│  thoxa (Rust host)                      │
│  - host-held status / Shell state       │
│  - read chunk, desugar, splice module   │
│  - cgen → cc -shared → dlopen           │
│  - thoxa_session_step(ThoxaShellState*) │
│  - host fast paths: cd, |, status       │
└─────────────────────────────────────────┘
```

- **Scripts (no `main`):** splice + synthetic `main` → normal executable (no
  dlopen).
- **Programs:** `fn main` or directory `Main.thx` — batch path unchanged.
- **Prelude:** `session_prelude.thx` is still **compiler-embedded** (not yet a
  product `std` module).

## Build products

Everything generated lives under one tree:

| Path | Role |
|------|------|
| `target/rust/` | Cargo (compiler + xtask binaries, deps) |
| `target/c/` | C runtime objects + `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C from Thoxa sources |
| `target/thoxa/bin/` | Linked Thoxa program binaries |
| `target/thoxa/session/` (and related) | REPL step objects / shared libs (host-managed) |

Prefer `cargo make …` over ad-hoc scripts. `cargo make clean` removes `target/c`
and `target/thoxa`; `cargo make clean --all` also removes `target/rust`.

## Import roots (as-built)

| Form | Meaning |
|------|---------|
| `./…` | Relative to current module |
| `~/…` | Relative to **entry** file’s directory |
| `std/…` | Product std tree (`THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to binary) |
| `ffi/…` | **Deferred** |

## Runtime notes

- Heap today: `String`, `Bytes`, dynamic `Array`, `Buffer` — simple RC.
- Extern from `"c"` (+ optional C symbol); cgen pulls headers when needed.
- Spawn helper `thoxa_spawn_wait` used by session Shell / dogfood shell.
- Pipeline helper `thoxa_pipeline` (multi-stage pipe/fork/execvp; last-stage
  status) used by `Shell.pipeline` / command sugar `a | b`.
- Phase A: **no** product tree-walk interpreter; **no** QBE backend.

## Related freezes

| Topic | Spec |
|-------|------|
| Kernel / value model | [`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md) |
| HTTP path | [`specs/2026-07-19-post-r9-http-completion-plan.md`](specs/2026-07-19-post-r9-http-completion-plan.md) |
| Editor / LSP | [`specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md) |
| Session / REPL | [`specs/2026-07-24-thoxa-session-repl.md`](specs/2026-07-24-thoxa-session-repl.md) |
