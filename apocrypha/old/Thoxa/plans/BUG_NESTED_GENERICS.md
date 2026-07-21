# Nested Generic Function Calls Issue

## Problem

When a generic function calls another generic function, the nested function doesn't get instantiated properly.

## Example

```thoxa
fn double<T>(x: T) -> T { x }

fn process<T>(x: T) -> T {
    double(x)  // This call to double<T> doesn't get instantiated
}

fn main() {
    let result = process(42);  // process<Int> is created, but double<Int> is not
}
```

## Root Cause

The issue is in the monomorphization process. When we instantiate `process<Int>`, we:

1. Substitute types in the function signature (parameters and return type)
2. Generate code for the function body
3. **BUT** we don't substitute types in the function body AST

So when generating `process<Int>`, the body still contains `double(x)` where `x` has generic type `T`, not the concrete type `Int`. The call generation then can't determine that it needs `double<Int>`.

## Attempted Fix

We implemented iterative processing of pending instantiations with duplicate detection:
- Process pending instantiations in a loop until none remain
- Skip already-processed instantiations
- Register instantiations to prevent duplicates

This fixes the infrastructure but doesn't solve the root cause.

## Required Solution

We need to implement full AST substitution for function bodies during monomorphization:

```rust
// In process_pending_instantiations
fn substitute_types_in_body(
    body: &mut Vec<Statement>, 
    type_param_map: &HashMap<String, TypeId>
) {
    // Walk the AST and substitute:
    // - Type references in variable declarations
    // - Type arguments in generic function calls
    // - Type references in expressions
    // - Type references in patterns
}
```

This is a significant task requiring:
1. AST visitor/transformer pattern
2. Deep cloning with modification
3. Handling all expression and statement types
4. Preserving spans for error reporting

## Workaround

For now, users must use explicit type arguments for nested generic calls:

```thoxa
fn process<T>(x: T) -> T {
    double<T>(x)  // Explicit type argument
}
```

But this doesn't work because `T` is still a type parameter, not a concrete type.

## Impact

- Type inference works for simple cases
- Complex generic function compositions fail
- Limits the usefulness of generic functions

## Priority

Medium - This is a significant limitation but has a clear (though complex) solution path.