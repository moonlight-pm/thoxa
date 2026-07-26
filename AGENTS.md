# Thoxa — agent handoff

**Read this first.** Then open only what the task needs. Do **not** re-walk the whole tree, re-derive the design ladder, or re-read every compiler module at session start.

Living progress: **[docs/status.md](docs/status.md)** ← update when a rung finishes or priorities change.

---

## What this is

Native Thoxa: **Rust compiler → C** + **C runtime** (`thoxa_rt`). Python oracle lives at **`apocrypha/old/ThoxaPython`** (SPEC + conformance) — reference only, not the product. Grow by the **example ladder** toward HTTP; a feature is done only when a checked-in `.thx` compiles, runs, and matches `.out`.

## Session bootstrap (cheap path)

1. Read **`docs/status.md`** (current rung, next work, open design notes).
2. If implementing a rung: skim the matching row in **`docs/specs/2026-07-18-thoxa-native-kernel-design.md`** §6 and **`docs/decisions.md`**.
3. Touch SPEC only when reopening surface: **`apocrypha/old/ThoxaPython/spec/SPEC.md`**.
4. Run **`cargo make test`** after substantive compiler/runtime changes.

User phrases like **“next item” / “next rung”** mean: implement the **Next** line in `docs/status.md` (one rung), not redesign the world.

## Commands

```bash
cargo make test              # conformance (preferred day-to-day gate)
cargo make run path.thx
cargo make shell             # product session REPL (bare `thoxa`)
cargo make shell --login     # same, clean login-like env (default-shell readiness)
cargo make emit-c path.thx
cargo make runtime           # libthoxa_rt.a + compile_commands.json
cargo make zed               # sync extras → local Zed (LSP binary + grammar + lang)
cargo make zed --grammar     # grammar WASM only
cargo make zed --all         # also rebuild extension.wasm
cargo test -p thoxa          # Rust unit tests
cargo run -p thoxa --                 # session REPL (D88)
cargo run -p thoxa -- path.thx        # program or session script
cargo run -p thoxa -- run path.thx
cargo run -p thoxa -- check path.thx   # typecheck, no cgen
cargo run -p thoxa -- lsp              # editor language server (stdio)
```

All build products under **`target/{rust,c,thoxa}/`**. Prefer `cargo make …` over ad-hoc scripts (D10).

## Layout (where to edit)

| Path | Role |
|------|------|
| `crates/compiler/src/` | Lexer → parser → AST → MIR → cgen; CLI in `main.rs`, pipeline in `driver.rs`; **`session`/`command`/`repl`**; **`lsp/`** |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` |
| `crates/make/` | xtask (`cargo make`) |
| `tests/conformance/` | `*.thx` + `*.out` (incl. `session_*`) |
| `extras/` | Non-product: `tree-sitter-thoxa` + `zed`; **`shell/`** Shell API dogfood (product REPL is bare `thoxa`) |
| `docs/status.md` | **Session progress** (edit when state changes) |
| `docs/decisions.md` | Locked / deferred decisions (D-ids) |
| `docs/specs/` | Full design docs |

Compiler pipeline (stable): `.thx` → lexer → parser → AST → MIR (+ simple RC) → cgen → `cc` + `libthoxa_rt`.

## Working rules

- **One rung at a time.** Match oracle surface; don’t invent syntax without checking SPEC / decisions.
- **Conformance is the definition of done** for a feature.
- **No QBE / no tree-walk product interpreter** in phase A.
- **Heap today:** `String`, `Bytes` — simple single-threaded RC (dup on copy, drop at fn end). Perceus later.
- After finishing a rung: mark ladder ✅ in the kernel design doc, update **`docs/status.md`**, add decision rows if you locked something new.
- Prefer small diffs; don’t drive-by-refactor unrelated rungs.
- Do **not** commit unless the user asks.

## Do not re-research unless needed

Already known (see status + decisions):

- R0–R9 + post-R9 HTTP path **complete** through bounded server (D67) + `Main.thx` dir entry (D68); also early `return` (D63), User+heap RC (D64), `Array<User>` (D65), match discard (D66), Request/Response (D69), Router type (D70), Io OpenMode/errno polish (D80–D82).
- **Session/REPL (D88–D96, D93 v2, D100):** bare `thoxa` REPL; no-main scripts → `Shell.step`; command sugar (incl. flags like `ls -al`, not Thoxa Sub); decls accumulate in REPL; **in-process dlopen** `thoxa_session_step` with cached preamble object (host `cd` + `\|` remain fast paths); interactive loads `~/.config/thoxa/shell.thx`; optional `fn prompt() -> String` / `fn path(…)` also via step .so (+ `THOXA_STATUS`). Plan: `docs/specs/2026-07-24-thoxa-session-repl.md`.
- **std/ (D97, D99):** `import { … } from "std/Str"` (also Path/Ansi/Env/Git/Prompt). Root: `THOXA_STD` / workspace `std/`. Ansi: 16 colors + `fgHex`/`parseRgba` (`#RRGGBB` or `#RRGGBBAA`); `defaultPrompt(signature: … = "#3B9DC2")`.
- **Param defaults (D98):** `name: T = expr` on non-`!` formals; omit at call (named or trailing positional).
- **Editor:** phase 1 + phase 2 complete (D71–D79): `check_source` (+ overlays), `thoxa lsp`, extras, resolver, hover/def/refs/outline/rename, cross-module jump (`./`/`~/`, open buffer wins). MIR lower not yet on shared resolver. **No Python** in product path.
- Extern: `extern fn … from "c"` [+ `"c_symbol"`]; marshal Int32/Int64/UInt32/UInt64/Byte/String/Bytes/Void/`Address<T>` (D19–D22, D26–D28, **D82** String *returns* via `thoxa_string_from_cstr`); cgen pulls fcntl/unistd/socket headers when externs present.
- C-layout: body fields, `T()` zero-init, field/index assign, fixed `Array<T,N>`, implicit Address on mut places (D23–D29).
- Modules: `import` selective/namespace, `export`, `./`+`~/` loader, module-level let/mut (D30–D35); **`thoxa run <dir>` → `<dir>/Main.thx`** (D68); `~/` = entry file’s directory.
- Bytes: `fromString`, `Bytes(n)`, `.slice`, `.find(needle[, from])` → `May<Int64>` (`Has(i)`/`Not`), count/get/set/toString; defer `"…" as Bytes` (D14–D18, D36–D37, D46′/D85).
- Buffer: `Buffer()`/`Buffer(cap)`, COW append/appendBytes, freeze→Bytes (D50–D52).
- Array: `Array<T>` (Int64/Bool/String/Bytes/**User**), literals, empty `[]` + ann, `.count`/`[i]`/append/dropLast, `for x in arr` (D53–D55, D62, **D65** inline User + `elem_size`); fixed `Array<T,N>` still field-only; `user.arrField[i]` for dynamic Array fields (D69).
- File I/O: `readFile(path) -> Trial<Bytes,String>` (D56); OpenMode Int32 lets `oRdonly`/`oWronly`/`oRdwr` (D81); Fail via `thoxa_errno` + libc `strerror` (D80). String `==`/`!=` (D60).
- HTTP: `Header`/`Request`/`Response`; `parseRequest`/`writeResponse`/`ok`/`notFound`/`badRequest` (D69); Router `handle(req) -> Response` (D70); **bounded server** (D67, `http_server`).
- Control flow: `while` / `loop` / `for` ranges + collection / `break` / `continue` / early `return` (D47, D55, D63). Constructor params: Int64/Bool/String/Bytes/**Array** (D48–D49, D69).
- Value model hybrid A; errors `May`/`Trial` + `?` / `match` — `Trial<Ok,Err>` monomorphs (Void/String/User/ints; D40–D45); **`May<T>` / `Has`/`Not` shipped (D85)** same payload set; `?` stays within family (no May↔Trial mix). **User+heap in Trial safe (D64)** — method field Dup, Trial extract/`Pass` via `needs_drop`, match bind fresh places. Match discard `Pass(_)`/`Fail(_)` (D66); nullary `Not` pattern. **Value match (D87):** String/Int64/Bool literal arms + required `else` (cascade of `==` / `string_eq`).

## Optional Grok memory

Committed files above beat experimental memory for “where are we.” If memory is enabled (`[memory] enabled` / `/memory`), still treat **`docs/status.md` + this file** as source of truth after a reset.
