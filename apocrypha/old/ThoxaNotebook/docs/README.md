# Thoxa Documentation

This directory contains the design and implementation documentation for the Thoxa programming language interpreter, implemented in Python.

## Documentation Structure

### Core Documents

1. **[LANGUAGE.md](LANGUAGE.md)** - Complete language specification
   - Syntax and semantics
   - Type system overview
   - Control flow constructs
   - Pattern matching
   - Error handling with Trial/May types
   - Module system

2. **[ARCHITECTURE.md](ARCHITECTURE.md)** - Interpreter architecture
   - System overview
   - Core components (Lexer, Parser, Interpreter)
   - Python implementation approach
   - Memory management via Python GC
   - Testing strategy

3. **[TYPE_SYSTEM.md](TYPE_SYSTEM.md)** - Type system design
   - Static typing with inference
   - Value representation
   - Generic types (May, Trial)
   - Pattern matching implementation
   - Interface system

4. **[IMPLEMENTATION.md](IMPLEMENTATION.md)** - Development guide
   - Project structure
   - Core component implementations
   - Testing approach
   - Development workflow
   - Performance considerations
   - Future optimizations

## Quick Start

If you're new to Thoxa, read the documents in this order:

1. **LANGUAGE.md** - Understand the language design
2. **ARCHITECTURE.md** - Learn how the interpreter works
3. **IMPLEMENTATION.md** - Start contributing

## Implementation Status

The Thoxa interpreter is being developed in Python as a tree-walking interpreter. This approach provides:

- Rapid prototyping and iteration
- Clear, readable implementation
- Excellent debugging capabilities
- Easy integration with Python ecosystem

### Completed Features
- ✅ Lexical analysis with camelCase enforcement
- ✅ Recursive descent parser
- ✅ Basic type system
- ✅ Control flow (if, loop, while)
- ✅ Functions with named/positional parameters
- ✅ Pattern matching design
- ✅ Union types (May, Trial)

### In Progress
- 🚧 Full interpreter implementation
- 🚧 Type inference
- 🚧 Pattern matching runtime
- 🚧 Module system
- 🚧 Standard library

### Planned
- 📋 REPL
- 📋 Bytecode compilation
- 📋 Language server protocol
- 📋 Package manager

## Contributing

See [IMPLEMENTATION.md](IMPLEMENTATION.md) for development setup and contribution guidelines.

## Design Philosophy

Thoxa aims to be:
- **Simple** - Easy to learn and understand
- **Safe** - Static typing with automatic memory management
- **Expressive** - Pattern matching, generics, clean syntax
- **Practical** - Good error messages, useful built-ins

The Python implementation prioritizes clarity and correctness over performance, making it ideal for:
- Language design experimentation
- Educational purposes
- Prototyping
- Small to medium programs

Future optimizations (PyPy, Cython, bytecode) will improve performance while maintaining the clean implementation.