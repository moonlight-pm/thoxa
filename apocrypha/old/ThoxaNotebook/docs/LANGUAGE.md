# Thoxa Language Specification

## Overview

Thoxa is an experimental programming language focused on simplicity, safety, and expressiveness. This document defines the complete language syntax and semantics.

## Core Principles

1. **Everything is an expression** - Most constructs return values
2. **Static typing with inference** - Types checked at compile time, often inferred
3. **Named parameters by default** - Function calls are self-documenting
4. **Automatic memory management** - No manual memory allocation/deallocation
5. **Pattern matching** - Powerful control flow based on structure

## Lexical Structure

### Comments
```thoxa
# Single-line comment
let x = 42  # End-of-line comment
```

### Identifiers
- Must use camelCase for variables and functions: `myVariable`, `calculateTotal`
- Must use PascalCase for types: `Point`, `HttpServer`
- Reserved keywords cannot be used as identifiers

### Keywords
```
let mut fn type interface import export from
if else match loop while for in break continue return
and or not true false
Has Not Pass Fail
```

## Types

### Primitive Types

```thoxa
# Integers
Int8, Int16, Int32, Int64     # Signed
UInt8, UInt16, UInt32, UInt64  # Unsigned
Int = Int64                    # Default
Byte = UInt8                   # Alias

# Floating Point
Float32, Float64
Float = Float64                # Default

# Others
Bool                           # true or false
Char                           # Unicode character
String                         # UTF-8 text
Void                          # No value
```

### Type Literals

```thoxa
# Integers
42          # Int (Int64)
42i32       # Int32
42u8        # UInt8
0xFF        # Hexadecimal
0b1010      # Binary

# Floats
3.14        # Float (Float64)
3.14f       # Float32
1.5e10      # Scientific notation

# Others
true, false # Bool
'A'         # Char
"hello"     # String
```

### Composite Types

```thoxa
# Arrays
Array<T>                       # Dynamic array
let nums = [1, 2, 3]          # Array<Int>
let empty: Array<String> = [] # Empty with type

# Pointers
Ptr<T>                        # Pointer to T
let p = &value                # Take address

# Union Types
type Shape = Circle | Rectangle  # Union of types

# Generic Types
May<T> = Has<T> | Not         # Optional value
Trial<T, E> = Pass<T> | Fail<E>  # Result type
```

## Variables and Bindings

```thoxa
# Immutable binding
let x = 42
let name: String = "Alice"    # Explicit type

# Mutable binding
let mut count = 0
count = count + 1              # Allowed

# Destructuring (future)
let Point(x, y) = point       # Extract fields
```

## Functions

### Basic Functions

```thoxa
# Simple function
fn add(x: Int, y: Int) -> Int {
    x + y  # Implicit return
}

# Void function
fn greet(name: String) -> Void {
    print("Hello, {name}")
}

# Early return
fn safeDivide(a: Float, b: Float) -> Float {
    if b == 0 {
        return 0  # Early return
    }
    a / b
}
```

### Parameter Types

```thoxa
# Named parameters (default)
fn connect(host: String, port: Int = 8080) -> Connection

# Positional parameters (marked with !)
fn print(message!: String) -> Void

# Mixed parameters
fn format(template!: String, width: Int = 80) -> String

# Variadic parameters
fn printf(format!: String, ...args: Any) -> Void
```

### Function Calls

```thoxa
# Named arguments
connect(host: "localhost", port: 3000)
connect(port: 3000, host: "localhost")  # Order doesn't matter

# Positional arguments
print("Hello")                           # Must be in order

# Shorthand for named
let host = "localhost"
connect(host, port: 3000)                # Expands to host: host

# Mixed
format("Template", width: 100)
```

## Control Flow

### Blocks

```thoxa
# Block expression
let result = {
    let a = 10
    let b = 20
    a + b        # Block value (no semicolon)
}

# Void block
{
    doSomething();
    doMore();    # Semicolon makes block Void
}
```

### Conditionals

```thoxa
# If expression
let status = if x > 0 {
    "positive"
} else if x < 0 {
    "negative"
} else {
    "zero"
}

# If statement (Void)
if condition {
    doSomething();
}
```

### Loops

```thoxa
# Infinite loop
loop {
    doWork()
    if done {
        break
    }
    if skip {
        continue
    }
}

# While loop
while condition {
    process()
}

# For-in loop
for item in collection {
    use(item)
}

# Range iteration (future)
for i in 0..10 {
    print(i)
}
```

## Pattern Matching

### Match Expression

```thoxa
# Basic match
let description = match value {
    0 { "zero" }
    1 { "one" }
    2 { "two" }
} else { "many" }

# Type patterns
match shape {
    c: Circle {
        "Circle with radius {c.radius}"
    }
    r: Rectangle {
        "Rectangle {r.width}x{r.height}"
    }
}

# Destructuring patterns
match point {
    Point(0, 0) { "origin" }
    Point(x, 0) { "on x-axis at {x}" }
    Point(0, y) { "on y-axis at {y}" }
    Point(x, y) { "at ({x}, {y})" }
}

# With guards (future)
match x {
    n if n < 0 { "negative" }
    n if n > 0 { "positive" }
    _ { "zero" }
}
```

## Types and Interfaces

### Type Definitions

```thoxa
# Simple type
type Point {
    export let x = 0.0
    export let y = 0.0
    
    export fn distance() -> Float {
        sqrt(x * x + y * y)
    }
}

# Type with constructor
type Circle(centerX: Float, centerY: Float, radius: Float) {
    export let x = centerX
    export let y = centerY
    export let radius = radius
    
    export fn area() -> Float {
        3.14159 * radius * radius
    }
}

# Mutable fields
type Counter(initial: Int = 0) {
    export let mut count = initial
    
    export fn increment() {
        count = count + 1
    }
}
```

### Interfaces

```thoxa
interface Drawable {
    export fn draw() -> Void
}

interface Bounded {
    export fn getBounds() -> Rectangle
}

# Types implicitly implement interfaces
type Circle(x: Float, y: Float, radius: Float) {
    export let x = x
    export let y = y
    export let radius = radius
    
    export fn draw() -> Void {
        # Implementation
    }
    
    export fn getBounds() -> Rectangle {
        Rectangle(x - radius, y - radius, radius * 2, radius * 2)
    }
}
```

## Error Handling

### May Type (Optional)

```thoxa
# Definition
type Has<T>(value: T) {
    export let value = value
}
type Not {}
type May<T> = Has<T> | Not

# Usage
fn find(items: Array<T>, pred: (T) -> Bool) -> May<T> {
    for item in items {
        if pred(item) {
            return Has(item)
        }
    }
    Not()
}

# Pattern matching
match find(nums, \x -> x > 10) {
    h: Has { "Found: {h.value}" }
    Not { "Not found" }
}
```

### Trial Type (Results)

```thoxa
# Definition
type Pass<T>(value: T) {
    export let value = value
}
type Fail<E>(error: E) {
    export let error = error
}
type Trial<T, E> = Pass<T> | Fail<E>

# Usage
fn divide(a: Float, b: Float) -> Trial<Float, String> {
    if b == 0 {
        return Fail("Division by zero")
    }
    Pass(a / b)
}

# The ? operator
fn calculate() -> Trial<Float, String> {
    let x = divide(10, 2)?  # Returns early if Fail
    let y = divide(x, 5)?
    Pass(y * 2)
}
```

## Strings

### String Interpolation

```thoxa
let name = "World"
let greeting = "Hello, {name}!"  # "Hello, World!"

let x = 10
let y = 20
let sum = "{x} + {y} = {x + y}"  # "10 + 20 = 30"

# Escape braces
let text = "Use {{braces}} for literal"  # "Use {braces} for literal"
```

### Multi-line Strings

```thoxa
let message = "
    This is a multi-line string
    with automatic dedenting.
    Relative indentation preserved.
"

# Dedenting removes common leading whitespace
let code = "
    fn example() {
        print(\"hello\")  # Stays indented relative to fn
    }
"
```

## Arrays

### Array Operations

```thoxa
# Creation
let nums = [1, 2, 3, 4, 5]
let empty: Array<Int> = []

# Access
let first = nums[0]
nums[1] = 10  # If mut

# Properties
let len = nums.count

# Methods (future)
nums.push(6)
let last = nums.pop()  # May<Int>
nums.insert(2, 99)
nums.remove(3)

# Iteration
for n in nums {
    print(n)
}
```

## Modules

### Module Files

```thoxa
# math.thx
export let PI = 3.14159

export fn sqrt(x!: Float) -> Float {
    # Implementation
}

export type Vec2(x: Float, y: Float) {
    export let x = x
    export let y = y
}

fn helper() {  # Private
    # ...
}
```

### Imports

```thoxa
# Import specific items
import { sqrt, PI } from "math"

# Import as namespace
import math from "math"
let x = math.sqrt(16)

# Relative imports
import { utils } from "./helpers"
import { format } from "../lib/format"

# Re-export
export { sqrt } from "math"
```

## Operators

### Arithmetic
- `+` addition
- `-` subtraction
- `*` multiplication
- `/` division
- `%` modulo

### Comparison
- `==` equal
- `!=` not equal
- `<` less than
- `>` greater than
- `<=` less than or equal
- `>=` greater than or equal

### Logical
- `and` logical AND
- `or` logical OR
- `not` logical NOT

### Assignment
- `=` assignment
- `+=`, `-=`, `*=`, `/=` compound assignment (future)

## Built-in Functions

```thoxa
# I/O
print(message: String) -> Void
println(message: String) -> Void
readLine() -> String

# Math
sqrt(x: Float) -> Float
abs(x: Number) -> Number
min(a: T, b: T) -> T
max(a: T, b: T) -> T

# Type checking
typeOf(value: Any) -> Type
isType(value: Any, type: Type) -> Bool

# Assertions
assert(condition: Bool, message?: String) -> Void
```

## Planned Features

- Generic user-defined types
- Async/await
- Closures and lambdas
- Operator overloading
- Macros/compile-time evaluation
- Foreign function interface (FFI)