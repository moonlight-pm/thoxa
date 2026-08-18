# Experiment: built-in validation (types as inhabitation)

**Status:** **experiment branch** `experiment/builtin-validation` — design
open, not on master, not a freeze, not handbook. No D-id yet.  
**Date:** 2026-08-17  
**Sparked by:** Crema `POST /api/orders` (`examples/http/app/api.thx`) — shape
and refinements restated as match piles; D141 tree codec is not the validator.  
**Pickup:** this file · branch `experiment/builtin-validation` · root
[`CURRENT.md`](../../CURRENT.md)  
**Related:** [`2026-08-15-json-codec.md`](2026-08-15-json-codec.md) (tree
`decode`/`encode` stays); JSX / interp closed tables (same “compiler can see
the type”); [`2026-08-15-tostring-repr-and-traits.md`](2026-08-15-tostring-repr-and-traits.md)
(do **not** pull traits).

---

## Goal

Validation is a **language / type** feature, not a `std/json` helper pack.

A type is a predicate on values. JSON `read`, form parse, and in-language
construction are introduction forms of the same type. Constraints are
refinements of existing types (`Int64`, `String`, products, literal unions).
Error text lives on the refinement when you write it; otherwise the compiler
writes a generic one.

Crema’s order body should look like:

```thx
type Qty = Int64 where >= 1
type CreateOrder(slug: String, qty: Qty)

export fn createOrder(req!: Request) -> Response {
    match CreateOrder.read(req.body.toString()) {
        Fail(e) => jsonErr(e)
        Pass(o) => jsonOk(ticketOf(o))
    }
}
```

---

## Locked (do not re-litigate on this branch)

1. **Type-first.** Define the input type; `read` / `from` inhabit it. No Zod
   schema values, no combinator library, no traits.
2. **Refinements are real types.** `Qty` is not `Int64`. `CreateOrder(qty: 0)`
   is a **type error**. Escape hatch is explicit and fallible (`Qty.from(n)`).
3. **Compile time when the value is known.** Literal `0` against
   `Int64 where >= 1` is a compiler diagnostic. Runtime only when the value
   is not a constant (`Qty.from(n)`, `CreateOrder.read(s)`).
   **Empty refinements are compile errors** — `Qty where <= 0` when `Qty`
   is `>= 1` has no inhabitants.
4. **Messages are optional.** Omitted → generated
   (`"expected integer >= 1, got 0"` / `"slug: expected string, got integer"`).
   Written → that string is the fail payload’s `message`. Unknown situations
   always have a generic fallback. Never require a message.
5. **Closed constraint catalog v0.** `min`/`max` as compares; inclusive
   ranges; length of `String` / `Array` via the same range syntax (interpreted
   by the base type). Literal unions already exist. `May<T>` = missing or null.
   No regex, no email, no predicate closures.
6. **Fail-fast `Issue`.** Path + generated-or-written message. Collect-all is
   a later operator.
7. **Tree `decode`/`encode` stay.** Syntax errors remain `Trial<Json, String>`.
   Shape + refinement are `read` / `from` → `Trial<T, Issue>`.
8. **No `jsonResponse` in `std/json`.** HTTP wrap stays in the example.
9. **No traits.** Closed compiler table, same family as `"{x}"` and JSX `{e}`.

---

## Syntax — locked: `where` + `&` / `|` / `(…)`

`where` **introduces** the validation clause. Inside that clause, `&` is
and, `|` is or, parens scope. Outside `where`, `|` is still a type union.

Range / compare meaning is **by base type**: on `Int64` / `Float64` a range
is the value; on `String` / `Array` it is `.count`.

Optional message is a **template** after that conjunct, not a live
`"{n}"` (there is no value at the `type` line). Holes are a closed set:

| Hole | Fills with |
|------|------------|
| `{value}` | the value, same stringify as `"{x}"` |
| `{expected}` | generated description of **this** conjunct (`integer >= 1`) |
| `{path}` | dotted path (`qty`, `items.0.slug`); empty at a bare alias |

Unknown holes are a compile error. Omitted message → generated text, which
already includes value + expected. Same template is used for a compile-time
literal fail and for `from` / `read`.

Narrowing a named type is the same clause on the left-hand type:

```thx
type Qty   = Int64 where >= 1
type Dozen = Qty   where <= 12        ## Int64 where >= 1 & <= 12
type Nope  = Qty   where <= 0         ## compile error: empty
```

The compiler intersects the inherited clause with the new one. Empty
intersection → diagnostic, not a type nobody can inhabit.

### Several rules on one type

```thx
type Qty     = Int64 where >= 1 & <= 12
type Percent = Int64 where 0..100
type Port    = Int64 where (1..1023) | (8080..8090)
type Slug    = String where 1..64 & != "admin"
type Id      = (Int64 where >= 1) | (String where 1..64)
```

`|` inside `where` is or-of-constraints. `|` outside `where` is a type union
(`Id`). Parens required when mixing `&` and `|` , or when a union of refined
types would otherwise eat the clause.

### Messages (per conjunct, optional)

```thx
type Qty = Int64 where >= 1 "must be at least 1" & <= 12 "at most a dozen"

type Qty = Int64 where >= 1 "need {expected}, got {value}"
                 & <= 12 "at most a dozen, got {value}"

type CreateOrder(
    slug: String where 1..64 "{path} {value} is not 1–64 characters",
    qty:  Int64  where >= 1  "{path} must be at least 1, got {value}"
)
```

### Narrowing a type you already have

```thx
type Qty    = Int64 where >= 1
type Dozen  = Qty   where <= 12
type Shot   = Qty   where <= 4
type Mid    = Dozen where >= 5          ## Int64 where >= 5 & <= 12
```

```thx
type DrinkSlug = String where "espresso" | "mocha" | "latte"
type HotSlug   = DrinkSlug where "espresso" | "mocha"   ## ok, subset
```

### Empty — compile error

```thx
type Qty  = Int64 where >= 1
type Nope = Qty   where <= 0            ## >= 1 & <= 0
type Gap  = Int64 where 1..10
type Miss = Gap   where 11..20          ## disjoint ranges
type Hot  = DrinkSlug where "cold-brew" ## not a member
```

### A product is several field clauses

```thx
type CreateOrder(
    slug: String where 1..64,
    qty:  Int64  where >= 1 & <= 12
)

type SmallOrder(
    slug: String where 1..64,
    qty:  Qty    where <= 2            ## Qty already >= 1
)
```

Cross-field `where` on the product itself is **not** v0.

---

## Also locked (session 2026-08-17)

10. **`Qty` → base is implicit.** `o.qty * price` works. Reverse is not
    (`Qty.from` / a literal that inhabits).
11. **Refinement subtyping.** `Dozen` inhabits `Qty` when it is strictly
    tighter on the same base. Not the other way.
12. **`T.from` / `T.read` are compiler-attached.** No traits, no
    `fn read<T>`. `Issue` lives in `std/json` for v0 (JSON `read` is the
    first customer).
13. **Type args v0:** write `May<Qty>`, not `May<Int64 where >= 1>`.
14. **Literal inhabitation:** `let a: Qty = 2` is ok when `2` satisfies.

---

## Open

- Cross-field `where` on a product (`CreateOrder where qty <= …`) — not v0.

---

## Inhabitation (all spellings)

```thx
let a: Qty = 2          ## literal inhabits → compile ok
let b: Qty = 0          ## literal fails → compile error (generated message)
let c: Qty = n          ## n: Int64 → type error; not implicit
let d = Qty.from(n)?    ## runtime Trial<Qty, Issue>
let o = CreateOrder.read(body)?   ## syntax + shape + refinements
```

`CreateOrder(slug: "mocha", qty: 0)` does not compile (`0` is not `Qty`).
`CreateOrder(slug: "mocha", qty: 2)` does (`2` inhabits `Qty`).

---

## `Issue` (v0 product — sum later)

```thx
type Issue(path: String, expected: String, got: String, message: String)
```

Factories: `issueMissing` / `issueMismatch` / `issueRejected`. The designed
sum (`Missing | Mismatch | Rejected`) waits on residual-union RC (`?` rewrap
double-frees). Path is dotted. `message` is generated or written.

---

## Non-goals (this experiment)

- Traits / `impl Validate`
- Schema values / combinators / closures
- Collect-all errors
- Regex, email, custom predicates
- Route-clause JSON binding (call `T.read` from the handler)
- Changing tree `decode`’s `Fail(String)`
- Handbook / D-id until the surface sticks

---

## Slices

1. **`Issue` + `Type.read` of plain products** — **done.** Gate `json_read`.
2. **`where` refinements + `T.from` + compile-time literals** — **done.**
   Gate `where_refine`. `type Qty = Int64 where >= 1`. `let q: Qty = 0` is a
   compile error. `Qty.from(n)` runtime. Qty → Int64 implicit. Empty
   `Qty where <= 0` is a compile error. Crema `CreateOrder.qty: Qty`.
   Custom `{value}` templates parsed; runtime still uses generated text.
3. Encode of products (Ticket 200 body).
4. Literal-union slugs if we want unknown-drink as a type error.
5. Restore `Issue` as `Missing | Mismatch | Rejected` when residual-sum RC is
   fixed. May fields. Field-level `where` checks in `read` (stored, not
   applied yet — use a named refined type).
