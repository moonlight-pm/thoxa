# Capabilities — as-built progress

**Purpose:** Single engineering view of **what exists** vs **what remains** for
each product capability. Target design lives in freezes under
[`specs/`](specs/). Session priority lives in root
[`CURRENT.md`](../CURRENT.md). User product docs live in
[`handbook/`](handbook/) (shipped only).

**Update this file in the same change** whenever capability status or gaps
change. See [`progress-model.md`](progress-model.md).

**Status vocabulary:** `shipped` · `partial` · `spec’d` · `planned` · `idea`

**As of:** 2026-08-05

---

## Language kernel

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| driver-cgen | Compile-to-C driver, hello, C emit | shipped | `hello` | kernel | none for v0 | yes |
| lets-arith-if | `let`, arith, `if` | shipped | `arithmetic`, `lets_if` | R1 | — | yes |
| trial | `Trial` + `match` + `?` monomorphs | shipped | `trials`, `trials_mono` | D40–D45 | Nested May/Array payloads deferred | yes |
| may | `May` / `Has` / `Not` + `?` | shipped | `mays` | D85 | No May↔Trial mix; no inferred fallible returns | yes |
| value-match | String/Int64/Bool literal match + `else` | shipped | `match_value` | D87 | No bare binds / or-patterns / guards | yes |
| match-discard | `Pass(_)` / `Fail(_)` | shipped | `match_discard` | D66 | — | yes |
| user-types | User `type` + methods (scalars) | shipped | `types` | R3 | — | yes |
| string | String, interpolation, simple RC | shipped | `strings` | R4 | Perceus later | yes |
| string-eq | String `==` / `!=` | shipped | `router` (uses) | D60 | — | yes |
| bytes | Bytes API + slice + find→May | shipped | `bytes` | D14–D18, D36–D37, D46′/D85 | `"…" as Bytes` deferred | yes |
| buffer | Growable Buffer COW → freeze | shipped | `buffer` | D50–D52 | — | yes |
| mut-assign | `mut` + assign | shipped | `bytes` | R5 | Method mut-field write-back limited | partial |
| control-flow | while / loop / for / break / continue | shipped | `control_flow`, `loops` | D47, D55 | — | yes |
| early-return | `return` / `return expr` | shipped | `early_return` | D63 | — | yes |
| array-dyn | Dynamic `Array<T>` (incl. User) | shipped | `arrays`, `array_user`, `array_heap_elems` | D53–D55, D62, D65 | Fixed `Array<T,N>` still field-only | yes |
| user-rc | User+heap field RC in Trial/match/method | shipped | `user_rc` | D64 | — | yes |
| string-fields | String/Bytes/Array ctor fields | shipped | `string_fields` | D48–D49, D69 | — | yes |
| param-defaults | Formal defaults M3 subset | shipped | `param_defaults` | D98 | Methods/ctors still all-required; no `!`/`extern` defaults | yes |
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
| file-io | `readFile` + OpenMode lets + errno Fail | shipped | `file_io` | D56, D80–D82 | File type / method mut-field write-back deferred | yes |
| net-http | Net sockets + parse/write Request/Response | shipped | `http`, `http_wire`, `http_parse` | D69, R9 | Net errno strings polish deferred | yes |
| router | Router type `handle(Request) -> Response` | shipped | `router` | D70 | — | yes |
| http-server | Bounded multi-request static server | shipped | `http_server` | D67 | — | yes |

## Session / shell

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| session-script | No-main scripts → `Shell.step` + synthetic main | shipped | `session_script`, `session_step` | D88–D90, D94 | — | yes |
| command-sugar | Command skin + flag trees (`ls -al`) | shipped | `session_cmd`; `command::` | D92 | Shell control-flow sugar rejected | yes |
| session-repl | Bare `thoxa` REPL, decl accumulate, multiline | shipped | `repl::` | D88, D93, D95 | Crude line editor only | yes |
| session-dlopen | In-process step `.so` + preamble `.o` cache | shipped | `repl::` | D93 v2 | Host still owns some fast paths | yes |
| session-rc | `~/.config/thoxa/shell.thx` + `prompt()` | shipped | `repl::` | D96 | Piped REPL does not load rc | yes |
| session-path | `fn path(parent) -> Array<String>` | shipped | `repl::` | D100 | — | yes |
| session-pipeline | Thoxa-side `\|` → `Shell.pipeline` + RT | shipped | `session_pipe`; `command::` | D101 | Nested Array stages N/A (Bytes blob) | yes |
| session-redir | File `>` / `>>` / `<` → `Shell.runIo` | shipped | `session_redir`; `command::` | D102 | No `2>`; redir+pipe combo; ops need whitespace | yes |
| session-host-pipe | Host OS `\|` fast path in REPL | shipped (removed) | `repl::pure_step_*` | D103 | Replaced by Thoxa-side pipeline (D101) | yes |
| session-prelude | Shell API in product `std/Shell.thx` | shipped | `session::`; session scripts | D104 | Still auto-injected (not `import` by default); compile-time embed fallback | yes |
| session-host-builtins | Drop host `cd`/`status`/`\|` for pure step | shipped | `repl::pure_step_*` | D103 | All session commands via step .so | yes |
| session-tcc | Faster REPL compile via tcc | planned | — | session polish | Still system `cc` | no |
| extras-shell | Pure-Thoxa dogfood Shell | shipped | `cargo make run extras/shell` | D95 | **Not** product face | no |

## Standard library

| ID | Capability | Status | Gate | Spec / D-ids | Gaps | Handbook |
|----|------------|--------|------|--------------|------|----------|
| std-root | Named `std/…` resolution | shipped | `stdlib` | D97 | — | yes |
| std-str | Str helpers | partial | `stdlib` | D97 | Thin; grow as needed | yes |
| std-path | Path helpers | partial | `stdlib` | D97 | Thin | yes |
| std-ansi | 16 colors + hex SGR | shipped | `stdlib` | D99 | — | yes |
| std-env | getenv / host identity helpers | partial | `stdlib` | D97 | Thin | yes |
| std-git | branch helpers | partial | `stdlib` | D97 | Thin | yes |
| std-prompt | `defaultPrompt` | shipped | `stdlib` | D97, D99 | — | yes |
| std-io | Richer Io modules in `std/` | planned | — | CURRENT | Free `readFile` / iolib tests only | no |

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
