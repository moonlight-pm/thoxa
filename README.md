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

## Editor / C language server

The runtime lives under `runtime/` with headers in `runtime/include/`. Editors using
**clangd** need that include path (otherwise `thoxa_rt.h` is missing). On **NixOS**,
clangd also will not see gcc’s system headers (`stdio.h`, `fputs`, `stdout`) unless
those paths are provided — real `cc` works fine; only the language server is confused.

```bash
cargo make runtime    # builds libthoxa_rt.a and writes compile_commands.json
```

That `compile_commands.json` includes `-Iruntime/include` **and** the host `cc`
system include dirs. Then **reload the window / restart clangd**.

Fallback: committed [`.clangd`](.clangd) with `-Iruntime/include`.

## Docs

- [Native kernel design](docs/specs/2026-07-18-thoxa-native-kernel-design.md)
- [Decisions](docs/decisions.md)
