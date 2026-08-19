# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Session priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-08-19 (**D146** SSE `peerFd` / `streamed()`;
**D145** `htmlFragment` / `cached`;
**built-in validation** on master, not product; `Type.read`;
**D144** `sql"…"`;
**D143** `std/sqlite`;
**D142** `Cookie` / `withCookie`;
**D141** `std/json` `decode`/`encode`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case modules + `main.thx` entry;
**D137** Str parse + Http wrappers; **D136**
`std/http/server` + `Request.query`; **D135** nested User / Node ctor fields;
JSX `Node` is `Element | Text | Fragment` + `{...}` spreads on master;
D133/D134 Map; D132 fn pointers; route-clause + `-->` nowait on master, not
freeze; program `run`/`build` reuse stamped binaries)

---

## One sentence

Native Thoxa is a **Rust compiler that emits C**, linked with a **C runtime**
(`thoxa_rt`). Programs use system `cc`; interactive session steps compile via
**vendored libtcc** in-process (D105) and call `thoxa_session_step` without a
step `.so` on the happy path.

## Pipeline

```text
.thx → lexer → parser → AST → MIR (+ simple RC) → cgen
  → programs: cc + libthoxa_rt  (reuse bin when inputs unchanged)
  → session steps: libtcc (memory) + libthoxa_rt.a  (D105)
```

| Stage | Location | Notes |
|-------|----------|--------|
| CLI / driver | `crates/compiler/src/main.rs`, `driver.rs` | `run` / `build` / `check` (`--json`) / `emit-c` / `lsp` / `version` / bare REPL. Program `run`/`build` stamp package+std+runtime+compiler and skip `cc` when fresh. |
| Frontend | `lexer`, `parser`, `ast`, `resolver`, `route_desugar`, `validate_desugar`, `refine` | Shared resolver used by LSP; parse soft-recover + `Error.recovered_module` for check multi-diag. **Experiments:** type-body `GET "/…"` desugars in resolve + package lower (`route_desugar.rs`); tag literals desugar in the parser to `__jsx_el` / `__jsx_child` / `{...}` spreads; **`Type.read` / `where` / `T.from`** generate `__read_T` / `__fromJson_T` / `__from_T` (`validate_desugar.rs` + `refine.rs`) |
| Diagnostics | `diagnostic.rs`, `error.rs` | Structured IR (D124): codes/help/labels → pretty CLI, JSON, LSP |
| MIR | `mir.rs` | RC; generic monomorphs; `std/fallible` via import (D118/D123); multi-error block lower; module globals not end-of-fn dropped (D122); **Never** for break-free `loop` (D128); **return-type inference** probe before body lower (**D129**); tag-only bare union patterns (**D130**); route-clause bodies after desugar; builtin **`Ty::Node`** (`Element` / `Text` / `Fragment` / `Attr`; `html_el` / `html_text` / `html_frag` / `html_render`); **`Ty::Fn`** unboxed pair (**D132**) |
| Cgen | `cgen.rs` | Topo typedef order for nested union monomorphs; pure-lit scalars; no dummy `return 0` after diverging body; D132 thunks + `ThoxaFn` |
| Session | `session.rs`, `command.rs`, `std/shell.thx` | Splice + command sugar + product Shell prelude (D104); Shell inject loads `std/fallible` + `std/logging` (+ deps) |
| Loader | `loader.rs` | `./` `~/` `std/…`; combined import maps (D127); `load_package_with_entry_ast`; inject deps |
| libtcc | `tcc.rs` + `build.rs` + `vendor/tinycc` | Vendored TinyCC; memory compile; build stamps `THOXA_GIT_HASH` / dirty |
| REPL host | `repl.rs` | Decl accumulate; libtcc step image only (D105) |
| LSP | `lsp/` | stdio server; multi-diag + D124; nav; hover shows D129 inferred return types (cached on publish); call-arg formals; identity log |

## Crates and trees

| Path | Role |
|------|------|
| `crates/compiler` | `thoxa` binary + compiler library (+ libtcc link) |
| `crates/make` | xtask (`cargo make …`) |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` (incl. `ThoxaHtml` / `THOXA_TAG_HTML`; **`ThoxaFn`** D132; `thoxa_sql_*` D143) |
| `vendor/tinycc` | Vendored TinyCC sources (session steps only) |
| `vendor/sqlite` | Vendored SQLite amalgamation (D143; compiled into `libthoxa_rt`) |
| `std/` | Product std (`Shell`, `Fallible`, `Logging`, `Io`/`Net`/`Http` + `http/server`/`http/static`, `Json`, `Sqlite`, Str/Path/Env/…) |
| `tests/conformance/` | `.thx` + `.out` gate |
| `examples/` | Runnable demos (`http` Crema-lite, `kilo` terminal editor); not the gate |
| `extras/` | Non-product: tree-sitter, Zed extension, dogfood shell |
| `apocrypha/` | Historical + Python oracle |

**Fallible (`std/fallible`):** `Has` / `Not` / `May` / `Pass` / `Fail` /
`Trial` are ordinary exported generic unions. Programs **import** them (no
ambient inject for compiled modules). Session Shell inject still pulls
`std/fallible` into the package when the product Shell prelude is spliced
(Shell itself imports May/Has/Not). They lower as `Ty::User` monomorphs.
Builtin `Ty::May` / `Ty::Trial` variants and their cgen paths are gone.

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
- **Programs:** `fn main` or directory `main.thx` — batch path unchanged.
- **Shell prelude:** product `std/shell.thx` preferred at inject time;
  compile-time `include_str` fallback when std is not discoverable (D104).
  Shell imports May/Has/Not; inject also ensures `std/fallible` is in the
  package (D123). Fallible has its own embed fallback for the same case.
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
| `target/thoxa/bin/` | Linked Thoxa program binaries + sidecar `*.stamp` |
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

- Heap today: `String`, `Bytes`, dynamic `Array`, `Map`, `Buffer` — simple RC.
- Cooperative tasks (on master, not freeze): `thoxa_task_start` /
  `thoxa_wait_ms` / `thoxa_wait_fd` / `thoxa_sched_drain` — one OS thread,
  stackful fibers; poll + timers. `std/net` accept/connect/recv/send go
  through `thoxa_*` and park on EAGAIN. Generated `main` drains after
  `thoxa_fn_main`. RC stays non-atomic.
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
| Map / value collections | [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md) |
