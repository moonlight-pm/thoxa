# Type Inference in Thoxa

## Overview
Thoxa implements a constraint-based type inference engine using a Hindley-Milner style algorithm with unification. This eliminates the need for explicit type arguments in most cases and provides better type error messages.

## Architecture

### Core Components

#### 1. Type Representation (`inference/types.rs`)
```rust
pub enum InferenceType {
    Concrete(TypeId),                    // Known concrete type
    Variable(TypeVar),                   // Type variable to be inferred
    Application { constructor, args },   // Generic type application
}
```

#### 2. Constraint System (`inference/constraints.rs`)
- **Equality constraints**: Two types must be equal
- **Field constraints**: Type must have a specific field
- **Method constraints**: Type must have a specific method
- **Numeric constraints**: Type must be numeric

#### 3. Constraint Generation (`inference/generate.rs`)
- Traverses the AST and generates constraints
- Creates fresh type variables for unknowns
- Handles expressions, statements, and patterns
- Special handling for:
  - Generic function calls
  - Match expressions
  - Array literals
  - Trial/May constructors

#### 4. Unification Solver (`inference/unify.rs`)
- Solves constraints using unification algorithm
- Performs occurs check to prevent infinite types
- Handles special Thoxa types (Trial, May, arrays)
- Produces substitution map from type variables to concrete types

#### 5. Type Substitution (`inference/substitute.rs`)
- Applies solved substitutions back to the AST
- Updates all type annotations
- Preserves source locations for error reporting

## Features

### ✅ Implemented
- **Basic type inference**: Literals, variables, binary operations
- **Generic function inference**: Automatic type parameter deduction
- **Array type inference**: Element types from literals
- **Match expression inference**: Type from arm bodies
- **Control flow inference**: If/else branches, loops
- **Constructor inference**: Trial/May type parameters
- **Nested generics**: Multi-level generic instantiation
- **Recursion protection**: Prevents infinite inference loops
- **Break with value**: Type inference for loop breaks

### 🚧 Limitations
- **No bidirectional checking**: Cannot use expected type to guide inference
- **No let-polymorphism**: Local bindings aren't generalized
- **No higher-ranked types**: Cannot express `forall` in function types
- **No first-class functions**: Functions aren't values (separate issue)

## How It Works

### Example: Simple Function Call
```thoxa
fn identity<T>(x: T) -> T { x }

let result = identity(42);  // No type arguments needed!
```

1. **Constraint Generation**:
   - Create type variable `?1` for the call result
   - Generate constraint: `identity<?2>(Int) = ?1`
   - Constraint: `?2 = Int` (from argument)
   - Constraint: `?1 = ?2` (from return type)

2. **Unification**:
   - Solve: `?2 = Int`
   - Solve: `?1 = Int`

3. **Substitution**:
   - Instantiate `identity<Int>`
   - Type `result` as `Int`

### Example: Match Expression
```thoxa
let maybe = Has(42);
let result = match maybe {
    Has(x) -> x + 1,
    Not() -> 0
};
```

1. **Constraint Generation**:
   - `maybe` has type `May<Int>` (from Has(42))
   - First arm returns `Int` (from x + 1)
   - Second arm returns `Int` (from literal 0)
   - Result type must unify both arms

2. **Unification**:
   - Both arms return `Int` ✓
   - `result` has type `Int`

## Implementation Details

### Recursion Protection
The inference engine tracks functions being inferred to prevent infinite recursion:
- Maximum depth limit of 10
- HashSet of functions currently being inferred
- RAII guards for automatic cleanup

### Error Handling
- Constraints track source locations
- Errors include context about what was being inferred
- Multiple errors can be reported at once

### Performance Considerations
- Type variables are interned for efficiency
- Substitutions are cached
- Constraint simplification reduces work

## Test Coverage

### Passing Tests
- `test_type_inference_basic.thx` - ✅ Literals, variables, operations
- `test_type_inference_control.thx` - ✅ Control flow, pattern matching
- `test_nested_generics.thx` - ✅ Multi-level generic calls
- `test_may_inference.thx` - ✅ May type inference
- `test_generics_basic.thx` - ✅ Basic generic functions

### Blocked Tests
- `test_type_inference_generics.thx` - ❌ Requires first-class functions

## Future Improvements

### Bidirectional Type Checking
Use expected types to guide inference:
```thoxa
let nums: [Int] = [];  // Use annotation to infer element type
```

### Let-Polymorphism
Allow polymorphic local bindings:
```thoxa
let id = fn<T>(x: T) -> T { x };  // Currently not supported
let a = id(42);
let b = id("hello");
```

### Better Error Messages
- Show inference steps in errors
- Suggest type annotations where helpful
- Group related constraints

## Related Documentation
- `/GENERICS.md` - Generic type system
- `/docs/TYPES.md` - Type registry and representation
- `/plans/LOW_LEVEL_PLAN.md` - First-class functions (needed for full inference)