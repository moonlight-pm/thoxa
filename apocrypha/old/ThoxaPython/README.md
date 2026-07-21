# Thoxa

> ⚠️ **UNFROZEN · pre-alpha · syntax will change.** Thoxa is under active language design.
> Nothing here is stable. A feature is only real when it has a passing test in `spec/conformance/`.

**Thoxa** is an expression-oriented **systems programming language** aiming for the safety and
expressiveness of Rust *without* the borrow checker or the learning cliff: automatic memory
management, static typing with local inference, named parameters by default, string interpolation,
and a friendlier algebraic-error vocabulary (`May`/`Trial` instead of `Option`/`Result`).

```thoxa
fn half(x: Int64) -> Trial<Int64, String> {
    if x % 2 == 0 { Pass(x / 2) } else { Fail("odd") }
}

fn main() -> Int64 {
    match half(10) {
        Pass(n) { println("half is {n}") }
        Fail(e) { println("no: {e}") }
    }
    0
}
```

## Status

This is the fourth and (intentionally) final incarnation. Three prior attempts live read-only
under `old/`; each threw its backend away. This revival keeps **one interpreter** behind a stable
typed-AST seam and iterates on the *language* first, adding native compilation only once the
surface is frozen. See [`docs/specs/2026-07-12-thoxa-revival-design.md`](docs/specs/2026-07-12-thoxa-revival-design.md).

- **Vision:** [`spec/VISION.md`](spec/VISION.md)
- **Language spec (source of truth):** [`spec/SPEC.md`](spec/SPEC.md)
- **Architecture:** [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- **Changelog (honest):** [`CHANGELOG.md`](CHANGELOG.md)

## Quickstart

```bash
uv sync                       # create the venv, install dev deps
uv run thoxa run examples/http    # a multi-file project (examples/http/Main.thx)
uv run thoxa repl             # interactive REPL
uv run pytest                 # unit + conformance tests
```

## Roadmap

| Milestone | Contents | Status |
|-----------|----------|--------|
| M0 | Clean base: vision, spec, architecture, conformance harness. | ✅ |
| M1 | Core: lexer, parser, AST, interpreter — bindings, functions, types, control flow, arrays, interpolation. | ✅ |
| M2 | `May`/`Trial` unions, `match`, `?`. | ✅ |
| M3 | Named-argument dispatch. | ✅ |
| M4 | `as`/`can`/`is` + conservative static checker (incl. `match` exhaustiveness). | ✅ |
| M5 | Literate interactive notebook (`thoxa notebook`). | ✅ |
| M6 | Reference web server in Thoxa — retired, folded into the FFI below. | ✅ |
| M7 | **libc FFI**: `extern fn`, the `Bytes` buffer, real syscalls (`Int`/`Float` removed as ambiguous). TCP echo + an HTTP responder in pure Thoxa (`examples/http/`). | ✅ |
| M8 | **Module system**: cross-file imports, logical resolution (`./`, `~/`, named roots), selective + namespace imports; PascalCase module files (each names a type) bound as camelCase instances; a `Main.thx` project entrypoint (`thoxa run <dir>`); a closed auto-imported prelude (`std/Prelude`) + `std/Math`; two-phase load with circular imports. | ✅ |
| — | *Deferred:* module-as-value/functors + a package manager; typed `Ptr`/structs; typed IR + QBE/transpile-C backends, Perceus RC, a concurrency model, static inference, WASM notebook. | later |

All milestones pass a **211-test** suite (16 conformance programs + unit tests). Run `uv run pytest`.

## License

TBD.
