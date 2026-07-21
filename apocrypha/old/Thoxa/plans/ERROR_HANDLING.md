# Error Handling with Trial<T, E>

## Overview

Trial<T, E> is Thoxa's primary error handling mechanism, similar to Rust's Result type. It provides a type-safe way to handle operations that can fail, with automatic error propagation via the `?` operator.

## Design Principles

1. **Explicit Error Handling** - Errors are part of the type system
2. **Composable** - Errors can be propagated and transformed
3. **Zero-cost** - No runtime overhead compared to manual error checking
4. **Ergonomic** - The `?` operator makes error handling concise

## Implementation Plan

### Remaining Work

#### Generic Type Parameters
- [ ] Support generic type parameters Trial<T, E>
- [ ] Handle type equality for different Trial instantiations

### Parser Support

#### Type Annotations
- [ ] Parse `Trial<T, E>` in type positions
- [ ] Support nested types: `Trial<Trial<Int, String>, String>`
- [ ] Handle type inference where possible


### Pattern Matching Enhancements
- [ ] Ensure exhaustive matching for Trial patterns

### Try Operator (?)

#### Operator Semantics
- [ ] On Pass: unwrap value and continue
- [ ] On Fail: return early with Fail
- [ ] Require function return type to be Trial

#### Code Generation
- [ ] Generate conditional branch on variant tag
- [ ] Generate early return for Fail case
- [ ] Pass through error without modification

[8 bytes: value pointer] Pointer to T or E value
```

#### 5.3 Reference Counting
- [x] Trial itself is reference counted
- [x] Payload values are NOT reference counted (simplification)
- Note: Currently payloads are raw pointers/integers, proper boxing needed for complex types

### Phase 6: Standard Library

#### 6.1 Query Functions
- [ ] `isPass() -> Bool` - Check if Trial is Pass
- [ ] `isFail() -> Bool` - Check if Trial is Fail

#### 6.2 Extraction Functions
- [ ] `unwrap() -> T` - Get value or panic
- [ ] `unwrapOr(default: T) -> T` - Get value or default
- [ ] `unwrapError() -> E` - Get error or panic

#### 6.3 Transformation Functions
- [ ] `map(fn(T) -> U) -> Trial<U, E>` - Transform success value
- [ ] `mapError(fn(E) -> F) -> Trial<T, F>` - Transform error value
- [ ] `andThen(fn(T) -> Trial<U, E>) -> Trial<U, E>` - Chain operations

## Usage Examples

### Basic Error Handling
```thoxa
fn divide(a: Int, b: Int) -> Trial<Int, String> {
    if b == 0 {
        Fail("Division by zero")
    } else {
        Pass(a / b)
    }
}

fn main() -> Int {
    match divide(10, 2) {
        Pass(result) => {
            printInt(result);
            0
        },
        Fail(err) => {
            println(err);
            1
        }
    }
}
```

### Error Propagation with ?
```thoxa
fn calculate(x: Int, y: Int) -> Trial<Int, String> {
    let a = divide(x, y)?;     # May return early with error
    let b = divide(a, 2)?;     # May return early with error
    Pass(b + 1)
}
```

### Nested Error Handling
```thoxa
fn parseAndDivide(s: String, divisor: Int) -> Trial<Int, String> {
    let num = parseInt(s)?;    # Returns Trial<Int, String>
    divide(num, divisor)       # Also returns Trial<Int, String>
}
```

## Implementation Order

1. **Type System** - Add Trial type with generics
2. **Constructors** - Pass/Fail expressions
3. **Pattern Matching** - Basic match support
4. **Memory Layout** - Proper heap allocation
5. **Try Operator** - Implement ? with early returns
6. **Standard Library** - Helper functions

## Testing Strategy

### Unit Tests
- Type checking for Trial instantiations
- Constructor type validation
- Pattern matching exhaustiveness

### Integration Tests
- Error propagation chains
- Nested Trial types
- Memory management (no leaks)

### Example Programs
- File I/O with error handling
- Network operations (future)
- Parser with detailed errors

## Related Features

- **Option<T>** - Similar implementation but simpler (Some/None)
- **Pattern Matching** - Required for ergonomic Trial usage
- **Generics** - Trial is the primary driver for generic types
- **Memory Management** - Heap allocation for variants

## Open Questions

1. Should we support implicit conversions from T to Trial<T, E>?
2. How do we handle error type coercion/compatibility?
3. Should we have a standard Error interface?
4. Do we want Result as an alias for Trial?