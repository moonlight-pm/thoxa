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

## Which `thoxa` is Zed using?

Zed launches **`thoxa` on PATH** (after `cargo make zed`, usually
`~/.local/bin/thoxa` → `target/rust/debug/thoxa`).

1. Rebuild: `cargo build -p thoxa` or `cargo make zed`
2. In Zed: **`editor: restart language server`**
3. Confirm identity:
   - Shell: `thoxa version` → `thoxa 0.1.0+<hash>[.dirty]  path=…`
   - Zed: **`zed: open language server logs`** — look for  
     `thoxa-lsp: thoxa 0.1.0+<hash>  path=…`  
     (stderr on start + `window/logMessage` after initialize; **no toast**)

If the hash in logs does not match `git rev-parse --short=12 HEAD` (or
`thoxa version` after a rebuild), the running LSP process is still old —
restart again.

## What works

| Feature | Notes |
|---------|--------|
| Diagnostics | Same engine as `thoxa check` (**D124**). **Multi-diag** + **structured codes** (`THX…`), help text (folded into the LSP message), and `relatedInformation` when labels exist. **Unused imports** are **warnings** (`THX010`, severity Warning). Parse recovery still typechecks the rest of a function after a bad token. Programs with `fn main` reject bare top-level statements with a clear span. Mid-expression recovery is still limited. |
| Syntax color | Tree-sitter Thoxa grammar. JSX island paints tags/attrs; `<>` fragments are best-effort. `sql"…"` injects Zed's SQL grammar into the body; `{holes}` stay Thoxa. `-->` is a keyword (nowait, **D148**). Compiler `where` is a keyword (**D149**); Tree-sitter may still paint it as an ident. Type-body `GET`/`POST`/… are keywords (**D150**) |
| Hover | Signature + contiguous `##` docs. Unannotated free **`fn`** show **typecheck return type** (**D129** inference, e.g. `Trial<Void, String>`, `Int64`); unannotated methods/`extern` still default **`-> Void`** unless annotated; **`main` → `Int64`**; fallback same if lower failed. **`let` / `mut`** show `let a: Type` when nav has a type hint (annotation or inferred init). **Call arguments** show the formal (`backlog: Int64` on `listen(16)`) when the callee is resolvable (local or cross-module). Builtin types include **`Node`** / `Element` / `Text` / `Fragment` / `Attr` (**D147**); `.html()` itself has no dedicated hover (MIR builtin, same as `String.count`) |
| Go to definition | Incl. some cross-module (`./`, `~/`) |
| Go-to-def / hover on methods | Linked when receiver has a type hint: annotation, ctor, method/fn return (generic head + type-param subst), copy-let, match payload, if/else, block, **match**, **module-level let** inits, or **`?` on Trial/May** (success type) |
| Cross-module jump | `./`, `~/`, and **`std/…`** named roots; open buffer preferred over disk. Namespace free fns (`net.createSocket`) and **typed methods on values from those calls after `?`** (`server.reuseAddr` → `Socket.reuseAddr` in `Net.thx`) |
| References | Local file + open buffers + **package-tree `.thx` on disk** (under nearest `main.thx` / `.git` root; same-dir fallback) that selective-import the export |
| Rename | Local + open buffers + **package-tree disk** selective-import renames when renaming an export |
| Document symbols | Outline |
| Workspace symbols | `workspace/symbol` — search open buffers + package-tree `.thx` (same root walk as multi-file refs) |
| Open buffers | Overlay wins over disk for package check |

Library buffers without `main` typecheck as libraries (no false “requires main”).
Nested files in a directory package resolve `~/…` from the nearest `main.thx`
(same as `thoxa run examples/http`), not from the open file’s directory.

## Limits (partial)

- Typed member navigation still misses some **untyped** expressions (no recoverable type).
- Call-arg hover needs a resolvable callee (local or import path on disk / open buffer).
- Package-tree refs/rename and **workspace symbols** use the same root walk
  (nearest `main.thx` / `.git`; capped — not an unbounded home walk / multi-root).
- Parse recovery is **statement-oriented** (top-level items + mid-block); not full mid-expression recovery.
- MIR lower still uses its own env (shared-resolver migration is deferred architecture, not day-to-day polish).
- **No Python** in the product editor path.
- `build` / `run` still stop at errors (soft-recovered AST is for **check / LSP** multi-diag only).

## C runtime in editors (clangd)

Runtime headers live under `runtime/include/`. Generate
`compile_commands.json` with:

```bash
cargo make runtime
```

Then reload the window / restart clangd. Fallback: committed `.clangd` with
`-Iruntime/include`.
