# Capabilities — as-built progress

**Purpose:** Single engineering view of **what exists** vs **what remains** for
each product capability. Target design lives in freezes under
[`specs/`](specs/). Session priority lives in root
[`CURRENT.md`](../CURRENT.md). User product docs live in
[`handbook/`](handbook/) (shipped only).

**Update this file in the same change** whenever capability status or gaps
change. See [`progress-model.md`](progress-model.md).

**Status vocabulary:** `shipped` · `partial` · `spec’d` · `planned` · `idea`

**As of:** 2026-08-07

---

## Language kernel

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| driver-cgen | Compile-to-C driver, hello, C emit | shipped | `hello` | kernel | none for v0 | yes |
| lets-arith-if | `let`, arith, `if` | shipped | `arithmetic`, `lets_if` | R1 | — | yes |
| trial | `Trial` + `match` + `?` as prelude unions | shipped | `trials`, `trials_mono`, `trials_nested`, `trials_nested_trial`, `http_*` | D40–D45, D113–D114, D118 | Ambient `Pass`/`Fail`/`Trial` generics; cross-module residual rewrap | yes |
| may | `May` / `Has` / `Not` + `?` as prelude unions | shipped | `mays`, `trials_nested`, `bytes` (find) | D85, D113–D114, D118 | Ambient `Has`/`Not`/`May` generics; nested ok | yes |
| sum-types | User unions, literals, tagged layout, type `!`/`?` marks | shipped | `sums_*`, `sums_generic`, `sums_generic_methods` | D118–D119; [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md) | Exhaustive union match names missing arms; rejects arms after `else`. OpenMode sample via file-io | yes |
| value-match | Literals + or + bind/`else` + guards | shipped | `match_value`, `match_or`, `match_bind`, `match_guard` | D87, D108, D110, D111 | — | yes |
| match-discard | `Pass(_)` / `Fail(_)` | shipped | `match_discard` | D66 | — | yes |
| user-types | User `type` + methods (scalars) | shipped | `types` | R3 | — | yes |
| string | String, interpolation, simple RC | shipped | `strings` | R4 | Perceus later | yes |
| string-eq | String `==` / `!=` | shipped | `router` (uses) | D60 | — | yes |
| bytes | Bytes API + slice + find→May | shipped | `bytes` | D14–D18, D36–D37, D46′/D85 | String→Bytes is `fromString` only; **no** `"…" as Bytes` (D15 rejected) | yes |
| buffer | Growable Buffer COW → freeze | shipped | `buffer` | D50–D52 | — | yes |
| mut-assign | `mut` + assign + method field write-back | shipped | `bytes`, `method_field_writeback` | R5, D107 | — | yes |
| control-flow | while / loop / for / break / continue | shipped | `control_flow`, `loops` | D47, D55 | — | yes |
| early-return | `return` / `return expr` | shipped | `early_return` | D63 | — | yes |
| array-dyn | Dynamic `Array<T>` (incl. User) | shipped | `arrays`, `array_user`, `array_heap_elems` | D53–D55, D62, D65 | — | yes |
| array-fixed | Fixed `Array<T,N>` fields **and locals** | shipped | `cstruct_ffi`, `fixed_array_local` | D27, D117 | No whole-array assign/bind without lit; ABI scalar elems only | yes |
| user-rc | User+heap field RC in Trial/match/method | shipped | `user_rc` | D64 | — | yes |
| string-fields | String/Bytes/Array ctor fields | shipped | `string_fields` | D48–D49, D69 | — | yes |
| param-defaults | Formal defaults M3 (+ methods/ctors) | shipped | `param_defaults`, `defaults_mod` | D98, D106, D116 | No `!`/`extern` defaults; non-default cannot follow defaulted; free names in defaults use defining module | yes |
| modules | import/export, `./` `~/`, module let/mut | shipped | `modules`, `namespaces`, `modstate` | D30–D35 | Nested import/type in functions deferred (D91) | yes |
| pkg-main | Dir entry `Main.thx` | shipped | `pkgmain` | D68 | — | yes |

## FFI and C layout

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| extern-c | `extern fn … from "c"` [+ symbol] | shipped | `extern_libc` | D19–D22, D26–D28, D82 | String returns via `thoxa_string_from_cstr` | yes |
| c-layout | Body fields, `T()`, Address, fixed Array, bitwise | shipped | `cstruct_ffi` | D23–D29 | — | yes |
| ffi-root | Named `import … from "ffi"` | planned | — | D30 deferred | No product `ffi/` root | no |

## I/O and HTTP

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| file-io | `readFile` / `writeFile` / `appendFile` + OpenMode + **File** (seek/position) | shipped | `file_io`, `file_type`, `file_write`, `file_seek`, `file_openmode` | D56, D80–D82, D109, D112, D115 | OpenMode nullary union + `openModeBits`; Int32 flag lets for creat/trunc/append | yes |
| net-http | Net sockets + parse/write Request/Response | shipped | `http`, `http_wire`, `http_parse` | D69, R9 | Net errno strings polish deferred | yes |
| router | Router type `handle(Request) -> Response` | shipped | `router` | D70 | — | yes |
| http-server | Bounded multi-request static server | shipped | `http_server` | D67 | — | yes |

## Session / shell

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| session-script | No-main scripts → `Shell.step` + synthetic main | shipped | `session_script`, `session_step` | D88–D90, D94 | — | yes |
| command-sugar | Command skin + flag trees (`ls -al`) | shipped | `session_cmd`; `command::` | D92 | Shell control-flow sugar rejected | yes |
| session-repl | Bare `thoxa` REPL, decl accumulate, multiline | shipped | `repl::` | D88, D93, D95 | Crude line editor only | yes |
| session-dlopen | Legacy step `.so` + `cc -shared` | removed | — | D93 v2; D105 | Superseded by libtcc-only steps (escape hatch deleted) | no |
| session-rc | `~/.config/thoxa/shell.thx` + `prompt()` | shipped | `repl::` | D96 | Piped REPL does not load rc | yes |
| session-path | `fn path(parent) -> Array<String>` | shipped | `repl::` | D100 | — | yes |
| session-pipeline | Thoxa-side `\|` → `Shell.pipeline` + RT | shipped | `session_pipe`; `command::` | D101 | Nested Array stages N/A (Bytes blob) | yes |
| session-redir | File `>` / `>>` / `<` → `Shell.runIo` | shipped | `session_redir`; `command::` | D102 | No `2>`; redir+pipe combo; ops need whitespace | yes |
| session-host-pipe | Host OS `\|` fast path in REPL | shipped (removed) | `repl::pure_step_*` | D103 | Replaced by Thoxa-side pipeline (D101) | yes |
| session-prelude | Shell API in product `std/Shell.thx` | shipped | `session::`; session scripts | D104 | Still auto-injected (not `import` by default); compile-time embed fallback | yes |
| session-host-builtins | Drop host `cd`/`status`/`\|` for pure step | shipped | `repl::pure_step_*` | D103 | All session commands via step image (libtcc) | yes |
| session-libtcc | In-process libtcc for REPL steps (no spawn / no step `.so`) | shipped | `tcc::`; `repl::pure_step_*` | D105; [`specs/2026-08-05-session-libtcc.md`](specs/2026-08-05-session-libtcc.md) | Linux x86_64 only; later polish (base cache / license notice) not CURRENT | no |
| extras-shell | Pure-Thoxa dogfood Shell | shipped | `cargo make run extras/shell` | D95 | **Not** product face | no |

## Standard library

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| std-root | Named `std/…` resolution | shipped | `stdlib` | D97 | — | yes |
| std-str | Str helpers | partial | `stdlib` | D97 | Thin; extend only when a sample needs it | yes |
| std-path | Path helpers | partial | `stdlib` | D97 | Thin; sample-driven | yes |
| std-ansi | 16 colors + hex SGR | shipped | `stdlib` | D99 | — | yes |
| std-env | getenv / host identity helpers | partial | `stdlib` | D97 | Thin; sample-driven | yes |
| std-git | branch helpers | partial | `stdlib` | D97 | Thin; sample-driven | yes |
| std-prompt | `defaultPrompt` | shipped | `stdlib` | D97, D99 | — | yes |
| std-io | Richer Io in `std/` | idea | — | sample-driven | Free `readFile` / iolib tests only; not a CURRENT todo | no |

## Editor / LSP

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| check-source | Typecheck API + overlays | shipped | unit / LSP | D71–D79 | — | yes |
| lsp-diag | `thoxa lsp` diagnostics | shipped | manual + unit | phase 1 | Multi-diag collection limited | yes |
| lsp-nav | hover / def / refs / outline / rename | partial | unit | D77–D79 | Typed-member nav incomplete; refs/rename single-file | partial |
| lsp-xmodule | Cross-module jump (`./` `~/`) | partial | unit | D78 | Named roots jump polish; open buffer wins | partial |
| tree-sitter-zed | Grammar + Zed extension extras | shipped | `cargo make zed` | editor plan | Not full IDE | yes |
| mir-on-resolver | MIR lower on shared resolver | planned | — | editor plan | Not started | no |

## Runtime / heap

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| simple-rc | Single-threaded RC String/Bytes/Array | partial | strings/bytes/arrays | kernel D4-ish | Perceus precision later | partial |
| spawn-wait | `thoxa_spawn_wait` for session | shipped | session / extras | D86 | Not a general language spawn surface | no |
| perceus | Precise RC / Perceus | idea | — | kernel | Deferred megawork | no |
| effects | Algebraic effects | idea | — | kernel | Out until forced | no |

## Meta / docs

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| progress-docs | CURRENT + capabilities + handbook model | shipped | this tree | progress-model | Generators / CI truth deferred | n/a |
| handbook | User manual shipped-only | partial | handbook pages | progress-model | Grow with surfaces; keep honest | yes |

---

## Historical ladder (R0–R9+)

The example ladder through bounded HTTP is **complete**. Track day-to-day work
in the matrices above, not by re-checking R0–R9.

| Rung | Summary | Conformance |
|------|---------|-------------|
| R0 | hello / driver / C emit | `hello` |
| R1 | let, arith, if | `arithmetic`, `lets_if` |
| R2 | Trial, match, `?` | `trials` |
| R3 | user `type` + methods | `types` |
| R4 | String, interpolation, RC | `strings` |
| R5 | mut + Bytes | `bytes` |
| R6 | extern from `"c"` | `extern_libc` |
| R7 | C-layout / Address / fixed Array | `cstruct_ffi` |
| R8 | modules | `modules`, `namespaces`, `modstate` |
| R9 | HTTP subset | `http` |
| R9+ | monomorphs, control flow, Buffer, Array, Io, Router, server, May, Main.thx, … | see matrix |

Kernel freeze ladder table:
[`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md) §6.
