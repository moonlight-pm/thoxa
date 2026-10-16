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

What `main` returns is the process exit status, `0` to `255`. Any other value
panics once the program's tasks finish (`thoxa panic: main returned 256; an exit
status is 0 to 255`), since the kernel would keep only the low 8 bits and
`256` would read as success (**D173**).

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
| Scalars | `Int64`, `Int32`, `UInt32`, `UInt64`, `Byte`, `Float64`, `Bool`, `Void` |
| Heap | `String` (interpolation, `.count`, `\xHH` escapes for ASCII `\x00`–`\x7F` (**D159**), `\0` kept inside the string, **newlines kept**), `sql"…"` tagged interpolator (**D144**, may span lines), `Bytes` (`.insertAt`/`.removeAt`/`.equals`), growable `Buffer`, dynamic `Array<T>` (write-back `.append` / `.dropLast` / `.insertAt` / `.removeAt` on `mut`), `Map<K, V>` (`.has` / `.get`→`May` / write-back `.set`), builtin **`Node`** (tag literals, **D147**) |
| User types | `type Name(fields) { methods }` — including heap fields, **Node**, and nested user types / sums (**D135**); method field write-back. Recursive fields are an error (use `Array` / `Map`) |
| Generics | `type Box<T>(value: T) { methods }`; generic unions `type Opt<T> = Box<T!> \| Empty?` (monomorphized) |
| Unions | `type U = A \| B` (products, pure literals, nested flatten); marks `T!` / residual `?`; singleton alias `type Home = "home"`. Member product fields may be scalars (incl. `Float64`), String/Bytes, User, `Array`, or `Map` (**D141**). String-literal unions subtype `String` (**D149**) |
| Literal types | Bare `"home"`, `0`, `true` are types in formals / returns / annotations; inline `"a" \| "b"` in those positions too (**D131**) |
| Refinements | `type Qty = Int64 where >= 1` — real types; `T.read` / `T.from` (**D149**) |
| Multi-clause fns | Same name, different formal types → one function (type-driven match sugar, **D131**) |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132**); no closures |
| C-layout | Body fields, `T()` zero-init, `Address<T>`, fixed `Array<T,N>` fields/locals (element get/set **and** whole-array assign/bind; an index outside `0..N-1` panics). A `mut` struct local passed where `Address<T>` is expected passes its address, to C or to a Thoxa function; an `Address<T>` parameter passes on unchanged. A value of a `drop` type passes its box pointer (**D168**) |
| Errors | `May<T>` / `Trial<Ok,Err>` from `std/fallible` with `?` / `match` (ok/err may nest) |
| Equality | String `==` / `!=`; usual scalar compares (including `Float64`). Comparison `<` needs a space after it (`a < b`, not `a<b`) — glued `<name` is a [tag](#tag-literals-and-node-jsx) |

**Integers (**D157**):** `Int64` arithmetic wraps on overflow (two's
complement); it is never undefined. `/` and `%` by zero panic with
`division by zero`. `a..=b` includes `b` and stops there, even at the largest
`Int64`.

**Shifts (**D161**):** `a << n` wraps like `*`; `a >> n` keeps the sign. A
count of 64 or more shifts every bit out (`<<` gives 0, `>>` gives 0 or -1).
A negative count panics with `negative shift count`.

**Narrower integers (**D167**):** arithmetic happens in `Int64`. A value passed,
returned, or stored where `Int32`, `UInt32`, `UInt64`, or `Byte` is expected
converts when it fits; when it does not, the program panics
(`300 does not fit in Byte`). A literal that cannot fit is a compile error.
To keep the low bits instead (a bit pattern, `-1` for an unsigned C
parameter), say so:

```thx
let allOnes = UInt64(truncating: -1)   ## 18446744073709551615
let low = Byte(truncating: 300)        ## 44
let back = Int64(truncating: allOnes)  ## -1
```

`Int64`, `Int32`, `UInt64`, `UInt32`, and `Byte` take `truncating:`. Widening
(`Byte` → `Int64`) never checks. A `UInt64` above the `Int64` maximum panics
when it meets `Int64` arithmetic or a comparison.

**`if` without `else` (**D158**):** a statement with no value. Where a value is
expected, write the `else`:

```thx
fn sign(n!: Int64) -> Int64 {
    if n < 0 {
        -1
    } else {
        1
    }
}
```

**A line that starts with `-x` (**D162**)** begins a new expression, so the
`-1` tail after an `if` above is a value, not a subtraction. To continue an
expression onto the next line, put a space after the `-` (`- x`) or end the
line with the operator. Match patterns take negative literals (`-1 => …`).

**Shadowing:** a `let`, match binding, or `for` variable can reuse an outer
name. Inside its block it is a new variable (it may have another type); after
the block the outer one is unchanged.

**`Float64` (**D140**):** IEEE-754 binary64. Literals `1.5`, `1e2`, `1.5e-1`,
`3.14f64`. `1..5` is still a range (a `.` is a fraction only when a digit
follows). Arithmetic `+ - * /` and compares; no `%`. Integers coerce to
`Float64`; the reverse is not implicit. No `Float32`. Interpolation prints a
shortest round-trip (`2.0` stays `2.0`).

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

**Reference counting:** simple single-threaded RC (dup on copy, drop when a
local is overwritten or the function ends). Module-level heap bindings outlive
the function that wrote them (D122). `cargo make test` links programs with
LeakSanitizer (**D151**). Match binds and union-match results drop with
the rest of the function (**D152**). Overwrite of a live heap local drops the
previous occupant, including `if` / `match` results reused in a loop
(**D153**). Heap arguments are copies the callee owns
(**D154**). `return` / `?` drop every live heap local, including temps
created later in a loop. A product may define `fn drop()` with no formals
and `Void` return (**D155**): values are heap boxes; last RC runs `drop`
then frees. `File` / `Socket` / `Db` close that way; explicit `close` is
still a no-op afterward. Only the module that defines such a type may call
its constructor (**D168**): `Db(handle: 1)` elsewhere is an error, so no code
can wrap a raw handle in a second owner. Copies in a `Map`, an `Array`, or
module state share the one box, so a `close` through any copy is seen by
all. Optional AddressSanitizer (`cargo make test-asan`)
covers use-after-free on non-fiber programs; it is not the default suite
(ucontext fibers). Precise RC / Perceus is **not** shipped.

## Refinements and `Type.read`

A type is a predicate. **`where`** narrows it; **`T.read` / `T.from`** inhabit
it (**D149**). Not a schema library and not traits.

```thx
import { Issue, message } from "std/json"
import { Pass, Fail, Trial } from "std/fallible"

type Qty = Int64 where >= 1
type DrinkSlug = "espresso" | "mocha"
type CreateOrder(slug: DrinkSlug, qty: Qty)

let a: Qty = 2              ## literal inhabits
let n: Int64 = a            ## Qty → Int64 is implicit
## let b: Qty = n           ## type error — use Qty.from(n)
match CreateOrder.read(body) {
    Fail(e) => info(message(e))
    Pass(o) => info(o.encode())
}
```

| Piece | Behavior |
|-------|----------|
| `where` | After a type. Inside: `&` and, `\|` or, parens. Outside, `\|` is still a union |
| Ranges | On `Int64` / `Float64` = the value; on `String` / `Array` = `.count` |
| Messages | Optional template after a conjunct (`{value}`, `{expected}`, `{path}`) |
| Narrowing | `type Dozen = Qty where <= 12` intersects clauses. Empty types are errors |
| `T.from(x)` | Runtime base / string → `Trial<T, Issue>` |
| `T.read(s)` | JSON string → `Trial<T, Issue>` (tree `decode` first, then shape + `where`) |
| `t.encode()` / `t.toJson()` | Back to compact JSON / `Json` |
| `Issue` | `Missing \| Mismatch \| Rejected` in [`std/json`](std.md). Fail-fast. `message(e)` |
| `May<T>` fields | Missing or JSON `null` → `Not`. Write `May<Qty>`, not `May<Int64 where …>` |

Closed slugs are **literal unions** (`"espresso" \| "mocha"`), a subtype of
`String` — prefer that over `String where "a" \| "b"`. Tree `decode` /
`encode` of `Json` stay **D141** (`Fail(String)` for syntax). No collect-all,
no cross-field `where` on a product, no regex.

## Control flow

`if`, `while`, `loop`, `for` (ranges and collections), `break`, `continue`,
early `return` / `return expr`. Branch values may inject into an expected union
(`-> Outcome` / `let x: U = if …`).

**Diverging loop (**D128**):** a `loop { … }` with no `break` that exits *that*
loop is bottom-typed (Never). It unifies with any expected return type, so an
infinite server loop can be the last expression of `-> Trial<…>` / `-> Int64`
without a dummy `Pass(Void)` or `0` after it. Nested loops' breaks do not count
for the outer. `while` / `for` still yield `Void` (they can finish).

**`return` is Never (**D164**):** an `if` / `match` arm that returns fits beside
any other arm, Void or a value:

```thx
match found {
    Not {
        return Fail("missing")
    }
    Has(_) { Void }
}
```

## Match

- **Trial / May / user unions / `Node`:** constructor arms (`Pass(x)` / `Fail(e)`,
  `Has(x)` / `Not`, `Element(tag, attrs, kids)` / `Text(s)` / `Fragment(kids)`
  for **D147** `Node`, custom tags), discard `_`, optional **`if` guards**
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
be taken as values. To start a **named** function without waiting, see
[`-->`](#nowait) below.

Parameter defaults (M3 subset) on free functions, **methods**, **constructor
params**, and **`!` formals** (including `extern` — defaults expand at the
call site before the C call). Free names inside default expressions resolve
in the **defining** module (so importers need not re-export constants used
only as defaults). A non-default formal may not follow a defaulted one.

**Return types (**D129**, **D174**):** an explicit `-> T` is the contract (body
must match). Unannotated non-`main` **free functions** and **methods** **infer**
the return type from exits when possible:

- value `return` / block tail → that type (`42` ⇒ `Int64`; empty ⇒ `Void`)
- residual `?` → structural collapse of residual-marked monomorphs (success
  type-args to `Void`, keep residual extract), then sum with value exits
- `x?` then `Pass(value)` (or `Has(value)` after a `May`) → the payload is the
  success type: `Trial<Int64, String>`
- bare `Pass`/`Fail` alone cannot pick a union name → write `-> …`
- self/mutual recursion unannotated → write `-> …` (the error names each
  function or `Type.method` on the cycle)

```thx
type Box(width: Int64, height: Int64) {
    export fn area() {          ## Int64
        width * height
    }
}
```

Unannotated `main` still defaults to **`Int64`**, and `drop` is always
**`Void`**. Still **`Void`** when unannotated: multi-clause methods and methods
of generic types (annotate if they return a value).

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

## Nowait

Token **`-->`** begins a named free-function call and does **not** wait for it
(**D148**). Not a word (`async` / `go` / `spawn`). Expression type is **`Void`**;
the callee’s return is dropped. No Future / `Task` handle.

```thx
--> tick("a")
--> serveOne(peer, handle)
```

Callee is **one ident** in scope (`--> name(args)`), including defaults. Import
a name to start a function from another module. **Not:** a function pointer,
`instance.method`, `extern`, or `--> { … }`.

The process is **colorless**: user code does not write `await`. `std/net`
socket ops park the current task when they would block. If the callee never
parks, it finishes before `-->` returns. Generated `main` waits until idle
before exiting. One OS thread (not `fork` / not threads).

`std/http/server.serve` uses `--> serveOne` per accept so keep-alive peers can
park without stalling the listener. See [std.md](std.md).

A panic inside a task ends **that task only** (**D166**). It prints
`thoxa panic: …` and `thoxa: task ended by panic`; `main` and the other tasks
keep running. The task's frames are abandoned, so the memory they held leaks.
Sockets the task owns (`net.ownByTask(socket)`, see [std.md](std.md)) are
closed, and every copy of each reads closed (`fd` is -1).
A panic in `main` itself still ends the program.

## Route clauses

HTTP methods in a **type body** are routes (**D150**). The type must be
nullary (no constructor params / fields). The compiler generates
`route(req: Request) -> Response`.

```thx
import http, { Request, Response, ok, notFound, badRequest } from "std/http"

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

let res = Routes().route(req)
```

| Piece | Behavior |
|-------|----------|
| Verbs | `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS` |
| String / omitted `->` | Wrap `ok(…, "text/plain")` |
| `-> Response` | Pass through |
| Miss | `notFound()` — no fallthrough |
| Captures | `:name` in any segment; `String` or `Int64` (bad digits → `badRequest("invalid {name}")`) |
| `Request` formal | Optional, any name; not a path capture |
| Imports | Module must import the names the desugar calls (`Request`, `Response`, `ok`, `notFound`, and `badRequest` if any `Int64` capture) |

`route` reads `req.method` / `req.path` only. JSON / form bodies are
**not** compiler work — call `T.read` in the handler. See [std.md](std.md).

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
  panics out of range. Nested `Array` / `Map` elements are heap-RC (a
  returned `[["x"]]` keeps the inner arrays).
- **Map keys:** `String`, `Int64`, `Bool`. Ident keys in `{ Cookie: … }`
  are the string `"Cookie"`. Empty `{}` is still a block; empty map is `Map()`.
  A `type Header(…)` may be a **value** (`Map<String, Header>`), not a key.
  A Map may itself be a constructor-param field.
- **Map reads:** `m[k]` and `.get(k)` are `May<V>` (import Fallible). `.has(k)`
  is `Bool`. Duplicate keys in one literal are an error.
- **Map writes:** `.set` / `.remove` / `m[k] =` write back. Missing `.remove`
  is a no-op.
- **Iteration:** `for x in arr`; `for k, v in map` (insertion order).
  `.keys()` / `.values()` snapshot to `Array`. The loop variable is a nested
  binding — an outer `let x` is unchanged after the loop.
- Fixed `Array<T, N>` and Bytes splice are unchanged (inline / functional).

## Tag literals and Node (JSX)

Builtin **`Node`** is an HTML tree: `Element | Text | Fragment` (same value).
No import. Tags are **expressions**. `n.html()` is escaped markup (no doctype,
no pretty indent). **D147.**

```thx
type Card(title: String) {
    export fn view(children: Array<Node>) -> Node {
        <section class="card">
            <h1>{title}</h1>
            {children}
        </section>
    }
}

let n = <Card title="Hi">
    <p>Hello, {name}</p>
    <input disabled />
</Card>
info(n.html())
```

| Piece | Behavior |
|-------|----------|
| HTML tag | `<div class="x">…</div>` — hyphenated names (`data-*`) ok |
| Component | PascalCase `<Card title="Hi">…</Card>` is eager `Card(…).view(children:)` — a type that **produces** `Node`, not a subtype |
| Fragment | `<>…</>` (empty `<>` is a valid `Node`) |
| `{e}` children | `String` (escaped text); `Int64` / `Float64` / `Bool` (stringify, same as interpolation); `Node`; `Array<Node>` splices |
| `{for x in xs { child }}` | Collects `Array<Node>` (child position only; statement `for` is unchanged) |
| Attrs | `"…"` or `{e}` (same scalars as children). Bare `disabled` renders as `disabled` |
| Spreads | HTML `{...xs}` is `Array<Attr>` (later attrs append). Component `{...p}` copies product fields |
| `<` vs tag | `<name` / `<>` with **no space** after `<` is a tag, including after another expr. Compare with `a < b`. Glued `a<b` is an error |
| Raw text | Whitespace-only gaps drop. Bare `#` is a line comment. **`\#` is a literal `#`** |
| Void tags | `br` `hr` `img` `input` `meta` `link` — no close in `.html()` |
| Match | `Element(tag, attrs, kids)` / `Text(s)` / `Fragment(kids)` |
| Ctors | Ambient `Element` / `Text` / `Fragment` / `Attr` |

`std/http.htmlResponse(root)` sends `<!DOCTYPE html>` plus `root.html()`.
`htmlFragment` is the snippet without the doctype. See [std.md](std.md).

`Node` does **not** interpolate in `"{n}"` — use `.html()`.

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

**Mutating methods (**D163**):** a method that writes a field (as `add` does)
needs a writable receiver: a `mut` binding, or `items[i]` on a `mut` Array
(the element is stored back). On a `let`, a parameter, a `for` or match
binding, or a temporary it is a type error, as `append` is. From outside its
type a field is read-only, so `pair.left.add(by: 1)` is an error; inside
`Pair`'s own methods `left.add(by: 1)` writes back. Types with `fn drop()` are
shared boxes (**D155**) and are exempt.

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
stems are kebab-case (`http.thx`, `math-utils.thx`); types inside stay
PascalCase. `main.thx` remains the directory entry. Specifiers match the
file: `"std/http"`, `"std/http/server"`, `"./math-utils"`.

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
`Array` / `String`), with an optional annotation where the value alone does
not say the type (**D168**):

```thx
mut connection: May<Db> = Not()
mut subscribers: Map<Int64, Socket> = Map()
```

Reassigning a module `mut` drops the previous value and
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
  `htmlResponse` (**D137**), `Cookie` / `withCookie` / `clearCookie` (**D142**).
- **`std/http/server`:** `serve(port, handle)`, `serveOne`, `readBody`
  (**D136**/**D139**).
- **`std/http/static`:** static-file `Router` (`handle` → Response; GET/HEAD).
- **`std/json`:** `Json` tree, `decode` / `encode` (**D141**).
- **`std/sqlite`:** amalgamation in the runtime; `open` / `exec` / `query`
  (**D143**). Write `sql"… {value} …"` (**D144**); holes are embedded as
  SQL. `exec` / `query` take `Sql`.

## Not shipped (do not expect)

- Full oracle SPEC surface
- Algebraic effects
- Named `ffi` import root
- Shell dialect control flow as language syntax
- Tree-walk interpreter product path

Engineering detail and gaps: [`../capabilities.md`](../capabilities.md).
