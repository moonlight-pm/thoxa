# extras/ — non-product DX

Personal editor glue for native Thoxa. **Not** on the compile-to-C critical path.

| Path | Role |
|------|------|
| [`tree-sitter-thoxa/`](tree-sitter-thoxa/) | Thin lexer-level Tree-sitter grammar (committed `src/parser.c`) |
| [`zed/`](zed/) | Side-loaded Zed **dev** extension (highlighting + `thoxa lsp`) |

Product language server: **`thoxa lsp`** in `crates/compiler` (Rust). No Python.

## Quick start (Zed)

1. Build the native binary:
   ```sh
   cargo build -p thoxa
   # target/rust/debug/thoxa
   ```
2. Commit `extras/` (Zed pins a git `rev` for the grammar), then set
   `extras/zed/extension.toml` → `[grammars.thoxa].rev` to that SHA:
   ```sh
   git rev-parse HEAD
   ```
3. Zed → **Install Dev Extension** → choose `extras/zed`.
4. Open `tests/conformance/hello.thx`.

Optional user setting for semantic tokens:

```json
{ "languages": { "Thoxa": { "semantic_tokens": "combined" } } }
```

Full notes: [`zed/README.md`](zed/README.md). Grammar regenerate:
[`tree-sitter-thoxa/README.md`](tree-sitter-thoxa/README.md).

## Smoke

| Check | Expect |
|-------|--------|
| Color on `.thx` | keywords, PascalCase types, strings, `#` comments |
| Type error (`if 3 { … }`) | squiggle from `thoxa lsp` |
| Fix error | squiggle clears |
| LSP logs if broken | `zed: open language server logs`; confirm `thoxa` on PATH or `target/rust/debug/thoxa` |

## Layout decisions

See `docs/decisions.md` **D71** (extras root), **D75** (thin grammar), **D72** (`thoxa lsp`).
