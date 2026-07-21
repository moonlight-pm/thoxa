# Units - User-Defined Data Types in Thoxa

## Overview

Units are Thoxa's fundamental building blocks for composing data. While defined with the `type` keyword, units are conceptually distinct from other types (enums, primitives, etc.). They are Thoxa's version of structs/classes.

**Key Insight**: Units, modules, and types all share the same structure - they're blocks of code with visibility rules. A module is just a singleton unit, and a type is a reusable, parameterized module.

## Design Principles

1. **Explicit visibility** - Members must be marked `export` to be accessible
2. **Named parameters by default** - Constructor and function parameters are named unless marked positional with `!`
3. **Composition over inheritance** - No class inheritance; use composition and interfaces
4. **Closure-based methods** - Methods close over instance data (no explicit `self`)
5. **Module unification** - Units and modules share the same structure and visibility rules
6. **Execution on construction** - Statements in type body run when instance is created

## Syntax

### Basic Unit Definition

```thoxa
type Person {
    export let name: String
    export let age: Int
}
```

### Unit with Mutable Fields

```thoxa
type Counter {
    export let mut value: Int
    export let label: String
}
```

### Unit with Constructor Parameters

```thoxa
type Server(host!: String, port: Int = 8080, debug: Bool = false) {
    export let host = host
    export let port = port
    let debug = debug  # Not exported, internal only
    
    # Constructor logic - runs on instantiation
    assert(port > 0 && port < 65536)
    if debug {
        println("Server created on {host}:{port}")
    }
    
    export fn getInfo() -> String {
        # Methods close over instance data - no 'self' needed
        "{host}:{port}"  # Accesses instance variables directly
    }
}
```

### Nested Units

```thoxa
type Address {
    export let street: String
    export let city: String
    export let zip: Int
}

type Employee {
    export let person: Person
    export let address: Address
    export let salary: Float
}
```

### Generic Units (Future)

```thoxa
type Pair<T, U> {
    export let first: T
    export let second: U
}

type Box<T> {
    export let value: T
}
```

## Construction

Units are constructed using function-call syntax with named parameters:

```thoxa
# Named parameters (default)
let server = Server("localhost", port: 3000, debug: true)

# With defaults
let server = Server("localhost")  # port=8080, debug=false

# Variable shorthand
let port = 3000
let debug = true
let server = Server("localhost", port, debug)  # Expands to port: port, debug: debug

# Nested construction
let employee = Employee(
    person: Person(name: "Carol", age: 28),
    address: Address(
        street: "123 Main St",
        city: "Springfield",
        zip: 12345
    ),
    salary: 75000.0
)
```

## Field Access

```thoxa
let p = Person { name: "Dave", age: 35 }
let name = p.name  # Field access
let age = p.age

# Nested field access
let emp = Employee { ... }
let empName = emp.person.name
let city = emp.address.city

# Mutable field update
let mut counter = Counter(value: 0, label: "clicks")
counter.value = counter.value + 1  # Only works for mut fields
```

## Pattern Matching

```thoxa
type Point {
    export let x: Int
    export let y: Int
}

fn distance(p: Point) -> Float {
    match p {
        Point { x: 0, y: 0 } -> 0.0,
        Point { x, y } -> sqrt(x * x + y * y)
    }
}
```

## Memory Layout

Units are heap-allocated with reference counting:

```
Unit instance (16 bytes on stack):
+----------------+----------------+
| Type ID (8)    | Data Ptr (8)   |
+----------------+----------------+
                        |
                        v
            Heap allocation:
            +----------------+
            | Ref Count (4)  |
            | Flags (4)      |
            | Field 1        |
            | Field 2        |
            | ...            |
            | Method closures| (Phase 1)
            +----------------+
```

**Optimization Note**: In Phase 1, methods are stored as closures with each instance. In Phase 2, we'll optimize to shared methods with hidden `self` parameter where possible.

## Implementation Plan

### Phase 1: Parser Support
1. Parse type bodies as statement lists (like module files)
2. Parse `export let` member declarations
3. Parse `export fn` method declarations  
4. Parse constructor parameters with `!` for positional
5. Allow any statement in type body (initialization code)
6. Parse field access expressions

### Phase 2: Type System
1. Add unit types to type registry
2. Track field names and types
3. Implement type checking for construction
4. Implement type checking for field access
5. Handle generic unit instantiation

### Phase 3: Code Generation
1. Generate unit type metadata
2. Implement unit allocation and initialization
3. Execute type body statements on construction
4. Generate method closures that capture instance data
5. Generate field access code
6. Implement reference counting for units
7. Generate pattern matching for units

### Phase 4: Runtime Support
1. Unit allocation functions
2. Field offset calculations
3. Reference counting integration
4. Destructor support

## Example Test Cases

```thoxa
# Simple unit
type Vec2 {
    export let x: Float
    export let y: Float
}

fn main() -> Int {
    let v = Vec2(x: 3.0, y: 4.0)
    print("Vector: (")
    printFloat(v.x)
    print(", ")
    printFloat(v.y)
    print(")\n")
    return 0
}
```

```thoxa
# Unit with Trial
type Result {
    export let value: Int
    export let error: String
}

fn divide(a!: Int, b!: Int) -> Trial<Result, String> {
    if b == 0 {
        Fail("Division by zero")
    } else {
        Pass(Result { value: a / b, error: "" })
    }
}
```

```thoxa
# Generic unit
unit Container<T> {
    value: T,
    id: Int
}

fn main() -> Int {
    let c1 = Container { value: 42, id: 1 }
    let c2 = Container { value: "hello", id: 2 }
    return c1.value
}
```

## Open Questions

1. **Field visibility** - Should we support private fields? Or are all fields public within a module?
2. **Default values** - Should fields support default values?
3. **Computed fields** - Should we support getter/setter syntax later?
4. **Inheritance** - No inheritance, but should we support composition helpers?
5. **Methods** - Separate impl blocks (future feature) or no methods at all?

## Decision: Start Simple

For the initial implementation:
- All fields are public within the module
- No default values (all fields must be specified)
- No computed fields
- No inheritance or composition helpers
- No methods (just data)

This gives us a solid foundation to build on while keeping the implementation manageable.