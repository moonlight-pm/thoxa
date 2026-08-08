# Language (shipped subset)

Native Thoxa is a small, growing language. This page lists **what the compiler
implements with conformance**, not the full oracle SPEC.

## Programs

```thx
fn main() -> Int64 {
    println("hello")
    0
}
```

Directory packages may use `Main.thx` as the entry file.

## Values and types (highlights)

| Area | Shipped |
|------|---------|
| Scalars | `Int64`, `Int32`, `UInt32`, `UInt64`, `Byte`, `Bool`, `Void` |
| Heap | `String` (interpolation), `Bytes`, growable `Buffer`, dynamic `Array<T>` |
| User types | `type Name(fields) { methods }` — including heap fields; method field write-back |
| Generics | `type Box<T>(value: T) { methods }`; generic unions `type Opt<T> = Box<T!> \| Empty?` (monomorphized) |
| Unions | `type U = A \| B` (products, pure literals, nested flatten); marks `T!` / residual `?` |
| C-layout | Body fields, `T()` zero-init, `Address<T>`, fixed `Array<T,N>` fields/locals (element get/set **and** whole-array assign/bind) |
| Errors | Ambient prelude `May<T>` / `Trial<Ok,Err>` with `?` / `match` (ok/err may nest) |
| Equality | String `==` / `!=`; usual scalar compares |

**Fallibility (ambient — no import):** same surface as always, now ordinary
generic unions (`std/Fallible.thx` / compiler inject):

```thx
type Has<T>(value: T)
type Not
type May<T> = Has<T!> | Not?

type Pass<T>(value: T)
type Fail<E>(error: E)
type Trial<P, F> = Pass<P!> | Fail<F?>
```

`?` unwraps the success extract or early-returns the residual (rewraps across
compatible Trial monomorphs, including across modules). User unions with a
residual mark use the same `?` operator.

**Reference counting:** simple single-threaded RC (dup on copy, drop at function
end). Precise RC / Perceus is **not** shipped.

## Control flow

`if`, `while`, `loop`, `for` (ranges and collections), `break`, `continue`,
early `return` / `return expr`. Branch values may inject into an expected union
(`-> Outcome` / `let x: U = if …`).

## Match

- **Trial / May / user unions:** constructor arms (`Pass(x)` / `Fail(e)`,
  `Has(x)` / `Not`, custom tags), discard `_`.
- **Value match:** String / Int64 / Bool **literal** arms, **or-patterns**
  (`0 | 1 { … }`), optional **`if` guards** on those arms, and a final
  catch-all: `else` or a **bare bind** (`n { … }` binds the scrutinee; bare
  `_` discards).

## Functions

```thx
fn add(a: Int64, b: Int64 = 0) -> Int64 {
    a + b
}
```

Parameter defaults (M3 subset) on free functions, **methods**, **constructor
params**, and **`!` formals** (including `extern` — defaults expand at the
call site before the C call). Free names inside default expressions resolve
in the **defining** module (so importers need not re-export constants used
only as defaults). A non-default formal may not follow a defaulted one.

## Methods and field write-back

Inside a method, bare names for constructor-param fields (and C-layout `mut`
fields) are assignable. Assignment updates the **receiver** as well as the
local field binding:

```thx
type Counter(n: Int64) {
    export fn add(by: Int64) -> Int64 {
        n = n + by
        n
    }
}

mut c = Counter(n: 10)
println(c.add(by: 5))   ## 15
println(c.n)            ## 15
```

## Modules

```thx
import { readFile } from "./Io"
import * as Net from "~/Net"
export fn helper() -> Int64 { 1 }
```

| Origin | Meaning |
|--------|---------|
| `./…` | Relative to current file |
| `~/…` | Relative to **entry** file directory |
| `std/…` | Product standard library |

Module-level `let` / `mut` are supported. General in-function `import`/`type`
is **not** a language feature (session host may lift decls).

## Extern

```thx
extern fn puts(s!: String) -> Int32 from "c"
extern fn sysErrno() -> Int32 from "c" "thoxa_errno"
```

Marshals common scalars, `String`, `Bytes`, `Void`, `Address<T>`. String
*returns* are copied via the runtime.

## I/O and HTTP (library shape)

- Free `readFile` / `writeFile` / `appendFile` (`Trial<…, String>`).
- **OpenMode:** nullary union `Rdonly | Wronly | Rdwr` with `openModeBits(m)` for
  `openFile` flags; Int32 lets `oRdonly` / `oCreat` / … still compose with `|`.
- **`File` type:** `openFile` / `readAll` / `write` / `seek` / `position` /
  `close` / `isOpen` (see conformance `file_type`, `file_write`, `file_seek`,
  `file_openmode` / `iolib/Io.thx`).
- HTTP: `Request` / `Response` / `Header`, parse/write helpers, Router, bounded
  static server (`http_server` conformance).

## Not shipped (do not expect)

- Full oracle SPEC surface
- Algebraic effects
- Named `ffi` import root
- Shell dialect control flow as language syntax
- Tree-walk interpreter product path

Engineering detail and gaps: [`../capabilities.md`](../capabilities.md).
