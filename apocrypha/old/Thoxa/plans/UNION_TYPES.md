# Union Types Implementation Plan

## Overview

Thoxa uses union types (sum types) as the foundation for algebraic data types. Types are standalone entities that can be composed into unions: `type Shape = Circle | Rectangle`.

## Core Design Principles

1. **Types are independent** - Each type can exist and be used on its own
2. **Unions compose types** - Union types are just compositions of existing types
3. **Pattern matching on constructors** - Match destructures based on constructor parameters
4. **No special enum syntax** - Everything is just types and unions

## Phase 1: Foundation ✅ COMPLETE

### Hardcoded Support ✅
Currently Trial and May are hardcoded as special cases:
- `Pass(value)` and `Fail(error)` for Trial
- `Has(value)` and `Not` for May
- Special parser support for these keywords
- Dedicated codegen functions

### Union Type Implementation ✅
- Union type syntax parsing (A | B | C)
- Pattern matching with typed patterns (a: A -> {...})
- Proper memory layout with variant type IDs
- Runtime functions for union creation/extraction
- Field access in match arms with automatic unboxing

## Phase 2: Boxing System ✅ COMPLETE

### Goals
- Unified boxing for all value types
- Automatic boxing when values enter unions
- Efficient unboxing during pattern matching

### Tasks
- [ ] Design box type representation
- [ ] Implement thoxa_box_value(type_id, value) 
- [ ] Implement thoxa_unbox_value(box_ptr)
- [ ] Add type tags to boxed values
- [ ] Integrate with reference counting

### Box Structure
```c
typedef struct {
    thoxa_header_t header;  // Standard memory header
    uint16_t type_id;       // Type of boxed value
    union {
        int64_t  i64;
        int32_t  i32;
        float    f32;
        double   f64;
        void*    ptr;
    } value;
} thoxa_box_t;
```

## Phase 3: Union Type Syntax

### Parser Support
```thoxa
# Type definitions
type Circle(x: Float, y: Float, radius: Float) { ... }
type Rectangle(x: Float, y: Float, width: Float, height: Float) { ... }

# Union composition
type Shape = Circle | Rectangle;
```

### Tasks
- [ ] Parse union type syntax (type A = B | C)
- [ ] Store union definitions in type registry
- [ ] Generate discriminated union structure
- [ ] Implicit conversions (Circle → Shape)

### Runtime Representation
```c
typedef struct {
    thoxa_header_t header;
    uint32_t variant_tag;   // Which type in the union
    void* variant_data;     // Pointer to actual type instance
} thoxa_union_t;
```

## Phase 4: Pattern Matching Enhancement

### Constructor-Based Matching
```thoxa
type Widget(width!: Float, height: Float = 100, ...tags: String) {
    export width = width;
    export mut height = height;
}

match widget {
    Widget(100) { ... }           # Match width=100
    Widget(w, height) { ... }     # Extract both
    Widget(_, height: h) { ... }  # Named extraction
    Widget(w, ...t) { ... }       # Variadic extraction
}
```

### Rules
1. **All constructor params matchable** (not just exported)
2. **Immutable params** → constructor value
3. **export mut params** → current value
4. **Positional params** can be renamed
5. **Named params** cannot be renamed
6. **Variadic params** need `...` prefix

## Phase 5: Convert Trial and May

### Make Trial a Union
```thoxa
type Pass<T>(value: T) {
    export value = value;
}

type Fail<E>(error: E) {
    export error = error;
}

type Trial<T, E> = Pass<T> | Fail<E>;
```

### Make May a Union
```thoxa
type Has<T>(value: T) {
    export value = value;
}

type Not {}

type May<T> = Has<T> | Not;
```

### Tasks
- [ ] Remove hardcoded Pass/Fail/Has/Not from parser
- [ ] Define as standard library types
- [ ] Update pattern matching to use general mechanism
- [ ] Maintain backward compatibility

## Phase 6: Generic Unions

### Support Generic Parameters
```thoxa
type Result<T, E> = Ok<T> | Err<E>;
type Option<T> = Some<T> | None;
```

### Tasks
- [ ] Generic type parameter parsing
- [ ] Type parameter substitution
- [ ] Monomorphization strategy
- [ ] Type inference for constructors

## Phase 7: Advanced Features

### Recursive Types
```thoxa
type Leaf(value: Int) { export value = value; }
type Branch(left: Tree, right: Tree) { 
    export left = left;
    export right = right;
}
type Tree = Leaf | Branch;
```

### Multiple Unions
```thoxa
type Distance = Meters | Feet | Miles;
type Result<T> = Ok<T> | Err<String> | Pending;
```

## Implementation Order

1. **Boxing System** (Phase 2) - Foundation for everything
2. **Union Syntax** (Phase 3) - Parse and represent unions
3. **Pattern Matching** (Phase 4) - Full constructor matching
4. **Convert Trial/May** (Phase 5) - Prove the system works
5. **Generics** (Phase 6) - Full type parameters
6. **Advanced** (Phase 7) - Recursive types, etc.

## Testing Strategy

### Boxing Tests
- Box/unbox all primitive types
- Reference counting with boxes
- Type preservation through boxing

### Union Tests
```thoxa
# Basic union
type Shape = Circle | Rectangle;
let s: Shape = Circle(0, 0, 5);

# Pattern matching
match s {
    c: Circle { print("Circle: {c.radius}"); }
    r: Rectangle { print("Rect: {r.width}x{r.height}"); }
}
```

### Trial/May Tests
```thoxa
# Ensure backward compatibility
let t: Trial<Int, String> = Pass(42);
match t {
    Pass(v) { print("Value: {v}"); }
    Fail(e) { print("Error: {e}"); }
}
```

## Success Criteria

1. All existing Trial/May code continues to work
2. Users can define their own union types
3. Pattern matching works uniformly for all unions
4. No performance regression vs hardcoded enums
5. Clean, composable syntax