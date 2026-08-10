# CLI

The product binary is **`thoxa`** (`cargo run -p thoxa -- …`, or via
`cargo make`).

## Invocations

| Command | Behavior |
|---------|----------|
| `thoxa` (no args) | Session **REPL** (interactive; loads rc on TTY) |
| `thoxa repl` | Same as bare REPL |
| `thoxa path.thx` | Program if `fn main` exists; else **session script** |
| `thoxa dir/` | Runs `dir/Main.thx` if present |
| `thoxa run path` | Compile and run (program or session script) |
| `thoxa build path -o out` | Compile to a binary |
| `thoxa check path` | Typecheck only (no C emit) |
| `thoxa emit-c path` | Emit C under `target/thoxa/c/` |
| `thoxa lsp` | Language server (stdio) |

Prefer **`cargo make`** helpers from a workspace checkout:

```bash
cargo make test              # conformance suite
cargo make run path.thx
cargo make run examples/http # static file server demo (port 9091)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt  # TTY editor
cargo make shell             # bare thoxa REPL
cargo make shell --login     # clean login-like env
cargo make emit-c path.thx
cargo make runtime           # libthoxa_rt.a + compile_commands.json
cargo make zed               # sync Zed extras + LSP binary
```

## Build products

| Path | Role |
|------|------|
| `target/rust/` | Cargo outputs |
| `target/c/` | Runtime objects + `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C |
| `target/thoxa/bin/` | Linked program binaries |

## Environment (selected)

| Variable | Role |
|----------|------|
| `THOXA_STD` | Override product `std/` root |
| `THOXA_ROOT` | Workspace/product root for std resolution |
| `THOXA_SHELL_RC` | Override session rc path |
| `THOXA_NORC` | Skip session rc when set |
| `THOXA_STATUS` | Set by REPL host for prompt helpers |

Details for session: [session.md](session.md).
