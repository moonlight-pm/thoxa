# Thoxa Architecture

## Overview

Thoxa is a systems programming language built with a modular architecture using Rust. The compiler uses a traditional pipeline with modern libraries for each stage.

## Compiler Pipeline

```
Source Code (.thx)
    ↓
Lexer (logos)
    ↓
Parser (chumsky)
    ↓
Symbol Table & Type Checking
    ↓
Code Generation (cranelift)
    ↓
JIT/AOT Output
```

## Project Structure

```
src/
├── main.rs           # CLI entry point
├── lib.rs           # Library root
├── lexer.rs         # Token definitions and lexical analysis
├── parser.rs        # AST construction with chumsky
├── ast.rs           # Abstract syntax tree types
├── symbols.rs       # Symbol table and name resolution
├── modules.rs       # File-based module system
├── import_resolver.rs # Import resolution and binding
├── codegen.rs       # Shared code generation logic
├── codegen_jit.rs   # JIT-specific implementation
├── codegen_aot.rs   # AOT-specific implementation
└── codegen_symbols.rs # Symbol resolution for codegen

std/
└── Io.thx           # Standard library I/O module

examples/
├── HelloWorld.thx   # Basic examples
├── KitchenSink.thx  # Comprehensive feature demo
└── ...             # Various feature demonstrations
```

## Key Design Decisions

### Type System
- **Types as first-class values**: Types can be manipulated at compile time (Zig-inspired)
- **Modules are types**: Unifies module system with type system
- **No implicit conversions**: Everything must be explicit

### Memory Management
- **Stack allocation only** (currently): All values live on the stack
- **Reference safety**: Compile-time validation prevents dangling references
- **Explicit references**: `&` required for taking addresses

### Module System
- **File-based modules**: Each .thx file is implicitly a module
- **Export control**: `export` keyword for public members
- **Import system**: Path-based imports with destructuring support
- **Circular dependency detection**: DFS-based cycle detection

### Function System
- **Module-scoped functions**: Functions exist within modules
- **Implicit module binding**: Functions can access module members without `self`
- **Two-phase compilation**: Declaration then definition for mutual recursion
- **C ABI compatibility**: SystemV calling convention for future FFI

## Implementation Status

### Completed
- Basic compiler pipeline (lexer → parser → codegen)
- JIT and AOT compilation modes
- Module system with imports/exports
- Function calls with recursion support
- String literals and print builtins
- Control flow (if/else, loops with break/continue)
- Reference expressions and memory safety
- Circular dependency detection

### In Progress
- Type checking infrastructure
- Advanced control flow (while/for loops)
- Arrays and compound types

### Future
- Heap allocation
- Compile-time evaluation
- Generic types
- FFI support