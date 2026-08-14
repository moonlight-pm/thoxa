# Freeze: Function pointers and bound methods (D132)

**Status:** shipped (v0)  
**Decision:** D132  
**Gate:** `fn_pointers`  
**As of:** 2026-08-13  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D132 in
[`decisions.md`](../decisions.md)  
**Related (still parked):** full FCF / lexical closures / ORC
([`ideas/2026-08-11-first-class-functions-and-orc.md`](../ideas/2026-08-11-first-class-functions-and-orc.md));
traits (separate surface; not this freeze)

---

## Intent

Pass and call a function without inventing closures. A function type is a
**code pointer**, optionally paired with a **receiver** when the value was
formed as `instance.method`. That pair is the moral equivalent of writing
`instance.method(args)` later. It is **not** lexical capture.

Crema’s host face after this ships:

```thx
server.serve(9091, routes.handle)      # free fn
server.serve(9091, app.handle)         # bound method — same type
```

```thx
export fn serve(
    port!: Int64,
    handle!: (req: Request) -> Response
) -> Trial<Void, String> {
    …
    let res = handle(req)
}
```

---

## Locks (v0)

### 1. Function types

The type is written like a header:

```thx
(req: Request) -> Response
(a: Int64, b: String) -> Bool
() -> Void
```

- Valid in **all type positions**: formals, returns, `let` / `mut` annotations,
  type-alias RHS (`type Handler = (req: Request) -> Response`).
- Formal **names** are documentation / hover only. Type equality is **arity +
  parameter types + return type** (after alias normalize).
- **Not** oracle `Fn(Request) -> Response`. Do not take the word `Fn` (that
  name stays free for a later fat/closure story, if any).
- No defaults, no `!`, no variadics in the type. `!` / defaults on the
  *definition* do not appear on the pointer type. A call through a pointer
  passes **every** argument; no default fill.

### 2. Forming a value

| Expression | Meaning | Type |
|------------|---------|------|
| `handle` / `routes.handle` | Free function (module member) | that fn’s signature |
| `Crema.handle` | **Unbound** method. `Crema` is a **type qualifier**, not a value (not M4 types-as-values). | `(self: Crema, …args) -> R` |
| `app.handle` (no call) | **Bound** method. Holds this instance. | `(…args) -> R` (`self` hidden) |

`app.handle(req)` remains an ordinary method call. The new form is
`app.handle` used as a **value** (not followed by `(`).

Only **single-clause** Thoxa free functions and methods. Multi-clause as a
value stays out (D131). `extern fn` as a pointer is out of v0.

### 3. Call

```thx
let res = handle(req)
```

Same syntax as a named call. All arguments required. The callee may be any
expression of function type (local, param, module binding, call result).

`-->` of a function *value* is out of v0. Existing `--> name(args)` still
starts a **named** free Thoxa function only.

### 4. Bound method = receiver + code, not a closure

`app.handle` remembers **only** `app`. Later `handle(req)` is
`app.handle(req)`. No other `let` / `mut` from the forming scope is captured.

```thx
let prefix = "/menu"
let h = fn(req) { … prefix … }    # illegal — nested / lexical fn
```

No nested functions, no `|args|` sugar, no partial apply.

### 5. Representation (normative enough to lock RC)

A function value is an **unboxed pair** `{ receiver, code }`:

- Free fn / unbound method: `receiver` is null; copy is a word copy; no drop.
- Bound method: `receiver` is the instance; **copy dups** it; **drop** drops it.

The pair is **not** a heap object. There is no RC on the callable itself.
`needs_drop` is true iff the receiver is non-null.

### 6. Where they may live

| Place | v0 |
|-------|----|
| Argument / return | **yes** |
| Local `let` / `mut` | **yes** |
| Module `let` / `mut` | **yes** — process state; assign drops the previous receiver |
| Constructor-param field | **no** — D48 allowlist unchanged |
| C-layout body field | **no** |
| `Array<…>` element | **no** |
| Generic product field (`Box<T>` when `T` is a function type) | **no** |

Storing on a **module** binding is allowed on purpose (`install(handler)`).
That is not the object graph: module → pair → instance is a tree. The cycle
`obj → field → bound method → obj` has nowhere to sit because fields cannot
hold a function type.

This is a **type error**, not a lint / warning.

### 7. Operations that do not exist (v0)

- `==` / `!=` on function types
- Interpolating / `toString` of a function value
- JSX `{e}` child or attr of function type
- Comparing / hashing

### 8. Relation to other decisions

| Decision | Relation |
|----------|----------|
| **D48** | **Unchanged.** Function types are not added to the ctor-field allowlist. |
| **D131** | Multi-clause **values** remain out of v0. |
| **D122** | Module `mut` of a bound method follows module-heap RC (overwrite drops the old receiver). |
| **start-tasks** | `-->` still requires a named free fn; not a pointer. |
| Traits | **Not this freeze.** `serve(app)` as a trait object / `impl App` is a separate design. |

---

## Explicit non-goals (v0)

- Lexical closures, nested fns, env records, cycle collector / ORC  
- Oracle `Fn(…)` spelling or handbook claim of “first-class functions”  
- Bound methods that capture anything except `self`  
- Function types on fields / in arrays  
- Multi-clause or `extern` as pointer values  
- `-->` of a pointer  
- Traits / protocols  
- Partial application  

---

## Implementation notes (non-normative shape)

1. **Parse:** function types in type position; `TypeName.method` and
   `value.method` without `(` in expr position.  
2. **Ty:** e.g. `Ty::Fn { params, ret }` (names kept for hover only).  
3. **Lower:** pair of places or a small aggregate; call is indirect.  
4. **cgen:** C function pointer + nullable `void *` / typed self.  
5. **LSP:** hover on a function value shows the function type; outline
   unchanged.  
6. **Conformance:** gate `fn_pointers` — free fn pass/call; bound method
   reads instance fields; unbound `Type.method` needs the receiver as first
   arg; module `mut` install/overwrite; field / array store is an **error**;
   nested fn is an **error**.

Dogfood after ship: `examples/http` `server.serve(port, handle)` — drop the
`App` type import if the host takes a function. Traits, if any, come later.

---

## Acceptance

**Design acceptance (this freeze):**

- [x] Locks recorded (this document + D132)  
- [x] Capability row **spec’d**  
- [x] `CURRENT.md` lists D132 as the open language implement slice  

**Code acceptance:**

- [x] `cargo make test` green with `fn_pointers`  
- [x] Crema dogfood (`serve(port, handle)`)  
- [x] Handbook updated (only when shipped)

---

## Changelog

| Date | Note |
|------|------|
| 2026-08-13 | Initial freeze: function types `(a: T) -> R`; free + unbound + bound; module store yes; fields no; no closures |
| 2026-08-13 | **Shipped:** `Ty::Fn` + unboxed `{recv, code}`; gate `fn_pointers`; Crema `serve(port, handle)` |
