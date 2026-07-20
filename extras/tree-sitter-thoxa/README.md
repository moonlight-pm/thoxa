# tree-sitter-thoxa

A **deliberately thin, lexer-level** Tree-sitter grammar for native Thoxa. It
tokenizes rather than fully parses: keywords, casing-based type-vs-value
identifiers, `#`/`##` comments, string/char/number literals, and operators.

## Why so thin?

Thoxa’s surface still moves. Its *lexical* layer is far more stable than full
syntax, so tracking only that keeps this grammar cheap to maintain. Function vs
variable refinements come from **`thoxa lsp`** semantic tokens (Zed
`semantic_tokens = "combined"`). Do not grow this into a full CST.

## Consumed by

The Zed dev extension at [`../zed`](../zed) references this grammar via
`file://…/Thoxa` + `path = "extras/tree-sitter-thoxa"`. Generated `src/parser.c`
is committed so consumers need not run `tree-sitter generate` just to install.

## Developing

Requires the `tree-sitter` CLI and a C compiler. On NixOS the default `gcc`
often fails to link the test shared object; use `clang`:

```sh
nix-shell -p tree-sitter clang --run 'export CC=clang
  tree-sitter generate      # regenerate src/ after editing grammar.js
  tree-sitter test          # run test/corpus
  tree-sitter parse path.thx'
```

Push the grammar into the local Zed install (SIDE_MODULE WASM via `emcc`):

```sh
cargo make zed --grammar
# or full editor sync: cargo make zed
```

The native-link quirk only affects local `tree-sitter test`; Zed uses the
WASM produced by `cargo make zed`.
