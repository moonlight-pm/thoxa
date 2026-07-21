# Code Generation for Different Types

This document describes how different types are handled during code generation in Thoxa.

## Overview

Thoxa's code generation phase translates typed AST nodes into Cranelift IR (Intermediate Representation). Each type requires specific handling for memory layout, operations, and runtime behavior.

## Tagged Pointers (Uniform Value Representation)

As of January 2025, Thoxa uses tagged pointers for ALL values:

```
64-bit tagged pointer:
- 0bXXXXXXX1 = small integer (63-bit signed)
- 0bXXXXX000 = pointer to heap object (8-byte aligned)
- 0bXXXXX010 = false
- 0bXXXXX110 = true
- 0bXXXX0100 = nil/null
- 0bXXXX1100 = undefined (reserved)
- 0bXXXXX100 = (reserved for future special values)
```

This representation enables:
- Efficient value representation without boxing for common cases
- Runtime type information through tags and object headers
- The `Any` type for polymorphic programming
- Type guards and safe runtime conversions

### Type Identification

For tagged pointers:
- **Integers**: Check tag bits == 0x1
- **Booleans**: Check value == 0x2 (false) or 0x6 (true)
- **Pointers**: Check tag bits == 0x0, then read type ID from object header

Object header format (at beginning of heap objects):
```c
struct ObjectHeader {
    u32 type_id;    // BuiltinTypeId or custom type ID
    u32 size;       // Object size in bytes
}
```

### Operations with Tagged Pointers

All operations extract values before computation:

```rust
// Binary operation example for integers
let left_tagged = translate_typed_expr(left, ...);
let right_tagged = translate_typed_expr(right, ...);

// Extract actual values (shift right by 3 for integers)
let left_val = extract_value(builder, left_tagged);
let right_val = extract_value(builder, right_tagged);

// Perform operation
let result = builder.ins().iadd(left_val, right_val);

// Tag result as integer (shift left by 3, OR with 0x1)
tag_integer(builder, result)
```

## Integer Types

All integers are stored as tagged values with tag 0x1:
- Value is stored in upper 61 bits (63-bit signed range)
- Operations shift right by 3 to extract value
- Results are shifted left by 3 and OR'd with 0x1

Currently all integer types (Int8, Int16, Int32, Int64) use the same representation. Type checking ensures proper bounds at compile time.

## String Type

Strings are heap-allocated objects with tag 0x0 (pointer tag):

Object layout:
```
[0..4]    type_id (u32) = BuiltinTypeId::String  
[4..8]    size (u32)
[8..16]   length (u64) - actual string length, NOT including null
[16..16+N] data (bytes) - UTF-8 encoded string content  
[16+N]    null (u8)    - always '\0' for C compatibility
```

Operations:
- `@strlen(s)`: Returns length field (O(1))
- `@cstr(s)`: Returns pointer to data (for C interop)

## Array Type

Arrays are heap-allocated objects with tag 0x0 (pointer tag):

Object layout:
```
[0..4]     type_id (u32) = BuiltinTypeId::Array
[4..8]     size (u32)
[8..16]    length (u64) - number of elements
[16..16+N*8] elements (tagged pointers, 8 bytes each)
```

Operations:
- `array[index]`: Bounds-checked indexing
- `array.length()`: Returns element count

## Struct Types

User-defined types are heap-allocated with custom type IDs:

```rust
type Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
}
```

Memory layout:
- Object header with custom type_id
- Each field is a tagged pointer (8 bytes)
- Fields are stored at offsets 8, 16, 24, etc.
- Field access compiles to pointer arithmetic

## The Any Type

`Any` is not a runtime type but a static annotation meaning "accept any type". Since all values are tagged, runtime type checking is always available:

```thoxa
fn process(value: Any) -> Void {
    if value is Int64 {
        # Type narrowed to Int64 here
        let n = value as Int64;
    }
}
```

## Type Guards and Conversions

Type guards check tags and object headers at runtime:

```rust
// For integers: check tag == 0x1
let tag_mask = builder.ins().iconst(types::I64, 0x7);
let tag = builder.ins().band(value, tag_mask);
let is_int = builder.ins().icmp_imm(IntCC::Equal, tag, 0x1);

// For booleans: check value == 0x2 or 0x6
let is_false = builder.ins().icmp_imm(IntCC::Equal, value, 0x2);
let is_true = builder.ins().icmp_imm(IntCC::Equal, value, 0x6);
let is_bool = builder.ins().bor(is_false, is_true);

// For objects: check tag == 0x0, then load type_id from header
let is_ptr = builder.ins().icmp_imm(IntCC::Equal, tag, 0x0);
// If pointer, load type_id from offset 0
```

## Boolean Type

Booleans use special tagged values:
- `false`: 0x2 (tag 010)
- `true`: 0x6 (tag 110)

No heap allocation needed - the entire value fits in the tagged pointer.

## Future Enhancements

1. **Inline small strings**: Use remaining tag patterns for short strings
2. **Custom type conversions**: User-defined `as` implementations
3. **Pattern matching**: Match on type tags for exhaustive handling
4. **Reflection API**: Expose type information to user code