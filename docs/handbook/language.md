# Language (shipped subset)

Native Thoxa is a small, growing language. This page lists **what the compiler
implements with conformance**, not the full oracle SPEC.

## Programs

```thx
import logging, { info } from "std/Logging"

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    info("hello")
    0
}
```

There is **no** ambient `println` / `print` (**D125**). Use
[`std/Logging`](std.md): preferred face is **combined**
`import logging, { info, … } from "std/Logging"` (**D127**) — namespace for
configure, selective names for short emits. Default sink is **stderr** with full
timestamps; for golden/stdout data lines use `toStdout()` +
`setFormat(formatPlain)` as above.

Optional CLI arguments:

```thx
import logging, { info } from "std/Logging"

fn main(arguments: Array<String>) -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    ## arguments[0] is the program path; further entries are user args
    if arguments.count > 1 {
        info("{arguments[1]}")
    }
    0
}
```

Directory packages may use `Main.thx` as the entry file.

### Program vs session script

| Shape | Top level allows |
|-------|------------------|
| **Program** (`fn main` present) | Declarations only: `import`, `type`, `fn`, `let` / `mut`, `extern`, `export` |
| **Session script** (no `main`) | Declarations **plus** bare statements (`info("hi")`, assignments, …) |

A bare name like `FOOBAR` at file scope is a **statement**, not a declaration.
In a program it is an error (clear span). In a session script it is the script
body (and fails later if the name is undefined). See [session.md](session.md).

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

**Diverging loop (**D128**):** a `loop { … }` with no `break` that exits *that*
loop is bottom-typed (Never). It unifies with any expected return type, so an
infinite server loop can be the last expression of `-> Trial<…>` / `-> Int64`
without a dummy `Pass(Void)` or `0` after it. Nested loops' breaks do not count
for the outer. `while` / `for` still yield `Void` (they can finish).

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
info("{c.add(by: 5)}")   ## 15 (after import { info } + toStdout + formatPlain)
info("{c.n}")            ## 15
```

## Modules

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
import logging, { info, warn } from "std/Logging"   ## namespace + selective (D127)
import { readFile } from "std/Io"
import net, { Socket } from "std/Net"
export fn helper() -> Int64 { 1 }
```

| Form | Meaning |
|------|---------|
| `import { a, b as c } from "…"` | Selective — bind exported names (optional `as`) |
| `import ns from "…"` | Namespace — camelCase instance; members via `ns.x` |
| `import ns, { a, b } from "…"` | Combined — both in one clause (**D127**) |

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

## I/O and HTTP (`std/`)

Product modules (**D126**). Return types use `Trial` / `May` from
`std/Fallible` — import Fallible when you annotate or construct those values.
Full surface: [`std.md`](std.md).

- **`std/Io`:** free `readFile` / `writeFile` / `appendFile`; `File` +
  `openFile` / `readAll` / `write` / `seek` / `position` / `close`; OpenMode
  union + Int32 flag lets.
- **`std/Net`:** TCP `Socket` (`createSocket`, bind/listen/accept/connect,
  `receiveRequest`, `sendAll`).
- **`std/Http`:** `Request` / `Response` / `Header`, `parseRequest` /
  `writeResponse`, status helpers (`ok`, `notFound`, `badRequest`,
  `forbidden`, `methodNotAllowed`).
- **`std/HttpStatic`:** static-file `Router` (`handle` → Response; GET/HEAD).

## Not shipped (do not expect)

- Full oracle SPEC surface
- Algebraic effects
- Named `ffi` import root
- Shell dialect control flow as language syntax
- Tree-walk interpreter product path

Engineering detail and gaps: [`../capabilities.md`](../capabilities.md).
