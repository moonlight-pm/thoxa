# Command line

The product binary is `thoxa`. In a checkout, run it as
`target/rust/debug/thoxa`, `cargo run -p thoxa -- …`, or through the
`cargo make` tasks below.

## Commands

| Command | Behaviour |
|---------|-----------|
| `thoxa` | Prints usage to standard error and exits 2 (`thoxa help` prints it and exits 0) |
| `thoxa path.thx` | Same as `thoxa run path.thx` |
| `thoxa dir/` | Runs `dir/main.thx` |
| `thoxa run path [args…]` | Compiles and runs; the remaining arguments go to `main(arguments: Array<String>)` |
| `thoxa build path -o out` | Compiles to an executable |
| `thoxa check path` | Typechecks only; reports as many errors as it can |
| `thoxa check path --json` | The same, as one JSON object |
| `thoxa emit-c path` | Writes the generated C under `target/thoxa/c/` |
| `thoxa lsp` | Language server on standard input and output |
| `thoxa version` | Build identity: package version, git hash (`.dirty` when the tree had changes), binary path |

`thoxa --version` and `thoxa -V` are the same as `thoxa version`. The order of
`check`'s arguments is free: `thoxa check --json path` works.

`run` and `build` reuse the binary in `target/thoxa/bin/` when the package,
`std/`, the runtime, and the compiler are unchanged; `build` then prints
`up to date`. Set `THOXA_FORCE_BUILD=1` to rebuild anyway. Every C compile
passes `-fwrapv`, so integer overflow wraps as the language requires.

## Diagnostics

`thoxa check` prints diagnostics for people by default:

```text
error[THX001]: undefined name `x`
 --> main.thx:1:22
   |
 1 | fn main() -> Int64 { x }
   |                      ^
  = help: declare the name, import it, or fix the spelling
```

| Code | Meaning |
|------|---------|
| `THX001` | undefined or unknown name |
| `THX002` | type mismatch |
| `THX003` | unknown type |
| `THX004` | `May` / `Trial` used without importing `std/fallible` |
| `THX005` | a statement at the top level of a file (statements go in `fn main`) |
| `THX006` | the program has no `fn main` |
| `THX008` | syntax error |
| `THX010` | unused import (a warning) |

Not every message has a code yet; uncoded errors print without one.

The exit status is 0 when there are no errors; warnings alone still succeed.
`--json` prints a single object with `ok` (the same rule), `compiler.version`,
`compiler.path`, and `diagnostics[]`. Each diagnostic has `code`, `severity`,
`message`, `path`, 1-based `line` and `column`, byte spans, `help`,
`expected` / `actual`, and `related`.

## Exit status

`thoxa run` exits with the program's status: what `main` returned (0 to 255;
outside that range the program panics; 0 when `main` returns nothing), 134 after a panic, or 128 + signal
when a signal ended the program.

## Signals

A compiled program keeps the default signal actions, with one exception: as
PID 1 (a container's entrypoint, where the kernel ignores signals that have no
handler) SIGTERM and SIGINT end the program at once with status 128 + signal,
so `podman stop` does not wait for SIGKILL. A request in flight is cut off;
there is no graceful drain.

## Tasks in a checkout

`cargo make` is a Cargo alias for the repository's task runner. Prefer it to
ad-hoc scripts.

```bash
cargo make test              # conformance suite under LeakSanitizer, expected failures, unit tests
cargo make test-asan         # conformance under AddressSanitizer (programs without tasks)
cargo make run path.thx      # compile and run
cargo make run examples/http # the example HTTP server (port 9091)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt   # the kilo editor (needs a terminal; Ctrl-Q quits, three times if unsaved)
cargo make emit-c path.thx   # generated C only
cargo make runtime           # libthoxa_rt.a and compile_commands.json
cargo make zed               # install the Zed extension and the language server
cargo make clean             # remove target/c and target/thoxa (--all also removes target/rust)
```

## Build products

| Path | Contents |
|------|----------|
| `target/rust/` | Cargo outputs (the compiler and task runner) |
| `target/c/` | Runtime objects and `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C |
| `target/thoxa/bin/` | Linked programs and `*.stamp` freshness fingerprints |
| `target/thoxa/bin-leak/` | Programs linked with LeakSanitizer (`THOXA_SANITIZE=leak`) |
| `target/thoxa/bin-address/` | Programs linked with AddressSanitizer (`THOXA_SANITIZE=address`) |

## Environment

| Variable | Effect |
|----------|--------|
| `THOXA_STD` | The `std/` directory to use |
| `THOXA_ROOT` | The root to find `std/` under |
| `THOXA_FORCE_BUILD` | `1` ignores the binary stamp and rebuilds |
| `THOXA_SANITIZE` | `leak` or `address` links programs with that sanitizer. Unset by default |
| `THOXA_PROFILE` | Set to print the time MIR lowering takes on standard error |
| `THOXA_LOG_LEVEL`, `THOXA_LOG_SINK` | Initial logger level and sink (see [std/logging](std.md#stdlogging)) |

`cargo make test` always sets `THOXA_SANITIZE=leak`. `cargo make test-asan`
uses `address` on the programs that start no tasks (the ones listed in
`tests/conformance/asan-skip` use tasks, whose stack switching AddressSanitizer
does not follow).
