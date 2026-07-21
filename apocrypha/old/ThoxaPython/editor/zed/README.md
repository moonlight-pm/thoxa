# Thoxa — Zed extension

Personal, side-loaded Zed extension for editing `.thx` files:

- **Syntax highlighting** from the thin [`tree-sitter-thoxa`](../tree-sitter-thoxa)
  grammar (keywords, casing-based types-vs-values, literals, comments).
- **A language server** (`thoxa lsp`, pure Python, reusing the compiler) for
  live **diagnostics** and a **semantic-token** overlay that refines identifiers
  (functions, parameters, named-argument labels) beyond what casing alone gives.

The server *is* the real compiler, so its errors can never drift from the
language — the whole point of the LSP-first design.

## Prerequisites (NixOS)

1. **The Thoxa venv** — from the repo root: `uv sync`. This creates
   `.venv/bin/thoxa`, which the extension launches as `thoxa lsp`.
2. **Rust + WASM target** (to build the shim): `rustup target add wasm32-wasip2`.
3. Zed downloads its own `wasi-sdk`/clang to compile the grammar to WASM; if the
   grammar step fails, it's almost always a network hiccup — retry.

## Install

1. Open Zed.
2. Command palette → **`zed: install dev extension`**.
3. Select this directory: `~/Workspace/Thoxa/apocrypha/old/ThoxaPython/editor/zed`
   (historical Python-oracle extension; native product is the parent `Thoxa` repo).

Zed compiles the Rust shim and the grammar (it clones the Thoxa repo at the
`rev` pinned in `extension.toml` via `file://`, so it uses the **committed**
grammar). Open any `.thx` file to confirm.

## Optional: enable the semantic-token refinement

Diagnostics and base coloring work out of the box. To also get the LSP's
identifier overlay (function / parameter / named-arg colors), add to your Zed
`settings.json` (NOT the extension's `config.toml` — Zed rejects it there):

```json
{
  "languages": {
    "Thoxa": { "semantic_tokens": "combined" }
  }
}
```

This needs a recent Zed. If Zed flags `semantic_tokens` as an unknown setting,
your version predates the feature (it landed after 0.218) — just skip it;
diagnostics and grammar-based coloring are unaffected.

## Smoke test

1. Open `examples/http/Main.thx` → keywords, types, strings, comments are colored.
2. Type a type error, e.g. change a body to `if 3 { }` → a red squiggle appears:
   *"the if condition must be a Bool, found Int"*.
3. Fix it → the squiggle clears on the next keystroke.

If nothing launches, open Zed's language-server logs (command palette →
`zed: open language server logs`). The shim returns a clear error if neither
`.venv/bin/thoxa` nor a `thoxa` on `PATH` is found.

## Updating

- **LSP change** (`thoxa/lsp/*`): nothing to rebuild — the server is launched
  fresh each session. Restart it in Zed (`editor: restart language server`) or
  reopen the file.
- **Grammar change** (`../tree-sitter-thoxa/grammar.js`): regenerate, commit,
  then bump `rev` in `extension.toml` and reinstall the dev extension:
  ```sh
  cd ../tree-sitter-thoxa
  nix-shell -p tree-sitter clang --run 'export CC=clang; tree-sitter generate && tree-sitter test'
  # commit, then set [grammars.thoxa].rev in ../zed/extension.toml to the new SHA
  ```
