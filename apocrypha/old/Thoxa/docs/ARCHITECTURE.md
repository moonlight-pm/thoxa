# Thoxa Compiler Architecture

## Overview

The Thoxa compiler follows a traditional multi-phase architecture with a focus on simplicity and correctness over premature optimization.

## Compilation Pipeline

1. **Lexical Analysis** - Tokenization with camelCase identifier enforcement
2. **Parsing** - Recursive descent parser building AST
3. **Type Checking** - Basic type validation (full system coming)  
4. **Code Generation** - QBE IR generation
5. **Assembly** - QBE compiles IR to native assembly
6. **Linking** - Links with Thoxa runtime libraries

## Core Components

### Lexer (`src/lexer/`)
- Hand-written tokenizer with lookahead
- Enforces camelCase/PascalCase naming conventions
- Supports string literals with escape sequences
- Integer and float literals with type suffixes

### Parser (`src/parser/`)
- Recursive descent parser
- Generates strongly-typed AST
- Supports expressions, statements, and control flow
- Pattern matching with constructor parameter destructuring
- Union type syntax parsing (planned)

### AST (`src/ast.rs`)
- Complete type-safe representation
- Supports all Thoxa language constructs
- EnumVariant expression (being generalized to union variants)
- Match expressions with pattern support
- Prepared for generic types and unions

### Code Generator (`src/codegen/`)
- Converts AST to QBE intermediate representation
- Symbol table with lexical scoping
- Terminator tracking prevents dead code generation
- External function name mapping (camelCase → snake_case)

### Type System (`src/codegen/types.rs`)
- Built-in types: Int, Float, Bool, Char, String, Ptr
- Type registry for user-defined types
- Union types for composing sum types
- Generic type parameters (planned)
- Pattern matching on union variants

### Memory System (`runtime/thoxa_memory.c`)
- Automatic reference counting (ARC)
- Discriminated union approach for all heap types
- Type dispatch tables for polymorphic operations
- See [Memory Documentation](MEMORY.md) for details

### Runtime Libraries
- `thoxa_runtime.c` - Basic I/O functions
- `thoxa_memory.c` - Memory management with ref counting
- `thoxa_trial.c` - Trial<T,E> runtime support (being generalized)
- `thoxa_may.c` - May<T> optional type runtime (being generalized)

## Key Design Decisions

### Memory Management
Uses discriminated union with 16-byte universal header:
- Atomic reference counting for thread safety
- Type tags enable runtime type checking
- Flags field for future optimizations (SSO, interning)
- Single allocation API with type safety

### Value Representation
- Stack types: Primitives (Int, Bool, etc.)
- Heap types: Reference-counted with header
- Future: 16-byte uniform values for boxing

### Union Types and Error Handling
- Union types: `type Shape = Circle | Rectangle`
- Types are standalone, unions compose them
- Trial<T,E> and May<T> as union types:
  - `type Trial<T,E> = Pass<T> | Fail<E>`
  - `type May<T> = Has<T> | Not`
- Pattern matching on constructor parameters
- The ? operator for Trial propagation (planned)
- See [Error Handling Plan](../plans/ERROR_HANDLING.md)

### Control Flow
- Structured control flow (if, while, loop)
- Break/continue with proper terminator tracking
- For loops planned but syntax under consideration

## Build System

### Compilation Steps
1. `cargo build` - Build Thoxa compiler
2. `make runtime` - Build C runtime libraries
3. `thoxa file.thx` - Compile and run
4. `thoxa -c file.thx -o output` - Compile only

### Dependencies
- Rust toolchain for compiler
- QBE for IR → assembly compilation
- System C compiler for linking
- No external runtime dependencies

## Testing Strategy

### Unit Tests
- Parser tests for each construct
- Code generation tests with QBE verification
- Symbol table and scoping tests

### Integration Tests
- End-to-end compilation tests
- Memory leak detection
- Runtime behavior verification

## Future Enhancements

### Near Term
- Pattern matching for Trial<T,E>
- The ? operator for error propagation
- User-defined enums
- Generic type parameters

### Long Term
- Generic types
- Module system
- Interface types
- Custom allocators
- See [Future Optimizations](FUTURE.md)

## Related Documentation

- [Memory System](MEMORY.md) - Detailed memory management
- [Language Specification](LANGUAGE.md) - Syntax and semantics
- [Future Optimizations](FUTURE.md) - Planned improvements
- [Type System](TYPES.md) - Type system design