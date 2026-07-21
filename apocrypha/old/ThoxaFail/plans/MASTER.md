# Thoxa Master Development Plan

## Overview

This document contains the master development plan for the Thoxa programming language. It tracks completed work, current implementation status, and future roadmap.

## Implementation Status

### ✅ Completed Features

1. **Module System Phase 1**:
   - [x] Proper memory allocation for module instances
   - [x] Execute module initialization code during instantiation
   - [x] Generate correct code for field access on any expression
   - [x] Implement reference code generation for pointers
   - [x] Fix AOT compilation to return actual values

2. **Function Calls (Phase 2)**:
   - [x] Function call syntax `func(arg1, arg2)` - parsing complete
   - [x] Symbol resolution for functions - functions stored in symbol table
   - [x] Function compilation to cranelift - functions are real compiled code
   - [x] Parameter passing and return values - working with direct calls
   - [x] C ABI calling convention - SystemV on Unix for C compatibility
   - [x] Module member functions - compiled with proper signatures
   - [x] Direct function execution tests - functions execute correctly
   - [x] Function calls within expressions - fully working with cranelift call instructions

3. **File-based Modules (Phase 3)**:
   - [x] Import statement parsing (`import Name from "path"`, destructured imports)
   - [x] Module resolver for file paths (with .thx extension handling)
   - [x] File module compilation (implicit module wrapping for .thx files)
   - [x] Module naming from file names (PascalCase.thx convention)
   - [x] Integration with main compilation pipeline
   - [x] Import binding and symbol resolution
   - [x] Cross-module dependency compilation
   - [x] Codegen integration for imported symbols
   - [x] Function calls on imported modules
   - [x] Field access on imported modules
   - [x] Destructured imports (`import { add, pi } from "./Math"`)
   - [x] Standard library with pure Thoxa I/O (`std/Io.thx`)
   - [x] Intra-module function calls with implicit module binding

4. **Type System Phase 1**:
   - [x] Basic type checking infrastructure - Type enum and TypedExpr
   - [x] Type checker module with expression validation
   - [x] Integration into compilation pipeline (JIT and AOT)
   - [x] Type error reporting with clear messages
   - [x] Support for new type names (Int64, Uint64, String, etc.)
   - [x] Type environment tracking for variables, functions, and modules
   - [x] Type checking for all expressions, let bindings, module members, and function calls
   - [x] Type compatibility checking with safe integer widening (Int32 → Int64)
   - [x] Prevent unsafe narrowing conversions (Int64 → Int32) and signed/unsigned mismatches
   - [x] Types as first-class values - can store types in variables and compare them
   - [x] Built-in types available without imports
   - [x] Support for type imports from modules

5. **Control Flow and Core Features**:
   - [x] Control flow (if/else) - conditionals fully working
   - [x] Basic loops (loop/break/continue) - implemented with proper control flow
   - [x] Proper String type - length-prefixed with O(1) operations
   - [x] String builtins - `@strlen(s)` for O(1) length, `@cstr(s)` for C interop
   - [x] Expression sequences with semicolons
   - [x] Block expressions with braces

6. **Printing API Foundation**:
   - [x] Template literals with `${expr}` interpolation in regular strings
   - [x] Duck typing infrastructure - method resolution based on structural compatibility
   - [x] Method call syntax - `object.method(args)`
   - [x] Built-in toString() methods for String and Int types
   - [x] Type system tracks methods on types
   - [x] Compile-time method checking with clear error messages
   - [x] Variadic function syntax parsing (`...args: Type`)

7. **Array Types**:
   - [x] Array literal syntax `[elem1, elem2, ...]`
   - [x] Array indexing syntax `array[index]`
   - [x] Array type in type system with element type and size
   - [x] Type checking for homogeneous arrays
   - [x] Built-in array methods: `length()` and `toString()`
   - [x] Comment handling fix in custom lexer

8. **Type System Redesign** (July 2025):
   - [x] Replace `module` keyword with `type` keyword
   - [x] Add explicit type conversions with `as` keyword
   - [x] Add conversion checking with `can` keyword
   - [x] Type conversion blocks in type definitions
   - [x] Automatic conversion chaining (A→B→C paths)
   - [x] Template literals with automatic conversions
   - [x] Built-in conversions for Int→String

9. **Arrays and Variadic Functions** (July 2025):
   - [x] Array literal syntax and indexing
   - [x] Array type in type system
   - [x] Array memory layout (length-prefixed)
   - [x] Built-in array methods: length(), toString()
   - [x] Bounds checking for array access
   - [x] Variadic function syntax (`...args: Type`)
   - [x] Variadic function type checking and code generation
   - [x] Automatic array creation from variadic arguments

8. **Tagged Pointer System** (August 2025):
   - [x] Simplified to exclusively 64-bit tagged pointers
   - [x] Removed all conditional pointer type compilation
   - [x] Consistent tagging scheme for all values
   - [x] Major codebase simplification

9. **Codebase Refactoring** (August 2025):
   - [x] Split large monolithic files into manageable modules
   - [x] src/codegen/expr.rs → 7 modules under 350 lines
   - [x] src/type_checker.rs → 4 focused modules
   - [x] src/parser.rs → 4 logical modules
   - [x] Improved maintainability for context-limited environments

### 🚧 Current Focus

**Printing API** (see plans/PRINTING.md for details):
- [x] Any type added to type system ✅
- [x] Runtime type information for Any values ✅
- [x] Type guards with `is` operator ✅
- [x] Runtime conversions with `as` operator ✅
- [ ] Implement spread operator (`...array`)
- [ ] Create polymorphic print functions with Any
- [ ] Update examples to use new print functions
- [ ] Deprecate @print and @printStr builtins

**Type System Enhancements**:
- [ ] User-defined conversion code generation
- [ ] Array type syntax parsing (`[Int64]`, `[String; 10]`)
- [ ] Reserve global type names (Type, Any, Never, Void)
- [ ] Proper integer type code generation (not just i64)
- [ ] Type aliases (`type Size = Uint64;`)

### 📋 Future Roadmap

1. **Type System Completion**:
   - [ ] Generic types and type parameters
   - [ ] Type inference for local variables
   - [ ] Structural typing for modules
   - [ ] Type-level computation
   - [ ] Compile-time type checking for all operations
   - [ ] Method overloading

2. **Language Features**:
   - [ ] Standard library print functions (replace builtins)
   - [ ] Error handling with Result types
   - [ ] Pattern matching
   - [ ] For loops with iterators
   - [ ] Heap allocation
   - [ ] Closures
   - [ ] Traits/interfaces
   - [ ] Generics

3. **Runtime and Performance**:
   - [ ] Garbage collection or manual memory management
   - [ ] Optimization passes in code generation
   - [ ] Inline caching for method calls
   - [ ] Link-time optimization support

4. **Developer Experience**:
   - [ ] Language server protocol (LSP) implementation
   - [ ] Debugger support
   - [ ] Package manager
   - [ ] Standard library expansion
   - [ ] Documentation generator

5. **Platform Support**:
   - [ ] Windows support
   - [ ] Linux ARM64 support
   - [ ] WebAssembly target
   - [ ] Cross-compilation support

## Current Limitations

- **Type System**: 
  - No generics or type parameters
  - All integers generate as i64 in codegen (type checking validates correctly though)
  - No type aliases yet
  - No type inference (all types must be explicit)

- **Memory Management**:
  - No heap allocation (only stack allocation)
  - Fixed-size stack slots for modules
  - Arrays use stack-based allocation with length prefix

- **Missing Features**:
  - No while/for loops (only basic loop/break)
  - No comptime evaluation yet
  - No nested modules (modules can only contain let bindings and functions)
  - String operations limited (no concat, slice, etc. yet)

## Technical Debt

1. **AOT Compilation Issues**:
   - Linking failures with recent string type changes
   - Alignment warnings in generated object files

2. **Code Generation**:
   - Integer types all compile to i64 regardless of declared type
   - Module function pointers stored but not used for dynamic dispatch

3. **Parser**:
   - Empty blocks return `Block([Void])` instead of `Block([])`
   - Some AST inconsistencies between single expressions and blocks

## Implementation Notes

- The compiler uses a hybrid library/binary crate structure
- Common code generation logic extracted to `codegen.rs`
- JIT and AOT specific implementations in separate files
- Functions compile to real cranelift functions with C ABI
- Module instances use stack memory with proper alignment
- Symbol table tracks all definitions across compilation
- Type checking happens before code generation

## Testing Strategy

- Unit tests for each compiler phase (lexer, parser, type checker, codegen)
- Integration tests for end-to-end compilation
- Example programs demonstrating all features
- Test coverage for error conditions and edge cases