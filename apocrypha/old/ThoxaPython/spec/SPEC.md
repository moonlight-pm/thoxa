# Thoxa Language Specification

> **Status: UNFROZEN (pre-alpha).** This spec is the single source of truth for the surface
> syntax and semantics. It is *executable*: every feature marked ✅ has a passing test under
> `spec/conformance/`. Features marked 🚧 are specified but not yet implemented. A feature is
> never described as working until its conformance test passes.

Milestone tags: **[M1]** core · **[M2]** unions/match · **[M3]** parameters · **[M4]** types.

---

## 1. Lexical structure

### 1.1 Encoding & whitespace
Source is UTF-8. Whitespace (spaces, tabs, newlines) separates tokens but is **not otherwise
significant**: there are no required statement separators — neither newlines nor semicolons.
Expression and statement boundaries are determined entirely by the grammar.

### 1.2 Comments
```thoxa
# line comment to end of line
let x = 42   # end-of-line comment

## doc comment (feeds the notebook / reference)
## may span multiple ## lines immediately above a declaration
fn add(a: Int64, b: Int64) -> Int64 { a + b }
```
There are no block comments.

### 1.3 Identifiers — enforced casing
Identifiers are letters and digits only, starting with a letter. **Underscores are illegal.**
Casing is enforced by the lexer:

- **Value / function** names must be `camelCase`: start lowercase (`myValue`, `calculateTotal`).
- **Type** names must be `PascalCase`: start uppercase (`Point`, `HttpServer`).

Violations are a compile error, e.g. `my_value` → *"identifiers must be camelCase"*,
`httpServer` used as a type → *"types must be PascalCase"*. Whether a bare identifier is a
value or a type is determined by its first letter's case.

### 1.4 Keywords
```
let mut fn type export import from
if else loop while for in break continue return match
and or not as can is
true false
```
`Void` is a type name, not a keyword. `Has`, `Not`, `Pass`, `Fail` are **ordinary constructor
types** (defined in the prelude), not keywords.

### 1.5 Literals
```thoxa
# Integers default to Int64; optional sized suffixes; 0x / 0o / 0b bases; _ separators
42        1_000_000     0xFF      0o644     0b1010     42i32     255u8

# Floats default to Float64; optional f32 / f64 suffix; exponent
3.14      1.5e10        2.0f32    3.14f64

# Char (single Unicode scalar), String (interpolated), Bool
'A'       "hello"       true      false

# Byte literal: a Byte (0..255) written in character form — an integer literal, so
# it is a numeric Byte everywhere (arithmetic, bitwise, comparison, match patterns).
b'q'      b'\n'         b'\x1b'   b'\xff'
```
Escape sequences in `'…'`/`"…"` and `b'…'`: `\n \t \r \\ \' \" \0`, plus `\xNN`
(exactly two hex digits). In a `Char`/`String`, `\xNN` denotes the character with
codepoint `0xNN` and is **restricted to `0x00`–`0x7f`** (ASCII) — a `Char` is a
Unicode scalar and a `String` is UTF-8, so a lone byte `≥ 0x80` is ambiguous. A byte
literal `b'…'` accepts the full `0x00`–`0xFF` `\xNN` range, and its *bare* (unescaped)
character must be ASCII (`0x00`–`0x7f`). A `Char` is never a byte/number; use `Byte`
(and `b'…'`) for byte/ASCII work — e.g. across the C FFI.

### 1.6 Operators & punctuation
```
+  -  *  /  %                 arithmetic
== != <  >  <= >=             comparison
and or not                    logical (keywords)
&  |  ^  ~  << >>             bitwise: and, or, xor, not (unary), shift left/right
=                             assignment (mut bindings only)
+= -= *= /=                   compound assignment (mut bindings only)   [M1]
?                             postfix error-propagation                 [M2]
as can is                     conversion / guard                        [M4]
|                             union-type separator (in `type` aliases); also bitwise-or in
                              expression position
->                            function return type
.  [ ]  ( )                   member access, index, call
..  ..=                       exclusive / inclusive range
<  >                          also generic argument brackets (disambiguated by the parser)
<< >>                         shift in expressions; a `>>` closing nested generics is re-split
```
A bare `!` is only valid as a positional-parameter marker in a parameter declaration
(`name!: T`) and as part of `!=`.

---

## 2. Types

### 2.1 Primitives
```
Int8 Int16 Int32 Int64        signed
UInt8 UInt16 UInt32 UInt64     unsigned
Float32 Float64               floating point  (Byte = UInt8)
Bytes                         mutable byte buffer (FFI boundary)
Bool  Char  String  Void
```
There is no unsized `Int`/`Float`: every numeric width is explicit, and literals default to
`Int64` / `Float64`. `Bytes` is a mutable byte buffer used across the C FFI (see the FFI design
doc). `Void` is the unit type; its only value is written `Void`.

### 2.2 Types are first-class values (hybrid)
A type is a value of type `Type`. Types may be bound and passed:
```thoxa
let numberKind = Int64          # numberKind : Type
```
Primitives are provided by the compiler (there is no in-language `@intType`). **[M4]**

### 2.3 Composite & generic types
```thoxa
Array<T>                       # dynamic array
[1, 2, 3]                      # Array<Int64> literal
let empty: Array<String> = []

May<T>       = Has<T> | Not              # optional (prelude)
Trial<T, E>  = Pass<T> | Fail<E>         # result  (prelude)

Fn(Int64, Int64) -> Int64            # function type                              [M3]
```
Generic arguments use angle brackets. `Array<T>` is the only compiler-provided generic;
everything else is user- or prelude-defined.

---

## 3. Bindings
```thoxa
let x = 42                     # immutable
let name: String = "Ada"       # explicit type
mut count = 0                  # mutable
count = count + 1              # reassignment (mut only)
count += 1                     # compound assignment (mut only)
```
`let` bindings are immutable; reassigning one is a compile error. `mut` introduces a mutable
binding. Type annotations are optional and inferred when omitted (**[M4]**; until then, the
interpreter is dynamically typed and annotations are recorded but not enforced).

---

## 4. Functions
```thoxa
fn add(x: Int64, y: Int64) -> Int64 {
    x + y                      # implicit return: last expression
}

fn greet(name: String) -> Void {
    println("Hello, {name}")
    Void
}

fn firstPositive(xs: Array<Int64>) -> Int64 {
    for x in xs {
        if x > 0 { return x }  # early return
    }
    0
}
```
- The last expression of the body is the return value; `return expr` returns early.
- The return type annotation is optional; when omitted the body's type is used (`Void` if none).

### 4.1 Parameters — named by default **[M3]**
```thoxa
fn connect(host: String, port: Int64 = 8080, tls: Bool = false) -> Conn
fn print(message!: String) -> Void        # ! => positional
fn format(template!: String, width: Int64 = 80) -> String
fn sum(first: Int64, ...rest: Int64) -> Int64    # variadic (last)
```
Call-site rules:
- **Named parameters are labelled and passed in declaration order** (order is enforced).
- Parameters with **defaults may be omitted**.
- **Positional** parameters (`!`) take no label, cannot have a default, and come first.
- **A bare name puns.** An unlabelled identifier that matches a keyword parameter is
  shorthand for `name: name` — `connect(host)` means `connect(host: host)`. Only bare
  identifiers pun (never a literal or compound expression), and only against *keyword*
  parameters: a bare name matching a positional (`!`) parameter stays positional, and one
  matching a keyword parameter is *always* the pun, so it can't fill a positional slot.
```thoxa
connect(host: "localhost", port: 3000)     # ok
connect(port: 3000, host: "localhost")     # ✗ error: named args out of order
format("hi", width: 40)                     # positional first, then named
print("hi")                                 # positional
let host = "localhost"
connect(host, port: 3000)                   # pun: `host` ≡ `host: host`
```

---

## 5. Aggregates — `type`
```thoxa
type Point(x: Float64, y: Float64) {
    export fn lengthSquared() -> Float64 {
        x * x + y * y                       # implicit field access, no `self`
    }
    export fn movedBy(dx: Float64, dy: Float64) -> Point {
        Point(x: x + dx, y: y + dy)
    }
}

type Counter(start: Int64) {
    mut value = start                       # private, mutable field
    let limit = 100                          # private, immutable field
    export fn increment() -> Void {
        if value < limit { value = value + 1 }
        Void
    }
    export fn get() -> Int64 { value }
}
```
- Constructor parameters are in scope throughout the body and initialise fields.
- Members declared with `let`/`mut` are fields; `fn` are methods.
- Members are **private by default**; prefix with `export` to expose them.
- A constructor parameter may itself be marked `export` to publish that field under the
  same name — no body re-export needed:
  `export type Header(export name: String, export value: String) {}` then `h.name`.
  Body `export let` / `export fn` still work for renames, derived fields, and methods.
  `export` on a function or `extern` parameter is a parse error.
- Prefer **keyword** constructor parameters (`name: T`) so call sites stay self-documenting;
  reserve positional (`name!: T`) for small internals and prelude payloads.
- Methods access fields and constructor params by bare name; there is **no `self`**.
- Construction uses the parameter convention: `Point(x: 1.0, y: 2.0)`.
- A constructor whose field initializer (or parameter default) uses `?` is **fallible**:
  construction returns `Trial<Self, E>` — `Pass(instance)` on success, or the short-circuited
  `Fail` — and is threaded with `?` at the call site (`Server(port: 9091)?`). See §10.
- Field access is `p.x`; method call is `p.lengthSquared()` (only `export`ed members are visible
  from outside the type).

---

## 6. Sum types & the prelude **[M2]**
Sum types are built from separate constructor types unified by a union alias:
```thoxa
type Circle(radius: Float64) {}
type Rect(w: Float64, h: Float64) {}
type Shape = Circle | Rect

# prelude
type Has<T>(value: T) {}
type Not {}
type May<T> = Has<T> | Not

type Pass<T>(value: T) {}
type Fail<E>(error: E) {}
type Trial<T, E> = Pass<T> | Fail<E>
```
Constructors are ordinary types usable on their own: `Has(3)`, `Not()`, `Pass(x)`.

These live in the auto-imported **`std/Prelude`** module. The **closed prelude** — the only
names ambient without an `import` — is exactly: the algebraic types above
(`May`/`Trial`/`Has`/`Not`/`Pass`/`Fail`), `Array<T>` and its methods, and `print`/`println`.
Everything else (e.g. `sqrt` from `std/Math`) must be imported (§12).

---

## 7. Expressions

### 7.1 Blocks
A `{ … }` block evaluates its statements and yields its **last expression** (or `Void` if the
last item is a binding/statement):
```thoxa
let total = {
    let a = 10
    let b = 20
    a + b                       # block value = 30
}
```

### 7.2 `if` / `else`
```thoxa
let sign = if x > 0 { "pos" } else if x < 0 { "neg" } else { "zero" }
```
`if` is an expression; all branches must yield the same type. An `if` without `else` yields `Void`.

### 7.3 Loops
```thoxa
loop { if done { break }  continue }
while cond { work() }
for x in xs { use(x) }
for i in 0..10 { println("{i}") }      # 0..10 exclusive; 0..=10 inclusive
```
Loops yield `Void`. `break` and `continue` control the nearest enclosing loop.

### 7.4 `match` **[M2]**
```thoxa
let label = match value {
    0 { "zero" }
    1 { "one" }
    else { "many ({value})" }          # catch-all arm; required unless exhaustive
}

match shape {
    Circle(radius) { area(radius) }     # constructor destructuring
    Rect(w, h)     { w * h }
}                                       # exhaustive over `Shape` => no `else` needed
```
- Arms are `Pattern { body }` (block bodies, no arrow, no commas).
- The catch-all is a final **`else { body }`** arm; it is **required unless the match is provably
  exhaustive** over the scrutinee's union.
- Patterns: integer/string/bool/char literals, constructor destructuring `Ctor(binding, …)`,
  a bare `camelCase` **binding** (matches anything, binds the value), and `else`.
- `match` is an expression; all arm bodies must yield the same type.

### 7.5 Operators & precedence
Lowest → highest binding:
```
or
and
== !=
< <= > >=
.. ..=
|
^
&
<< >>
+ -
* / %
unary: not  -  ~          (prefix)
postfix: ?  as  can  is   f(args)  a[i]  a.b
```
`and` / `or` short-circuit. There is no operator overloading. Bitwise operators (`& | ^ ~ << >>`)
take integer operands only and bind tighter than comparison — `x & mask == 0` is
`(x & mask) == 0`.

---

## 8. Strings
```thoxa
let g = "Hello, {name}!"                 # interpolation
let s = "{x} + {y} = {x + y}"            # arbitrary expressions
let t = "use {{braces}} for literal"     # {{ and }} escape to { and }
let e = "\x1b[2J"                        # \xNN ASCII escape (0x00–0x7f); e.g. VT100
let m = "
    multi-line with
    automatic dedent
"
```
Strings are immutable UTF-8; the escapes of §1.5 apply, including `\xNN` for ASCII
(`0x00`–`0x7f`) — the form used to write terminal control sequences.
**Interpolation is the only concatenation mechanism** — there is no
string `+`. Interpolated expressions are converted to `String` via their `toString` (built in for
primitives; user types via `as String` once conversions land in **[M4]**).

---

## 9. Arrays
```thoxa
let xs = [1, 2, 3]
let first = xs[0]
xs[1] = 10                     # only if `xs` is mut
let n = xs.count               # length
for x in xs { use(x) }
```
Indexing out of bounds is a runtime error (a trap), never silent. Additional array methods
(`append`, `dropLast`, `insertAt(at, item)`, `removeAt(at)`, …) are provided by the
prelude/builtins. `insertAt`/`removeAt` are **functional** — they return a new array (a copy)
with the element inserted/removed, leaving the source unchanged; strict bounds
(`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove, else a trap).

### 9.1 `Bytes` and `Buffer`
`Bytes` is a fixed byte buffer (the FFI boundary): `Bytes.fromString(s)`, `.count`, `.get(i)`,
`.set(i, b)`, `.slice(start, end)` — a **fresh copy** of the half-open range `[start, end)`
with strict bounds (`0 ≤ start ≤ end ≤ count`, else a trap) — and `.insertAt(at, byte)` /
`.removeAt(at)`, each returning a **fresh copy** with a byte inserted/removed (strict bounds
`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove; `byte` masked to 0–255). `.find(needle)`
searches for the `Bytes` `needle` and returns `May<Int64>` — `Has(i)` at the **leftmost**
index where it occurs, else `Not` (`May`-typed, never a `-1` sentinel; empty needle →
`Has(0)`, needle longer than the buffer → `Not`). `.equals(other)` returns `Bool` — `true` iff `other` has the same length and the same
bytes (structural, length-counted, embedded-NUL safe; `Bytes ==` is identity, this is content
equality). A growable `Buffer`
builds one up (`.append`, `.appendBytes`, `.reserve`, `.freeze()`); see the FFI / growable-buffer
design docs.

---

## 10. Error handling **[M2]**
```thoxa
fn half(x: Int64) -> Trial<Int64, String> {
    if x % 2 == 0 { Pass(x / 2) } else { Fail("odd") }
}

fn quarter(x: Int64) {         # inferred Trial: a `?` in the body, no annotation
    let a = half(x)?           # unwrap Pass, or short-circuit the Fail
    half(a)                    # tail is already a Trial — passed through, not re-wrapped
}

fn nonEmpty(n: Int64) {        # inferred Trial from the `Fail` construction, no `?`
    if n == 0 { Fail() } else { n }   # bare `n` is auto-Pass'd; `Fail()` carries Void
}
```
Postfix `?` on a `Trial<T,E>` yields `T` or returns `Fail<E>` from the enclosing function; on a
`May<T>` it yields `T` or returns `Not`. The enclosing function's return type must be compatible.

**Inferred Trial threading.** A function or method is *fallible* — Trial- or May-returning *by
inference* — when its body uses `?` **or directly constructs a variant** (`Fail`, `Pass`, `Not`,
`Has`). It then need not annotate `-> Trial<…>`/`-> May<…>` nor wrap its success: the success tail
is auto-wrapped, a `?` short-circuits, and a tail that already yields a Trial/May — a `Pass`/`Fail`,
a `Not`/`Has`, or a call to another fallible function — is passed through, not double-wrapped.
`?` and any `Pass`/`Fail` select the **Trial** family (wrap the tail in `Pass`); a lone `Not`/`Has`
selects **May** (wrap in `Has`). An explicit non-Trial/May return type (e.g. `main -> Int64`) opts
out, keeping the raw value. A constructor is fallible only through `?` — a `?` in a field
initializer short-circuits construction, yielding `Pass(instance)` or the propagated failure.
`Fail`'s payload defaults to `Void`, so `Fail()` is a failure that carries no information.

---

## 11. Conversions **[M4]**
No implicit coercion. The explicit triad:
```thoxa
let s = n as String            # convert (must be a defined conversion)
if n can String { … }          # is a conversion n→String defined?
if v is Int64 { … }              # runtime type guard (narrows v in the branch)
```
Safe integer widening (same signedness, e.g. `Int8`→`Int64`) is permitted implicitly; narrowing
and signed↔unsigned are not.

---

## 12. Modules ✅
Each `.thx` file is a module. A module file **describes a type**, so its name is `PascalCase`
(`Http.thx`); importing it binds a singleton **instance**, so the binding is `camelCase`
(`http`). `export` publishes a top-level `let` / `fn` / `type` / `extern fn`; everything else is
private. A top-level `mut` is always private — module state is reached through exported functions.

```thoxa
import { half, quarter } from "./MathUtils"    # selective
import { sqrt as root }  from "std/Math"        # selective + alias
import http              from "./Http"          # namespace instance → http.ok200(…)
```

**Naming.** A file module's name — the specifier's last segment — is `PascalCase`; it names the
type the file describes. The namespace binding is a `camelCase` value name chosen freely by the
importer (`import client from "./Http"` is fine). The named roots `std` and `ffi` are lowercase
(roots, not modules); std's members are modules, hence `std/Math`, `std/Prelude`.

**Resolution.** The `from "…"` string is a logical specifier: `./…` is local (siblings and
children; a `..` segment is an error — reach up via the project root instead), `~/…` is the
project root, and a bare first segment is a **named root** (`std/…` the standard library, `ffi`
the built-in FFI module). The `.thx` extension is implicit.

**Namespaces.** `import m from "…"` binds a namespace whose members are reached with `m.name`; it
is **not a value** (valid only left of a `.`). The binding is any `camelCase` name; a `PascalCase`
binding (which would name a type) is an error. There is no glob import, and no namespace `as` —
the binding is already free; a *selective* `as` may not change casing kind.

**Entry.** A runnable project is a directory with a `Main.thx` that **exports** `fn main`. The
runtime calls that `main` from outside the module (so it must be exported), and the directory is
the project root for `~/…`. `thoxa run <dir>` resolves `<dir>/Main.thx`; `thoxa run <file>` runs a
single file directly, calling its `main` if present. An imported module is declarations-only; the
entry keeps its script body. `main` may optionally declare one `arguments: Array<String>`
parameter: `thoxa run <module> a b` calls it with `[<module>, "a", "b"]` (C-style — the program
path is `arguments[0]`, the trailing tokens follow); a zero-parameter `main()` is unchanged.

**Prelude.** The closed prelude (§6) is delivered as an auto-imported `std/Prelude`; every name in
scope is either the prelude or an explicit `import`.

**Loading.** Modules load in two phases — names are registered, then `let`/`mut` values initialize
in dependency order, each module exactly once. Mutually-recursive `fn`/`type` across files are
fine; a cycle among *value* initializers is an error.

See [`docs/specs/2026-07-12-thoxa-module-system-design.md`](../docs/specs/2026-07-12-thoxa-module-system-design.md)
and [`docs/specs/2026-07-13-thoxa-pascalcase-modules-and-main-entrypoint-design.md`](../docs/specs/2026-07-13-thoxa-pascalcase-modules-and-main-entrypoint-design.md)
for the full design and rationale.

---

## 13. Semantics notes

- **Immutability by default.** `let` values never change; `mut` is required for mutation.
- **Memory.** Automatic; invisible to user code (no refcount intrinsics ever appear in Thoxa
  source). The interpreter currently relies on the host's memory management as *scaffolding*; the
  language commits to Perceus-style precise reference counting before 1.0.
- **Evaluation.** Eager, left-to-right. `and`/`or` short-circuit.
- **Entry point.** A program's top-level statements run in order; if a `fn main() -> Int64` exists,
  the CLI uses its return value as the process exit code.

---

## 14. C FFI ✅

### Typed C structs and `Address<T>`

A `type` whose fields are all fixed-width scalars (`Int8`…`Int64`, `UInt8`…`UInt64`,
`Byte`, `Float32/64`) or fixed inline arrays `Array<T, N>`, with no constructor
params and no methods, is a **C-ABI struct**: it is laid out in declaration order
with natural C alignment, and its fields are all public.

```thoxa
type Termios {
    mut cIflag: UInt32
    mut cLflag: UInt32
    let cLine:   Byte
    mut cCc: Array<Byte, 32>   // fixed inline array of 32 bytes
    let cIspeed: UInt32
}
```

`Termios()` constructs a zero-initialized value. Read/write fields with `t.cLflag`
and `t.cLflag = v` (a write truncates `v` to the field's width, native byte order);
read/write a fixed-array element with `t.cCc[i]` / `t.cCc[i] = b`.

An `Address<T>` is a pointer to a C-ABI struct `T`, used in `extern fn`
signatures. Passing a struct value to an `Address<T>` parameter takes its address
implicitly, so C can read or write it in place:

```thoxa
extern fn tcgetattr(fd!: Int32, p!: Address<Termios>) -> Int32 from "c"
mut raw = Termios()
tcgetattr(0, raw)   // the kernel fills raw
```

Deferred: an explicit address-of operator, const/read-only pointers, struct
value-copy, pointer returns, and `sizeof`.
