# Language (shipped subset)

Native Thoxa is a small, growing language. This page lists **what the compiler
implements with conformance**, not the full oracle SPEC.

## Programs

```thx
import logging, { info } from "std/logging"

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    info("hello")
    0
}
```

There is **no** ambient `println` / `print` (**D125**). Use
[`std/logging`](std.md): preferred face is **combined**
`import logging, { info, … } from "std/logging"` (**D127**) — namespace for
configure, selective names for short emits. Default sink is **stderr** with full
timestamps; for golden/stdout data lines use `toStdout()` +
`setFormat(formatPlain)` as above.

Optional CLI arguments:

```thx
import logging, { info } from "std/logging"

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

Directory packages may use `main.thx` as the entry file.

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
| Heap | `String` (interpolation, `.count`, `\xHH` escapes), `Bytes` (`.insertAt`/`.removeAt`/`.equals`), growable `Buffer`, dynamic `Array<T>` (write-back `.append` / `.dropLast` / `.insertAt` / `.removeAt` on `mut`), `Map<K, V>` (`.has` / `.get`→`May` / write-back `.set`) |
| User types | `type Name(fields) { methods }` — including heap fields, **Node**, and nested user types / sums (**D135**); method field write-back. Recursive fields are an error (use `Array` / `Map`) |
| Generics | `type Box<T>(value: T) { methods }`; generic unions `type Opt<T> = Box<T!> \| Empty?` (monomorphized) |
| Unions | `type U = A \| B` (products, pure literals, nested flatten); marks `T!` / residual `?`; singleton alias `type Home = "home"` |
| Literal types | Bare `"home"`, `0`, `true` are types in formals / returns / annotations; inline `"a" \| "b"` in those positions too (**D131**) |
| Multi-clause fns | Same name, different formal types → one function (type-driven match sugar, **D131**) |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132**); no closures |
| C-layout | Body fields, `T()` zero-init, `Address<T>`, fixed `Array<T,N>` fields/locals (element get/set **and** whole-array assign/bind) |
| Errors | `May<T>` / `Trial<Ok,Err>` from `std/fallible` with `?` / `match` (ok/err may nest) |
| Equality | String `==` / `!=`; usual scalar compares. Comparison `<` needs a space after it (`a < b`, not `a<b`) |

**Fallibility** — ordinary generic unions in `std/fallible`; **import required**:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"

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

- **Trial / May / user unions / `Node`:** constructor arms (`Pass(x)` / `Fail(e)`,
  `Has(x)` / `Not`, `Element(tag, attrs, kids)` / `Text(s)` / `Fragment(kids)`,
  custom tags), discard `_`, optional **`if` guards**
  (`Pos(n) if n > 0 { … }` — binds visible to the guard; guarded arms do not
  exhaust a tag).
- **Tag-only bare ctors:** bare `Pass` / `Fail` / `Has` / user tags match the
  tag **without** binding the payload (same idea as `Pass(_)` when you ignore
  the value). Use `Name(x)` when you need the payload.
- **Arm bodies:** `{ … }` for multi-statement arms (one statement per line), or
  **`=> expr`** for a single-expression arm (sugar for a one-expr block).
  Guards work with both: `Pat if g => e` / `Pat if g { … }`. No commas between
  arms. `->` is only for types; `=>` is only for match arms.
- **Value match:** String / Int64 / Bool **literal** arms, **or-patterns**
  (`0 | 1 { … }` or `0 | 1 => …`), optional **`if` guards** on those arms, and
  a final catch-all: `else` or a **bare bind** (`n { … }` / `n => …` binds the
  scrutinee; bare `_` discards).

## Functions

```thx
fn add(a: Int64, b: Int64 = 0) -> Int64 {
    a + b
}
```

**Function pointers (**D132**):** a function type is written like a header,
`(x: Int64) -> Int64`. Values are a code pointer, optionally paired with a
receiver when formed as `instance.method` (bound — later `h(args)` is
`instance.method(args)`). Not lexical closures.

```thx
fn apply(f: (x: Int64) -> Int64, v: Int64) -> Int64 {
    f(v)
}

let n = apply(double, 21)       # free fn
let n = apply(counter.add, 5)   # bound method
let n = apply2(Counter.add, c, 7)  # unbound: extra self argument
```

They may be arguments, returns, locals, and **module** `let`/`mut`. They may
**not** be constructor fields or array elements (type error). No `==`, no
`-->` of a pointer, no nested `fn`. Multi-clause and `extern` functions cannot
be taken as values.

Parameter defaults (M3 subset) on free functions, **methods**, **constructor
params**, and **`!` formals** (including `extern` — defaults expand at the
call site before the C call). Free names inside default expressions resolve
in the **defining** module (so importers need not re-export constants used
only as defaults). A non-default formal may not follow a defaulted one.

**Return types (**D129**):** an explicit `-> T` is the contract (body must match).
Unannotated non-`main` **free functions** **infer** the return type from exits
when possible:

- value `return` / block tail → that type (`42` ⇒ `Int64`; empty ⇒ `Void`)
- residual `?` → structural collapse of residual-marked monomorphs (success
  type-args to `Void`, keep residual extract), then sum with value exits
- bare `Pass`/`Fail` alone cannot pick a union name → write `-> …`
- self/mutual recursion unannotated → write `-> …`

Unannotated `main` still defaults to **`Int64`**. Unannotated **methods** still
default to **`Void`** (annotate if they return a value).

**Multi-clause functions (**D131**):** several `fn` definitions with the **same
name** in one scope (module free function **or** methods on one type) are **one
function** — sugar for an ordered `match` on formals whose types differ.
Formals stay `name: Type` (not pattern formals). Dispatch may use **any** types,
including bare singleton literals (`"home"`, `0`, `true`).

```thx
fn page(path: "home") -> String {
    "home-page"
}
fn page(path: "admin") -> String {
    "admin-page"
}

type Router {
    export fn get(path: "home", req: Request) -> String { "home" }
    export fn get(path: "admin", req: Request) -> String { "admin" }
}
```

Rules (v0):

- Equal arity; **no parameter defaults** on multi-clause sets
- Source order = first match wins
- Domain is the join of clause types (as specific as possible — not forced to
  `String`); call sites must assign into that closed domain
- Duplicate domains, later unreachable clauses, and export mismatch (`export`
  on some clauses only) are **errors**
- One runtime entry; returns join like a match (per-clause `->` is a local
  expect; free-fn return inference applies to the merged body)

Bare literal types and inline member-sets (`"a" | "b"`) are also valid in
formals, returns, and annotations in general. Singleton alias:
`type Home = "home"`.

## Arrays and maps (value collections)

`Array<T>` and `Map<K, V>` are heap values with Swift / `Buffer` copy-on-write
(**D133** / **D134**). `let ys = xs` shares storage until a write; a write
through a `mut` place copies if anyone else still holds the object.

```thx
import { Has, Not, May } from "std/fallible"

mut xs = [1, 2, 3]
xs.append(4)          ## Void — not xs = xs.append(4)
xs[1] = 99

mut m: Map<String, Int64> = Map()
m.set("a", 1)
m["b"] = 2
match m["a"] {
    Has(v) => info("{v}")
    Not => info("missing")
}
let lit = { Cookie: "a=1", Host: "localhost" }
for k, v in lit {
    info("{k}={v}")
}
```

- **Array writes** (`.append` / `.dropLast` / `.insertAt` / `.removeAt` /
  `a[i] =`) require a `mut` place and return `Void`. Index get `a[i]` still
  panics out of range.
- **Map keys:** `String`, `Int64`, `Bool`. Ident keys in `{ Cookie: … }`
  are the string `"Cookie"`. Empty `{}` is still a block; empty map is `Map()`.
  A `type Header(…)` may be a **value** (`Map<String, Header>`), not a key.
  A Map may itself be a constructor-param field.
- **Map reads:** `m[k]` and `.get(k)` are `May<V>` (import Fallible). `.has(k)`
  is `Bool`. Duplicate keys in one literal are an error.
- **Map writes:** `.set` / `.remove` / `m[k] =` write back. Missing `.remove`
  is a no-op.
- **Iteration:** `for x in arr`; `for k, v in map` (insertion order).
  `.keys()` / `.values()` snapshot to `Array`.
- Fixed `Array<T, N>` and Bytes splice are unchanged (inline / functional).

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
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"
import logging, { info, warn } from "std/logging"   ## namespace + selective (D127)
import { readFile } from "std/io"
import net, { Socket } from "std/net"
export fn helper() -> Int64 { 1 }
```

| Form | Meaning |
|------|---------|
| `import { a, b as c } from "…"` | Selective — bind exported names (optional `as`) |
| `import ns from "…"` | Namespace — camelCase binding; members via `ns.x`. Not a value |
| `import ns, { a, b } from "…"` | Combined — both in one clause (**D127**) |

A library file is a **module** (a scope), not a type (**D138**). Non-entry
stems are kebab-case (`http.thx`, `http-server.thx`); types inside stay
PascalCase. `main.thx` remains the directory entry. Specifiers match the
file: `"std/http"`, `"./math-utils"`.

| Origin | Meaning |
|--------|---------|
| `./…` | Relative to current file |
| `~/…` | Relative to the **package root** (nearest ancestor `main.thx`; otherwise the entry file’s directory) |
| `std/…` | Product standard library |

Fallibility types are **not** ambient — import them from
[`std/fallible`](std.md#modules-current) (or selective subsets). Matching on a
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

Product modules (**D126** / **D136** / **D137**). Return types use `Trial` / `May` from
`std/fallible` — import Fallible when you annotate or construct those values.
Full surface: [`std.md`](std.md).

- **`std/io`:** free `readFile` / `writeFile` / `appendFile`; `File` +
  `openFile` / `readAll` / `write` / `seek` / `position` / `close`; OpenMode
  union + Int32 flag lets.
- **`std/net`:** TCP `Socket` (`createSocket`, bind/listen/accept/connect,
  `receiveRequest`, `sendAll`).
- **`std/http`:** `Request` / `Response` / `Header`, `parseRequest` /
  `writeResponse`, `Request.query`, header/query/form/cookie helpers,
  `seeOther` / `finalize` (**D136**), `queryParam` / `formField` / `cookie` /
  `htmlResponse` (**D137**).
- **`std/http-server`:** `serve(port, handle)`, `serveOne`, `readBody`
  (**D136**).
- **`std/http-static`:** static-file `Router` (`handle` → Response; GET/HEAD).

## Not shipped (do not expect)

- Full oracle SPEC surface
- Algebraic effects
- Named `ffi` import root
- Shell dialect control flow as language syntax
- Tree-walk interpreter product path

Engineering detail and gaps: [`../capabilities.md`](../capabilities.md).
