# ThoxaNative

Native implementation of [Thoxa](../Thoxa): compile-to-C, hybrid value layout, simple RC heading toward Perceus.

> **Pre-alpha.** Syntax and semantics track `../Thoxa/spec` only as features earn conformance tests here.

## Quickstart

```bash
# Build runtime + compile and run the hello conformance program
cargo make test

# Run a Thoxa file
cargo make run tests/conformance/hello.thx

# Emit C only (under targets/)
cargo make emit-c tests/conformance/hello.thx
```

Compiler CLI (also available via `cargo run -p thoxa -- …`):

```bash
cargo run -p thoxa -- run tests/conformance/hello.thx
cargo run -p thoxa -- build tests/conformance/hello.thx -o hello
cargo run -p thoxa -- emit-c tests/conformance/hello.thx
```

## Layout

| Path | Role |
|------|------|
| `crates/compiler` | `thoxa` CLI + compiler library |
| `crates/make` | xtask (`cargo make …`) |
| `runtime/` | C runtime (`thoxa_rt`) |
| `tests/conformance/` | `.thx` + `.out` programs |
| `targets/` | All build products |
| `docs/specs/` | Design docs |
| `docs/decisions.md` | Decision log |

## Docs

- [Native kernel design](docs/specs/2026-07-18-thoxa-native-kernel-design.md)
- [Decisions](docs/decisions.md)
