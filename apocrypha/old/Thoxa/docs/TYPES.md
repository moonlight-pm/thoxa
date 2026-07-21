# Thoxa Type System

## Core Design: Static Typing with Runtime Introspection

Thoxa uses static type checking at compile time for safety and performance, while maintaining runtime type information for introspection, debugging, and safe downcasting. All functions and types must be known at compile time - there is no runtime code generation or evaluation.

## Type Categories

Thoxa has several categories of types:

1. **Primitive Types**: Int, Float, Bool, Char, String, Void
2. **Units**: User-defined composite types (structs/classes) defined with `type` keyword
3. **Modules**: Singleton units loaded from files
4. **Enums**: Sum types with variants (future)
5. **Generic Types**: Parameterized types like Array<T>, Ptr<T>, Trial<T,E>
6. **Function Types**: First-class function types like (Int, Int) -> Int

Units and modules share the same structure - modules are just singleton units instantiated from files.

## Value Representation

Every value in Thoxa is 16 bytes:

```c
struct Value {
    Type* type;     // 8 bytes - pointer to type object
    uint64_t data;  // 8 bytes - inline data or pointer
};
```

## Type Representation

Types themselves are heap-allocated structures:

```c
struct Type {
    Type* type;           // Type of this type (self-referential for base Type)
    const char* name;     // Human-readable name
    size_t size;          // Size of values of this type
    size_t alignment;     // Memory alignment requirement
    
    // For parameterized types (Ptr<T>, Array<T>)
    Type* base_type;      // Points to Ptr or Array base type
    Type** parameters;    // Type parameters
    size_t param_count;   // Number of parameters
    
    // Type-specific data
    union {
        struct {
            Type* element_type;
            size_t length;
        } array;
        
        struct {
            size_t field_count;
            Field* fields;
        } struct_;
        
        struct {
            Type* return_type;
            size_t param_count;
            Type** param_types;
        } function;
    } data;
};
```

## Parameterized Types

Built-in parameterized types like `Ptr<T>` and `Array<T>` share base implementations:

```c
// Single base types
static Type PTR_BASE_TYPE = { .name = "Ptr", ... };
static Type ARRAY_BASE_TYPE = { .name = "Array", ... };

// Ptr<Int> is created as:
Type* ptr_int_type = {
    .type = &TYPE_TYPE,
    .name = "Ptr<Int>",
    .base_type = &PTR_BASE_TYPE,
    .parameters = { &INT_TYPE },
    .param_count = 1,
    ...
};
```

This approach:
- Avoids creating separate pointer implementations for each type
- Maintains type safety (Ptr<Int> ≠ Ptr<Float>)
- Enables future user-defined generic types

## Built-in Enum-like Types

Thoxa provides built-in `May<T>` and `Trial<T, F>` types that support multiple constructors:

### May<T>
- `Has(x)` - Creates May<T> containing value x
- `Not` - Represents absence of value

### Trial<T, F>
- `Pass(x)` - Creates Trial<T, F> with success value
- `Fail(f)` - Creates Trial<T, F> with failure value

These are implemented as built-in types with compiler support for:
- Efficient tagged union representation in 16 bytes
- Type parameter caching to avoid proliferation
- The `?` operator for Trial propagation

## Type Interning

Types are interned (deduplicated) to ensure pointer equality works:

- `Int` always points to the same Type object
- `Array(Int, 5)` creates one Type object, reused everywhere
- Structural equality via pointer comparison

## Benefits

1. **First-class types**: Pass types to functions, store in variables
2. **Runtime introspection**: Query any value's type
3. **Type constructors**: `Array(T)` is just a function returning a Type*
4. **Extensible**: Types can carry arbitrary metadata
5. **Simple FFI**: C-compatible Value struct

## Examples

```thoxa
# Types are compile-time known values
let IntType = Int                    # Reference to Int type
let ArrayType = Array<Int>           # Reference to Array<Int> type

# Runtime type checking
fn is_integer(val: Any) -> Bool {
    return val.type == Int
}

# Type instantiation (runs constructor)
let point = Point(3.0, 4.0);        # Executes type body as constructor
```

## Type Definition vs Instantiation

```thoxa
# Type definition (compile-time)
type Counter(initial: Int) {
    mut count = initial;
    print("Counter created with {initial}");  # Runs during instantiation
    
    export fn increment() {
        count += 1;
    }
}

# Import (no code execution)
import { Counter } from "counter"

# Instantiation (runtime code execution)
let c = Counter(0);  # NOW prints "Counter created with 0"
```

## Static vs Runtime

### Compile Time
- All type checking
- Interface satisfaction verification  
- Method resolution
- Generic instantiation (future)

### Runtime
- Type introspection (`value.type.name`)
- Safe downcasting (`value as ConcreteType`)
- Type equality tests (`value.type == SomeType`)
- Serialization type tags

## Implementation Notes

- All types known at compile time
- Type metadata stored as static data
- Methods compile to regular QBE functions
- Interface dispatch through vtables (when needed)
- No runtime code generation or interpretation