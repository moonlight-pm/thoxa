# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Session priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-10-15 (deadline-aware fd waits; checked shifts; `tests/conformance/fail/`. 2026-10-14: batch CLI on a large-stack thread; session steps run through `thoxa_session_call`; guarded task stacks; no `_Thread_local` in the runtime; 2026-09-04: session compile cache: re-lower `Shell.step` + libtcc base `.o`; **D156** session emacs line editor; **D155** resource `drop()`; **D153** gate done: empty `lsan-skip` + bounded ASan; **D152** match/init RC; **D151** LeakSanitizer gate; **D150** route clauses shipped; **D149** `where` / `Type.read` shipped; **D148** `-->` nowait shipped; **D147** JSX/`Node` shipped;
`containers/landing` thoxa.dev live; `containers/sola` sola.computer live;
`containers/oath` oath.wicket.cloud live; static JPEG/font MIME;
**D146** SSE `peerFd` / `streamed()`;
**D145** `htmlFragment` / `cached`;
**D144** `sql"…"`;
**D143** `std/sqlite`;
**D142** `Cookie` / `withCookie`;
**D141** `std/json` `decode`/`encode`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case modules + `main.thx` entry;
**D137** Str parse + Http wrappers; **D136**
`std/http/server` + `Request.query`; **D135** nested User / Node ctor fields;
D133/D134 Map; D132 fn pointers; program `run`/`build` reuse stamped binaries)

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
| CLI / driver | `crates/compiler/src/main.rs`, `driver.rs` | `run` / `build` / `check` (`--json`) / `emit-c` / `lsp` / `version` / bare REPL / `-c` / `-l`. Program `run`/`build` stamp package+std+runtime+compiler and skip `cc` when fresh. `THOXA_SANITIZE=leak` / `address` (**D151**) |
| Frontend | `lexer`, `parser`, `ast`, `resolver`, `route_desugar`, `validate_desugar`, `refine` | Shared resolver used by LSP; parse soft-recover + `Error.recovered_module` for check multi-diag. Tag literals desugar in the parser to `__jsx_el` / `__jsx_child` / `{...}` spreads (**D147**). **`Type.read` / `where` / `T.from`** generate `__read_T` / `__fromJson_T` / `__from_T` (`validate_desugar.rs` + `refine.rs`, **D149**). Type-body `GET "/…"` desugars in resolve + package lower (`route_desugar.rs`, **D150**) |
| Diagnostics | `diagnostic.rs`, `error.rs` | Structured IR (D124): codes/help/labels → pretty CLI, JSON, LSP |
| MIR | `mir.rs` | RC (**D155** product `drop()` boxes; **D154** callee-owned heap params; **D153** overwrite-drop + if/match result overwrite-drop + early-return drops from the final `drop_places` set; **D152** match-bind one Dup; union/Node match result dropped); generic monomorphs; `std/fallible` via import (D118/D123); multi-error block lower; module globals not end-of-fn dropped (D122); **Never** for break-free `loop` (D128); **return-type inference** probe before body lower (**D129**); tag-only bare union patterns (**D130**); route-clause bodies after desugar; builtin **`Ty::Node`** (`Element` / `Text` / `Fragment` / `Attr`; `html_el` / `html_text` / `html_frag` / `html_render`); **`Ty::Fn`** unboxed pair (**D132**) |
| Cgen | `cgen.rs` | Topo typedef order for nested union monomorphs; pure-lit scalars; no dummy `return 0` after diverging body; D132 thunks + `ThoxaFn`; **D148** start thunks + `thoxa_sched_drain` after `main` / session step; module heap/User globals `__attribute__((used))` (**D152**); pointer locals NULL after Drop (**D153**); **D155** boxed `drop` types + `thoxa_set_dtor` |
| Session | `session.rs`, `command.rs`, `std/shell.thx` | Splice + command sugar + product Shell prelude (D104); Shell inject loads `std/fallible` + `std/logging` (+ deps) |
| Loader | `loader.rs` | `./` `~/` `std/…`; combined import maps (D127); `load_package_with_entry_ast`; `load_package_virtual` (REPL); inject deps |
| libtcc | `tcc.rs` + `build.rs` + `vendor/tinycc` | Vendored TinyCC; memory compile + object output for session base; `THOXA_TCCDIR` / `THOXA_C_INCLUDE` / `THOXA_TCC_LIBPATHS`; `-lc` via `/proc/self/maps` libc dir (NixOS/Oath have `libc.so.6`, not `/usr/lib/libc.so`). Build stamps `THOXA_GIT_HASH` / dirty |
| REPL host | `repl.rs`, `line_edit.rs` | Decl accumulate; libtcc step image only (D105); TTY emacs editor + Tab complete (**D156**) |
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
| `tests/conformance/` | `.thx` + `.out` gate; `fail/` holds programs that must be rejected or panic (`.err` lines must appear in stderr) |
| `examples/` | Runnable demos (`http` Crema-lite, `kilo` terminal editor); not the gate |
| `containers/` | Deployed Thoxa programs (`landing` = thoxa.dev, `sola` = sola.computer, `oath` = oath.wicket.cloud); not the gate |
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
│  - TTY emacs editor (D156) / cooked pipe│
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
- **REPL steps:** vendored **libtcc only** (`TCC_OUTPUT_MEMORY`). Decl-stable
  cache: virtual `repl.thx` load; reuse prepared package + MIR maps; re-lower
  only `Shell.step`; `emit_session_base` → one `.o`; each chunk
  `emit_session_step` + `add_file` that object + `libthoxa_rt.a`. Keep
  `TCCState` until superseded. No step `.so` / no session `cc` spawn. Optional
  debug C: `THOXA_REPL_KEEP_C=1`. Freeze:
  [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md)
  (D105). Soft perf (release, after first line): ~**6 ms/step** (freeze §8.2).
  The cache key is decls plus module file mtimes; the base `.o` is named
  `session_base_<pid>_<id>.o`. Each step runs through the image's runtime
  `thoxa_session_call` (sigsetjmp; panic → 134, fault → 128 + signal; handlers
  and alt stack restored after the step, **D160**). The REPL and `-c` stay on
  the process main thread: libtcc-compiled code resolves thread-local storage
  as the main thread's, so the runtime keeps no `_Thread_local` data.
- **Batch CLI** (`run` / `build` / `check` / `emit-c` / `lsp`) runs on a worker
  thread with a 256 MiB stack, so deep but legal ASTs do not overflow; the
  parser caps expression nesting at 1000 levels.

## Build products

Everything generated lives under one tree:

| Path | Role |
|------|------|
| `target/rust/` | Cargo (compiler + xtask binaries, deps) |
| `target/c/` | C runtime objects + `libthoxa_rt.a` |
| `target/c/tinycc/` | Installed libtcc + libtcc1 (from `build.rs`) |
| `target/thoxa/c/` | Generated C from Thoxa sources |
| `target/thoxa/bin/` | Linked Thoxa program binaries + sidecar `*.stamp` |
| `target/thoxa/bin-leak/` | LSan-linked programs (`THOXA_SANITIZE=leak`; **D151**) |
| `target/thoxa/bin-address/` | ASan-linked programs (`THOXA_SANITIZE=address`) |
| `target/thoxa/repl/` | Optional session keep-C dumps; no step `.so` on libtcc path |

Session may write `$XDG_CACHE_HOME/thoxa/repl/session_base_{id}.o` (process
lifetime; dropped with the cache) and keep-C `.c` when `THOXA_REPL_KEEP_C=1`.
No per-chunk `.thx`.

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

- Heap today: `String`, `Bytes`, dynamic `Array`, `Map`, `Buffer`, and
  `fn drop()` products (**D155**) — simple RC.
  Conformance leak gate: GCC/Clang LeakSanitizer (**D151**). Simple-RC
  correctness campaign (**D153**) gate done: empty `lsan-skip` and documented
  ASan (`cargo make test-asan`, non-fiber). `thoxa_alloc` panics if header
  plus payload wraps `size_t`. Product `fn drop()` boxes the type and runs
  on last RC (**D155**). Perceus not unparked. Freeze
  [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md)
  · [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md).
- Cooperative tasks (**D148**): `thoxa_task_start` /
  `thoxa_wait_ms` / `thoxa_wait_fd` / `thoxa_sched_drain` — one OS thread,
  stackful fibers; poll + timers. `std/net` accept/connect/recv/send go
  through `thoxa_*` and park on EAGAIN. Generated `main` drains after
  `thoxa_fn_main`. RC stays non-atomic. Task stacks: 512 KiB mmap'd with a
  PROT_NONE guard page, pooled for reuse; a SIGSEGV in the guard prints
  `stack overflow in task`. `accept` waits 50 ms on EMFILE / ENFILE; `send`
  passes `MSG_NOSIGNAL`. An fd wait may carry a deadline (`wake_at_ns` with
  `wait_fd`; the fiber wakes with `timed_out` set): `thoxa_recv_until` /
  `thoxa_send_until` fail with ETIMEDOUT at a `thoxa_now_ms` deadline
  (**D165**). A panic with a current task does not abort: `thoxa_panic`
  closes the task's owned fds (`thoxa_task_own_fd`, indexed by fd; released
  by `thoxa_close` or a normal return), frees the packed arguments, marks the
  fiber done, and switches to the scheduler from the panic site, so the dead
  frames are never unwound (**D166**). Under LSan
  (`__lsan_register_root_region` present) that stack stays mapped as a root.
- Integer `/` and `%` call `thoxa_int_div` / `thoxa_int_rem` (panic on zero),
  `<<` / `>>` call `thoxa_int_shl` / `thoxa_int_shr` (**D161**);
  fixed-array subscripts go through `thoxa_fixed_index`; programs build with
  `-fwrapv` (**D157**), the site Containerfiles too. `thoxa_panic` is
  `noreturn`.
- Extern from `"c"` (+ optional C symbol); cgen pulls headers when needed.
- Spawn helper `thoxa_spawn_wait` used by session Shell / dogfood shell.
  Children reset SIGPIPE / SIGINT / SIGQUIT to default; the parent ignores
  SIGINT / SIGQUIT while it waits (**D160**).
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
| Node / tag literals | [`specs/2026-08-22-jsx-node.md`](specs/2026-08-22-jsx-node.md) (**D147**) |
| Nowait `-->` | [`specs/2026-08-22-start-tasks.md`](specs/2026-08-22-start-tasks.md) (**D148**) |
| Refinements / `Type.read` | [`specs/2026-08-23-type-read.md`](specs/2026-08-23-type-read.md) (**D149**) |
| Route clauses | [`specs/2026-08-23-route-clauses.md`](specs/2026-08-23-route-clauses.md) (**D150**) |
| Simple-RC correctness | [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md) (**D153**) |
| Resource `drop` | [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md) (**D155**) |
