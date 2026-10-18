# Editor

Editor support targets [Zed](https://zed.dev): a Tree-sitter grammar for
syntax colour, and `thoxa lsp`, a language server that runs the compiler's own
front end, so its diagnostics match `thoxa check`.

## Setup

From a checkout:

```bash
cargo make zed            # grammar, language files, and thoxa for the language server
cargo make zed --grammar  # the grammar WASM only
cargo make zed --all      # also rebuild the extension's WASM shim
```

See [`extras/README.md`](../../extras/README.md). After syncing, restart the
language server; colour changes in the grammar may need Zed to be quit and
started again.

## Which thoxa Zed runs

Zed starts the `thoxa` on `PATH`. After `cargo make zed` that is usually
`~/.local/bin/thoxa`, a link to `target/rust/debug/thoxa`.

1. Rebuild: `cargo build -p thoxa` or `cargo make zed`.
2. In Zed, run `editor: restart language server`.
3. Compare identities. In a shell, `thoxa version` prints
   `thoxa 0.1.0+<hash>[.dirty]  path=…`. In Zed,
   `zed: open language server logs` shows
   `thoxa-lsp: thoxa 0.1.0+<hash>  path=…` (written to standard
   error at start and sent as a log message after initialize; there is no
   notification).

If the hash in the log differs from `git rev-parse --short=12 HEAD` (or from
`thoxa version` after a rebuild), the running server is an old process;
restart it again.

## Features

| Feature | Notes |
|---------|-------|
| Diagnostics | The same engine as `thoxa check`: several diagnostics per file, `THX…` codes, help text (folded into the message), and related locations. Unused imports are warnings (`THX010`). After a syntax error the parser recovers at the next statement and keeps checking the rest of the function; recovery inside an expression is limited. A statement at the top level of a file is reported at its span |
| Syntax colour | Tree-sitter. `<` reads as the compiler reads it: `Array<Int64>` is type arguments, `<div` / `<>` (no space) starts a tag, `a < b` compares. Tags, attributes (boolean and `{...spread}`), and fragments are coloured. A string's `{holes}` are coloured as code, including strings inside them; `\n`, `\xHH`, and `\u{…}` are escapes. The body of `sql"…"` uses Zed's SQL grammar, with holes still Thoxa. `-->`, `where`, and the route verbs in type bodies are keywords. With `semantic_tokens = "combined"` the server also colours calls, tag names, and attributes |
| Hover | The signature and the contiguous `##` comments above it. Functions and methods without `->`, `main` included, show their inferred return type; `extern` shows `-> Void` unless annotated. `let` / `mut` show their type when it is known (`let a: Response`). A call argument shows its parameter (`backlog: Int64` on `listen(16)`) when the callee can be resolved. The built-in `Node`, `Element`, `Text`, `Fragment`, and `Attr` have hovers; built-in members such as `.html()` and `String.count` do not |
| Go to definition | Local, `./`, `~/`, and `std/…` modules; an open buffer is preferred to the file on disk. Namespace functions (`net.createSocket`) and methods on a value whose type is known: from an annotation, a constructor, a function or method return (including generic types), a copy, a match payload, `if` / `match` / block results, a module `let`, or `?` on a `Trial` / `May` |
| References | The file, open buffers, and `.thx` files in the package tree that import the name |
| Rename | The same scope as references. Renaming a parameter or a constructor field also renames the labels that pass it (`deadline:`). Where a bare name fills the parameter it names, renaming either one writes the label out, so `connect(host, deadline)` becomes `connect(host, deadline: limit)` or `connect(host, timeout: deadline)`. A rename that would make a positional argument fill a labelled parameter instead is refused |
| Document symbols | The outline of the file |
| Workspace symbols | A substring search over open buffers and the package tree, up to 100 results |
| Open buffers | An unsaved buffer is what the server checks, not the file on disk |

A file without `main` is checked as a library (no "requires main" error).
In a directory package, `~/…` resolves from the nearest `main.thx`, as
`thoxa run` does, not from the open file's directory. The package tree is the
directory of the nearest `main.thx` or `.git`, searched to a fixed depth.

## Limits

- Member navigation fails on expressions whose type the server cannot recover.
- Call-argument hover needs a callee it can resolve.
- Parse recovery works per statement, not inside expressions.
- `thoxa build` and `thoxa run` stop at the first error; only `check` and the
  language server recover and report more.
- The server re-checks the whole package on every change (no debounce).

## C runtime headers (clangd)

The runtime's headers are in `runtime/include/`. Generate
`compile_commands.json` with:

```bash
cargo make runtime
```

then restart clangd. On NixOS this also records the host compiler's system
include directories, which clangd cannot find by itself. A committed `.clangd`
with `-Iruntime/include` is the fallback.
