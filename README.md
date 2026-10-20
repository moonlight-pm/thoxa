# Thoxa

Thoxa is a statically typed, expression-oriented programming language. The
compiler, written in Rust, translates Thoxa to C; a C compiler links that with
a small C runtime into a native executable.

**Site:** [thoxa.moonlight.pm](https://thoxa.moonlight.pm) · **Manual:**
[thoxa.moonlight.pm/manual](https://thoxa.moonlight.pm/manual) (the same text as
[`docs/handbook/`](docs/handbook/))

> **Pre-alpha.** The syntax and semantics still change, there are no releases,
> and it runs on Linux x86-64 only.

```thx
import { Pass, Fail } from "std/fallible"
import { messages } from "std/json"
import logging, { info } from "std/logging"

type Quantity = Int64 where >= 1 & <= 12
type Size = "small" | "large"
type Line(drink: String, size: Size, quantity: Quantity = 1)

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match Line.read("{{\"drink\": \"mocha\", \"size\": \"huge\", \"quantity\": 40}}") {
        Pass(line) => info(line.encode())
        Fail(issues) => info(messages(issues))
    }
    0
}
```

```text
size: expected "small" | "large", got huge; quantity: expected integer <= 12, got 40
```

## What is in it

- Products, unions, literal types, refinements (`Int64 where >= 1`), generic
  types, function types; local inference; pattern matching with guards.
- Errors as values: `May<T>` and `Trial<T, E>` from `std/fallible`, with `?`.
- Reference counting with copy-on-write arrays and maps; `fn drop()` for
  resources. No garbage collector, no borrow checker.
- `-->` starts a task; socket operations park it. One OS thread, no
  `async`/`await`.
- JSON and form readers, and encoders, generated from type definitions.
- Tag literals (HTML as typed values), route clauses, an HTTP/1.1 server,
  `sql"…"` with SQLite.
- `extern fn` to call C, C-layout types, `Address<T>`.
- `thoxa run`, `build`, `check` (with `--json`), `lsp`; a Zed extension.

What it does not have yet: closures, generic functions, traits, threads, a
package manager, releases. The [manual](https://thoxa.moonlight.pm/manual) states
exactly what works.

## Building

You need a Rust toolchain and a C compiler (`cc`: GCC or Clang).

```bash
git clone https://github.com/moonlight-pm/thoxa
cd thoxa
cargo build -p thoxa            # the compiler: target/rust/debug/thoxa
cargo make test                 # conformance suite, site samples, unit tests
```

`cargo make` is a Cargo alias for the repository's task runner
([`crates/make`](crates/make)).

## Using it

```bash
thoxa run path.thx [args…]      # compile and run (a directory runs its main.thx)
thoxa build path.thx -o out     # compile to a binary
thoxa check path.thx            # typecheck; --json for machine-readable diagnostics
thoxa lsp                       # language server (stdio)
```

From a checkout, `cargo run -p thoxa -- …` or `target/rust/debug/thoxa …`.
Programs to try:

```bash
cargo make run tests/conformance/hello.thx
cargo make run examples/http                  # a small web app with SQLite (port 9091)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt   # a terminal text editor
cargo make run workloads/landing              # the site itself (port 8088)
```

## Repository

```text
crates/compiler   the thoxa binary: compiler and language server
crates/make       task runner (cargo make …)
runtime/          the C runtime (thoxa_rt)
std/              the standard library (std/… imports)
tests/conformance programs with expected output; the definition of done
examples/         runnable programs: examples/http (web app), examples/kilo (editor)
workloads/        deployed Thoxa programs: landing = thoxa.moonlight.pm
extras/           editor support (Tree-sitter grammar, Zed)
vendor/           the SQLite amalgamation
docs/             manual (docs/handbook) and engineering docs
apocrypha/        earlier attempts and the Python prototype; history only
```

Build output goes under `target/`: `target/rust/` (Cargo), `target/c/` (the
runtime library), `target/thoxa/` (generated C and program binaries).
`cargo make clean` removes `target/c` and `target/thoxa`; `--all` also removes
`target/rust`.

## Documentation

| Document | What it is |
|----------|------------|
| [`docs/handbook/`](docs/handbook/) | The manual: shipped behaviour only, rendered at thoxa.moonlight.pm/manual |
| [`docs/capabilities.md`](docs/capabilities.md) | Each capability's status, gates, and known gaps |
| [`docs/decisions.md`](docs/decisions.md) | Design decisions by id (D1…) |
| [`docs/architecture.md`](docs/architecture.md) | Pipeline, crates, runtime layout |
| [`docs/specs/`](docs/specs/) | Design freezes |
| [`CURRENT.md`](CURRENT.md) | Current focus |
| [`AGENTS.md`](AGENTS.md) | How work is done here (slices, conformance, docs) |

A feature counts as done when a program in `tests/conformance/` compiles,
runs, and matches its `.out` under `cargo make test`, which also runs every
program under LeakSanitizer.

## Editor setup

**Zed:** `cargo make zed` builds the grammar, installs the language files, and
links `thoxa` for the language server; restart Zed afterwards. See
[`docs/handbook/editor.md`](docs/handbook/editor.md).

**clangd** (for the C runtime): `cargo make runtime` writes a
`compile_commands.json` with `-Iruntime/include` and the host compiler's
system include directories. The committed [`.clangd`](.clangd) is a fallback.

## Problems

Report bugs and questions at
[github.com/moonlight-pm/thoxa/issues](https://github.com/moonlight-pm/thoxa/issues),
with a short `.thx` file, the command, and the output of `thoxa version`.
