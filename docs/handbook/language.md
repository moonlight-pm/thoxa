# Language

This chapter defines the language as the compiler implements it. Each rule is
stated once, in the section where you would look for it; examples follow the
rules. Programs in this chapter that have a `main` compile and run as shown.

## Lexical structure

### Source text

A source file is UTF-8 text with the extension `.thx`. The language is
case-sensitive. Spaces, tabs, and newlines separate tokens; how a line break
affects an expression is described in [Line breaks](#line-breaks).

### Comments

`#` starts a comment that runs to the end of the line. There are no block
comments. By convention `##` marks documentation: the editor shows the
contiguous `##` lines above a declaration when you hover its name.

```thx
## Price in cents, tax included.
fn total(cents: Int64) -> Int64 {
    cents + cents / 10   # flat 10% tax
}
```

Inside the text of a [tag literal](#tag-literals-and-node) a bare `#` also
starts a comment; write `\#` for a literal `#`.

### Identifiers

An identifier is an ASCII letter followed by any number of ASCII letters and
digits. Underscores are not allowed inside identifiers. A lone `_` is a name;
in a pattern it discards the value.

Conventions: values, functions, parameters, and namespace bindings are
`camelCase`; types and union members are `PascalCase`; module file names are
`kebab-case`. Case is significant in one place the compiler checks: in a tag
literal a lowercase name is an HTML element and a `PascalCase` name is a
component.

### Keywords

These words are reserved:

```text
and       as        break     continue  else      export    extern
false     fn        for       from      if        import    in
let       loop      match     mut       not       or        return
true      type      where     while
```

Some words have a meaning only in one position and are otherwise ordinary
names: `sql` directly before a string literal (`sql"…"`), and the HTTP verbs
`GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS` at the start of an entry
in a type body (see [Route clauses](#route-clauses)).

### Literals

**Integers** are decimal digits (`42`) or hexadecimal with a `0x` / `0X`
prefix (`0xFF`). There are no digit separators and no octal or binary forms.
The value must fit in `Int64`; a minus sign is the unary operator.

**Floats** are digits with a fraction or an exponent: `1.5`, `1e2`,
`1.5e-1`. An `f64` suffix is allowed (`3.14f64`, `1f64`). A `.` starts a
fraction only when a digit follows, so `1..5` is a range. There is no
`Float32`; an `f32` suffix is a compile error.

**Booleans** are `true` and `false`. The value of type `Void` is written
`Void`.

**Strings** are written in double quotes. A string may span lines; newlines
are kept, and there is no indentation stripping.

| Escape | Byte |
|--------|------|
| `\n` | line feed |
| `\t` | tab |
| `\r` | carriage return |
| `\\` | backslash |
| `\"` | double quote |
| `\0` | NUL (kept inside the string; the rest of the string follows it) |
| `\xHH` | one ASCII byte, `\x00` to `\x7F` |

`\xHH` above `\x7F` is a compile error: strings are UTF-8, and one high byte
is not a character. Write the character itself, or build `Bytes`. Any other
escape is a compile error.

`{expr}` inside a string is an interpolation hole: the expression's value is
formatted into the string. A hole may contain strings of its own
(`"{greet("ada")}"`). `{{` and `}}` are literal braces. Holes accept `String`,
the integer types, `Float64`, and `Bool`. A `Node` does not interpolate; call
`.html()`.

```thx
let name = "ada"
let line = "hello {name}, {1 + 2} new"     ## hello ada, 3 new
let json = "{{\"qty\": 2}}"                ## {"qty": 2}
```

`sql"…"` is a tagged string: the result is an `Sql` value and every hole is
embedded as SQL (strings quoted, numbers as digits). There must be no space
between `sql` and the quote. See [Sqlite](std.md#stdsqlite).

Array, map, and tag literals are described with their types:
[Array](#arrays), [Map](#maps), [tag literals](#tag-literals-and-node).

### Operators and punctuation

```text
+    -    *    /    %    &    |    ^    ~    <<   >>
==   !=   <    <=   >    >=   =    ?    !
.    ..   ..=  ...  :    ,    ->   =>   -->
(    )    [    ]    {    }
```

`->` appears only in types (`fn f() -> Int64`, `(x: Int64) -> Int64`). `=>`
appears only in match arms. `-->` starts a task (see
[Concurrency](#concurrency)). There are no compound assignment operators
(`+=`) and no increment operators.

### Line breaks

There are no statement terminators. A newline does not end an expression by
itself; an expression ends where the grammar says it ends, and the next
statement starts on a new line. Three rules decide the cases where that is
ambiguous:

1. **A line that starts with `-x`** (a minus sign followed directly by an
   operand) starts a new expression. It is not a subtraction from the line
   above. To continue an expression onto the next line, end the first line
   with the operator, or put a space after the minus (`- x`).
2. **A line that starts with `(` or `[`** continues the expression on the line
   above, as a call or an index. Start such a line with a `let` instead.
3. **`<` followed directly by a name or by `>`** (`<div`, `<>`) starts a tag
   literal. As an operator, `<` needs a space after it: `a < b`. `a<b` with no
   space on either side is a compile error.

```thx
fn sign(n: Int64) -> Int64 {
    if n < 0 {
        -1           ## a new expression, the value of this branch
    } else {
        1
    }
}

let total = first
    + second         ## continues: the line above ends the operand, `+ second` is a binary operator
```

Match patterns take negative literals (`-1 => …`).

## Programs

### Entry point

A program is a package with a function named `main`. `main` takes no
parameters, or one `Array<String>` of command-line arguments, where element 0
is the program path:

```thx
fn main(arguments: Array<String>) -> Int64 {
    arguments.count - 1      ## the number of user arguments, as the exit status
}
```

What `main` returns is the process exit status, `0` to `255`. An unannotated
`main` returns `Int64`. Any other value panics once the program's tasks have
finished: `thoxa panic: main returned 256; an exit status is 0 to 255`. (The
kernel would keep only the low 8 bits, so `256` would read as success.)
After `main` returns, the program keeps running until every task it started
has finished, then exits.

### Programs and session scripts

| Shape | Top level allows |
|-------|------------------|
| Program (`fn main` present) | Declarations only: `import`, `type`, `fn`, `let` / `mut`, `extern`, `export` |
| Session script (no `main`) | Declarations and bare statements (`info("hi")`, assignments, commands) |

A bare name such as `FOOBAR` at file scope is a statement, not a declaration.
In a program it is a compile error, reported at that span. In a session
script it is part of the script body (and fails later if the name is
undefined). Session scripts are described in [Session](session.md).

### Packages

`thoxa run dir/` runs `dir/main.thx`. The package root is the directory of the
nearest `main.thx` above the entry file (otherwise the entry file's
directory); `~/…` import paths start there. See [Modules](#modules).

## Bindings

`let name = expr` binds an immutable name. `mut name = expr` binds a mutable
one, which `name = expr` reassigns; the new value converts to the binding's
type as it would under an annotation (a member into its union, an integer
into `Float64`). Assigning to a `let` is a compile error.
An annotation gives the type explicitly and is required where the value alone
does not determine it:

```thx
let port = 8080
let ratio: Float64 = 1           ## the integer converts to Float64
mut names: Array<String> = []
mut seen = 0
seen = seen + 1
```

Overwriting a `mut` binding that holds a heap value releases the old value.

**Shadowing.** A `let`, a match binding, or a `for` variable may reuse an
outer name. Inside its block it is a new variable, possibly of another type;
after the block the outer variable is unchanged.

**Module bindings.** `let` and `mut` may appear at the top level of a module,
including heap values, with an optional annotation:

```thx
mut connection: May<Db> = Not()
mut subscribers: Map<Int64, Socket> = Map()
```

A module binding is initialized before `main` runs and lives for the rest of
the process. Assigning to a module `mut` releases the previous value and keeps
the new one; the value is not released when the assigning function returns.
In the interactive shell, top-level bindings keep their values across lines;
see [Session](session.md#repl-behavior).

There is no `import` or `type` inside a function.

## Types

### Integers

| Type | Range |
|------|-------|
| `Int64` | -2^63 to 2^63-1 (the default integer type) |
| `Int32` | -2^31 to 2^31-1 |
| `UInt32` | 0 to 2^32-1 |
| `UInt64` | 0 to 2^64-1 |
| `Byte` | 0 to 255 |

Arithmetic happens in `Int64`.

- **Overflow wraps** (two's complement); it is never undefined behaviour.
- **Division** `/` truncates toward zero and `%` takes the sign of the
  dividend: `-7 / 2` is `-3`, `-7 % 2` is `-1`. Dividing by zero panics with
  `division by zero`.
- **Shifts.** `a << n` wraps like `*`; `a >> n` keeps the sign. A count of 64
  or more shifts every bit out (`<<` gives 0, `>>` gives 0 or -1). A negative
  count panics with `negative shift count`.
- **Ranges.** `a..=b` includes `b` and stops there, even at the largest
  `Int64`.

**Narrowing.** A value passed, returned, or stored where `Int32`, `UInt32`,
`UInt64`, or `Byte` is expected converts when it fits. When it does not, the
program panics (`thoxa panic: 256 does not fit in Byte`). A literal that
cannot fit is a compile error. Widening (`Byte` to `Int64`) is never checked.
A `UInt64` above the `Int64` maximum panics when it meets `Int64` arithmetic
or a comparison.

To keep the low bits instead (a bit pattern, or `-1` for an unsigned C
parameter), say so with `truncating:`. `Int64`, `Int32`, `UInt64`, `UInt32`,
and `Byte` accept it:

```thx
let allOnes = UInt64(truncating: -1)   ## 18446744073709551615
let low = Byte(truncating: 300)        ## 44
let back = Int64(truncating: allOnes)  ## -1
```

### Float64

`Float64` is an IEEE-754 binary64 value (C `double`). It has `+ - * /` and
the comparisons; it has no `%` and no bitwise operators. Integers convert to
`Float64` where a `Float64` is expected (assignment, argument, field, return,
or a mixed arithmetic expression); `Float64` never converts to an integer
implicitly. There is no `Float32`. A NaN is not equal to itself.

Interpolation, tag-literal text, and JSON print the shortest decimal that
reads back as the same value. Between 1e-7 and 1e21 it is written out in full,
and an integral value keeps `.0`: `2.0`, `100.0`, `0.1`, `0.0000001`. Further
from 1 it takes an exponent: `1e+21`, `1e-08`, `2.5e-10`.

`Float64` values cannot be literal types, match-arm literals, or map keys.

### Bool and Void

`Bool` has the values `true` and `false`. `Void` is the type of an expression
with no value (a statement, an `if` without `else`, a function that returns
nothing); its one value is written `Void`. A `let` may bind a `Void` value
(`let finished = loop { … break … }`); the name then holds `Void`.

### String

A `String` is an immutable sequence of UTF-8 bytes. `.count` is its length in
bytes (`"héllo".count` is `6`). Strings support `==` and `!=` (byte
comparison); `<` and `>` on strings are a compile error.

| Method | Result |
|--------|--------|
| `.contains(needle)`, `.startsWith(prefix)`, `.endsWith(suffix)` | `Bool` |
| `.equals(other)` | `Bool`; `==` does the same |
| `.isEmpty()`, `.isAscii()` | `Bool` |
| `.toLower()`, `.toUpper()` | a new `String` |
| `.trim()`, `.trimStart()`, `.trimEnd()` | without leading and/or trailing white space |
| `.removePrefix(prefix)`, `.removeSuffix(suffix)` | without it; unchanged when it is not there |
| `.replace(old, new)`, `.replaceFirst(old, new)` | every / the first `old` replaced; an empty `old` changes nothing |
| `.repeat(count)` | the string `count` times; `""` when `count <= 0` |
| `.split(separator)` | `Array<String>`, empty pieces kept; an empty separator gives `[s]` |
| `.lines()` | `Array<String>`, split on `\n`, each without a trailing `\r` |

`.join(separator)` on an `Array<String>` puts the separator between the
elements. A [refined](#refinements) String has the same methods.

`contains`, `startsWith`, `endsWith`, and `equals` take `ignoreCase: true`,
which compares Unicode case folds:

```thx
"CAFÉ AU LAIT".contains("café", ignoreCase: true)    ## true
"Straße".equals("STRASSE", ignoreCase: true)         ## true
"Straße".toUpper()                                   ## "STRASSE"
```

Without `ignoreCase` the comparison is exact, byte for byte. `toLower` and
`toUpper` use Unicode's default case mappings, so one character may become
two (`ß` becomes `SS`); `Σ` always lowercases to `σ`, also at the end of a
word, and there are no language-specific rules (Turkish `i`). White space is
Unicode white space: space, tab, line ends, the no-break space, the
ideographic space, and the other `White_Space` characters. Neither case
folding nor `==` normalizes: a precomposed `é` is not `e` followed by a
combining accent. Counting and cutting by position are functions in
[`std/str`](std.md#utility-modules), and they count bytes.

### Bytes

`Bytes` is a fixed-length array of bytes. `Bytes(n)` makes `n` zero bytes;
`Bytes.fromString(s)` copies a string's bytes.
**A `Bytes` value is shared, not copied:** `let alias = b` refers to the same bytes, and `b.set(0, 65)` is
visible through `alias`.

| Member | Result |
|--------|--------|
| `.count` | length |
| `.get(i)` | the byte at `i` as `Int64`; out of range panics |
| `.set(i, byte)` | writes in place |
| `.slice(start, end)` | a new `Bytes` from `start` up to `end` |
| `.find(needle)`, `.find(needle, from)` | `May<Int64>`, the first index of `needle` |
| `.insertAt(at, byte)` | a new `Bytes` with `byte` inserted |
| `.removeAt(at)` | a new `Bytes` without the byte at `at` |
| `.equals(other)` | `Bool`; `==` does the same |
| `.toString()` | a `String` with the same bytes |

There is no `"…" as Bytes`; use `Bytes.fromString`.

### Buffer

`Buffer` is a growable byte buffer with value semantics (copy on write).
`Buffer()` is empty; `Buffer(capacity)` reserves space. Capacity doubles as
bytes are appended.

| Member | Result |
|--------|--------|
| `.append(byte)` | appends one byte |
| `.appendBytes(bytes)` | appends a `Bytes` |
| `.count` / `.capacity` | length / reserved size |
| `.freeze()` | the contents as `Bytes` |

### Arrays

`Array<T>` is a dynamic array with value semantics (see
[Collections](#collections)). A literal is `[a, b, c]`.

Where an array type is expected (an annotation, a parameter, a return type),
the literal takes its element type from it, and each element converts as it
would in any other expected position: members widen to a union, integers to
`Float64`, a nested `[]` takes the inner element type, and elements of a
refined or literal type are checked. Without an expected type, all elements
must have the same type, and `[]` needs an annotation.

```thx
let shapes: Array<Shape> = [Circle(radius: 1.0), Rect(width: 2.0, height: 3.0)]
let weights: Array<Float64> = [1, 2.5]
let quantities: Array<Qty> = [1, 2]        ## [0, 2] is a compile error
let grid: Array<Array<Int64>> = [[], [1, 2]]
mut names: Array<String> = []
```

| Member | Result |
|--------|--------|
| `a[i]` | the element; an index out of range panics with `array index out of bounds` |
| `a[i] = v` | replaces an element (needs a `mut` place) |
| `.count` | length |
| `.append(v)` | adds at the end |
| `.dropLast()` | removes the last element; panics on an empty array |
| `.insertAt(i, v)` | inserts before index `i` |
| `.removeAt(i)` | removes index `i` |

The writing members return `Void` and need a writable place: write
`xs.append(4)`, not `xs = xs.append(4)`. Arrays may nest; an inner array is
reference counted like any other heap value.

**Fixed arrays.** `Array<T, N>` is a fixed-length inline array, used as a
field of a [C-layout type](#c-layout-types) or as a local. Elements can be
read and written, and the whole array can be assigned or bound. An index
outside `0..N-1` panics with `fixed array index out of bounds`.

### Maps

`Map<K, V>` is a hash map that keeps insertion order. `Map()` is an empty map
(an annotation gives its types). A literal is `{ key: value, … }`: an
identifier key is the string of that name (`{ Host: "x" }` has the key
`"Host"`), any other key is an expression (`{ 1: "one", "content type": "x" }`).
An empty `{}` is a block, not a map. A key repeated in one literal is a compile
error. A map may be a field of a product.

```thx
mut ages: Map<String, Int64> = Map()
ages.set("ada", 36)
ages["alan"] = 41
match ages["ada"] {
    Has(age) => info("{age}")
    Not => info("missing")
}
for name, age in ages {
    info("{name} {age}")
}
```

| Member | Result |
|--------|--------|
| `m[k]`, `.get(k)` | `May<V>` (requires `std/fallible` in scope) |
| `.has(k)` | `Bool` |
| `m[k] = v`, `.set(k, v)` | inserts or replaces |
| `.remove(k)` | removes; a missing key is a no-op |
| `.count` | number of entries |
| `.keys()` / `.values()` | snapshots as `Array` |

Removing a key keeps the other keys in insertion order; a key set again after
its removal goes to the end. Writes need a writable place.

**Keys** may be `String`, `Int64`, `Bool`, or a user type whose fields hash by
value: integers, `Bool`, `String`, `Bytes`, refined and literal-union fields,
and nested such types. Two keys are the same key when they are `==`. A key
type holding `Float64`, `Array`, or `Map`, a type with `drop`, or a C-layout
type is a compile error. A call can be a literal key:

```thx
type Point(x: Int64, y: Int64)

mut grid: Map<Point, String> = Map()
grid.set(Point(x: 2, y: 3), "treasure")
let found = grid.has(Point(x: 2, y: 3))         ## true
let named = {Point(x: 0, y: 0): "origin"}
```

There is no `==` on maps.

### Products

A product type has named fields given as constructor parameters, and
optionally methods:

```thx
type Point(x: Int64, y: Int64) {
    export fn lengthSquared() -> Int64 {
        x * x + y * y
    }
}

let p = Point(x: 3, y: 4)
let d = p.lengthSquared()     ## 25
let x = p.x                   ## 3
```

A type with no parameters and no body (`type Empty`) has one value, written
`Empty()`. Fields may hold any type, including heap values, `Node`, other user
types, and unions. A field may not contain its own type, directly or through
other products (use an `Array` or a `Map`). Fields are readable from outside
the type and read-only there. Constructors take arguments like any call (see
[Calls and arguments](#calls-and-arguments)) and may declare defaults
(`qty: Int64 = 1`). Methods are described in
[User types and methods](#user-types-and-methods).

### Unions

A union lists its members with `|`:

```thx
type Circle(radius: Float64)
type Rect(width: Float64, height: Float64)
type Shape = Circle | Rect

fn area(shape: Shape) -> Float64 {
    match shape {
        Circle(radius) => 3.14159 * radius * radius
        Rect(width, height) => width * height
    }
}
```

Members are product types (with or without fields), literal values, or other
unions (which flatten). A value of a member converts to the union wherever the
union is expected: an argument, a return, an annotated `let`, or an `if` /
`match` branch whose result is expected to be the union. A union of string
literals is a subtype of `String`.

**Marks.** A member may carry `!` (the success case) and another `?` (the
residual case). The [`?` operator](#propagating-failure) unwraps the `!` member and
returns early with the `?` member. `!` on a type argument extracts that
payload; `!` on the whole member extracts the member itself:

```thx
type Hold(n: Int64)
type Empty
type Slot = Hold<Int64!> | Empty?      ## `slot?` yields the Int64
type Wrapped = Hold! | Empty?          ## `wrapped?` yields the Hold
```

`May` and `Trial` are unions defined this way in `std/fallible`; see
[Errors and panics](#errors-and-panics).

### Literal types

A literal (`"home"`, `0`, `true`) is a type with one value, usable in
parameters, returns, and annotations. Literal unions are written inline
(`mode: "read" | "write"`) or named:

```thx
type Home = "home"
type DrinkSlug = "espresso" | "mocha"
```

A string-literal union is the preferred way to write a closed set of strings.
Literal types are how [multi-clause functions](#multi-clause-functions)
dispatch on values.

### Refinements

`where` narrows a type to the values that satisfy a predicate. The result is a
distinct type:

```thx
type Qty = Int64 where >= 1
type Dozen = Qty where <= 12
type Slug = String where 1..64
type Order(slug: String, qty: Int64 where >= 1)
```

| Piece | Meaning |
|-------|---------|
| Comparisons | `>= 1`, `< 10`, `== 0`, `!= 0` |
| Ranges | `1..64` includes both ends (`1..=64` means the same); `..64` is at most 64, `1..` at least 1. On `Int64` / `Float64` they test the value; on `String` / `Array` they test `.count` |
| Literals | `String where "a" \| "b"` (prefer a literal union) |
| Combination | `&` (and), `\|` (or), parentheses. Outside a `where`, `\|` is still a union |
| Messages | A string after a predicate replaces its error message; it may use `{value}`, `{expected}`, `{path}` |
| Narrowing | `type Dozen = Qty where <= 12` adds to `Qty`'s predicate. A type that admits no value is a compile error |

A literal that satisfies the predicate converts implicitly (`let q: Qty = 2`).
A refined value converts to its base type implicitly (`let n: Int64 = q`).
Going from the base type to the refined one is checked at run time with
`value as T`, which gives a `Trial` (see [Conversions](#conversions)). There are no predicates across
fields of a product and no regular expressions.

### Generic types

Products and unions may take type parameters. Each use with concrete
arguments is compiled separately (monomorphized):

```thx
type Box<T>(value: T)
type Empty
type Opt<T> = Box<T!> | Empty?

let a: Opt<Int64> = Box(value: 7)
```

A field of a generic type may be any integer type, `Float64`, `Bool`,
`String`, `Bytes`, an `Array`, a `Map`, a user type, or `Void`.

There are no generic functions, no constraints on type parameters, and no
traits or interfaces. Methods of a generic type must annotate a non-`Void`
return type.

### Function types

A function type is written like a function header: `(x: Int64) -> Int64`.
See [Function values](#function-values).

### Node

`Node` is the built-in type of an HTML tree: `Element | Text | Fragment`. It
needs no import. Tag literals produce it. See
[Tag literals and Node](#tag-literals-and-node).

### C-layout types

A type with no constructor parameters whose body declares `mut` fields is laid
out as a C struct. See [C interop](#c-interop).

## Expressions

### Precedence

From lowest to highest. Binary operators at the same level associate to the
left.

| Level | Operators |
|-------|-----------|
| 1 | `or` |
| 2 | `and` |
| 3 | `==` `!=` `<` `<=` `>` `>=` |
| 4 | `\|` |
| 5 | `^` |
| 6 | `&` |
| 7 | `<<` `>>` |
| 8 | `+` `-` |
| 9 | `*` `/` `%` |
| 10 | `as` `as?` (see [Conversions](#conversions)) |
| 11 | unary `-` `not` `~` |
| 12 | postfix `?`, `.field`, `.method(…)`, `[index]`, `(arguments)` |

Bitwise operators bind tighter than comparisons, so `flags & 4 == 4` is
`(flags & 4) == 4`. A conversion binds tighter than arithmetic, so
`a * b as Float64` is `a * (b as Float64)`. Ranges (`a..b`, `a..=b`) are not expressions; they appear
only in `for` heads and `where` clauses.

### Operators

- `+ - * / %` on integers and `+ - * /` on `Float64`; see [Integers](#integers)
  for overflow and division.
- `& | ^ ~ << >>` on integers.
- `==` and `!=` on scalars, `String`, `Bytes`, and structurally on products,
  unions, and arrays; see [Equality](#equality).
- `< <= > >=` on numbers only.
- `and`, `or`, `not` on `Bool`. `and` and `or` short-circuit: the right
  operand is evaluated only when the left one does not decide the result, so
  `i < bytes.count and bytes.get(i) == 32` never reads past the end. Both
  operands must be `Bool`.

### Blocks

`{ … }` is a block: statements, one per line, whose value is the value of the
last expression. A function body is a block, and a block can be used as an
expression:

```thx
let total = {
    let base = 40
    base + 2
}
```

### Calls and arguments

A call passes its positional arguments first, then its named ones. Positional
arguments fill the first parameters in order; named arguments fill the rest,
in any order. A positional argument after a named one, or a named argument
for a parameter already given by position, is a compile error.

```thx
fn connect(host: String, port: Int64 = 8080, tls: Bool = false) -> String {
    "{host}:{port} tls={tls}"
}

connect("localhost", 9000)            ## positional
connect(port: 1, host: "a")           ## named, any order
connect("a", tls: true)               ## positional, then named
## connect(tls: true, "a")            ## compile error: positional after named
```

A parameter with a default may be omitted: by name anywhere, by position at
the end. Default expressions are evaluated at the call site, and free names in
them resolve in the module that defines the function, so callers need not
import them. A parameter without a default may not follow one with a default.

A parameter written `name!: T` is marked positional. The mark is required on
every parameter of an `extern` function and must match across the clauses of a
multi-clause function; calls are not checked against it.

There is no spread or variadic argument syntax in calls; a component's
`{...value}` in a tag literal is the one spread (see
[Tag literals and Node](#tag-literals-and-node)).

### Field access, methods, indexing

`value.field` reads a field. `value.method(args)` calls a method.
`value[index]` indexes an array, a fixed array, or a map. A `Type.method`
without a receiver is an unbound method (see
[Function values](#function-values)); `Type.read(…)` and the other
[typed-data](#typed-data) functions are called on the type.

### Equality

`==` and `!=` work on two values of the same type:

- numbers and `Bool` by value (`Float64` follows IEEE: NaN is never equal);
- `String` and `Bytes` byte by byte;
- products field by field, each field compared by its own type: a refined
  field by its base value, a literal-union field by which literal it is, a
  nested product field by field again;
- unions: equal when they are the same member with equal payloads, so `May`
  and `Trial` values compare. A union and one of its members compare as if the
  member were in the union;
- arrays: the same count and equal elements, in order.

```thx
type Point(x: Int64, y: Int64)
type Line(start: Point, end: Point)

let line = Line(start: Point(x: 0, y: 0), end: Point(x: 3, y: 0))
let same = Line(start: Point(x: 0, y: 0), end: Point(x: 3, y: 0))
let equal = line == same                   ## true

let found: May<Int64> = Has(2)
let two = found == Has(2)                  ## true
```

It is a compile error to compare values of different types, to use `<` or `>`
on a product, union, array, or string, or to compare types where some field or
member has no `==` (a `Map` or `Node` field). Types with `drop` and C-layout
types have no `==`. There is no `==` on maps or function values.

## Conversions

`x as T` converts `x` to the type `T`. There is at most one conversion for
each pair of types, and the result type says whether it can fail: `T` when it
cannot, `Trial<T, Array<Issue>>` when it can. The type after `as` is a name
with optional type arguments (`Order`, `Array<Order>`); a literal union or a
function type needs a named alias.

| From | To | Result |
|------|----|--------|
| a union member | its union | `T` |
| a literal union | its base (`String`, `Int64`, `Bool`) | `T` |
| a refined type | its base | `T` |
| an integer type | a wider integer type, or `Float64` | `T` |
| any type with a JSON encoding | `Json` | `T` (`toJson`) |
| the base | a refined type | `Trial<T, Array<Issue>>` (the `where` check) |
| `String` | a literal union | `Trial<T, Array<Issue>>` |
| `Int64` | `Int32`, `UInt32`, `UInt64`, `Byte` | `Trial<T, Array<Issue>>` |
| `Json` | any type with a reader | `Trial<T, Array<Issue>>` |

A literal takes `T` as its expected type first, as it would in an annotated
`let`: `3 as Quantity` is a `Quantity` (checked at compile time), `1 as
Float64` a `Float64`, and `[] as Array<Int64>` an empty array of `Int64`.

```thx
type Quantity = Int64 where >= 1 & <= 12
type Line(drink: String, quantity: Quantity = 1)

let rows = [Line(drink: "mocha")]
let body = rows as Json                       ## Json: cannot fail

match requested as Quantity {                 ## Trial<Quantity, Array<Issue>>
    Pass(quantity) => order(quantity)
    Fail(issues) => reject(messages(issues))
}

let lines = json as Array<Line>               ## a top-level array reads too
let small = 300 as Byte                       ## compile error: 300 does not fit
let big = count as Byte                       ## Trial: "300 does not fit in Byte" at run time
```

A failed conversion's issues are those of a read: `expected integer <= 12,
got 40`, `drink: expected string, got integer`, `0.quantity: …` inside an
array. Checked conversions need `std/json` in the program, for `Issue`.

**`x as? T`** is exactly `(x as T)?`: the converted value, or the `Fail`
returned from the enclosing function, under the rules of
[Propagating failure](#propagating-failure). It is written without a space,
and on a conversion that cannot fail it is a compile error (write `as`).

```thx
fn total(json: Json) -> Trial<Int64, Array<Issue>> {
    let lines = json as? Array<Line>
    mut sum = 0
    for line in lines {
        sum = sum + line.quantity
    }
    Pass(sum)
}
```

Not conversions, and compile errors that say so: text into a value
(`text as Order`; text is parsed with `T.read` or `T.readForm`), a `Float64`
into an integer type (no rounding rule is chosen), and keeping the low bits of
an integer (`Byte(truncating: x)`). Implicit narrowing at an expected type
still panics when the value does not fit; `as` is how to narrow without one.

### from clauses

A type declares conversions into itself with `from` clauses in its body:

```thx
type Celsius(degrees: Float64)

type Fahrenheit(degrees: Float64) {
    export from(celsius: Celsius) -> Fahrenheit {
        Fahrenheit(degrees: celsius.degrees * 1.8 + 32.0)
    }
}

type Email(address: String) {
    export from(text: String) -> Trial<Email, String> {
        if text.contains("@") {
            Pass(Email(address: text))
        } else {
            Fail("{text} is not an email address")
        }
    }
}

let hot = Celsius(degrees: 40.0) as Fahrenheit      ## Fahrenheit
let email = "ada@example.com" as Email              ## Trial<Email, String>
```

- A clause takes one parameter, the source, without a default, and returns the
  type itself (the conversion cannot fail) or `Trial<Type, E>` for any `E`.
  The return type is required.
- A clause has no receiver: the type's fields are not in scope. It is a
  constructor, not a method.
- A type may have several clauses for different source types; two for the
  same source, or one from the type itself, are compile errors. Generic types
  cannot declare clauses.
- `export` makes a conversion usable outside the type's module, as for
  methods.
- A clause replaces any built-in conversion for its pair, and `as?` on a
  fallible clause propagates its `E`.
- Only the target declares conversions into itself. Conversion into `Json` is
  the exception: a type declares it with its own `toJson` (see
  [Encoding](#encoding)).

A `from(json: Json)` clause is the type's JSON reader. It returns the type or
`Trial<Type, Array<Issue>>`, and `json as T`, `T.read(text)`, and the read of
any field of type `T` call it; a field read places its issues under the
field's path (`at: expected array, got object`). With its own `toJson` a type
owns both directions of its JSON:

```thx
## A point is written as [x, y].
type Point(x: Int64, y: Int64) {
    export from(json: Json) -> Trial<Point, Array<Issue>> {
        let pair = json as? Array<Int64>
        if pair.count != 2 {
            return Fail([issueRejected("", "a point is [x, y]")])
        }
        Pass(Point(x: pair[0], y: pair[1]))
    }

    export fn toJson() -> Json {
        [x, y]
    }
}
```

## Control flow

`if`, `match`, and blocks are expressions. `while`, `for`, and an `if` without
`else` are statements with type `Void`.

### if

```thx
let size = if count > 10 { "big" } else if count > 0 { "small" } else { "none" }
```

An `if` without `else` has no value. Where a value is expected, write the
`else`. A branch's value converts to an expected union type.

### while, loop, for

```thx
while queue.count > 0 {
    queue.dropLast()
}

loop {
    if done() {
        break
    }
}

for i in 0..3 { }             ## 0, 1, 2
for i in 1..=3 { }            ## 1, 2, 3
for name in names { }         ## array elements
for key, value in table { }   ## map entries, in insertion order
```

`break` leaves the innermost loop and `continue` starts its next iteration.
The loop variable is a new binding; an outer variable of the same name is
unchanged after the loop.

A `loop` with no `break` that exits it never finishes, and has the type
Never, which fits any expected type. So a server loop can be the last
expression of a function returning `Trial<…>` without a dummy value after it.
Breaks of nested loops do not count for the outer one.

### return

`return expr` (or `return` in a `Void` function) leaves the function early.
The value converts to the return type as a block's last value does: a union
member into the union, a literal into a literal or refined type, an integer
into `Float64`, `[]` and `Json` literals typed by it. `return` has the type
Never, so an `if` or `match` arm that returns fits beside
an arm of any other type:

```thx
match found {
    Not {
        return Fail("missing")
    }
    Has(_) { Void }
}
```

## Pattern matching

`match value { arms }` compares a value against patterns in order and
evaluates the first arm that matches. Arms are not separated by commas. An arm
body is a block, `Pattern { … }`, or a single expression, `Pattern => expr`.

**Union members.** `Member(a, b)` matches a member and binds its fields in
declaration order; `_` discards a field. A bare `Member` matches the member
without binding its fields. Patterns do not nest: match the outer value, then
match the bound inner value.

```thx
match result {
    Pass(value) => info("got {value}")
    Fail(error) => warn(error)
}
```

**Literal values.** `String`, integer, and `Bool` literal arms, with or-patterns
(`0 | 1 => …`). The final arm must catch everything else: `else`, `_`, or a bare
name, which binds the value.

```thx
fn describe(count: Int64) -> String {
    match count {
        0 => "none"
        1 | 2 => "a few"
        -1 => "unknown"
        n => "{n}"
    }
}
```

**Guards.** `Pattern if condition` adds a condition; the pattern's bindings are
visible in it. A guarded arm does not count toward covering its member.

**Exhaustiveness.** A match on a union must cover every member or end with
`else`; otherwise it is a compile error that names the missing members.

**Value.** A match's value is its arms' value. Where a type is expected (a
return, an annotated `let`, an argument), each arm converts to it as a
block's last value does, so arms may be different members of an expected
union, or values that encode into an expected `Json`. Without an expected
type, the arms must have the same type.

**`Node` values** match with `Element(tag, attrs, children)`, `Text(s)`, and
`Fragment(children)`.

## Functions

```thx
fn add(a: Int64, b: Int64 = 0) -> Int64 {
    a + b
}
```

The body is a block; its value is the return value. Parameters are described
in [Calls and arguments](#calls-and-arguments). Functions are declared at the
top level of a module (or as methods); there are no nested functions.

### Return types

An explicit `-> T` is the contract: the body must produce a `T`. A free
function or method without `->` infers its return type from its exits:

- a `return value` or the block's final value gives that type (`42` gives
  `Int64`; no value gives `Void`);
- `x?` followed by `Pass(value)` (or `Has(value)` after a `May`) gives
  `Trial<T, E>` (or `May<T>`) with the payload's type. A function that uses
  `?` and has no value to return ends with `Pass(Void)`, giving
  `Trial<Void, E>`; a `Void` final expression there is a type error;
- a bare `Pass` / `Fail` alone cannot name the union: write `->`;
- a recursive or mutually recursive function must be annotated; the error
  names each function on the cycle.

An unannotated `main` returns `Int64` and `drop` always returns `Void`.
Unannotated multi-clause methods and methods of generic types return `Void`.

### Multi-clause functions

Several functions with the same name in one scope (the module, or one type's
methods) are one function: an ordered match on the types of the parameters.

```thx
fn page(path: "home") -> String {
    "home page"
}
fn page(path: "admin") -> String {
    "admin page"
}
```

- All clauses have the same number of parameters and no defaults.
- Clauses are tried in source order; the first match wins.
- The function's domain is the union of the clause types; a call must pass a
  value in that domain.
- Duplicate domains, unreachable clauses, and `export` on only some clauses
  are compile errors.
- Return types join like match arms.

Multi-clause and `extern` functions cannot be used as values.

### Function values

A function type is written like a header: `(x: Int64) -> Int64`. A value of
that type is one of:

- a free function, by name: `apply(double, 21)`;
- a bound method, `instance.method`: calling it calls the method on that
  instance;
- an unbound method, `Type.method`: the receiver becomes the first argument.

```thx
fn double(x: Int64) -> Int64 { x * 2 }

fn apply(f: (x: Int64) -> Int64, v: Int64) -> Int64 {
    f(v)
}

let n = apply(double, 21)          ## 42
```

Function values may be arguments, return values, locals, and module `let` /
`mut`. They may not be fields or array elements, and have no `==`. There are
no closures: a function value captures nothing except a bound method's
receiver.

## User types and methods

### Methods

Methods are declared in a product type's body. A method sees the type's fields
by their bare names; there is no `self`. A method marked `export` can be
called from outside the type; one without `export` is private, and calling it
from outside is a compile error. A method cannot call another method of the
same type by bare name; put shared code in a module-level function.

### Field write-back

Inside a method, constructor-parameter fields (and the `mut` fields of a
C-layout type) are assignable. An assignment updates the receiver:

```thx
type Counter(n: Int64) {
    export fn add(by: Int64) -> Int64 {
        n = n + by
        n
    }
}

mut counter = Counter(n: 10)
let now = counter.add(by: 5)    ## 15
let stored = counter.n          ## 15
```

The same holds for in-place collection writes on fields (`items.append(x)`,
`items[i] = x`, `tags[k] = v`, `.remove`) and for calls to other types'
mutating methods on a field.

### Mutating methods

A method that writes a field needs a writable receiver: a `mut` binding, or
`items[i]` on a `mut` array (the element is stored back). Calling it on a
`let`, a parameter, a `for` or match binding, or a temporary is a compile
error. From outside its type a field is read-only, so `pair.left.add(by: 1)`
is an error; inside `Pair`'s own methods, `left.add(by: 1)` writes back. Types
with `drop` are exempt (they are shared boxes).

### Resources

A product may define `export fn drop()` with no parameters and no return
value. Values of such a type are heap boxes shared by every copy; when the
last reference goes away, `drop` runs and the box is freed. `File`, `Socket`,
and `Db` in the standard library close themselves this way; an explicit
`close` before that is allowed, and the later `drop` sees it closed.

Only the module that defines a type with `drop` may call its constructor:
`Db(handle: 1)` elsewhere is a compile error, so no code can wrap a raw handle
in a second owner. Copies in a `Map`, an `Array`, or module state share the one
box, so a `close` through any copy is seen by all.

## Collections

`Array`, `Map`, `Buffer`, and `String` have value semantics with copy on
write. `let ys = xs` shares storage until one of them is written; a write
through a `mut` place copies first if anyone else still holds the storage. So a
`let` alias never sees a later write, and a function that receives an array
cannot change the caller's array.

```thx
mut xs = [1, 2, 3]
let snapshot = xs
xs.append(4)
xs[1] = 99
## xs is [1, 99, 3, 4]; snapshot is still [1, 2, 3]
```

A write must reach a `mut` binding:

- array and map writes (`.append`, `a[i] =`, `.set`, `.remove`, …) need a
  `mut` place;
- an element of a `mut` array is a place: `grid[0].append(5)` changes the
  inner array;
- a field is not a place from outside its type (`board.names.append(x)` is an
  error, as `board.names = …` is), and neither is a temporary.

`Bytes` is the exception: it is shared, not copied (see [Bytes](#bytes)).
Appending to an unshared array is amortized O(1).

## Errors and panics

### May and Trial

Failure is a value. `std/fallible` defines two unions:

```thx
type Has<T>(value: T)
type Not
type May<T> = Has<T!> | Not?

type Pass<T>(value: T)
type Fail<E>(error: E)
type Trial<P, F> = Pass<P!> | Fail<F?>
```

`May<T>` is a value that may be absent; `Trial<P, F>` is a success `P` or a
failure `F`. They are not built in: import them.

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"

fn half(x: Int64) -> Trial<Int64, String> {
    if x % 2 == 0 { Pass(x / 2) } else { Fail("{x} is odd") }
}
```

Matching on a `May` or `Trial` returned from another module does not by itself
need the import; constructing one, annotating with the type, or calling a
built-in that produces one (`Bytes.find`, `map[key]`) does.

### Propagating failure

`expr?` on a union with marks unwraps the `!` member and otherwise returns the
`?` member from the enclosing function:

```thx
fn quarter(x: Int64) -> Trial<Int64, String> {
    let h = half(x)?        ## returns Fail early if half failed
    half(h)
}
```

The enclosing function's return type must hold the residual. A `Fail` is
rewrapped across `Trial` types with different success types (including across
modules). User unions with marks work the same way.

### Panics

A panic is an unrecoverable error. The program prints `thoxa panic: <message>`
to standard error and exits with status 134. A panic inside a task started
with `-->` ends only that task (see [Concurrency](#concurrency)). The panics a
program can hit:

| Message | Cause |
|---------|-------|
| `division by zero` | `/` or `%` by zero |
| `negative shift count` | `<<` or `>>` by a negative count |
| `N does not fit in T` | an implicit narrowing conversion out of range |
| `array index out of bounds` | `a[i]` outside the array |
| `fixed array index out of bounds` | `a[i]` outside an `Array<T, N>` |
| `Array.dropLast on empty array` | `dropLast()` on `[]` |
| `Array.insertAt out of range`, `Array.removeAt out of range` | index outside the array |
| `Bytes index out of range`, `Bytes.slice out of range`, `Bytes.insertAt out of range`, `Bytes.removeAt out of range` | index outside the `Bytes` |
| `Bytes length must be non-negative`, `Buffer capacity must be non-negative` | `Bytes(-1)`, `Buffer(-1)` |
| `main returned N; an exit status is 0 to 255` | `main` returned a value outside `0..255` |
| `stack overflow in task` | a task overflowed its 512 KiB stack |
| `out of memory`, `allocation size overflow` | allocation failed |

Other `thoxa panic:` messages indicate a runtime bug; please report them.

## Memory

Heap values (`String`, `Bytes`, `Buffer`, `Array`, `Map`, `Node`, user types
with heap fields, union values with heap payloads) are reference counted. The
count is not atomic; a program is single-threaded. Scalars and products of
scalars are stored inline: a `Point(x: Int64, y: Int64)` is two integers, not
two heap boxes.

- A copy increments the count; a value is released when the binding holding it
  is overwritten or goes out of scope at the end of the function, or when a
  `return` or `?` leaves the function early.
- Heap arguments are passed as copies the callee owns.
- Module bindings live until the process exits.
- A type with `drop` runs it when its last reference is released.

A value cannot contain itself (recursive fields are a compile error), so
reference cycles cannot be built from user types. Memory held by a task that
ends in a panic is not released. `cargo make test` runs the conformance suite
under LeakSanitizer.

## Modules

A file is a module: a scope of declarations. `export` makes a top-level
declaration visible to importers.

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"
import logging, { info, warn } from "std/logging"
import { readFile } from "std/io"
import net, { Socket } from "std/net"

export fn helper() -> Int64 { 1 }
```

| Form | Meaning |
|------|---------|
| `import { a, b as c } from "…"` | Binds the named exports (optionally renamed) |
| `import ns from "…"` | Binds a namespace: members are `ns.x`. A namespace is not a value |
| `import ns, { a, b } from "…"` | Both in one clause |

A module specifier names a file without its extension:

| Prefix | Resolves to |
|--------|-------------|
| `./…` | relative to the importing file |
| `~/…` | relative to the package root (the nearest ancestor directory with `main.thx`; otherwise the entry file's directory) |
| `std/…` | the standard library (see [Standard library](std.md)) |

Module file names are `kebab-case` (`math-utils.thx`, imported as
`"./math-utils"`); types inside stay `PascalCase`; a directory's entry file is
`main.thx`. An import that is never used is a warning.

## Concurrency

`--> name(args)` starts a call as a task and does not wait for it. The
expression has type `Void`; the callee's return value is discarded. There is
no future or task handle.

```thx
--> tick("a")
--> serveOne(peer, handle)
```

- The callee is one name in scope: a free Thoxa function (imported names
  work; defaults apply). Not a function value, a method, an `extern`, or a
  block.
- Code does not write `await`. Socket operations in `std/net` park the current
  task when they would block; other tasks run meanwhile. A callee that never
  parks runs to completion before `-->` returns.
- Everything runs on one OS thread. There are no threads and no `fork`.
- The program waits until every task has finished before it exits.
- A panic in a task ends that task only: it prints `thoxa panic: …` and
  `thoxa: task ended by panic`, and `main` and the other tasks keep running.
  The task's memory leaks. Sockets the task owns (`net.ownByTask`) are closed
  and read as closed (`fd` is -1) through every copy. A panic in `main` ends
  the program.

`std/http/server.serve` starts one task per connection this way.

## Typed data

A type is its own schema. The compiler attaches readers and encoders to every
type it can represent; there is no derive step and no schema library.

```thx
import { Issue, messages } from "std/json"
import { Pass, Fail, Trial } from "std/fallible"

type Qty = Int64 where >= 1
type DrinkSlug = "espresso" | "mocha"
type CreateOrder(slug: DrinkSlug, qty: Qty)

match CreateOrder.read(body) {
    Fail(issues) => info(messages(issues))     ## every bad field
    Pass(order) => info(order.encode())
}
```

| Function | Result |
|----------|--------|
| `x as T` | Converts a base value, a string, or a `Json` value into `T` (see [Conversions](#conversions)): `Trial<T, Array<Issue>>`; `json as Array<T>` reads a top-level array |
| `T.read(text)` | Parses JSON text into `T`: `Trial<T, Array<Issue>>` |
| `T.readForm(text)` | Reads `application/x-www-form-urlencoded` text into a product: `Trial<T, Array<Issue>>` |
| `t.encode()` | Compact JSON text |
| `t.toJson()` | A `Json` tree |

These need `std/json` somewhere in the program (`readForm` also needs
`std/http`). `Issue` is `Missing | Mismatch | Rejected` from `std/json`;
`message(issue)` formats one, `messages(issues)` a read's list, and
`errorsObject(issues)` builds a JSON error reply.

### Reading

A read checks every field and reports every problem, not only the first. Each
missing or mismatched field adds an issue; a nested product or an array element
adds the issues inside it, with a path such as `orders.1.slug`. A refined field
adds one issue, for its first failing predicate. Bad JSON syntax is one issue
at path `""`; a non-object where an object is expected is one `Mismatch`.

- A field of type `May<T>` that is missing or JSON `null` reads as `Not`.
  Write `May<Qty>`, not `May<Int64 where …>`.
- A missing key whose field has a constructor default takes the default.
- A union of products is read by its `"type"` key: a missing key is a
  `Missing` issue at `path.type`; an unknown one is a `Mismatch` listing the
  members.
- `Map<String, T>` fields read JSON objects; `Json` fields accept any value.

### Forms

`T.readForm(text)` takes a request body (`request.body.toString()`) or a query
string (`request.query`). Each field reads the form key of the same name; an
issue's path is that key.

| Field type | Value | Absent or empty |
|------------|-------|-----------------|
| `String` | the text | absent: `Missing`; empty: `""` |
| `Int64` / `Float64` | parsed as a JSON number | `Missing`; unparsable: `Mismatch` |
| `Bool` | present: `true` (a checked box) | `false` |
| refined / literal union | the base value, then the type's check | as the base type |
| `May<T>` | `Has(value)` | `Not` |
| `Array<T>` | one element per repeated key, in order | `[]` |
| nested product | its fields as `field.sub` keys | each field by its own rule |

Empty means absent for every type except `String`. A repeated key for a
non-array field takes the last value. Keys the type does not name (a CSRF
token) are ignored. Other field types (`Map`, `Json`, unions) are a compile
error at the `readForm` call that names the field. An absent `Bool` or
`Array` and a nested product take no constructor default.

### Encoding

`encode` / `toJson` work for every type JSON can hold:

| Type | JSON |
|------|------|
| `String` | string |
| integer types | number (a `UInt64` above the `Int64` maximum panics) |
| `Float64` | number |
| `Bool` | `true` / `false` |
| `Array<T>` | array |
| `Map<String, T>` | object, in insertion order |
| `May<T>` | the value, or `null` |
| `Json` | itself |
| refined type | its base value |
| literal union | the literal |
| product | an object of its fields in declaration order; a `Not` field is left out |
| union of products | the member's object with `"type": "Member"` first |

A type that defines its own exported `fn toJson() -> Json` is encoded by it,
including inside other types, and gets no generated reader (a
[`from(json: Json)` clause](#from-clauses) gives it one). `Bytes`, `Buffer`,
`Node`, functions, `Address`, types with `drop`, C-layout types, maps with
non-`String` keys, unions that mix literals and products, generic types, and
`Trial` have no encoding; using one is a compile error that names the type
(and the field, when there is one). Types in `std/` get no generated codec.

### Json literals and implicit encoding

Where a `Json` is expected — a parameter, a constructor field, a return, an
annotated `let`, an assignment, an element of `Array<Json>`, a branch of an
`if` or a `match` — a value of any type with a JSON encoding converts as
`x as Json` would. A map literal there is a JSON object and an array literal a
JSON array; `{}` is an empty object. `...value` inside an object literal
spreads a product's fields or a `Map<String, T>`'s entries; a later key
replaces an earlier one.

```thx
fn ticketJson(ticket: Ticket) -> Json {
    { ...ticket, ok: true }
}

jsonOk(rows)                                  ## Array<OrderRow> → Json
let event = Event(name: "order", data: line)  ## a Json field takes a Line
```

Only encoding is implicit. A `Json` where another type is expected is a
compile error that says to write `json as T`, because reading can fail. A
multi-clause function with clauses for both `T` and `Json` still picks the
clause by the argument's exact type.

Not available: field renaming, codecs for generic types, and encoding to a
form.

## Tag literals and Node

Tag literals are expressions that build a `Node`, an HTML tree. `n.html()` is
its escaped markup (no doctype, no indentation).

```thx
type Card(title: String) {
    export fn view(children: Array<Node>) -> Node {
        <section class="card">
            <h1>{title}</h1>
            {children}
        </section>
    }
}

let page = <Card title="Hi">
    <p>Hello, {name}</p>
    <input disabled />
</Card>
info(page.html())
```

| Piece | Behaviour |
|-------|-----------|
| HTML element | `<div class="x">…</div>`; hyphenated names (`data-*`) are allowed |
| Component | `<Card title="Hi">…</Card>` calls `Card(title: "Hi").view(children: […])` at once. A component is a type that produces a `Node`, not a subtype of it |
| Fragment | `<>…</>`; an empty `<></>` is a valid `Node` |
| `{e}` child | `String` (escaped text), integers, `Float64`, `Bool` (formatted as in interpolation), `Node`, `Array<Node>` (spliced) |
| `{for x in xs { child }}` | collects an `Array<Node>`; only in child position |
| Attributes | `"…"` or `{e}`, with the same value types as children. A bare `disabled` renders as `disabled` |
| Spreads | `{...attrs}` on an element takes an `Array<Attr>` (later attributes append); on a component it copies a product's fields |
| Text | Whitespace-only gaps between tags are dropped. `#` starts a comment; `\#` is a literal `#` |
| Void elements | `br` `hr` `img` `input` `meta` `link` render with no closing tag |
| Matching | `Element(tag, attrs, children)` / `Text(s)` / `Fragment(children)` |
| Constructors | `Element`, `Text`, `Fragment`, and `Attr` are built in |

`<` starts a tag only when a name or `>` follows it directly; see
[Line breaks](#line-breaks). `http.htmlResponse(root)` sends a page with a
doctype; see [Http](std.md#stdhttp).

## Route clauses

HTTP methods in a type body are routes. The type must have no constructor
parameters or fields. The compiler generates
`route(request: Request) -> Response`.

```thx
import { Request, Response, ok } from "std/http"

type Routes {
    GET "/home" {
        "Home"
    }

    GET "/user/:userId" (userId: Int64) -> String {
        "user {userId}"
    }

    POST "/echo" (request: Request) -> Response {
        ok(request.body, "text/plain")
    }
}

let response = Routes().route(request)
```

| Piece | Behaviour |
|-------|-----------|
| Verbs | `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS` |
| Body returns `String` (or no `->`) | wrapped as `ok(…, "text/plain")` |
| Body returns `Response` | passed through |
| No route matches | `notFound()`; there is no fallthrough |
| Captures | `:name` in any segment, bound to a parameter of type `String` or `Int64`; non-digits for an `Int64` give `badRequest("invalid name")` |
| `Request` parameter | optional, any name; not a capture |
| Body parameter | any other parameter, at most one: reads the body (or a GET's query) into its type, below |
| Imports | only what the clauses themselves write; the generated `route` names `std/http`'s `Request`, `Response`, `ok`, `notFound`, and `badRequest` itself |

`route` matches on `request.method` and `request.path`.

**Body parameters.** A parameter that is neither a capture nor the `Request`
reads the request into its type, which must have a reader (see
[Typed data](#typed-data)):

| Request | Read as |
|---------|---------|
| `GET` / `HEAD` | the query string, as a form (`T.readForm(request.query)`) |
| `Content-Type: application/json` | JSON (`T.read(body)`) |
| `Content-Type: application/x-www-form-urlencoded` | a form (`T.readForm(body)`) |
| any other or no `Content-Type` | `415 Unsupported Media Type` |

When the parameter's type is `T` and the read fails, the clause does not run:
a JSON request gets `400` with `errorsObject(issues)`, a form or query `400`
with `messages(issues)` as text. When the type is `Trial<T, Array<Issue>>`,
the clause always runs and handles the failure itself (a form page, a
redirect). A type that a form cannot hold (a `Map` field, a union) answers a
form or query with one issue, `T is read from JSON only`. Body parameters need
`std/json` in the program.

```thx
type Routes {
    POST "/api/orders" (order: CreateOrder) -> Response {
        place(order)                          ## a bad body already got a 400
    }

    POST "/orders" (order: Trial<CreateOrder, Array<Issue>>, request: Request) -> Response {
        match order {
            Pass(found) => placed(found, request)
            Fail(issues) => formPage(request, issues)
        }
    }

    GET "/search" (search: Search) -> String {   ## /search?term=mocha
        results(search.term)
    }
}
```

## C interop

### extern

`extern fn` declares a C function. Every parameter is positional (`!`).
`from "c"` links against the C library and the runtime; a second string names
the C symbol when it differs from the Thoxa name. Without `->` the function
returns `Void`.

```thx
extern fn strlen(text!: String) -> UInt64 from "c"
extern fn sysWrite(fd!: Int32, data!: Bytes, count!: UInt64) -> Int64 from "c" "write"
```

| Thoxa | C |
|-------|---|
| `Int32`, `Int64`, `UInt32`, `UInt64`, `Byte` | `int32_t`, `int64_t`, `uint32_t`, `uint64_t`, `uint8_t` |
| `Float64` | `double` |
| `String` | a NUL-terminated `char *`; a returned `char *` is copied into a new `String` |
| `Bytes` | a pointer to the bytes |
| `Address<T>` | `T *` |
| `Void` | `void` |

Defaults on `extern` parameters are expanded at the call site before the C
call. `extern` functions cannot be used as values or started with `-->`.

The compiler writes each extern's C prototype from its Thoxa signature, under
a private name bound to the C symbol, so any function in the C library, the
runtime, or another linked library can be called whether or not a system
header declares it, and a header's own declaration never conflicts with it.

A variadic C function (`printf`, `open`, `ioctl`) is called through a fixed
prototype made from the extern's parameters. Integer, pointer, `String`, and
`Bytes` arguments arrive correctly. A `Float64` in the variadic part is not
guaranteed to: the x86-64 calling convention passes a variadic callee a count
of vector registers, which a fixed prototype does not set.

### C-layout types

A type without constructor parameters whose body declares `mut name: Type`
fields is laid out as a C struct, in declaration order. `T()` makes a
zero-filled value. Fields may be integer types, `Float64`, and fixed arrays
`Array<T, N>`. Methods may assign these fields.

```thx
type Regs {
    mut a: UInt32
    mut b: UInt32
    mut tag: Byte
    mut code: Array<Byte, 4>
}

extern fn memset(target!: Address<Regs>, value!: Int32, count!: UInt64) from "c"

mut regs = Regs()
regs.a = 0x01020304
regs.code[0] = 65
memset(regs, 0, 16)
```

A `mut` local of a C-layout type passed where `Address<T>` is expected passes
its address, to C or to a Thoxa function. The C function receives a pointer to
the struct, so the Thoxa type's fields must match the C struct's layout:

```thx
## struct utsname on Linux: six 65-byte fields.
type SystemName {
    mut sysname: Array<Byte, 65>
    mut nodename: Array<Byte, 65>
    mut release: Array<Byte, 65>
    mut version: Array<Byte, 65>
    mut machine: Array<Byte, 65>
    mut domainname: Array<Byte, 65>
}

extern fn uname(name!: Address<SystemName>) -> Int32 from "c"

mut name = SystemName()
uname(name)                     ## name.sysname now holds "Linux" and a NUL
``` An `Address<T>` parameter passes on
unchanged. A value of a type with `drop` passes its box pointer. C-layout types
have no `==`, cannot be map keys, and have no JSON encoding.

## Not in the language

So that nobody looks for them: closures, generic functions, traits or
interfaces, exceptions, threads, macros, operator overloading, tuples, nested
patterns, compound assignment, algebraic effects,
a package manager, a named `ffi` import root, and shell control flow (`if` / `for` in command syntax).
