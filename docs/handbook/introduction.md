# Introduction

Thoxa is a statically typed, expression-oriented programming language. The
compiler is written in Rust and translates a Thoxa program to C. A C compiler
then links that C with the Thoxa runtime (`thoxa_rt`, also C) into a native
executable. The same compiler backs an interactive shell (bare `thoxa`) whose
input is Thoxa.

This manual describes the language and tools as they are implemented today.
It does not describe plans. If a feature is not in this manual, assume it does
not exist.

## Status

- **Pre-alpha.** The syntax and semantics still change. There are no releases
  and no compatibility promises.
- **Linux x86-64 only.** The runtime, the standard library's file and socket
  code, and the interactive shell assume Linux. Other systems are out of scope
  for now.
- **Memory** is managed by simple, single-threaded reference counting. See
  [Memory](language.md#memory).
- A feature counts as implemented when a test program in
  `tests/conformance/` compiles, runs, and prints the expected output.

## Building from source

Requirements:

- a Rust toolchain (Cargo),
- a C compiler on `PATH` as `cc` (GCC or Clang),
- `make` and a POSIX shell (the build compiles the vendored TinyCC, which the
  interactive shell uses).

```bash
git clone https://github.com/moonlight-pm/thoxa
cd thoxa
cargo build -p thoxa          # the compiler: target/rust/debug/thoxa
cargo make test               # conformance suite + unit tests
```

`cargo make` is a Cargo alias for the repository's task runner
(`crates/make`); it needs nothing else installed. Build outputs go under
`target/` (see [Build products](cli.md#build-products)).

To put `thoxa` on your `PATH`, link or copy `target/rust/debug/thoxa` (or a
release build, `cargo build --release -p thoxa`). The compiler finds the
standard library through `THOXA_ROOT` / `THOXA_STD` or next to the binary; see
[Resolution order](std.md#resolution-order).

## A first program

```thx
import logging, { info } from "std/logging"

fn main() {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    info("hello, world")
}
```

```bash
thoxa run hello.thx              # compile and run
thoxa build hello.thx -o hello   # compile to ./hello
thoxa check hello.thx            # typecheck only
```

There is no built-in `print`. Output goes through the logger in
`std/logging`; `toStdout()` and `formatPlain` make `info` write plain lines to
standard output. A `main` that returns nothing exits with status 0; one
declared `-> Int64` returns the process exit status.

A program reads its command-line arguments by declaring them:

```thx
import logging, { info } from "std/logging"

fn main(arguments: Array<String>) -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    if arguments.count < 2 {
        info("usage: greet NAME")
        return 2
    }
    info("hello, {arguments[1]}")
    0
}
```

`thoxa run greet.thx ada` prints `hello, ada`. `arguments[0]` is the program
path.

Run `thoxa` with no arguments for the interactive shell; see
[Session](session.md).

## Notation

- Code is in `monospace`. A `thx` block is Thoxa source; a `bash` block is a
  command line.
- `## …` inside Thoxa examples is a comment that shows a value or the output
  of the line it follows.
- "Compile error" means `thoxa check` / `thoxa run` rejects the program with a
  diagnostic. "Panics" means the running program prints `thoxa panic: …` to
  standard error and exits with status 134. See [Panics](language.md#panics).

## Chapters

| Chapter | Contents |
|---------|----------|
| [Language](language.md) | Syntax, types, expressions, functions, modules, concurrency, C interop |
| [Standard library](std.md) | The `std/…` modules |
| [Session](session.md) | The interactive shell and session scripts |
| [Command line](cli.md) | `thoxa` commands, diagnostics, exit status, environment |
| [Editor](editor.md) | Zed support and `thoxa lsp` |

## Reporting problems

File issues at
[github.com/moonlight-pm/thoxa/issues](https://github.com/moonlight-pm/thoxa/issues).
A report is most useful with a short `.thx` file, the command you ran, and the
output of `thoxa version`.
