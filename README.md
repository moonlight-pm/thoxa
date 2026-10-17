# Thoxa

Native Thoxa: compile-to-C, hybrid value layout, simple RC heading toward
Perceus. Interactive **session** face is bare `thoxa` (REPL + no-main scripts).

> **Pre-alpha.** Syntax and semantics track the Python oracle at
> [`apocrypha/old/ThoxaPython/spec`](apocrypha/old/ThoxaPython/spec) only as
> features earn conformance tests here.

## Start here

| Doc | Purpose |
|-----|---------|
| [`AGENTS.md`](./AGENTS.md) | Agent / contributor workflow |
| [`CURRENT.md`](./CURRENT.md) | **Living** priority + locks |
| [`docs/capabilities.md`](./docs/capabilities.md) | As-built capability maturity |
| [`docs/handbook/`](./docs/handbook/) | User manual (shipped only) |
| [`docs/progress-model.md`](./docs/progress-model.md) | How focus / as-built / target / handbook fit |
| [`docs/README.md`](./docs/README.md) | Full documentation map |

## Quickstart

```bash
# Build runtime + run the conformance suite
cargo make test

# Run a Thoxa file
cargo make run tests/conformance/hello.thx

# Example: Little Crema (multi-route; Ctrl-C to stop; port 9091)
cargo make run examples/http

# thoxa.dev landing (Ctrl-C to stop; port 8088)
cargo make run workloads/landing

# sola.computer teaser (Ctrl-C to stop; port 8089)
cargo make run workloads/sola

# oath.wicket.cloud (Ctrl-C to stop; port 8090)
cargo make run workloads/oath

# Example: kilo editor (needs a real TTY; Ctrl-Q to quit)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt

# Session REPL (product shell)
cargo make shell
# or: cargo run -p thoxa --

# Emit C only (under target/thoxa/c/)
cargo make emit-c tests/conformance/hello.thx
```

Compiler CLI (also available via `cargo run -p thoxa -- …`):

```bash
cargo run -p thoxa -- run tests/conformance/hello.thx
cargo run -p thoxa -- build tests/conformance/hello.thx -o hello
cargo run -p thoxa -- emit-c tests/conformance/hello.thx
cargo run -p thoxa -- check path.thx
cargo run -p thoxa -- lsp
```

## Layout

```text
crates/compiler   thoxa CLI + compiler library (incl. session REPL + lsp)
crates/make       xtask (cargo make …)
runtime/          C runtime (thoxa_rt)
std/              product standard library (std/… imports)
tests/conformance definition of done (.thx + .out)
examples/         runnable demos (http server, kilo editor)
workloads/       deployed Thoxa programs (landing = thoxa.dev; sola = sola.computer)
vendor/           vendored upstream (TinyCC for session libtcc; SQLite amalgamation)
docs/             canonical engineering + handbook
CURRENT.md        living session handoff
extras/           non-product DX (tree-sitter, Zed, dogfood shell)
apocrypha/        history + Python oracle (not product)
```

### Build products (`target/`)

| Path | Role |
|------|------|
| `target/rust/` | Cargo (compiler + xtask binaries, deps) |
| `target/c/` | C runtime objects + `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C from Thoxa sources |
| `target/thoxa/bin/` | Linked Thoxa program binaries + freshness `*.stamp` |
| `target/thoxa/bin-leak/` | LSan-linked programs (`THOXA_SANITIZE=leak`; **D151**) |
| `target/thoxa/bin-address/` | ASan-linked programs (`THOXA_SANITIZE=address`) |

`cargo make clean` removes `target/c` and `target/thoxa`.  
`cargo make clean --all` also removes `target/rust`.

## Editor / C language server

The runtime lives under `runtime/` with headers in `runtime/include/`. Editors
using **clangd** need that include path. On **NixOS**, clangd also needs host
system include dirs — real `cc` works; only the language server is confused.

```bash
cargo make runtime    # builds libthoxa_rt.a and writes compile_commands.json
```

That `compile_commands.json` includes `-Iruntime/include` **and** the host `cc`
system include dirs. Then **reload the window / restart clangd**.

Fallback: committed [`.clangd`](.clangd) with `-Iruntime/include`.

### Thoxa in Zed

```bash
cargo make zed            # sync grammar + language files + build thoxa for LSP
cargo make zed --grammar  # grammar WASM only
cargo make zed --all      # also rebuild the WASM extension shim
```

See [`extras/README.md`](extras/README.md) and
[`docs/handbook/editor.md`](docs/handbook/editor.md).

## Docs

- [CURRENT — what next](CURRENT.md)
- [Capabilities](docs/capabilities.md)
- [Handbook (user)](docs/handbook/)
- [Progress model](docs/progress-model.md)
- [Documentation map](docs/README.md)
- [Decisions](docs/decisions.md)
- [Architecture](docs/architecture.md)
- [Roadmap](docs/roadmap.md)
