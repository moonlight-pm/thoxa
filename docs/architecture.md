# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Session priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-10-17 (`and` / `or` lower as `if`; array literals lower against the expected element type; `unwrap_refined` dups a heap base; a Void `let` binds the unit place; runtime `Float64` text found in scientific form, written in full between 1e-7 and 1e21; session desugar keeps keyword and block-opening lines as Thoxa; `cargo make test` adds the site samples step; thoxa.dev's image carries `docs/handbook` and the samples. 2026-10-16: structural `==` expanded in MIR; generated Map key ops and in-order compaction after `remove`; `main` exit status checked; the lexer follows interpolation holes. 2026-10-15: deadline-aware fd waits; checked shifts; `tests/conformance/fail/`. 2026-10-14: batch CLI on a large-stack thread; session steps run through `thoxa_session_call`; guarded task stacks; no `_Thread_local` in the runtime; 2026-09-04: session compile cache: re-lower `Shell.step` + libtcc base `.o`; **D156** session emacs line editor; **D155** resource `drop()`; **D153** gate done: empty `lsan-skip` + bounded ASan; **D152** match/init RC; **D151** LeakSanitizer gate; **D150** route clauses shipped; **D149** `where` / `Type.read` shipped; **D148** `-->` nowait shipped; **D147** JSX/`Node` shipped;
`workloads/landing` thoxa.dev live; `workloads/sola` sola.computer live;
`workloads/oath` oath.wicket.cloud live; static JPEG/font MIME;
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
| Frontend | `lexer`, `parser`, `ast`, `resolver`, `route_desugar`, `validate_desugar`, `refine` | Shared resolver used by LSP; parse soft-recover + `Error.recovered_module` for check multi-diag. Tag literals desugar in the parser to `__jsx_el` / `__jsx_child` / `{...}` spreads (**D147**). **`Type.read` / `where` / `T.from`** generate `__read_T` / `__fromJson_T` / `__from_T` (`validate_desugar.rs` + `refine.rs`, **D149**); every encodable product, union, refined, and literal type in a non-`std/` module gets `__toJson_T` / `__encode_T` once `std/json` is in the package, naming `std/json` items by their mangled names (**D182**). A generated reader reads each field into a `Trial<Field, Array<Issue>>`, gathers the issues, and builds the value only when there are none (arrays the same per element; **D182** §2). With `std/http` in the package a form-readable product also gets `__readForm_T` / `__fromForm_T` over `std/http`'s `FormPair`s, sharing the field walk and `__check_T_field` (**D182** §3). A union of readable products gets a `__fromJson_U` that picks the member by `"type"` and calls its reader; `Map<String, T>` / `Json` fields and constructor defaults are read in the same walk (**D182** §5). Synthesized AST is built with `ast_build.rs`. Type-body `GET "/…"` desugars in resolve + package lower (`route_desugar.rs`, **D150**). The lexer follows interpolation holes, so a `"` in a hole starts a nested string, kept as source for the hole's own parse (**D176**) |
| Diagnostics | `diagnostic.rs`, `error.rs` | Structured IR (D124): codes/help/labels → pretty CLI, JSON, LSP |
| MIR | `mir.rs` | RC (**D155** product `drop()` boxes; **D154** callee-owned heap params; **D153** overwrite-drop + if/match result overwrite-drop + early-return drops from the final `drop_places` set; **D152** match-bind one Dup; union/Node match result dropped); generic monomorphs (type applications gathered from every `let` annotation, nested blocks included); `std/fallible` via import (D118/D123); multi-error block lower; module globals not end-of-fn dropped (D122); **Never** for break-free `loop` (D128); **return-type inference** probe before body lower, free fns and methods in one callee-first worklist (**D129**, **D174**); tag-only bare union patterns (**D130**); structural `==` expanded inline over fields, union arms, and array elements, reading fields as borrows (**D175**, **D177**, **D178**); a user Map key passed by address (**D179**); a collection write on a field or temporary refused, on an array element written back (**D180**); route-clause bodies after desugar; `toJson` / `encode` on scalars, `Array`, `Map`, `May`, and `Json` lowered from synthesized AST with `std/json`'s names in scope (`mir/codec.rs`, **D182**); a map / array literal where a `Json` is expected lowered as a `Json` object / array, its values first, then built from them (`MapEntry::Spread` for `...`; **D182** §4); builtin **`Ty::Node`** (`Element` / `Text` / `Fragment` / `Attr`; `html_el` / `html_text` / `html_frag` / `html_render`); **`Ty::Fn`** unboxed pair (**D132**) |
| Cgen | `cgen.rs` | Per user Map key type: a hash, an equality, and a `ThoxaMapKeyOps` table, generated from the layouts (**D179**); topo typedef order for nested union monomorphs; pure-lit scalars; no dummy `return 0` after diverging body; D132 thunks + `ThoxaFn`; **D148** start thunks + `thoxa_sched_drain` after `main` / session step; module heap/User globals `__attribute__((used))` (**D152**); pointer locals NULL after Drop (**D153**); **D155** boxed `drop` types + `thoxa_set_dtor` |
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
| `workloads/` | Deployed Thoxa programs (`landing` = thoxa.dev, `sola` = sola.computer, `oath` = oath.wicket.cloud). `landing` renders `docs/handbook` at `/manual` (Markdown renderer + highlighter in Thoxa, per request) and shows `landing/samples` on `/`; those samples run in `cargo make test` (D189), the sites themselves are not the gate. Each image builds `thoxa` and emits the site's C in a digest-pinned `rust` stage, then runs it as UID 65534 from a digest-pinned `debian:bookworm-slim` |
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
  only `Shell.step`. `emit_session_base` + `libthoxa_rt.a` → one in-memory
  **base image** per decl set (**D169**); the host binds it to its slot table
  (`thoxa_session_bind`). Each chunk's `emit_session_step` compiles alone and
  links against the base image's symbols (`tcc_add_symbol`); module globals
  are host slots (`thoxa_session_slot` / `_ready` / `_mark_ready`, held by
  `SessionSlots`), reached in the base through `thoxa_slot_*` pointers and in
  the step through slot addresses baked into its C (tcc truncates a constant
  address used as an lvalue, so through a static pointer). Each initializer
  runs once per session. A base image that ran a step is kept for the session
  (values may point into it); one only compiled is dropped. No step `.so` /
  no session `cc` spawn. Optional debug C: `THOXA_REPL_KEEP_C=1`. Freeze:
  [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md)
  (D105). Soft perf (release, after first line): ~**8 ms/step** (freeze §8.2, 2026-10-16).
  The cache key is decls plus module file mtimes. Each step runs through the
  base image's runtime `thoxa_session_call` (sigsetjmp; panic → 134, fault →
  128 + signal; handlers and alt stack restored after the step, **D160**). The REPL and `-c` stay on
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
- `Map`: insertion-ordered slots plus an open-addressed index. Keys are i64
  bits, a String, or a user type through a generated `ThoxaMapKeyOps` (hash,
  equality, element retain / release); a stored user key is a refcounted box
  holding a copy, and a lookup passes the caller's address (**D179**).
  `remove` marks its slot removed and live slots are packed in order lazily,
  before reads by position (**D181**). Writes COW when shared (D134).
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
  closes the task's owned fds (indexed by fd; released
  by `thoxa_close` or a normal return), frees the packed arguments, marks the
  fiber done, and switches to the scheduler from the panic site, so the dead
  frames are never unwound (**D166**). Ownership records the Socket box
  (`thoxa_task_own_socket`; a box passes to `Address<T>` as its pointer), so
  the panic path also writes `fd = -1` into it (**D168**). Under LSan
  (`__lsan_register_root_region` present) that stack stays mapped as a root.
- Integer `/` and `%` call `thoxa_int_div` / `thoxa_int_rem` (panic on zero),
  `<<` / `>>` call `thoxa_int_shl` / `thoxa_int_shr` (**D161**);
  fixed-array subscripts go through `thoxa_fixed_index`; an implicit integer
  narrowing (MIR `NarrowChecked`) goes through `thoxa_narrow_signed` /
  `thoxa_narrow_unsigned`, and `T(truncating: x)` is a plain C cast
  (**D167**); programs build with
  `-fwrapv` (**D157**), the site Containerfiles too. `thoxa_panic` is
  `noreturn`.
- Generated batch `main` calls `thoxa_program_start()` first: as PID 1 it
  gives SIGTERM / SIGINT a handler that `_exit`s with 128 + signal, since the
  kernel drops default-action signals sent to a namespace's init (**D170**).
  It returns `thoxa_exit_status(main())` after the tasks drain: a status
  outside 0 to 255 panics instead of being cut to its low 8 bits (**D173**).
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
| Typed codecs | [`specs/2026-10-16-typed-codecs.md`](specs/2026-10-16-typed-codecs.md) (**D182**) |
