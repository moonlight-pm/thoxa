# Thoxa Programming Language

Thoxa is a systems programming language that compiles to native code via QBE intermediate representation.

## Installation

### Prerequisites

1. **Rust** (latest stable)
2. **QBE** - The QBE compiler backend
   - macOS: `brew install qbe`
   - Linux: Build from source at https://c9x.me/compile/
3. **C compiler** (cc/gcc/clang) for linking

### Building

```bash
# Build everything (compiler + runtime)
make

# Or build separately
cargo build --release
make runtime
```

## Usage

```bash
# Run a program
thoxa program.thx

# Compile to executable
thoxa -c program.thx -o myprogram

# Run tests
cargo test
```

## Example

```thoxa
fn main() -> Int {
    println("Hello, World!");
    0
}
```

## Documentation

- Language documentation: `docs/LANGUAGE.md`
- Implementation status: `PROGRESS.md`
- Development plans: `plans/`

## License

MIT OR Apache-2.0