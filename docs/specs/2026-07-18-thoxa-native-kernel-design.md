# ThoxaNative — Native Kernel Design

- **Date:** 2026-07-18
- **Status:** Approved
- **Repo:** `ThoxaNative` (sibling to Python `Thoxa`)

## 1. Purpose

Re-implement Thoxa natively so **layout, packing, and reference counting** are real shaping forces. Python Thoxa remains the language oracle and experiment bed; this repo is the systems path.

## 2. Locked decisions

| Topic | Choice |
|-------|--------|
| Growth style | Small kernel → grow by examples toward `examples/http` |
| Algebraic effects | Out until async forces the question |
| Errors | `May` / `Trial` + force-to-deal (`?` / `match`); signature inference OK |
| Value model | **Hybrid (A)** — unboxed scalars; inline tags for small sums; RC heap for String/Array/large types; C-layout structs for FFI |
| Backend (phase A) | **Compile-to-C** |
| Old QBE Thoxa (`../Thoxa/old/Thoxa`) | Design museum; steal good runtime ideas; do not revive AST→QBE megapass |
| Python Thoxa | Read-only SPEC + conformance oracle |
| Host language | Rust (compiler) |
| Runtime language | C (`thoxa_rt`) |
| RC day one | Correct simple single-threaded RC (dup/drop/move); Perceus precision later |

See also [docs/decisions.md](../decisions.md).

## 3. Architecture

```text
.thx → lexer → parser → AST
         → resolver (names/modules, when needed)
         → checker (types; Trial force-to-deal from R2)
         → MIR (typed, explicit ops)
         → rc-insert (simple RC first; Perceus later)
         → cgen (MIR → C)
         → cc + libthoxa_rt → binary under target/thoxa/
```

- **No QBE in phase A.**
- **No tree-walk product interpreter** — programs always compile to C.
- **MIR is the stable seam.** Only `cgen` knows C. Inference/checking never live inside the backend (lesson from old QBE Thoxa).

## 4. Value / layout model (hybrid A)

| Kind | Representation | RC |
|------|----------------|----|
| Sized ints/floats, `Bool`, `Char`, `Void` | Unboxed C types | No |
| `May<T>` / `Trial<T,E>` with small payloads | Inline tagged struct | Only if a payload is heap |
| `String`, growable `Array`, large user types | Heap: header + payload | Yes |
| C-ABI / fixed layout structs | Inline C layout (decl order, natural align) | No object header unless a field is a heap ref |

Heap header (16 bytes, non-atomic until threads):

```c
typedef struct {
    size_t   refcount;
    uint16_t type_tag;
    uint16_t flags;
    uint32_t reserved;
} ThoxaHeader;
```

**RC policy (phase A):** dup on copy, drop at end of lifetime, move on return of a local. No “payload not refcounted” shortcuts. Full Perceus reuse is a later MIR pass.

## 5. Language surface policy

- Source of truth: `../Thoxa/spec/SPEC.md` unless we deliberately reopen a choice.
- Implement **only** what the current example rung needs.
- A feature is done only when a checked-in `.thx` compiles, runs, and matches expected output.

## 6. Example ladder

| Rung | Program | Forces |
|------|---------|--------|
| R0 | `hello` — `main`, `println`, return `0` | driver, runtime, C emit skeleton | ✅ |
| R1 | arithmetic, `let`, `if` | locals, exprs, control | ✅ |
| R2 | `Trial` + `match` + `?` | tags, force-to-deal, early return; R2 monomorph is `Trial<Int64,Int64>` (String err → R4) | ✅ |
| R3 | user `type` + methods | layout, construction; R3 fields Int64/Bool only | ✅ |
| R4 | `String` / interpolation | heap string + simple RC (dup on copy, drop at fn end) | ✅ |
| R5 | `Bytes` + mut | fixed heap buffer (fromString/count/get/set/toString); `mut` + reassignment | ✅ |
| R6 | `extern fn` libc | `from "c"` [+ C symbol]; marshal Int32/Int64/UInt64/String/Bytes/Void | ✅ |
| R7 | C-layout struct | packing | ✅ |
| R8 | modules / multi-file | loader | ✅ |
| R9 | HTTP example (or subset) | integration | ✅ subset: Net+Http modules, Bytes(n)/slice, bind/listen |

## 7. Repository layout

```text
ThoxaNative/
  Cargo.toml                 # workspace
  .cargo/config.toml         # cargo make → xtask
  README.md
  docs/
    decisions.md
    specs/                   # design docs (not docs/superpowers)
  crates/
    compiler/                # thoxa CLI + library
    make/                    # xtask: build/runtime/test helpers
  runtime/
    include/thoxa_rt.h
    src/*.c
  tests/conformance/         # .thx + .out
  target/                    # all build products (gitignored)
    rust/                    # Cargo (see .cargo/config.toml target-dir)
    c/                       # C runtime .o / .a
    thoxa/                   # emitted .c and linked binaries
```

### Conventions

- **Rust sources** live under `crates/`.
- **Build automation** lives in `crates/make` (xtask). Prefer `cargo make …` over shell scripts.
- **All build products** live under `target/`, grouped by kind (`rust/`, `c/`, `thoxa/`).

## 8. CLI

```bash
cargo run -p thoxa -- build path/to/file.thx -o name
cargo run -p thoxa -- run path/to/file.thx
cargo run -p thoxa -- emit-c path/to/file.thx
```

Via xtask (preferred day-to-day):

```bash
cargo make test
cargo make runtime
cargo make run tests/conformance/hello.thx
```

## 9. Testing

1. Rust unit tests in `crates/compiler`.
2. Conformance: `tests/conformance/*.thx` + optional `*.out`, driven by `cargo make test`.
3. Generated C should compile cleanly with the system `cc`.

## 10. Collaboration

Fundamental forks pause for human approval. Between forks, implement one rung at a time. Decision log is updated when locks change.

## 11. Non-goals (phase A)

- Algebraic effects
- Full Perceus reuse optimization
- QBE/LLVM backends
- Porting the entire Python surface before R0–R2 work
- Package manager / multi-crate Thoxa ecosystem
```