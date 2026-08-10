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

Optional CLI arguments:

```thx
fn main(arguments: Array<String>) -> Int64 {
    ## arguments[0] is the program path; further entries are user args
    if arguments.count > 1 {
        println(arguments[1])
    }
    0
}
```

Directory packages may use `Main.thx` as the entry file.

## Values and types (highlights)

| Area | Shipped |
|------|---------|
| Scalars | `Int64`, `Int32`, `UInt32`, `UInt64`, `Byte`, `Bool`, `Void` |
| Heap | `String` (interpolation, `.count`, `\xHH` escapes), `Bytes` (`.insertAt`/`.removeAt`/`.equals`), growable `Buffer`, dynamic `Array<T>` (`.append` / `.dropLast` / `.insertAt` / `.removeAt`) |
| User types | `type Name(fields) { methods }` — including heap fields; method field write-back |
| Generics | `type Box<T>(value: T) { methods }`; generic unions `type Opt<T> = Box<T!> \| Empty?` (monomorphized) |
| Unions | `type U = A \| B` (products, pure literals, nested flatten); marks `T!` / residual `?` |
| C-layout | Body fields, `T()` zero-init, `Address<T>`, fixed `Array<T,N>` fields/locals (element get/set **and** whole-array assign/bind) |
| Errors | `May<T>` / `Trial<Ok,Err>` from `std/Fallible` with `?` / `match` (ok/err may nest) |
| Equality | String `==` / `!=`; usual scalar compares |

**Fallibility** — ordinary generic unions in `std/Fallible`; **import required**:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"

fn half(x: Int64) -> Trial<Int64, Int64> {
    if x % 2 == 0 { Pass(x / 2) } else { Fail(x) }
}

fn firstAb(hay!: Bytes) -> May<Int64> {
    hay.find(Bytes.fromString("ab"))
}
```

Defined as `May<T> = Has<T!> | Not?` and `Trial<P, F> = Pass<P!> | Fail<F?>`.
`?` unwraps the success extract or early-returns the residual (rewraps across
compatible Trial monomorphs, including across modules). User unions with a
residual mark use the same `?` operator.

**Reference counting:** simple single-threaded RC (dup on copy, drop at function
end for **locals**). Module-level heap bindings outlive the function that wrote
them (D122). Precise RC / Perceus is **not** shipped.

## Control flow

`if`, `while`, `loop`, `for` (ranges and collections), `break`, `continue`,
early `return` / `return expr`. Branch values may inject into an expected union
(`-> Outcome` / `let x: U = if …`).

## Match

- **Trial / May / user unions:** constructor arms (`Pass(x)` / `Fail(e)`,
  `Has(x)` / `Not`, custom tags), discard `_`, optional **`if` guards**
  (`Pos(n) if n > 0 { … }` — binds visible to the guard; guarded arms do not
  exhaust a tag).
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
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
import { readFile } from "./Io"
import net from "./Net"
export fn helper() -> Int64 { 1 }
```

| Origin | Meaning |
|--------|---------|
| `./…` | Relative to current file |
| `~/…` | Relative to **entry** file directory |
| `std/…` | Product standard library |

Fallibility types are **not** ambient — import them from
[`std/Fallible`](std.md#modules-current) (or selective subsets). Matching on a
`Trial`/`May` value returned from another module does not by itself require the
import; constructing, annotating, or calling builtins that produce May (e.g.
`Bytes.find`) does.

Module-level `let` / `mut` are supported (including heap types such as
`Array` / `String`). Reassigning a module `mut` drops the previous value and
keeps the new one for the rest of the process — the binding is **not** dropped
when the assigning function returns (D122). General in-function `import`/`type`
is **not** a language feature (session host may lift decls).

## Extern

```thx
extern fn puts(s!: String) -> Int32 from "c"
extern fn sysErrno() -> Int32 from "c" "thoxa_errno"
```

Marshals common scalars, `String`, `Bytes`, `Void`, `Address<T>`. String
*returns* are copied via the runtime.

## I/O and HTTP (library shape)

These live in sample/conformance libraries today (not a fat `std/Io`). Their
return types use `Trial` / `May` from `std/Fallible` — import Fallible when you
annotate or construct those values.

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
