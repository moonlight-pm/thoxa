# Future Language Features for Thoxa

This document describes planned language features that are not yet implemented. These represent the intended direction for Thoxa's evolution.

## Variant Types (Algebraic Data Types)

### Overview
Variant types (also known as sum types or tagged unions) allow defining types that can be one of several named variants, each potentially carrying different data.

### Syntax
```thoxa
type Trial<T> {
    variant Pass(value: T);
    variant Fail(error: String);
}
```

### Key Design Decisions
- **Variants require type qualification**: Always accessed as `Trial.Pass(42)`, never bare `Pass(42)`
- **Construction uses function-like syntax**: Both positional and named arguments supported
  - `Trial.Pass(42)` - positional
  - `Trial.Pass(value: 42)` - named
  - Standard rule: no positional arguments after named ones
- **Variants are heap-allocated**: Stored as tagged pointers (tag 0x0) with variant discriminator in object header
- **No special import syntax**: Access variants through the parent type with dot notation

### Example Usage
```thoxa
import Trial from "std/trial";

fn divide(x: Int64, y: Int64) -> Trial<Int64> {
    if y == 0 {
        Trial.Fail("Division by zero")
    } else {
        Trial.Pass(x / y)
    }
}
```

## Generic Types

### Syntax
- Type parameters use angle brackets: `Trial<T>`, `Map<K, V>`
- Type parameter constraints planned for future: `<T: Display>` (syntax TBD)

### Type Inference
- Type parameters can be inferred from usage context
- Explicit type annotations supported where needed

### Example
```thoxa
type List<T> {
    variant Nil;
    variant Cons(head: T, tail: List<T>);
}

let numbers: List<Int64> = List.Cons(1, List.Cons(2, List.Nil));
```

## Pattern Matching

### Syntax
```thoxa
match expression {
    Pattern1(binding) {
        # Block body
        result_expression
    },
    Pattern2(x, y) {
        # Another block
        other_result
    },
    Otherwise {
        # Catch-all case
        default_result
    }
}
```

### Key Features
- **Block syntax**: Each match arm is a block, no arrow needed
- **Exhaustiveness checking**: Compiler ensures all cases are covered
- **Otherwise clause**: Catch-all pattern that ignores the value
- **Result type coercion**: Mixed-type results coerce to `Any`

### Example
```thoxa
let result = match divide(10, x) {
    Trial.Pass(value) {
        @print(value);
        value
    },
    Trial.Fail(error) {
        @printStr(error);
        0  # Different type - will coerce to Any with Pass branch
    }
};
```

## Error Propagation Operator (?)

### Overview
The `?` operator provides early return for `Trial.Fail` cases, automatically propagating errors up the call stack.

### Semantics
- Only works with `Trial` type (not generic for any error type)
- Can be used in any expression context
- Unwraps `Trial.Pass` to its inner value
- Returns early with `Trial.Fail` if encountered

### Desugaring
```thoxa
# This:
let value = foo()?;

# Desugars to:
let value = match foo() {
    Trial.Pass(v) => v,
    Trial.Fail(e) => return Trial.Fail(e)
};
```

### Chaining Example
```thoxa
fn process() -> Trial<String> {
    let x = getNumber()?;      # Trial<Int64> -> Int64
    let y = calculate(x)?;     # Trial<Float> -> Float  
    let z = stringify(y)?;     # Trial<String> -> String
    Trial.Pass(z + "!")
}
```

## Type Coercion to Any

### Overview
When type ambiguity exists, Thoxa will coerce to `Any` rather than reporting an error. This enables gradual typing and flexibility.

### Coercion Rules
1. Any type can be coerced to `Any`
2. Generic types coerce their parameters: `Trial<Int64>` → `Trial<Any>`
3. Mixed-type match arms coerce to `Any`
4. Function returns with ambiguous types default to `Any`

### Example
```thoxa
# Automatic coercion in generic context
fn foo() -> Trial<Int64> { Trial.Pass(42) }
fn bar() -> Trial<Any> { 
    foo()?  # Trial<Int64> automatically coerces to Trial<Any>
}

# Mixed match arms
let value = match condition {
    true => 42,        # Int64
    false => "hello"   # String
};  # value: Any (not a union type)
```

## Implementation Notes

### Memory Representation
- Variants stored as heap objects (tagged pointer 0x0)
- Object header contains:
  - Type ID (for the variant type like Trial)
  - Variant discriminator (which variant: Pass/Fail)
  - Size information
- Variant data follows header

### Type System Extensions Required
1. Generic type parameters with substitution
2. Algebraic data types (sum types)
3. Pattern matching with exhaustiveness checking
4. Type coercion rules for Any
5. Proper type inference for variants and generics

### Parser Extensions Needed
- Generic type syntax: `Type<T>`, `Type<T, U>`
- Variant declarations within types
- Pattern matching syntax with blocks
- `?` operator parsing
- Qualified variant construction: `Type.Variant(...)`

## Open Questions

The following design decisions are still under consideration:

1. **Type inference for variant construction**: Should `Trial.Pass(42)` infer `Trial<Int64>` or require annotation?

2. **Pattern matching guards**: Should we support `Pattern(x) if x > 0 { ... }`?

3. **Nested patterns**: How deep should pattern matching destructuring go?

4. **Variant allocation strategy**: Always heap, or optimize small variants?

5. **Generic function monomorphization**: Compile separate versions per type or use boxing?

These questions will be resolved as implementation progresses and usage patterns emerge.