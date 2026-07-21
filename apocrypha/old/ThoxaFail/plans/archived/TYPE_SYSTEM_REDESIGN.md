# Type System Redesign Plan

## Overview

This document describes the redesign of Thoxa's type system, moving from a module-based approach to a type-based approach with explicit conversions.

## Status: ✅ COMPLETED (July 2025)

## Motivation

1. **Clarity**: The term "module" was confusing - we were really defining struct types
2. **Explicit Conversions**: Avoid JavaScript-style implicit coercion footguns
3. **Better Integration**: Types should be first-class with clear conversion semantics

## Design Decisions

### 1. Module → Type Keyword

**Before:**
```thoxa
module Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
}
```

**After:**
```thoxa
type Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
}
```

### 2. Explicit Conversions with `as`

**Syntax:**
```thoxa
let n: Int64 = 42;
let s: String = n as String;  # Explicit conversion
```

### 3. Conversion Definitions

Types can define how to convert to other types:

```thoxa
type Temperature {
    export let celsius: Int64;
    
    # Define conversion to String
    as String {
        "${celsius}°C"
    }
    
    # Define conversion to Fahrenheit
    as Fahrenheit {
        Fahrenheit { fahrenheit: celsius * 9 / 5 + 32 }
    }
}
```

### 4. Conversion Checking with `can`

Check if a conversion exists at runtime:

```thoxa
if value can String {
    let s: String = value as String;
    @printStr(s);
} else {
    @printStr("[unprintable]");
}
```

### 5. Automatic Conversion Chaining

The type system automatically finds conversion paths:

```thoxa
# If A can convert to B, and B can convert to C,
# then A can convert to C automatically
let c: C = a as C;  # Works even without direct A→C conversion
```

### 6. Template Literal Integration

Template literals automatically attempt conversions:

```thoxa
let age: Int64 = 25;
let message: String = "I am ${age} years old";  # age automatically converted to String
```

## Implementation Details

### AST Changes

- `ModuleDef` → `TypeDef`
- `ModuleMember` → `TypeMember`
- Added `Conversion` and `CanConvert` expression variants
- Added `Conversion` variant to `TypeMember` for conversion blocks

### Type System Changes

- `Module` type → `Struct` type with conversions field
- Added `conversions: HashMap<String, Type>` to track available conversions
- Implemented `find_conversion_path` for automatic chaining

### Parser Updates

- Removed `Module` token, added `As` and `Can` tokens
- Added parsing for conversion blocks in type definitions
- Added parsing for conversion and can-check expressions

### Type Checker Updates

- Type checking for conversion expressions
- Verification that conversions exist
- Template literal expressions now check for String conversions

### Code Generation

- Basic implementation for built-in conversions (Int→String)
- Runtime checking for `can` expressions
- TODO: User-defined conversion function calls

## Benefits

1. **Type Safety**: No implicit conversions prevent unexpected behavior
2. **Clarity**: Explicit `as` makes conversions visible in code
3. **Flexibility**: User-defined conversions allow rich type interactions
4. **Compatibility**: Template literals still "just work" with automatic conversions
5. **Discovery**: `can` keyword allows runtime conversion checking

## Future Work

1. **User-defined conversion code generation**: Generate actual function calls
2. **Error handling**: Better error messages for failed conversions
3. **Optimization**: Inline simple conversions
4. **Standard conversions**: Build a rich set of standard library conversions