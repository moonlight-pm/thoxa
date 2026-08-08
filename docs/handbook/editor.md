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
| Diagnostics | From native check / overlays |
| Syntax color | Tree-sitter Thoxa grammar |
| Hover | Signature + contiguous `##` docs |
| Go to definition | Incl. some cross-module (`./`, `~/`) |
| Go-to-def / hover on methods | Linked when receiver has a type hint (annotation, `Type(...)` init, or **method-return chain** like `p.moved().x`) |
| Cross-module jump | `./`, `~/`, and **`std/…`** named roots; open buffer preferred over disk |
| References | Local file + **open buffers** that selective-import the export; import uses also list defining module |
| Rename | Local + **open buffers** selective-import renames when renaming an export |
| Document symbols | Outline |
| Open buffers | Overlay wins over disk for package check |

Library buffers without `main` typecheck as libraries (no false “requires main”).

## Limits (partial)

- Typed member navigation is incomplete.
- Cross-module jump for named roots (`std/`) is limited.
- Multi-diagnostic depth and MIR-on-shared-resolver are not full product yet.
- **No Python** in the product editor path.

## C runtime in editors (clangd)

Runtime headers live under `runtime/include/`. Generate
`compile_commands.json` with:

```bash
cargo make runtime
```

Then reload the window / restart clangd. Fallback: committed `.clangd` with
`-Iruntime/include`.
