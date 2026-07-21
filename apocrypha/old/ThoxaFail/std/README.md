# Thoxa Standard Library

This directory contains the Thoxa standard library modules, written in Thoxa itself.

## Core Modules

### Input/Output

- `Io.thx` - Basic I/O operations with low-level syscall wrappers
- `IoVariadic.thx` - I/O with variadic functions for flexible printing
- `IoAny.thx` - Polymorphic I/O using the Any type (work in progress)

### Type System

- `Types.thx` - Built-in type definitions and aliases

## Usage

Standard library modules can be imported like any other Thoxa module:

```thoxa
import Io from "std/Io";

let io: Io;
io.println("Hello, World!");
```

## Design Philosophy

The Thoxa standard library is:

1. **Self-hosted** - Written in Thoxa itself
2. **Minimal** - Only essential functionality
3. **Safe** - Type-safe with runtime checks where needed
4. **Portable** - Platform differences abstracted away

## Current Status

The standard library is still evolving. Current limitations:

- I/O operations use direct syscalls (macOS only for now)
- Polymorphic printing with Any type needs runtime type checking
- Limited string manipulation functions
- No heap allocation support yet

## Future Plans

- Cross-platform I/O abstraction
- Complete polymorphic print implementation
- String manipulation functions
- Memory management utilities
- Collections (arrays, maps, etc.)
- File system operations
- Network operations