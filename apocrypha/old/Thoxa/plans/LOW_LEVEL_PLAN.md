# Low-Level Implementation Plan

## Overview
This document tracks the remaining foundational low-level work needed for Thoxa.

## Remaining Work

### 1. First-Class Functions
**Required for:** Functional programming patterns, higher-order functions
**Blocking:** `test_type_inference_generics.thx`

- [ ] Function type representation: `(Int, Int) -> Int`
- [ ] Function reference expressions (using function name without calling)
- [ ] Function pointer codegen in QBE
- [ ] Generic function references
- [ ] Closure support (capture environment variables)

### 2. Module System
**Required for:** Large programs, code organization

- [ ] Module dependency graph
- [ ] Initialization order resolution  
- [ ] Global variable support
- [ ] Static constructors/destructors
- [ ] Module import/export system

### 3. Weak References
**Required for:** Breaking reference cycles in ARC

- [ ] Weak reference type design
- [ ] Update refcounting for weak refs
- [ ] Cycle detection (debug mode)

### 4. Performance Optimizations

#### Boxing/Unboxing
- [ ] Audit current boxing operations
- [ ] Implement box elision where possible
- [ ] Explicit box/unbox operations

#### Varargs
- [ ] Design varargs syntax
- [ ] QBE varargs generation
- [ ] Type safety for varargs

## Priority Order
1. **First-Class Functions** - Unblocks remaining test failures
2. **Module System** - Essential for real programs
3. **Weak References** - When cycles become an issue
4. **Optimizations** - Performance tuning phase