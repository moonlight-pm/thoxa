# tree-sitter-thoxa

A **deliberately thin, lexer-level** Tree-sitter grammar for [Thoxa](../../). It
tokenizes rather than fully parses: keywords, casing-based type-vs-value
identifiers, `#`/`##` comments, string/char/number literals, and operators.

## Why so thin?

Thoxa is unfrozen. Its *lexical* layer barely moves as the grammar is
redesigned, so tracking only that keeps this grammar cheap to maintain. Anything
that needs real understanding — function vs variable, parameter labels — is
supplied by the Thoxa LSP's semantic tokens, which Zed overlays on top of this
base (`semantic_tokens = "combined"`). Don't grow this grammar to chase
structure; refine in the LSP instead.

## Consumed by

The Zed extension at [`../zed`](../zed) references this grammar locally
(`repository = "file://…/editor/tree-sitter-thoxa"`) and compiles it to WASM at
install time. The generated `src/parser.c` is committed so consumers don't need
to run `tree-sitter generate`.

## Developing

Requires the `tree-sitter` CLI and a C compiler. On NixOS the default `gcc`
fails to link the test shared object (`R_X86_64_32 against hidden symbol`); use
`clang`:

```sh
nix-shell -p tree-sitter clang --run 'export CC=clang
  tree-sitter generate      # regenerate src/ after editing grammar.js
  tree-sitter test          # run test/corpus
  tree-sitter parse FILE.thx'
```

Zed compiles the grammar to WASM itself, so the native-link quirk above only
affects local CLI testing, not the extension.
