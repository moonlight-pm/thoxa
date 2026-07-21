# The Thoxa Programming Language Reference

## Table of Contents

1. [Introduction](#introduction)
2. [Notation](#notation)
3. [Lexical Structure](#lexical-structure)
   - [Whitespace](#whitespace)
   - [Comments](#comments)
   - [Tokens](#tokens)
4. [Data Types](#data-types)
   - [Numeric Types](#numeric-types)
5. [Expressions](#expressions)
   - [Literal Expressions](#literal-expressions)
   - [Arithmetic Expressions](#arithmetic-expressions)
   - [Parenthesized Expressions](#parenthesized-expressions)
   - [Expression Sequences](#expression-sequences)
   - [Block Expressions](#block-expressions)
   - [Function Definitions](#function-definitions)
6. [Operators](#operators)
   - [Arithmetic Operators](#arithmetic-operators)
   - [Operator Precedence](#operator-precedence)
7. [Grammar](#grammar)
8. [Examples](#examples)

## Introduction

Thoxa is a systems programming language designed for simplicity, safety, and performance. This document describes the syntax and semantics of the Thoxa programming language.

### Design Principles

- **Simplicity**: Clear, minimal syntax
- **Performance**: Compiles to efficient native code
- **Safety**: Strong typing with explicit conversions
- **Expressiveness**: Everything is an expression

## Notation

This document uses the following notational conventions:

- `syntax` - Code examples and syntax patterns use monospace font
- *name* - Italic text represents metavariables or placeholders
- **term** - Bold text introduces new terms
- `[optional]` - Square brackets denote optional elements
- `element*` - Asterisk denotes zero or more repetitions
- `element+` - Plus denotes one or more repetitions
- `a | b` - Pipe denotes alternatives

## Lexical Structure

A Thoxa program consists of a sequence of tokens. Tokens are the basic lexical elements of the language.

### Whitespace

Whitespace characters (spaces, tabs, newlines, and form feeds) are ignored except as they serve to separate tokens. Whitespace is defined as:

```
whitespace = [ \t\n\f]+
```

### Comments

Line comments start with `#` and continue to the end of the line. Everything after the `#` character is ignored by the compiler.

```
comment = '#' [^\n]*
```

Examples:
```thoxa
# This is a comment
42 # The answer to everything

# Comments can span entire lines
1 + 1 # Or appear at the end of lines
```

### Tokens

The language recognizes the following token types:

```
token = number | operator | delimiter | identifier | keyword | builtin
```

#### Numbers

Integer literals consist of one or more decimal digits:

```
number = [0-9]+
```

Examples: `0`, `42`, `1234`

#### Operators

Arithmetic operators:

```
operator = '+' | '-' | '*' | '/'
```

#### Delimiters

Delimiters for grouping, sequences, and language constructs:

```
delimiter = '(' | ')' | '{' | '}' | ';' | ':' | '->' | '=' | '.' | '&'
```

#### Keywords

```
keyword = 'type' | 'fn' | 'let' | 'mut' | 'export' | 'import' | 'from' | 
          'as' | 'can' | 'if' | 'else' | 'loop' | 'break' | 'continue'
```

#### Identifiers

Identifiers follow camelCase (variables/functions) or PascalCase (types) conventions:

```
identifier = [a-zA-Z][a-zA-Z0-9]*
```

Examples: `myVariable`, `Int32`, `calculateSum`

#### Built-in Functions

Special built-in functions start with `@`:

```
builtin = '@intType' | '@print' | '@printStr' | '@strlen' | '@cstr' | 
          '@intToString' | '@syscall'
```

## Data Types

## Naming Conventions

Thoxa follows strict naming conventions:
- **Types**: PascalCase (e.g., `Type`, `Int32`, `String`)
- **Variables and functions**: camelCase (e.g., `myVariable`, `calculateSum`)
- **No underscores** in identifiers following the naming conventions

### Type System Philosophy

In Thoxa, **types are first-class values** that can be manipulated at compile time, similar to Zig. This means types themselves are expressions that evaluate to type values.

### Built-in Types

- `Void` - No value
- `String` - UTF-8 encoded text with length prefix
- `Int8`, `Int16`, `Int32`, `Int64` - Signed integers
- `Uint8`, `Uint16`, `Uint32`, `Uint64` - Unsigned integers

### Numeric Types

Thoxa's basic integer types are **defined in the language itself** using internal helpers, not hardcoded in the compiler:

| Type    | Description                  | Size    |
|---------|------------------------------|---------|
| `Int8`  | 8-bit signed integer         | 1 byte  |
| `Int16` | 16-bit signed integer        | 2 bytes |
| `Int32` | 32-bit signed integer        | 4 bytes |
| `Int64` | 64-bit signed integer        | 8 bytes |
| `Uint8` | 8-bit unsigned integer       | 1 byte  |
| `Uint16`| 16-bit unsigned integer      | 2 bytes |
| `Uint32`| 32-bit unsigned integer      | 4 bytes |
| `Uint64`| 64-bit unsigned integer      | 8 bytes |

### Built-in Type Helpers

The `@intType` function creates integer types:

```thoxa
@intType(signed: bool, bits: number) -> Type
```

Examples:
```thoxa
@intType(true, 32)   # Creates a 32-bit signed integer type
@intType(false, 64)  # Creates a 64-bit unsigned integer type
```

### String Type

Strings in Thoxa are length-prefixed and UTF-8 encoded:

```thoxa
let message: String = "Hello, World!";
let name: String = "Thoxa";
```

### Arrays

Arrays are homogeneous collections with built-in methods:

```thoxa
let numbers = [1, 2, 3, 4, 5];  # Array of Int64
numbers[0]        # Array indexing: 1
numbers.length()  # Built-in method: 5
```

## Expressions

In Thoxa, everything is an expression that evaluates to a value. Currently, the language supports arithmetic expressions.

### Literal Expressions

A numeric literal is the simplest form of expression:

```
literal_expr = number
```

Example: `42`

### Arithmetic Expressions

Binary arithmetic operations combine two expressions:

```
binary_expr = expr operator expr
```

Examples:
- `1 + 2`
- `10 - 5`
- `3 * 4`
- `20 / 4`

### Parenthesized Expressions

Parentheses can be used to override operator precedence:

```
paren_expr = '(' expr ')'
```

Example: `(2 + 3) * 4`

### String Literals and Template Literals

String literals support template interpolation with `${}`:

```thoxa
let age: Int64 = 25;
let message: String = "I am ${age} years old";  # Automatic conversion
```

### Expression Sequences

Multiple expressions can be sequenced using semicolons. The sequence evaluates all expressions in order and returns the value of the last expression:

```
sequence_expr = expr (';' expr)* [';']
```

Examples:
- `1; 2; 3` - Evaluates to `3`
- `42; 100` - Evaluates to `100`
- `1 + 1; 2 * 3;` - Evaluates to `6` (trailing semicolon allowed)

### Block Expressions

Block expressions group multiple expressions within braces. Like sequences, they evaluate all expressions and return the last value:

```
block_expr = '{' [expr (';' expr)* [';']] '}'
```

Examples:
- `{ 1; 2; 3 }` - Evaluates to `3`
- `{ 42 }` - Evaluates to `42`
- `{}` - Empty block, evaluates to `0`
- `{ 1 + 1; { 2; 3 }; 4 }` - Nested blocks, evaluates to `4`

### Function Definitions

Functions are defined within modules using the `fn` keyword with parameters, return type, and body:

```
function_def = 'fn' identifier '(' [parameter (',' parameter)*] ')' '->' type_expr block_expr
parameter = identifier ':' type_expr
```

Functions may optionally take `self` or `&self` as the first parameter for method-style calls:

Examples:
```thoxa
module Math {
    export fn add(a: Int64, b: Int64) -> Int64 { a + b }
    export fn getValue() -> Int64 { 42 }
}

module Counter {
    export let value: Int64 = 0;
    export fn getValue(self: &Counter) -> Int64 { self.value }
}
```

### Let Bindings

Let bindings create named value assignments with explicit type annotations:

```
let_binding = 'let' identifier ':' type_expr '=' expr
```

Examples:
- `let x: Int64 = 42` - Bind the value 42 to name x with type Int64
- `let result: Int64 = 2 + 3` - Bind expression result to name
- `let type_value: Type = Int32` - Types can be bound to variables

### Type Definitions

Types group related values and functions with export control:

```
type_def = 'type' identifier '{' [type_member (';' type_member)*] '}'
type_member = ['export'] (let_binding | function_def | conversion_def)
conversion_def = 'as' type_expr block_expr
```

Examples:
```thoxa
type Point { 
    export let x: Int64 = 0; 
    let y: Int64 = 0; 
}

type Temperature {
    export let celsius: Int64;
    
    as String {
        "${celsius}°C"
    }
}
```

### Field Access

Field access retrieves members from module instances:

```
field_access = expr '.' identifier
```

Examples:
- `p.x` - Access field x from object p
- `obj.field1.field2` - Chained field access

### Reference Expressions

Reference expressions create references to values (syntax parsing implemented, safety rules pending):

```
reference = '&' expr
```

Examples:
- `&value` - Create reference to value
- `&obj.field` - Create reference to field

### Type Conversions

Explicit type conversions use the `as` keyword:

```thoxa
let n: Int64 = 42;
let s: String = n as String;  # Explicit conversion

# Check if conversion exists
if n can String {
    let s = n as String;
}
```

### Variadic Functions

Functions can accept variable numbers of arguments:

```thoxa
type IO {
    export fn println(...args: Int64) -> Void {
        # Print all arguments
    }
}
```

## Operators

### Arithmetic Operators

| Operator | Name           | Example | Result |
|----------|----------------|---------|--------|
| `+`      | Addition       | `5 + 3` | `8`    |
| `-`      | Subtraction    | `5 - 3` | `2`    |
| `*`      | Multiplication | `5 * 3` | `15`   |
| `/`      | Division       | `20 / 4`| `5`    |

*Note: Division is integer division and truncates toward zero.*

### Operator Precedence

Operators follow standard mathematical precedence rules:

| Precedence | Operators | Associativity |
|------------|-----------|---------------|
| 1 (highest)| `*` `/`   | Left-to-right |
| 2 (lowest) | `+` `-`   | Left-to-right |

Examples:
- `2 + 3 * 4` evaluates as `2 + (3 * 4)` = `14`
- `10 - 6 / 2` evaluates as `10 - (6 / 2)` = `7`
- `2 * 3 + 4` evaluates as `(2 * 3) + 4` = `10`

## Grammar Summary

Key syntax elements (simplified):

```
# Type definitions
type_def = 'type' identifier '{' member* '}'
member = ['export'] (let_binding | function_def | conversion)
conversion = 'as' type '{' expr '}'

# Variables and functions  
let_binding = 'let' identifier ':' type '=' expr
function_def = 'fn' identifier '(' params ')' '->' type block
parameter = ['...'] identifier ':' type

# Expressions
expr = binary_expr | unary_expr | primary_expr
primary_expr = literal | identifier | call | index | conversion
conversion_expr = expr 'as' type
can_check = expr 'can' type

# Arrays and strings
array_literal = '[' expr_list ']'
string_literal = '"' (char | '${' expr '}')* '"'
```

## Examples

### Basic Arithmetic

```thoxa
1 + 1
// Result: 2

42
// Result: 42

10 - 3
// Result: 7
```

### Operator Precedence

```thoxa
2 + 3 * 4
// Result: 14 (not 20)

(2 + 3) * 4
// Result: 20

100 / 10 + 5
// Result: 15

100 / (10 + 5)
// Result: 6 (integer division)
```

### Complex Expressions

```thoxa
(10 + 5) * 3 - 20 / 4
// Result: 40
// Evaluation: (15) * 3 - (5) = 45 - 5 = 40

2 * (3 + 4) * 5
// Result: 70
// Evaluation: 2 * (7) * 5 = 14 * 5 = 70
```

### Type Conversions

```thoxa
# Explicit conversions
let n: Int64 = 42;
let s: String = n as String;
// Result: "42"

# Conversion checking
if n can String {
    @printStr("Can convert to string");
}

# Type guards (runtime type checking)
let x: Int64 = 42;
if x is Int64 {
    @printStr("x is an integer");
}

# Template literals with automatic conversion
let age: Int64 = 25;
let msg = "I am ${age} years old";
// Result: "I am 25 years old"
```

### Expression Sequences and Blocks

```thoxa
1; 2; 3
// Result: 3 (evaluates all expressions, returns last)

{ 10 + 5; 20 * 2 }
// Result: 40 (block with two expressions)

{ 1; { 2; 3 }; 4 }
// Result: 4 (nested blocks)
```

### Arrays and Variadic Functions

```thoxa
# Arrays
let nums = [1, 2, 3, 4, 5];
nums[2]         // Result: 3
nums.length()   // Result: 5

# Variadic function
type IO {
    export fn println(...args: Int64) -> Void {
        let i = 0;
        loop {
            if i >= args.length() { break; };
            @print(args[i]);
            i = i + 1;
        };
        @printStr("\n");
    }
}

let io: IO;
io.println(1, 2, 3);  # Prints: 1 2 3
```

### Working Language Examples

#### Let Bindings and Variables
```thoxa
let x: Int64 = 42;
x + 8
// Result: 50

let a: Int64 = 10;
let b: Int64 = 20;
a + b
// Result: 30

{ let x: Int64 = 5; x * 2 }
// Result: 10 (let binding in block scope)
```

#### Type System
```thoxa
type Point {
  export let x: Int64 = 10;
  export let y: Int64 = 20;
  
  as String {
    "(${x}, ${y})"
  }
}

let p: Point;
p.x                    // Result: 10
let s = p as String;   // Result: "(10, 20)"

# Multiple instances
let p1: Point;
let p2: Point;
p1.x + p2.y           // Result: 30
```

#### Complex Examples
```thoxa
// Type with calculations
type Rectangle {
  export let width: Int64 = 10;
  export let height: Int64 = 5;
  
  export fn area(self: &Rectangle) -> Int64 {
    self.width * self.height
  }
}

let r: Rectangle;
r.area()    // Result: 50
```

#### Reference Expressions (Working)
```thoxa
// References to type instances
type Data { export let value: Int64 = 100; }
let d: Data;
&d      // Returns the base address
&d.value  // Returns address of value field
```

#### Memory Allocation and Initialization
```thoxa
// Types allocate real memory on the stack
type Vec3 {
    export let x: Int64 = 1 * 10;
    export let y: Int64 = 2 * 10;
    export let z: Int64 = 3 * 10;
}

let v1: Vec3;  // Allocates 24 bytes
let v2: Vec3;  // Separate instance

v1.x + v1.y + v1.z  // Result: 60
```

## Currently Implemented Features

**Core Language**
- Arithmetic operations with proper precedence
- Let bindings and mutable variables
- Block expressions and scoping
- Line comments with `#`
- Control flow: if/else, loop/break/continue
- String literals with template interpolation `${expr}`
- Arrays with indexing and built-in methods
- Variadic functions with `...args` syntax

**Type System**
- Type definitions with `type` keyword
- Explicit type conversions with `as`
- Conversion checking with `can`
- User-defined conversions in type blocks
- Built-in types: integers, strings, arrays, void
- Type checking and inference
- Method calls with dot syntax

**Module System**
- File-based modules (every .thx file is a module)
- Import statements with destructuring
- Export control for type members
- Circular dependency detection
- Cross-module function calls

**Memory Management**
- Stack allocation for type instances
- Reference expressions with `&`
- Compile-time memory safety validation
- Array bounds checking

**Compiler**
- JIT compilation for development
- AOT compilation to native executables
- Symbol table and type environment
- Comprehensive error messages

## Planned Features

- User-defined conversion code generation
- Array type syntax (`[Int64]`, `[String; 10]`)
- Standard library print functions (deprecate builtins)
- Floating-point types
- Boolean type
- Pattern matching
- Error handling with Result types
- Generics
- Traits/interfaces
- Heap allocation
- Package manager

---

*This document describes Thoxa version 0.1.0. Last updated: 2025*