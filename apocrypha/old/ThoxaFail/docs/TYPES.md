# Thoxa Type System

## Overview

Thoxa uses a unified type system where **types are first-class values**. User-defined types (structs) are created with the `type` keyword. This creates a clean, consistent system inspired by Zig's compile-time type manipulation.

## Core Principles

1. **Types are values**: Can be stored, passed, and manipulated at compile time
2. **Explicit conversions**: No implicit type coercion - use `as` for conversions
3. **Explicit typing**: No type inference initially - all types must be explicit
4. **PascalCase convention**: Types use PascalCase, values use camelCase

## Type Hierarchy

```
Type                    # The type of all types
├── Void               # Unit type for side effects
├── String             # UTF-8 string type
├── Integer Types      # Int8..Int64, Uint8..Uint64
├── Array Types        # [ElementType] for homogeneous arrays
└── User Types         # User-defined types with `type` keyword
```

## Built-in Types

### Primitive Types

```thoxa
# Built-in types are available without imports
let x: Int64 = 42;
let msg: String = "Hello";
let nothing: Void = Void;

# Types are values - can be stored in variables
let MyInt: Type = Int64;
let y: MyInt = 100;  # Same as: let y: Int64 = 100;

# Types can be compared
if MyInt == Int64 {
    @printStr("Types are just values!");
};
```

### Integer Types

| Type | Description | Range |
|------|-------------|-------|
| `Int8` | 8-bit signed | -128 to 127 |
| `Int16` | 16-bit signed | -32,768 to 32,767 |
| `Int32` | 32-bit signed | -2³¹ to 2³¹-1 |
| `Int64` | 64-bit signed | -2⁶³ to 2⁶³-1 |
| `Uint8` | 8-bit unsigned | 0 to 255 |
| `Uint16` | 16-bit unsigned | 0 to 65,535 |
| `Uint32` | 32-bit unsigned | 0 to 2³²-1 |
| `Uint64` | 64-bit unsigned | 0 to 2⁶⁴-1 |

### String Type

```thoxa
# Strings are UTF-8 encoded text
let message: String = "Hello, Thoxa!";

# Currently strings are represented as pointers
# Future: Length-prefixed for safety
```

### Type Aliases

```thoxa
# Common aliases defined in std/Types
Byte  # Alias for Uint8
Size  # Alias for Uint64 (for sizes/indices)
Char  # Alias for Uint8 (UTF-8 code unit)
```

### Special Types

- `Type` - The type of types themselves
- `Void` - Unit type for functions with no meaningful return

## Creating Types

### Integer Types via Builtin

```thoxa
# The @intType builtin creates integer types
let MyInt: Type = @intType(true, 32);   # Signed 32-bit
let MyUint: Type = @intType(false, 16); # Unsigned 16-bit
```

### User-Defined Types

New types are created with the `type` keyword:

```thoxa
type Point {
    export let x: Int64 = 0;
    export let y: Int64 = 0;
}

# Point is now a type
let p: Point;  # Create instance of Point type
```

## Type Usage Examples

### Basic Type Usage

```thoxa
import { Int32, String, Void } from "../std/Types";

# Function with typed parameters
type Math {
    export fn add(a: Int32, b: Int32) -> Int32 {
        a + b
    };
    
    export fn greet(name: String) -> Void {
        @printStr("Hello, ");
        @printStr(name);
        @printStrLn("!");
    };
}
```

### Type as Parameter

```thoxa
# Types can be passed as compile-time parameters
fn identity(T: Type, value: T) -> T {
    value
}

let x: Int64 = identity(Int64, 42);
let s: String = identity(String, "hello");
```

## Type Conversions

Thoxa uses explicit type conversions to avoid implicit coercion bugs. All conversions must be explicitly requested using the `as` keyword.

### Basic Conversions

```thoxa
let n: Int64 = 42;
let s: String = n as String;  # Explicit conversion to String
```

### Defining Conversions

Types can define how to convert to other types:

```thoxa
type Temperature {
    export let celsius: Int64;
    
    # Define conversion to String
    as String {
        "${celsius}°C"
    }
}

let temp: Temperature = Temperature { celsius: 25 };
let display: String = temp as String;  # "25°C"
```

### Checking Conversions

Use the `can` keyword to check if a conversion exists:

```thoxa
if value can String {
    let s: String = value as String;
    @printStr(s);
} else {
    @printStr("[cannot convert to String]");
}
```

### Conversion Chaining

The type system automatically finds conversion paths. If A can convert to B, and B can convert to C, then A can convert to C:

```thoxa
# Celsius → Kelvin → String
type Celsius {
    export let value: Int64;
    
    as Kelvin {
        Kelvin { value: value + 273 }
    }
}

type Kelvin {
    export let value: Int64;
    
    as String {
        "${value}K"
    }
}

let c: Celsius = Celsius { value: 25 };
let s: String = c as String;  # Works via Celsius→Kelvin→String
```

### Template Literal Integration

Template literals automatically attempt conversions to String:

```thoxa
let age: Int64 = 25;
let name: String = "Alice";
let msg: String = "${name} is ${age} years old";  # age automatically converted
```

If a value cannot be converted to String, template literals will use "[unprintable]" as a fallback.

## Legacy Compatibility

For backward compatibility, the old type names are still available but deprecated:

- `I8`, `I16`, `I32`, `I64` → Use `Int8`, `Int16`, `Int32`, `Int64`
- `U8`, `U16`, `U32`, `U64` → Use `Uint8`, `Uint16`, `Uint32`, `Uint64`

## Future Type System Features

### Generic Types
```thoxa
# Planned syntax
module Array(T: Type, N: Size) {
    # Fixed-size array of T
}

let numbers: Array(Int32, 10);
```

### Type Constraints
```thoxa
# Planned syntax
fn add(T: Type, a: T, b: T) -> T 
    where T: Numeric 
{
    a + b
}
```

### Compile-time Type Manipulation
```thoxa
# Planned syntax
fn makeStructType(fields: []Field) -> Type {
    # Create new struct type at compile time
}
```

## Type Checking Strategy

The type system is being implemented in phases:

1. **Phase 1** ✅: Basic type checking for primitives and modules
   - Verify type annotations match usage
   - Check function parameter and return types
   - Ensure module field types are respected
   - Types work as first-class values
   - Built-in types available without imports
   - Type compatibility checking with safe widening coercions
   - Integer coercion rules (Int32 → Int64, but not Int64 → Int32)

2. **Phase 2**: Type inference for local variables
   - Infer types from initialization expressions
   - Still require explicit types for function signatures

3. **Phase 3**: Generic types and constraints
   - Type parameters for modules and functions
   - Constraint system for type bounds

4. **Phase 4**: Advanced features
   - Compile-time type manipulation
   - Structural typing
   - Type-level computation