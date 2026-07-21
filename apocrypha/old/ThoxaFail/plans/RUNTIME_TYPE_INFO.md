# Runtime Type Information Design

## Overview

This document outlines the design for runtime type information in Thoxa, which enables polymorphic programming with the Any type.

## Design Decisions

### 1. Memory Representation: Uniform Fat Pointers (128-bit)

**ALL values in Thoxa use fat pointers**:
```
[64 bits: type tag][64 bits: value or pointer]
```

This uniform representation means:
- Every value always carries its runtime type information
- No special handling needed for Any types
- No wrapping/unwrapping when assigning to Any variables
- Consistent memory layout simplifies code generation
- Enables runtime type checking for all values (useful for debugging, reflection)

Example:
```thoxa
let x: Int64 = 42;      # Stored as: [0x0000_0000_0000_0004][42]
let y: String = "Hi";   # Stored as: [0x0000_0000_0000_0010][ptr_to_string]
let z: Any = x;         # Just copies the fat pointer, no transformation needed
```

### 2. Type Tag Encoding: Hybrid Approach

```rust
// 64-bit type tag format:
// Bit 63: 0 = built-in type, 1 = pointer to TypeInfo
// Bits 0-62: type ID (if built-in) or pointer address (if custom)

// Built-in type IDs (bit 63 = 0)
0x0000_0000_0000_0000 = Void
0x0000_0000_0000_0001 = Int8
0x0000_0000_0000_0002 = Int16
0x0000_0000_0000_0003 = Int32
0x0000_0000_0000_0004 = Int64
0x0000_0000_0000_0005 = Uint8
0x0000_0000_0000_0006 = Uint16
0x0000_0000_0000_0007 = Uint32
0x0000_0000_0000_0008 = Uint64
0x0000_0000_0000_0010 = String
0x0000_0000_0000_0011 = Type
0x0000_0000_0000_0012 = Any
0x0000_0000_0000_0013 = Array

// Custom types (bit 63 = 1)
0x8000_0000_xxxx_xxxx = Pointer to TypeInfo struct
```

Benefits:
- Fast comparison for built-in types
- Extensible for user-defined types
- Supports runtime type creation

### Understanding the Any Type

**Important**: Any is not a "real" type that values have. It's a static type annotation that means "accept any type here". 

- Every value has a concrete type (Int64, String, User-defined, etc.)
- Values never "become" Any - they always retain their original type
- Any just tells the compiler to accept values of any type
- Since all values carry type tags, no special conversion is needed

```thoxa
fn example(param: Any) -> Void {
    # param could be Int64, String, or anything else
    # Its fat pointer contains the actual type tag
    if param is Int64 {
        # We can safely extract because we checked the type
        let n = param as Int64;
    }
}

# Calling with different types - all just pass their fat pointers
example(42);        # Passes [Int64_tag][42]
example("hello");   # Passes [String_tag][ptr]
example(myObject);  # Passes [MyType_tag][ptr]
```

### 3. Type Checking: Type Guards and Pattern Matching

**Phase 1: Type Guards**
```thoxa
if x is Int64 {
    # x is known to be Int64 in this scope
    let n = x as Int64;  # Safe cast
}
```

**Phase 2: Pattern Matching (future)**
```thoxa
match value {
    n: Int64 => print(n),
    s: String => printStr(s),
    _ => print("[unknown]")
}
```

### 4. Conversion Operators

Two distinct operators for type conversion:

#### `as` - Compile-time conversion
- Only works when types are known at compile time
- Compiler verifies conversion exists
- Never fails at runtime

```thoxa
let x: Int64 = 42;
let s = x as String;     # OK - compiler knows Int64→String exists

let y: Any = 42;
let s2 = y as String;    # COMPILE ERROR - cannot convert Any at compile time
```

#### `as?` - Runtime conversion
- Works with Any and other dynamic type scenarios
- Returns Result/Option monad (error path)
- Checks at compile time when possible, runtime when necessary

```thoxa
let x: Any = 42;
let s = x as? String;    # Returns Result<String, ConversionError>

# With future error handling syntax (TBD)
let s = try x as? String;  # Propagates error
let s = x as? String else "[error]";  # Default value
```

### 5. TypeInfo Structure

For custom types, the type tag points to:

```rust
struct TypeInfo {
    name: *const u8,           // Type name
    size: usize,               // Size in bytes
    conversions: *const Table, // Conversion functions
    methods: *const Table,     // Method implementations
}
```

## Implementation Plan

### Phase 1: Uniform Fat Pointers (✅ COMPLETE - January 2025)
1. **Update all value generation**:
   - Every value is created as a 128-bit fat pointer ✅
   - Numbers: `[Int64_tag][value]` ✅
   - Strings: `[String_tag][pointer]` ✅
   - Arrays: `[Array_tag][pointer]` ✅
   - User types: `[Custom_tag][pointer]` ✅

2. **Update memory layout**:
   - All stack slots are 16 bytes (2 x i64) ✅
   - All function parameters pass fat pointers (in progress)
   - All return values are fat pointers ✅

3. **Simplify Any handling**:
   - Any variables just store fat pointers directly ✅
   - No wrapping/unwrapping needed ✅
   - Type tag is always available ✅

### Implementation Details Completed:
- JIT updated to handle 128-bit returns while maintaining i64 interface
- All binary operations extract values, compute, then re-wrap
- Variable storage uses `value_type()` for 128-bit slots
- Array elements stored as 16-byte fat pointers
- Builtins extract values from fat pointers before operations
- Untyped expression handlers updated for compatibility

### Phase 2: Type Guards
1. **Implement `is` operator**:
   - Load first 64 bits of fat pointer (type tag)
   - Compare with expected type tag
   - Handle built-in vs custom types

2. **Type narrowing**:
   - After `if x is Type`, compiler knows x's type
   - Safe `as` casts within narrowed scope

### Phase 3: Runtime Conversions
1. **`as?` operator for runtime checks**
2. **Conversion lookup using type tags**
3. **Error handling for failed conversions**

### Phase 4: Full Integration
1. **Polymorphic print functions**
2. **TypeInfo generation**
3. **Reflection capabilities**

## Notes

- The error monad design is TBD but will be similar to Rust/Zig
- No unsafe casts - all conversions must be explicit and defined
- Types are first-class values, enabling runtime type creation