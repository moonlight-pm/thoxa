# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Thoxa is a new systems programming language written in Rust.

## Key Instructions

### Memory Management
- **Always keep CLAUDE.md, README.md, CHANGELOG.md, and /plans and /docs up-to-date**
- **Always make sure all tests run**
- **Update CHANGELOG.md when making significant changes**
- Do what has been asked; nothing more, nothing less
- NEVER create files unless they're absolutely necessary for achieving your goal
- ALWAYS prefer editing an existing file to creating a new one
- NEVER proactively create documentation files (*.md) or README files unless explicitly requested
- Try to keep source files under 20k bytes by splitting out into new files if necessary

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

### Planning Documents
- [Master Development Plan](plans/MASTER.md) - Main roadmap and implementation status
- [Printing API Plan](plans/PRINTING.md) - Remaining work for polymorphic print functions
- [Runtime Type Info](plans/RUNTIME_TYPE_INFO.md) - Advanced features for runtime type system

### Technical Documentation
- [Language Specification](docs/LANGUAGE.md) - Complete language reference
- [Architecture](docs/ARCHITECTURE.md) - Compiler architecture and design
- [Module System](docs/MODULES.md) - Module and import system details
- [Type System](docs/TYPES.md) - Type system documentation
- [Code Generation Types](docs/CODEGEN_TYPES.md) - Code generation for different types

## CLI Usage

```bash
# New CLI interface
thoxa examples/HelloWorld.thx           # Run file in JIT mode
thoxa -c examples/Working.thx           # Compile to a.out
thoxa -c examples/Working.thx -o myapp  # Compile with custom name
thoxa -e "1 + 2 * 3"                   # Evaluate expression (returns 7)
```

## Current Focus Areas

1. Complete polymorphic printing API with spread operator
2. Fix trap execution for runtime type mismatches
3. Implement user-defined type conversions
4. Expand standard library
5. Improve error messages and diagnostics

## Testing Requirements

Before committing any changes:
1. Run `cargo test` to ensure all tests pass
2. Run `cargo clippy` to check for code quality issues
3. Run `cargo fmt` to ensure consistent formatting
4. Test both JIT and AOT compilation modes
5. Verify examples still work correctly

## Architecture Notes

The project uses a modular structure with focused modules:
- `src/lib.rs` - Library root exporting all modules
- `src/main.rs` - CLI binary using the library
- `src/codegen/` - Code generation modules:
  - `expr/` - Expression translation split into 7 focused modules
  - `jit.rs` - JIT-specific implementation
  - `aot.rs` - AOT-specific implementation
  - `typed_expr/` - TypedExpr translation modules
- `src/parser/` - Parser modules:
  - `expressions.rs` - Complex expression parsing
  - `types.rs` - Type definition parsing
  - `symbols.rs` - Symbol table management
- `src/type_checker/` - Type checking modules:
  - `expressions.rs` - Basic expression type checking
  - `advanced.rs` - Advanced features type checking
  - `resolution.rs` - Type resolution utilities

## Key Design Principles

1. **Simplicity**: Keep the language and implementation simple
2. **Correctness**: Type safety and memory safety are paramount
3. **Performance**: Fast compilation and efficient runtime
4. **Clarity**: Clear error messages and predictable behavior
5. **Modularity**: Clean separation of compiler phases