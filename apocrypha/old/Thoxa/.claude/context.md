## Project Overview

Thoxa is a new systems programming language written in Rust.

### Development Commands

#### Building
- `cargo build` - Build the project in debug mode
- `cargo build --release` - Build optimized release version
- `cargo check` - Quick compile check without producing binary

#### Running
- `cargo run` - Build and run the project
- `cargo run --release` - Build and run in release mode

#### Testing
- `cargo test` - Run all tests
- `cargo test -- --nocapture` - Run tests with println! output visible
- `cargo test [test_name]` - Run specific test

#### Code Quality
- `cargo fmt` - Format code using rustfmt
- `cargo clippy` - Run Clippy linter for code improvements
- `cargo clippy -- -W clippy::pedantic` - Run with stricter linting

## File Organization

- **Source code**: `src/` - All Rust source files
- **Examples**: `examples/` - Example .thx programs
- **Tests**: `tests/` - Integration tests
- **Documentation**: `docs/` - Technical documentation
- **Plans**: `plans/` - Development plans and roadmaps

## References

- [Changelog](CHANGELOG.md)

### Planning Documents
- [Master Development Plan](plans/MASTER.md) - Main roadmap and implementation status
- [Code Generation Plan](plans/CODEGEN.md) - Incremental codegen implementation plan

### Technical Documentation
- [Language Specification](docs/LANGUAGE.md) - Complete language reference
- [Architecture](docs/ARCHITECTURE.md) - Compiler architecture and design
- [Type System](docs/TYPES.md) - Type system documentation

## Current Status

- **Lexer**: Complete with all tokens
- **Parser**: Fully implemented (Phases 1-12)
  - All expressions, statements, control flow
  - Functions, types, pattern matching
  - Arrays, try operator, module system
- **Code Generation**: Core features implemented
  - Functions with parameters and return values
  - Function calls with type checking
  - Variables, arithmetic, comparisons, assignments
  - If/else expressions with phi nodes
  - Loop expressions (infinite loops)
  - Block scoping and return statements
  - QBE IR generation
- **Compiler**: Full-featured command-line compiler
  - Compile-and-run mode (default)
  - Compile to native executables via QBE
  - Automatic cleanup of temporary files

## CLI Usage

```bash
# Run mode (default) - compile and execute immediately
thoxa examples/hello.thx                # Run file directly
thoxa examples/add.thx arg1 arg2        # Run with arguments

# Compile mode - generate executable
thoxa -c examples/hello.thx             # Compile to a.out
thoxa -c examples/hello.thx -o myapp    # Compile with custom name

# Example:
thoxa examples/calculator.thx  # Returns exit code 32
```

## Key Design Principles

1. **Simplicity**: Keep the language and implementation simple
2. **Correctness**: Type safety and memory safety are paramount
3. **Performance**: Fast compilation and efficient runtime
4. **Clarity**: Clear error messages and predictable behavior
5. **Modularity**: Clean separation of compiler phases
