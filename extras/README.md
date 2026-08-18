# extras/ — non-product DX

Personal editor glue for native Thoxa. **Not** on the compile-to-C critical path.

| Path | Role |
|------|------|
| [`tree-sitter-thoxa/`](tree-sitter-thoxa/) | Thin lexer-level Tree-sitter grammar (committed `src/parser.c`) |
| [`zed/`](zed/) | Zed extension shim + language config (highlighting + launch `thoxa lsp`) |
| [`shell/`](shell/) | Pure-Thoxa **dogfood** Shell API — **not** the product REPL |

Product session face: **bare `thoxa`** (`cargo make shell`).  
Product language server: **`thoxa lsp`** in `crates/compiler` (Rust). No Python.

Automation lives in the xtask (**not** shell scripts — D10):

```bash
cargo make shell            # product session REPL (bare thoxa)
cargo make run extras/shell # dogfood shell package only
cargo make zed              # languages + grammar WASM + build thoxa + PATH
cargo make zed --grammar    # grammar WASM only
cargo make zed --all        # also rebuild extension.wasm (rare)
```

## Day-to-day: what changes what

| You changed… | Command | Zed action after |
|--------------|---------|------------------|
| **Compiler / LSP** (`crates/compiler/…`) | `cargo build -p thoxa` *or* `cargo make zed` | `editor: restart language server` |
| **Base colors** (`extras/tree-sitter-thoxa/grammar.js`) | `cargo make zed --grammar` | Full quit + restart Zed if colors stale |
| **Highlight queries / lang config** (`extras/zed/languages/thoxa/*`) | `cargo make zed` | Reopen buffer or restart Zed |
| **Extension shim** (`extras/zed/src/lib.rs`) | `cargo make zed --all` | Restart Zed |

**Most days you only touch the compiler:**

```sh
cargo build -p thoxa
# in Zed: editor: restart language server
```

The binary is discovered as `~/.local/bin/thoxa` (after `cargo make zed`) or
`{worktree}/target/rust/debug/thoxa`.

## First-time install

```sh
cargo make zed --all   # needs emcc for grammar WASM; rustup wasm32-wasip2 for extension
# Restart Zed; open tests/conformance/hello.thx
```

Optional user setting for semantic tokens:

```json
{ "languages": { "Thoxa": { "semantic_tokens": "combined" } } }
```

Grammar regenerate (when editing `grammar.js`), if the CLI is available:

```sh
nix-shell -p tree-sitter clang --run 'export CC=clang; cd extras/tree-sitter-thoxa && tree-sitter generate && tree-sitter test'
cargo make zed --grammar
```

(`cargo make zed` also regenerates when `grammar.js` is newer than `parser.c` and `tree-sitter` is on `PATH`.)

## Smoke

| Check | Expect |
|-------|--------|
| Color on `.thx` | keywords, PascalCase types, strings, `#` comments; tag trees use `@tag` / `@markup`; `sql"…"` injects SQL |
| Type error (`if 3 { … }`) | squiggle from `thoxa lsp` |
| Fix error | squiggle clears |
| LSP logs if broken | `zed: open language server logs`; look for `thoxa-lsp: thoxa 0.1.0+<hash>  path=…` |
| Binary identity | `thoxa version` (same string as LSP start log / `serverInfo.version`) |
| Multi-diag | Syntax + undefined names can show together after soft recovery; restart LS after rebuild |

## Layout decisions

See `docs/decisions.md` **D71** (extras root), **D75** (thin grammar), **D72** (`thoxa lsp`), **D10** (xtask, no project shell scripts).
