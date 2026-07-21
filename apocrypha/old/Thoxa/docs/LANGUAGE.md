# Thoxa Language Specification

## Overview

Thoxa is a systems programming language that aims to [goals to be determined].

## Syntax

### Almost Everything is an Expression

In Thoxa, almost everything is a typed expression that produces a value. The few exceptions are variable bindings and type definitions, which are statements for clarity and to prevent confusing constructs.

### Comments

Single-line comments use `#`:
```thoxa
# This is a comment
let x = 42;  # End-of-line comment
```

### Blocks

Blocks are expressions delimited by `{` and `}`. They contain semicolon-delimited expressions and evaluate to their last expression.

```thoxa
let x = {
    let a = 10;    # Semicolon required between expressions
    let b = 20;    # let expressions evaluate to Void
    a + b          # Last expression (no semicolon) is the block's value
}  # x = 30, no semicolon after closing brace

# Void blocks end with semicolon
{
    print("hello");
    do_something();  # Semicolon makes block evaluate to Void
}
```

### Control Flow

#### If Expressions

```thoxa
# if is a typed expression
let result = if x > 0 {
    "positive"
} else if x < 0 {
    "negative"  
} else {
    "zero"
}  # No semicolon after block

# All branches must have same type
# if without else requires branches to be Void
if condition {
    do_something();
}
```

#### Loops

Loops always evaluate to Void:

```thoxa
loop {
    do_work();
    
    if done {
        break;  # Exit loop
    }
    
    if skip {
        continue;  # Next iteration
    }
}

# Future: for and while loops
```

### Functions

Standalone functions are defined with `fn`:

```thoxa
fn add(x: Int, y: Int) -> Int {
    x + y  # Last expression is implicit return
}

fn addWithCheck(x: Int, y: Int) -> Int {
    if x < 0 or y < 0 {
        return 0;  # Early return
    }
    x + y
}

fn greet(name: String) -> Void {
    print("Hello, {name}");
}

# main is the entry point
fn main() -> Void {
    let sum = add(3, 4);
    greet("World");
}
```

### Operators

#### Arithmetic
- `+` addition
- `-` subtraction  
- `*` multiplication
- `/` division
- `%` modulo

#### Comparison
- `==` equal
- `!=` not equal
- `<` less than
- `>` greater than
- `<=` less than or equal
- `>=` greater than or equal

#### Logical
- `and` logical AND
- `or` logical OR
- `not` logical NOT

#### Assignment
- `=` assignment

### Types and Modules

Types and modules share the same structure in Thoxa - both are blocks of code with visibility rules.

```thoxa
# Type definition (multiple instances)
type Counter(initial: Int = 0) {
    export let mut count = initial
    let startTime = now()  # Private, runs on construction
    
    export fn increment() {
        count += 1  # Closes over instance data
    }
}

# Module file (singleton instance)
# math.thx
export let PI = 3.14159
let TAU = PI * 2  # Private

export fn sqrt(x!: Float) -> Float {
    # Implementation
}
```

**Key Features:**
- No explicit `self` - methods close over instance data
- Members require `export` for external visibility  
- Constructor parameters are named by default (use `!` for positional)
- Type body statements run on construction
- Modules are singleton types instantiated on import

See [Units Documentation](../plans/UNITS.md) and [Modules Documentation](MODULES.md) for details.

### Type Definitions

Types are defined using the `type` keyword with implicit `self`. Types can take constructor parameters:

```thoxa
# Simple type with defaults
type Point {
    export let x = 0;  # Exported fields are read-only from outside
    export let y = 0;
    
    export fn distance() {
        sqrt(x * x + y * y)  # 'self.x' implied
    }
}

# Type with constructor parameters and initialization logic
type Point3D(x: Float, y: Float, z: Float) {
    # Type body executes during construction
    assert(isFinite(x) && isFinite(y) && isFinite(z));
    
    export let x = x;  # Parameter shadows field name
    export let y = y;
    export let z = z;
    
    # Computed fields
    let magnitude = sqrt(x * x + y * y + z * z);
    
    export fn distance() {
        magnitude  # Use precomputed value
    }
    
    # Constructor can have side effects
    print("Created Point3D at ({x}, {y}, {z})");
}

# Instance creation
let p1 = Point();              # Uses defaults
let p2 = Point3D(1.0, 2.0, 3.0);  # With parameters

# Mutable fields
type Counter(initial: Int) {
    mut count = initial;  # 'mut' instead of 'let' for mutable fields
    
    export fn increment() {
        count += 1;  # Allowed because count is mut
    }
}
```

Note: Method copying/rebinding is a future enhancement.

## Type System

Thoxa features a static type system with runtime introspection. All type checking happens at compile time, but values carry type information for debugging, safe downcasting, and serialization.

Key features:
- Static type checking at compile time
- Types are heap-allocated objects
- Runtime type information for introspection
- 16-byte value representation (type pointer + data)
- Interface-based polymorphism

See [Type System Documentation](TYPES.md) for details.

### Interfaces (Not Yet Implemented)

**Note: Interfaces are planned but not yet implemented.**

Interfaces will define contracts that types can implement:

```thoxa
interface Drawable {
    export fn draw();
}

interface Bounded {
    export fn getBounds() -> Rectangle;
}

# Types implicitly implement interfaces by having matching methods
type Circle(x: Float, y: Float, radius: Float) {
    export let x = x;
    export let y = y;
    export let radius = radius;
    
    export fn draw() {
        # Implementation
    }
    
    export fn getBounds() -> Rectangle {
        Rectangle(x - radius, y - radius, radius * 2, radius * 2)
    }
}

# Static polymorphism
fn renderAll(items: Array(Drawable)) {
    for item in items {
        item.draw();  # No runtime type check needed
    }
}
```

### Type Checking

```thoxa
# Compile-time type checking
fn process(shape: Drawable) {
    shape.draw();  # Statically verified
}

# Runtime type introspection (for specific use cases)
fn describe(value: Any) {
    print("Type: {value.type.name}");
    
    # Safe downcasting with runtime check
    if (value.type == Circle) {
        let c = value as Circle;  # Compiler allows this based on type test
        print("Radius: {c.radius}");
    }
}
```

### Type Inference

Thoxa uses limited type inference for clarity and simplicity:

```thoxa
# Variable type inference from literals
let x = 42;         # Inferred: Int64
let y = 3.14;       # Inferred: Float64  
let s = "hello";    # Inferred: String
let b = true;       # Inferred: Bool (8-bit)

# Type inference from constructors
let p = Point();    # Inferred: Point
let c = Circle(5.0, 5.0, 10.0);  # Inferred: Circle

# Array type inference
let nums = [1, 2, 3];        # Inferred: Array<Int>
let mixed = [1, 2.5, 3];     # Error: heterogeneous array

# Functions require explicit types
fn add(x: Int, y: Int) -> Int {  # All types required
    x + y
}

# Type fields can be inferred
type Config {
    let host = "localhost";   # Inferred: String
    let port = 8080;         # Inferred: Int
    export let debug = true; # Inferred: Bool
}

# Conditional branches must have same type
let result = if condition { 42 } else { 0 };     # OK: both Int
let bad = if condition { 42 } else { "hello" };  # Error: type mismatch
```

### Primitive Types

Thoxa provides the following primitive types:

```thoxa
# Integer types
Int8, Int16, Int32, Int64     # Signed integers
UInt8, UInt16, UInt32, UInt64  # Unsigned integers
Int = Int64                    # Default integer type
UInt = UInt64                  # Default unsigned type
Byte = UInt8                   # Byte alias

# Floating point types  
Float32, Float64               # IEEE 754 floating point
Float = Float64                # Default float type

# Other primitives
Bool                           # 8-bit boolean (0 or 1)
Char                           # Unicode character (32-bit)
String                         # UTF-8 string (pointer + length)
Void                           # No value (represented as 0u64)

# Reference types
Ptr<Type>                      # Pointer to Type

# Literal inference defaults
let i = 42;                    # Int64 (default)
let u = 42u;                   # UInt64 (default unsigned)
let f = 3.14;                  # Float64 (default)
let f = 3.14d;                 # Float64 (explicit double)
let f32 = 3.14f;               # Float32 (float suffix)
let b = true;                  # Bool
let c = 'A';                   # Char
let s = "hello";               # String

# Integer type suffixes
let a = 42i8;                  # Int8
let b = 42i16;                 # Int16
let c = 42i32;                 # Int32
let d = 42i64;                 # Int64
let e = 42u8;                  # UInt8
let f = 42u16;                 # UInt16
let g = 42u32;                 # UInt32
let h = 42u64;                 # UInt64

# Number bases
let hex = 0xFF;                # 255 in hexadecimal
let bin = 0b1010;              # 10 in binary
let dec = 42;                  # 42 in decimal
```

## Union Types

Thoxa supports union types (also known as sum types or algebraic data types) through type composition:

```thoxa
# Define individual types
type Circle {
    export let x: Float;
    export let y: Float;
    export let radius: Float;
}

type Rectangle {
    export let width: Float;
    export let height: Float;
}

# Create a union type
type Shape = Circle | Rectangle;

# Use with implicit conversion
let c = Circle { x: 0.0, y: 0.0, radius: 5.0 };
let s: Shape = c;  # Implicitly converts Circle to Shape
```

### Pattern Matching on Unions

Union types are matched using typed patterns in match expressions:

```thoxa
fn area(shape: Shape) -> Float {
    match shape {
        c: Circle -> {
            3.14159 * c.radius * c.radius
        },
        r: Rectangle -> {
            r.width * r.height
        }
    }
}

# Usage
let circle = Circle { x: 0.0, y: 0.0, radius: 5.0 };
let shape: Shape = circle;
let a = area(shape);  # Returns circle area
```

### Generic Unions

Unions work naturally with generic types:

```thoxa
# Generic types
type Has<T>(value: T) {
    export value = value;
}

type Not {}

# Generic union
type May<T> = Has<T> | Not;

# Usage
let m1: May<Int> = Has(42);
let m2: May<String> = Not();
```

## Error Handling

Thoxa uses union types for error handling through `Trial<T, E>` and `May<T>` types:

### May Type

The `May<T>` type represents optional values:

```thoxa
# Define May as a union
type Has<T>(value: T) {
    export value = value;
}
type Not {}
type May<T> = Has<T> | Not;

# Usage
let x: May<Int> = Has(42);
let y: May<Int> = Not();

fn findFirst(arr: Array<Int>, pred: Fn(Int) -> Bool) -> May<Int> {
    for item in arr {
        if pred(item) {
            return Has(item);
        }
    }
    return Not();
}
```

### Trial Type

The `Trial<T, E>` type represents failable operations:

```thoxa
# Define Trial as a union
type Pass<T>(value: T) {
    export value = value;
}
type Fail<E>(error: E) {
    export error = error;
}
type Trial<T, E> = Pass<T> | Fail<E>;

# Usage
let success: Trial<Int, String> = Pass(42);
let failure: Trial<Int, String> = Fail("Division by zero");

fn divide(a: Float, b: Float) -> Trial<Float, String> {
    if b == 0.0 {
        return Fail("Division by zero");
    }
    Pass(a / b)
}

# Using the ? operator
fn calculate() -> Trial<Float, String> {
    let x = divide(10.0, 2.0)?;   # Unwraps Pass, returns Fail early
    let y = divide(x, 5.0)?;      
    Pass(y * 2.0)
}
```

### Pattern Matching

Pattern matching uses `match` expressions with required block syntax:

```thoxa
# Match as typed expression
let result = match value {
    pattern {
        expression
    }
    pattern {
        expression
    }
} else {
    default_expression
}
```

#### Pattern Types

**Literal patterns** - match exact values:
```thoxa
match status {
    0 { "success" }
    1 { "warning" }
    2 { "error" }
} else { "unknown" }
```

**Type patterns** - match based on union variant type:
```thoxa
# Match on union types
match shape {
    c: Circle {
        print("Circle at ({c.x}, {c.y}) with radius {c.radius}");
    }
    r: Rectangle {
        print("Rectangle {r.width}x{r.height}");
    }
}
```

**Constructor patterns** - destructure based on constructor parameters:
```thoxa
# Positional parameters (marked with !)
type Point(x!: Float, y!: Float) { ... }

match point {
    Point(0, 0) { "origin" }        # Match specific values
    Point(x, 0) { "on x-axis" }     # Bind x, match y=0
    Point(_, y) { "y = {y}" }       # Ignore x, bind y
    Point(a, b) { "({a}, {b})" }    # Bind both to new names
}

# Named parameters
type Widget(width!: Float, height: Float = 100) { ... }

match widget {
    Widget(100) { "width is 100" }           # Match positional
    Widget(height: 50) { "height is 50" }    # Match named
    Widget(w, height) { "w={w}, h={height}" } # Mixed extraction
    Widget(width) { "width={width}" }        # Extract positional as width
}

# Variadic parameters
type Logger(level!: Int, ...tags: String) { ... }

match logger {
    Logger(0) { "debug level" }              # Match level, ignore tags
    Logger(1, ...t) { "info with {t}" }      # Extract tags as t
    Logger(_, ...tags) { "tags: {tags}" }    # Ignore level, extract tags
}
```

**Pattern matching rules**:

1. **Constructor parameters** - All constructor parameters are matchable:
   - Positional params (`name!`) match by position, can be renamed
   - Named params match by name only, cannot be renamed
   - Variadic params (`...name`) require `...` to unpack, can be renamed

2. **Value matching** - What value gets matched:
   - Immutable parameters → constructor value (never changes)
   - Parameters with `export mut` → current field value
   - Non-constructor fields → not matchable

3. **Partial matching**:
   - Use `_` to ignore positional parameters
   - Omitted parameters are automatically ignored (no `..` needed)
   - Named parameters can be selectively extracted

```thoxa
type Counter(initial: Int, step: Int = 1) {
    export initial = initial;        # Immutable: matches constructor value
    export mut step = step;          # Mutable: matches current value
    mut count = initial;             # Not a constructor param: not matchable
}

let c = Counter(0, 2);
c.step = 5;

match c {
    Counter(0, 5) { "matches!" }     # initial=0 (constructor), step=5 (current)
    Counter(i, s) { "i={i}, s={s}" } # i=0, s=5
}
```

Notes:
- All match arms must have the same type
- `else` required for non-exhaustive matches
- Union type matching checks the variant type first

## Strings

Strings in Thoxa are UTF-8 encoded, immutable, and heap-allocated with reference counting.

### String Literals

```thoxa
let greeting = "Hello, World!";
let empty = "";
```

### String Interpolation

String literals support interpolation with `{}`:
```thoxa
let name = "World";
let greeting = "Hello, {name}!";  # "Hello, World!"

let x = 10;
let y = 20;
let result = "Sum of {x} and {y} is {x + y}";  # Expressions allowed

# Use double braces for literal braces
let escaped = "Use {{braces}} for literal {name}";  # "Use {braces} for literal World"
```

### String Operations

Strings are immutable and do NOT support the `+` operator for concatenation. Currently available:

```thoxa
let s = "Hello, World!";
print(s);         # Print without newline
println(s);       # Print with newline
```

### String Methods (Planned)

String methods exist in the runtime but are not yet exposed to Thoxa code:
```thoxa
# These will be available in the future:
let len = s.length();              # Get string length
let sub = s.substring(0, 5);       # Extract substring
let idx = s.indexOf("World");      # Find substring position
let has = s.contains("World");     # Check if contains substring
let c = s.charAt(0);               # Get character at index
```

### Multi-line Strings (Planned)

All strings will be able to span multiple lines with automatic dedenting:
```thoxa
let message = "
    This is a multi-line string
    with automatic dedenting.
    It preserves relative indentation.
";

# The string starts at column 0, not indented
# But relative indentation is preserved:
let code = "
    fn example() {
        println(\"Hello!\");  # This line stays indented relative to fn
    }
";
```

The dedenting algorithm:
1. Finds the minimum indentation across all non-empty lines
2. Removes that amount of leading whitespace from each line
3. Preserves relative indentation between lines
4. Skips initial newline after opening quote if present

## Memory Model

Thoxa uses automatic reference counting (RC) for heap-allocated objects.

### Stack vs Heap

All values in Thoxa are 16 bytes and live on the stack. The distinction is what the `data` field contains:

**Stack-stored data (inline):**
- Primitives: Int*, UInt*, Float*, Bool, Char, Void
- Pointers: Ptr<T> stores the address inline

**Heap-allocated (with RC):**
- Type instances (all user-defined types)
- Arrays
- Strings  
- Closures (future)

```thoxa
# Stack examples - data stored inline
let x = 42;              # {INT_TYPE, 42}
let p: Ptr<Int> = &x;    # {PTR_INT_TYPE, address}

# Heap examples - data field contains pointer
let point = Point(3, 4); # {POINT_TYPE, heap_ptr} -> RC=1
let arr = [1, 2, 3];     # {ARRAY_TYPE, heap_ptr} -> RC=1
let s = "hello";         # {STRING_TYPE, heap_ptr} -> RC=1
```

### Reference Counting Rules

- Assignment increments reference count
- Variables going out of scope decrement reference count  
- When RC reaches 0, object is immediately freed
- Deterministic destruction (no GC pauses)

```thoxa
{
    let p1 = Point(3, 4);    # Heap allocation, RC=1
    let p2 = p1;             # RC=2
    let p3 = p1;             # RC=3
}  # p1, p2, p3 out of scope, RC=0, freed
```

### Cycle Detection

Future: Weak references and cycle detection will be added to handle circular references.

## Module System (Not Yet Implemented)

**Note: The module system is planned but not yet implemented. Currently, Thoxa only supports single-file programs.**

Each `.thx` file will be a module. Only functions and type definitions will be allowed at the module top level.

### Imports (Planned)

```thoxa
# Named imports
import { sqrt, abs } from "math"        # Imports specific exports
import { Vec3 } from "math/vectors"    # Nested paths (./math/vectors.thx)

# Default/namespace import  
import math from "math"                 # Import entire module
let x = math.sqrt(16);

# Relative imports
import { helper } from "../utils"       # Relative paths supported

# Re-exports
export { sqrt } from "math"             # Re-export from another module
```

### Exports

Only items marked `export` are accessible from other modules:

```thoxa
# math.thx
export fn sqrt(x: Float) -> Float {
    # Implementation
}

export type Vec2(x: Float, y: Float) {
    export let x = x;
    export let y = y;
}

fn helperFunction() {  # Not exported, module-private
    # ...
}
```

### Module Initialization

- Imports make names available (no code execution)
- Type bodies are constructor functions that run on instantiation, not import
- Circular imports are allowed (functions and types are static)
- No global variables - all module-level items must be functions or types
- No module initialization order issues since no code runs at import time

## Standard Library

[To be determined - core functions like print, sqrt, etc.]