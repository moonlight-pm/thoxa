# ThoxaNative — agent handoff

**Read this first.** Then open only what the task needs. Do **not** re-walk the whole tree, re-derive the design ladder, or re-read every compiler module at session start.

Living progress: **[docs/status.md](docs/status.md)** ← update when a rung finishes or priorities change.

---

## What this is

Native Thoxa: **Rust compiler → C** + **C runtime** (`thoxa_rt`). Python `../Thoxa` is the language **oracle** (SPEC + conformance), not the product. Grow by the **example ladder** toward HTTP; a feature is done only when a checked-in `.thx` compiles, runs, and matches `.out`.

## Session bootstrap (cheap path)

1. Read **`docs/status.md`** (current rung, next work, open design notes).
2. If implementing a rung: skim the matching row in **`docs/specs/2026-07-18-thoxa-native-kernel-design.md`** §6 and **`docs/decisions.md`**.
3. Touch SPEC only when reopening surface: **`../Thoxa/spec/SPEC.md`**.
4. Run **`cargo make test`** after substantive compiler/runtime changes.

User phrases like **“next item” / “next rung”** mean: implement the **Next** line in `docs/status.md` (one rung), not redesign the world.

## Commands

```bash
cargo make test              # conformance (preferred day-to-day gate)
cargo make run path.thx
cargo make emit-c path.thx
cargo make runtime           # libthoxa_rt.a + compile_commands.json
cargo test -p thoxa          # Rust unit tests
cargo run -p thoxa -- run path.thx
```

All build products under **`target/{rust,c,thoxa}/`**. Prefer `cargo make …` over ad-hoc scripts.

## Layout (where to edit)

| Path | Role |
|------|------|
| `crates/compiler/src/` | Lexer → parser → AST → MIR → cgen; CLI in `main.rs`, pipeline in `driver.rs` |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` |
| `crates/make/` | xtask (`cargo make`) |
| `tests/conformance/` | `*.thx` + `*.out` |
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

- R0–R9 done; R9+ Trial monomorphs + wire HTTP done. **Next = post-R9 pressure** (`Bytes.find`, Buffer, Array, loop, files — see `docs/status.md`).
- Extern: `extern fn … from "c"` [+ `"c_symbol"`]; marshal Int32/Int64/UInt32/UInt64/Byte/String/Bytes/Void/`Address<T>` (D19–D22, D26–D28).
- C-layout: body fields, `T()` zero-init, field/index assign, fixed `Array<T,N>`, implicit Address on mut places (D23–D29).
- Modules: `import` selective/namespace, `export`, `./`+`~/` loader, module-level let/mut (D30–D35).
- Bytes: `fromString`, `Bytes(n)`, `.slice`, `.find(needle[, from])` → index|-1, count/get/set/toString; defer `"…" as Bytes` (D14–D18, D36–D37, D46).
- Value model hybrid A; errors `May`/`Trial` + `?` / `match` — `Trial<Ok,Err>` monomorphs (Void/String/User/ints; D40–D45).

## Optional Grok memory

Committed files above beat experimental memory for “where are we.” If memory is enabled (`[memory] enabled` / `/memory`), still treat **`docs/status.md` + this file** as source of truth after a reset.
