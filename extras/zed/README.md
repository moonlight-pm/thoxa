# Thoxa — Zed extension (dev)

Personal, side-loaded Zed extension for editing `.thx` files against the
**native** Thoxa compiler:

- **Syntax highlighting** from [`extras/tree-sitter-thoxa`](../tree-sitter-thoxa)
  (keywords, casing-based types-vs-values, literals, comments).
- **Language server** `thoxa lsp` (Rust, same binary as `build`/`run`) for live
  **diagnostics** (structured codes/help when available; multi-diag) and
  **semantic-token** refinements (function vs variable).

No Python. The server *is* the native frontend, so squiggles cannot drift from
`thoxa check` / `thoxa build`.

## Prerequisites

1. **Native `thoxa` binary** — from the repo root:
   ```sh
   cargo build -p thoxa
   # → target/rust/debug/thoxa
   ```
   Or put `thoxa` on `PATH`. The extension probes PATH first, then
   `{worktree}/target/rust/debug/thoxa`.
2. **Rust + WASM target** (shim): `rustup target add wasm32-wasip2` (or the
   target your Zed version expects for extensions).
3. Grammar is compiled by Zed from committed `src/parser.c` (no local
   `tree-sitter generate` required for install).

## Install

1. **Commit** `extras/` (or at least `extras/tree-sitter-thoxa` with generated
   `src/`) so a git rev contains the grammar.
2. Set `[grammars.thoxa].rev` in `extension.toml` to that commit:
   ```sh
   git rev-parse HEAD   # paste into extension.toml rev = "…"
   ```
3. Open Zed → command palette → **`zed: install dev extension`**.
4. Select this directory: `…/Thoxa/extras/zed`.

Open any `.thx` file (e.g. `tests/conformance/hello.thx`) to confirm color + LSP.

## Optional: semantic-token refinement

Diagnostics and base coloring work out of the box. For the LSP identifier
overlay, add to user `settings.json` (**not** the extension `config.toml` —
Zed rejects it there):

```json
{
  "languages": {
    "Thoxa": { "semantic_tokens": "combined" }
  }
}
```

If Zed flags `semantic_tokens` as unknown, your build is older — skip it;
diagnostics and grammar coloring still work.

## Smoke checklist

1. Open `tests/conformance/hello.thx` → keywords, types, strings, comments colored.
2. Break a type, e.g. body `if 3 { 0 } 0` → red squiggle (`if` condition must be Bool).
3. Fix it → squiggle clears.
4. (Optional) with `semantic_tokens = "combined"`, `main` / `println` look like functions.

If the server does not start: command palette → **`zed: open language server logs`**.
Confirm `target/rust/debug/thoxa` exists or `thoxa` is on PATH.

## Updating (keep it simple)

See the table in [`../README.md`](../README.md). Short version:

| Change | Command | Zed |
|--------|---------|-----|
| Compiler / LSP | `cargo build -p thoxa` | `editor: restart language server` |
| Grammar / highlights / install sync | `cargo make zed` | restart LS; full quit for grammar colors |
| Extension Rust shim | `cargo make zed --all` | restart Zed |

Grammar regenerate (when you edit `grammar.js`):

```sh
cd ../tree-sitter-thoxa
nix-shell -p tree-sitter clang --run 'export CC=clang; tree-sitter generate && tree-sitter test'
cargo make zed --grammar
```

`cargo make zed` rebuilds the SIDE_MODULE grammar WASM (needs `emcc`) and
copies it into `~/.local/share/zed/extensions/installed/thoxa/`. You do **not**
need to re-run “Install Dev Extension” for normal day-to-day work.
