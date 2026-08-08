# Editor (Zed + `thoxa lsp`)

Native editor support targets **Zed** with a thin Tree-sitter grammar and a
real **`thoxa lsp`** that reuses the compiler frontend (diagnostics should not
drift from `thoxa check`).

## Setup (workspace)

```bash
cargo make zed            # grammar + language files + thoxa for LSP
cargo make zed --grammar  # grammar WASM only
cargo make zed --all      # also rebuild extension WASM shim
```

See [`../../extras/README.md`](../../extras/README.md). After sync, restart the
language server (grammar color changes may need a full quit/restart).

## What works

| Feature | Notes |
|---------|--------|
| Diagnostics | From native check / overlays; multi-diag for body/type-reg errors, unexpected lex chars, and parse recovery (top-level items + mid-block statements) |
| Syntax color | Tree-sitter Thoxa grammar |
| Hover | Signature + contiguous `##` docs |
| Go to definition | Incl. some cross-module (`./`, `~/`) |
| Go-to-def / hover on methods | Linked when receiver has a type hint: annotation, ctor, method/fn return (generic head + type-param subst), copy-let, match payload, if/else, block, **match**, or **module-level let** inits |
| Cross-module jump | `./`, `~/`, and **`std/…`** named roots; open buffer preferred over disk |
| References | Local file + open buffers + **package-tree `.thx` on disk** (under nearest `Main.thx` / `.git` root; same-dir fallback) that selective-import the export |
| Rename | Local + open buffers + **package-tree disk** selective-import renames when renaming an export |
| Document symbols | Outline |
| Open buffers | Overlay wins over disk for package check |

Library buffers without `main` typecheck as libraries (no false “requires main”).

## Limits (partial)

- Typed member navigation still misses some **untyped** expressions (no recoverable type).
- Package-tree refs/rename are capped (nearest `Main.thx` / `.git` root; not an unbounded home walk).
- Parse recovery covers top-level items and mid-block statements (not full mid-expression recovery).
- MIR lower still uses its own env (shared-resolver migration is deferred architecture, not day-to-day polish).
- **No Python** in the product editor path.

## C runtime in editors (clangd)

Runtime headers live under `runtime/include/`. Generate
`compile_commands.json` with:

```bash
cargo make runtime
```

Then reload the window / restart clangd. Fallback: committed `.clangd` with
`-Iruntime/include`.
