# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Session priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-08-08 (sum types + May/Trial prelude)

---

## One sentence

Native Thoxa is a **Rust compiler that emits C**, linked with a **C runtime**
(`thoxa_rt`). Programs use system `cc`; interactive session steps compile via
**vendored libtcc** in-process (D105) and call `thoxa_session_step` without a
step `.so` on the happy path.

## Pipeline

```text
.thx → lexer → parser → AST → MIR (+ simple RC) → cgen
  → programs: cc + libthoxa_rt
  → session steps: libtcc (memory) + libthoxa_rt.a  (D105)
```

| Stage | Location | Notes |
|-------|----------|--------|
| CLI / driver | `crates/compiler/src/main.rs`, `driver.rs` | `run` / `build` / `check` / `emit-c` / `lsp` / bare REPL |
| Frontend | `lexer`, `parser`, `ast`, `resolver` | Shared resolver used by LSP |
| MIR | `mir.rs` | RC; generic monomorphs; ambient Fallible prelude inject (D118) |
| Cgen | `cgen.rs` | Topo typedef order for nested union monomorphs; pure-lit scalars |
| Session | `session.rs`, `command.rs`, `std/Shell.thx` | Splice + command sugar + product Shell prelude (D104) |
| libtcc | `tcc.rs` + `build.rs` + `third_party/tinycc` | Vendored TinyCC; memory compile |
| REPL host | `repl.rs` | Decl accumulate; libtcc step image only (D105) |
| LSP | `lsp/` | stdio server; check overlays; navigation |

## Crates and trees

| Path | Role |
|------|------|
| `crates/compiler` | `thoxa` binary + compiler library (+ libtcc link) |
| `crates/make` | xtask (`cargo make …`) |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` |
| `third_party/tinycc` | Vendored TinyCC sources (session steps only) |
| `std/` | Product std (`Shell.thx`, `Fallible.thx` surface, …) |
| `tests/conformance/` | `.thx` + `.out` gate |
| `extras/` | Non-product: tree-sitter, Zed extension, dogfood shell |
| `apocrypha/` | Historical + Python oracle |

**Ambient Fallible:** `Has` / `Not` / `May` / `Pass` / `Fail` / `Trial` are
injected into every package at lower time (source of truth also in
`std/Fallible.thx`). They lower as ordinary generic unions, not separate
`Ty::May` / `Ty::Trial` layouts.

## Session execution (as-built)

```text
┌─────────────────────────────────────────┐
│  thoxa (Rust host)                      │
│  - host-held status / Shell state       │
│  - read chunk, desugar, splice module   │
│  - cgen → libtcc memory → call step     │
│  - thoxa_session_step(ThoxaShellState*) │
│  - pure step only for commands (D103)   │
└─────────────────────────────────────────┘
```

- **Scripts (no `main`):** splice + synthetic `main` → system `cc` executable
  (unchanged).
- **Programs:** `fn main` or directory `Main.thx` — batch path unchanged.
- **Prelude:** product `std/Shell.thx` preferred at inject time; compile-time
  `include_str` fallback of the same file when std is not discoverable (D104).
- **REPL steps:** vendored **libtcc only** (`TCC_OUTPUT_MEMORY`); feed
  `emit_session_combined` + `libthoxa_rt.a`; keep `TCCState` until superseded.
  No step `.so` / no session `cc` spawn. Optional debug: `THOXA_REPL_KEEP_C=1`.
  Freeze: [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md)
  (D105). Soft perf (debug, tight piped loops): ~**12–14 ms/step** (see freeze
  §8.1 for historical vs-cc numbers).

## Build products

Everything generated lives under one tree:

| Path | Role |
|------|------|
| `target/rust/` | Cargo (compiler + xtask binaries, deps) |
| `target/c/` | C runtime objects + `libthoxa_rt.a` |
| `target/c/tinycc/` | Installed libtcc + libtcc1 (from `build.rs`) |
| `target/thoxa/c/` | Generated C from Thoxa sources |
| `target/thoxa/bin/` | Linked Thoxa program binaries |
| `target/thoxa/repl/` | Optional session keep-C dumps; no step `.so` on libtcc path |

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
- Spawn+IO `thoxa_spawn_wait_io` for session `>` / `>>` / `<` (`Shell.runIo`).
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
