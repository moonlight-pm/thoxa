# Parameter System in Thoxa

## Overview

Thoxa uses **named parameters by default** for functions and type constructors, with opt-in positional parameters for cases where they make sense (like print functions or operators).

## Design Philosophy

1. **Named by default** - Most parameters benefit from names for clarity
2. **Explicit positional** - Mark with `!` suffix when position matters
3. **No positional defaults** - Positional parameters must always be provided
4. **Order matters for positional** - Positional parameters must come first

## Syntax

### Function Definitions

```thoxa
# Pure named parameters (default)
fn connect(host: String, port: Int = 8080, timeout: Int = 5000) -> Connection

# Positional parameters (marked with !)
fn print(text!: String) -> Void

# Mixed (positional first, then named)
fn format(template!: String, color: Color = Black, width: Int = 80) -> String

# Variadic (always last, always positional)
fn printf(format!: String, ...args: Any) -> Void
```

### Type Constructors

```thoxa
# Same rules apply to type constructors
type Server(host!: String, port: Int = 8080, debug: Bool = false) {
    export let host = host
    export let port = port
    # debug is internal only
}

type Person {
    export let name: String
    export let age: Int
}
```

## Calling Conventions

### Named Parameters

```thoxa
# All named
connect(host: "example.com", port: 443, timeout: 10000)

# Order doesn't matter for named
connect(timeout: 10000, host: "example.com", port: 443)

# Can omit parameters with defaults
connect(host: "example.com")  # Uses port: 8080, timeout: 5000

# Shorthand when variable name matches parameter
let host = "example.com"
let port = 443
connect(host, port)  # Expands to host: host, port: port
```

### Positional Parameters

```thoxa
# Must provide in order
print("Hello, World!")

# Cannot use named syntax
print(text: "Hello")  # ERROR: Cannot use named syntax for positional parameter

# Mixed calling
format("Hello {}", color: Red, width: 100)
format("Hello {}")  # Uses default color and width
```

### Constructor Calls

```thoxa
# Named parameters for types
let server = Server("localhost", port: 3000)
let person = Person(name: "Alice", age: 30)

# With defaults
let server = Server("localhost")  # port: 8080, debug: false
```

## Design Rationale

### Why `!` for Positional?

1. **Visual distinction** - Makes positional parameters stand out as exceptional
2. **Suggests "required"** - The `!` implies "must be provided"
3. **Discourages overuse** - Slightly ugly syntax nudges toward named parameters
4. **Consistent with other uses** - `!` often means "emphatic" or "required" in programming

### Why Named by Default?

1. **Self-documenting** - Code reads more clearly with named parameters
2. **Refactoring-friendly** - Add/remove parameters without breaking callers
3. **Order-independent** - Reduces cognitive load
4. **Modern** - Aligns with current best practices in API design

### Built-in Functions

Standard library functions that are positional for familiarity:

```thoxa
fn print(text!: String) -> Void
fn println(text!: String) -> Void
fn printInt(value!: Int) -> Void
fn printFloat(value!: Float) -> Void
fn printChar(char!: Char) -> Void
```

## Future Considerations

### Method Receivers

When we add methods to types, `self` will be implicit and always positional:

```thoxa
type String {
    export fn concat(other: String) -> String {
        # self is implicit
    }
}

let result = str.concat(other: " world")
```

### Operator Overloading

If we add operator overloading, operators would naturally be positional:

```thoxa
fn +(left!: Point, right!: Point) -> Point {
    Point(x: left.x + right.x, y: left.y + right.y)
}
```

## Implementation Notes

- Parser needs to track parameter position and `!` suffix
- Type checker validates positional parameters come first
- Code generator handles both calling conventions
- Error messages should be clear about parameter expectations