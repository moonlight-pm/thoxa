# Generic Types Implementation Plan

## Overview

Generics allow Thoxa to write type-safe, reusable code that works with multiple types. This plan outlines the complete implementation of generic types, starting with May<T> and Trial<T,E> and extending to user-defined generic types.

## Current State

### What We Have
- Union types working with concrete types
- Pattern matching with typed patterns
- Boxing/unboxing infrastructure for value types
- Type registry and type IDs
- Hardcoded May and Trial types (non-generic)

### What We Need
- Generic type parameter parsing
- Type parameter substitution
- Generic instantiation/monomorphization
- Update May<T> and Trial<T,E> to be truly generic
- User-defined generic types

## Phase 1: Parser Support for Generics

### 1.1 Generic Type Parameter Parsing
```thoxa
# Parse angle bracket syntax for type parameters
type Box<T> { ... }
type Result<T, E> { ... }
type Map<K, V> { ... }
```

**Tasks:**
- [ ] Add `<` and `>` tokens to lexer
- [ ] Parse type parameter lists in type declarations
- [ ] Store type parameters in AST TypeDecl
- [ ] Parse generic type arguments in type expressions

### 1.2 Generic Function Parsing
```thoxa
fn map<T, U>(array: Array<T>, f: fn(T) -> U) -> Array<U> {
    ...
}
```

**Tasks:**
- [ ] Parse type parameters on function declarations
- [ ] Store in AST Function node
- [ ] Parse generic type arguments in function calls

### 1.3 AST Changes
```rust
// ast.rs
pub struct TypeDecl {
    pub name: String,
    pub type_params: Vec<String>,  // NEW: ["T", "E"]
    pub params: Vec<Param>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub span: Span,
}

pub enum Type {
    Named {
        name: String,
        args: Vec<Type>,  // NEW: generic arguments
        span: Span,
    },
    // ...
}
```

## Phase 2: Type System Support

### 2.1 Type Registry Updates
```rust
// Type registry needs to track:
// 1. Generic type definitions (with parameters)
// 2. Instantiated generic types (with concrete types)

struct GenericTypeDef {
    name: String,
    type_params: Vec<String>,
    body: TypeBody,
}

struct InstantiatedType {
    generic_def: TypeId,
    type_args: Vec<TypeId>,
    instantiated_id: TypeId,
}
```

### 2.2 Type Substitution
```rust
// When we see May<Int>, we need to:
// 1. Look up generic definition of May<T>
// 2. Substitute T = Int throughout
// 3. Generate or retrieve instantiated type
```

**Tasks:**
- [ ] Implement type parameter substitution
- [ ] Cache instantiated types to avoid duplicates
- [ ] Generate unique names for instantiated types (e.g., "May_Int")

### 2.3 Type Checking with Generics
```thoxa
let x: May<Int> = Has(42);  // Need to verify Int matches T
```

**Tasks:**
- [ ] Track type parameter bindings during type checking
- [ ] Verify type arguments match constraints
- [ ] Infer type arguments where possible

## Phase 3: Code Generation

### 3.1 Monomorphization Strategy
Each unique instantiation of a generic type generates separate code:
- `May<Int>` generates `may_int` type
- `May<String>` generates `may_string` type
- Shared runtime functions where possible

### 3.2 Runtime Type Information
```c
// Runtime needs to track generic instantiations
typedef struct {
    uint16_t generic_type_id;    // ID of May<T>
    uint16_t num_type_args;       // 1 for May, 2 for Trial
    uint16_t* type_arg_ids;      // [Int] for May<Int>
} thoxa_generic_info_t;
```

### 3.3 QBE Generation
```qbe
# May<Int> becomes:
type :may_int = { w, w, l, l }  # Similar to union

# Trial<Int, String> becomes:
type :trial_int_string = { w, w, l, l }
```

**Tasks:**
- [ ] Generate unique type names for each instantiation
- [ ] Generate constructor functions for each variant
- [ ] Generate pattern matching code with proper types

## Phase 4: Update Built-in Types

### 4.1 Convert May to Generic
```thoxa
# From hardcoded:
type Has(value) { ... }
type Not { ... }

# To generic:
type Has<T> {
    export let value: T;
}

type Not { }

type May<T> = Has<T> | Not;
```

**Tasks:**
- [ ] Remove hardcoded Has/Not from parser
- [ ] Define as generic types in prelude
- [ ] Update pattern matching codegen
- [ ] Maintain backward compatibility

### 4.2 Convert Trial to Generic
```thoxa
type Pass<T> {
    export let value: T;
}

type Fail<E> {
    export let error: E;
}

type Trial<T, E> = Pass<T> | Fail<E>;
```

**Tasks:**
- [ ] Remove hardcoded Pass/Fail from parser
- [ ] Define as generic types in prelude
- [ ] Update try operator (?) implementation
- [ ] Test error propagation with different types

## Phase 5: User-Defined Generics

### 5.1 Simple Generic Types
```thoxa
type Box<T> {
    export let value: T;
}

type Pair<A, B> {
    export let first: A;
    export let second: B;
}
```

### 5.2 Generic Unions
```thoxa
type Result<T, E> = Ok<T> | Err<E>;
type Option<T> = Some<T> | None;
```

### 5.3 Nested Generics
```thoxa
type Tree<T> = Leaf<T> | Branch<Tree<T>>;
let tree: Tree<Int> = Branch(Leaf(1), Leaf(2));
```

## Phase 6: Advanced Features

### 6.1 Type Constraints (Future)
```thoxa
# Require T to implement certain interfaces
fn sort<T: Comparable>(array: Array<T>) { ... }
```

### 6.2 Associated Types (Future)
```thoxa
interface Iterator {
    type Item;
    fn next() -> May<Item>;
}
```

### 6.3 Higher-Kinded Types (Future)
```thoxa
type Functor<F<_>> = {
    fn map<A, B>(fa: F<A>, f: fn(A) -> B) -> F<B>;
}
```

## Implementation Order

1. **Parser Support** (Phase 1)
   - Add angle bracket syntax
   - Parse type parameters
   - Update AST

2. **Type System** (Phase 2)
   - Type registry for generics
   - Type substitution
   - Type checking

3. **Code Generation** (Phase 3)
   - Monomorphization
   - QBE generation
   - Runtime support

4. **Built-in Updates** (Phase 4)
   - Convert May<T>
   - Convert Trial<T,E>
   - Test compatibility

5. **User Generics** (Phase 5)
   - Enable user-defined generics
   - Test with examples

## Testing Strategy

### Parser Tests
```thoxa
# Should parse correctly
type Box<T> { }
type Map<K, V> { }
fn identity<T>(x: T) -> T { return x; }
```

### Type Checking Tests
```thoxa
# Should type check
let x: May<Int> = Has(42);
let y: Trial<String, Int> = Pass("hello");

# Should fail type checking
let z: May<Int> = Has("wrong");  // Error: String != Int
```

### Codegen Tests
```thoxa
# Should generate correct code
let opt: May<Int> = Has(10);
match opt {
    Has(x) -> { print(x); }    # x should be Int
    Not -> { print("none"); }
}
```

### Performance Tests
- Measure compilation time with many instantiations
- Check binary size with monomorphization
- Verify no runtime overhead vs hardcoded types

## Success Criteria

1. **Backward Compatibility**: All existing May/Trial code works
2. **Type Safety**: Generic types catch type errors at compile time
3. **Performance**: No runtime overhead vs concrete types
4. **Usability**: Clean, intuitive syntax for generics
5. **Completeness**: Users can define their own generic types

## Known Challenges

### Challenge 1: Type Inference
- When to infer type arguments vs require explicit annotation
- Bidirectional type checking may help

### Challenge 2: Monomorphization Explosion
- Many instantiations can bloat binary size
- Consider sharing code where possible

### Challenge 3: Error Messages
- Generic error messages can be confusing
- Need clear reporting of type mismatches

### Challenge 4: Recursive Types
- `type List<T> = Cons<T, List<T>> | Nil`
- Need careful handling to avoid infinite recursion

## Timeline Estimate

- Phase 1 (Parser): 2-3 days
- Phase 2 (Type System): 3-4 days
- Phase 3 (Codegen): 3-4 days
- Phase 4 (Built-ins): 2-3 days
- Phase 5 (User Generics): 2-3 days
- Testing & Debugging: 3-4 days

**Total: 2-3 weeks for full implementation**

## Next Steps

1. Start with parser support for angle brackets
2. Add type parameters to AST nodes
3. Implement basic type substitution
4. Test with May<Int> as proof of concept
5. Gradually expand to full generics support