# Sum types, unions, and general fallibility (`?` / `!`)

**Date:** 2026-08-07  
**Status:** target (accepted product intent; implemented for v0)  
**Implementation:** shipped  
**Gaps:** none for v0  

**As-built:** [`../capabilities.md`](../capabilities.md) ·
[`../architecture.md`](../architecture.md) · focus
[`../../CURRENT.md`](../../CURRENT.md)  

**Shipped surface (v0):** user unions + marks + generics; **May/Trial as
exported `std/Fallible` unions** (**import required**, **D123** — no ambient
inject); monomorphs; methods on generic products; residual `?` rewrap;
exhaustiveness names missing arms / rejects post-`else` arms; OpenMode sample
(`file_openmode`); union match guards (**D120**, `sums_match_guard`); tag-only
bare ctors + `=>` arm bodies (**D130**, `match_arm_sugar`); **no** builtin
`Ty::May`/`Ty::Trial`. Gates green under `cargo make test`.

> **For agentic workers:** Ladder and May/Trial cutover are **done**. Do not
> re-open ambient inject. Further work only when `CURRENT.md` schedules a slice.
> Assign D-ids in `docs/decisions.md` as each new step ships.

**Oracle lean:** **TypeScript** (unions, literal types, assignability) for
typespace; native product face stays Thoxa. Old Python SPEC M2 is historical
reference only where it agrees.

**Related:** D3 (May/Trial error model — refined, not replaced by effects),
D4 (hybrid values / small sums), D2 (effects deferred; may subsume some
fallibility later), D81 (OpenMode ADT deferred here), **D118–D120** (sums /
guards), **D123** (Fallible import, no ambient), **D130** (tag-only / `=>`).

---

## 0. Product thesis

Thoxa should have **one** sum/union mechanism. Domain tags (`Key`, `Shape`),
literal unions (`0 | 1`), and fallibility (`May`, `Trial` from `std/Fallible`)
are the **same** feature. `?` is not hardwired to two magic types; it is
defined by **marks on the union** that name what to extract on success and
what to early-return on residual.

**One sentence:**  
Unions are TypeScript-shaped; `!` / `?` on types mark extract and residual for
a general `?` operator; May/Trial become ordinary `std/Fallible` unions.

---

## 1. Typespace vs representation

- **Types do not occupy memory.** Literal type `1`, union aliases, etc. are
  constraints and names.
- **Bindings and fields have representation** when live (registers, slots,
  heap). Policy for programmers who care about size:
  - Prefer const-fold / no slot when possible.
  - If a slot is required for a **pure literal** (or pure-literal union):
    **narrowest scalar that holds every member**, chain starting at
    **`Byte` → … → `Int32` → `Int64`** (existing int widths in order).
  - Widen at use sites that require a wider type (existing coerce story).
- **Pure-literal unions** (e.g. `0 | 1`): one scalar cell, no tag.
- **Any other union** (named types, mixed literal+structured, nullary tags):
  **tagged sum** — tag + optional payload; nullary arm = tag only.
- Backend “inline small sums” is an optimization detail under this policy, not
  a typespace rule.

---

## 2. Declaring types and unions

### 2.1 Nullary / named types

```thx
type Up              ## valid alone — nullary tag type (≈ unique brand)
type Circle(radius: Int64)
type Has<T>(value: T)   ## generic product (field form as product types today)
```

- No required `{}` body for empty types.
- **Construct:** always call form — nullary **`Up()`** (bare `Up` is not an
  instance).
- **Pattern:** bare **`Up`** allowed for nullary; payload arms use
  named and/or **positional** bindings (same rules as functions).

### 2.2 Bare name in expression position

- Today / until “types are values”: bare `Up` in **expr** → **error**
  (“not a value; use `Up()`”).
- Reserved so later `Up` may denote a **type-value**; do not repurpose bare
  names as nullary instances.

### 2.3 Nominal atoms · transparent union aliases

**Locked product rule (D119):**

> **Every standalone `type Name` / product is a unique nominal atom.**  
> **`type Alias = A | B | …` is a transparent name for that set of atoms.**  
> **Union equality is by member set** (order irrelevant).  
> **Inference yields that set** — it does **not** search for an alias name.

| Kind | Example | Identity |
|------|---------|----------|
| **Atom (generative)** | `type Foo` · `type Bar` · `type Done(n: Int64)` | Distinct by **declaration**. Same empty shape does **not** unify `Foo` and `Bar`. |
| **Alias (transparent)** | `type Outcome = Done \| Quit` | **Not** a new brand. Same type as `Done \| Quit` and as any other alias with the same members. |
| **Union as set** | Inferred or written `Done \| Quit` | Equal iff same members (`Quit \| Done` ≡ `Done \| Quit`). |

```thx
type Foo
type Bar
## Foo ≠ Bar (nominal atoms)

type Done(n: Int64)
type Quit
type Outcome = Done | Quit
type Result = Done | Quit
## Outcome ≡ Result ≡ Done | Quit  (transparent aliases / set equality)

fn step(ok: Bool) {
    if ok { Done(n: 3) } else { Quit() }
}
## unannotated infer: Done | Quit  (not “look up Outcome”)
```

- True type of a constructed value remains the **atom** (`Done`, `Quit`, `Foo`).
- Use where a union is expected: **member of the set** (widen / inject), not
  “atom became another atom.”
- Expanding aliases when comparing is how the checker implements transparency;
  no global “find a named type equal to this sum” search.

**Optional later (not required):** opaque / newtype wrapper around a union when
a **nominal** sum brand is needed (`WireOutcome` ≠ `Done | Quit` without wrap).

### 2.4 Union aliases (syntax)

```thx
type Shape = Circle | Rect
type Key = Press | Up | Down
type Flag = 0 | 1
type Slot = 0 | Circle          ## mixed OK
type May<T> = Has<T> | Not
```

- **`|` in type position** joins members (expr `|` remains BitOr / other uses).
- Members may be: **named types**, **other union aliases**, **builtins /
  primitives**, **literals** (literal types), and **structured** products.
- **Open (TS-like), not** closed enum-only ADTs — though all-named-tag unions
  are the common ADT style.
- **Partial / curry types** (`type X = Point(x: 0)`) — **parked** until forced.

### 2.5 Type application vs construct

| Syntax | Position | Meaning |
|--------|----------|---------|
| `Has<T>` | type | Generic type applied to `T` |
| `Has(…)` | value | Construct / destructure instance |
| `Pass<P>` | type | Generic success arm type |
| `Pass(x)` / `Pass(value: x)` | value | Construct (function call rules) |

Unions list **types** → prefer **`Has<T>`**, not `Has(T)`, in aliases.

### 2.6 Constructor call rules

Same as **functions**: positional and named args, same defaults / `!` formal
rules as the rest of the language. Not a special sum dialect.

### 2.7 Assignability (true type + expect)

- A value’s **true type** is its constructor / atom (e.g. `Circle`, `Done`).
- It is assignable where a **union** (alias or written `A | B`) is expected if
  that atom is a **member of the union’s set** (widen / inject).
- **`Outcome` and `Done | Quit`** compare equal when aliases are transparent
  (§2.3); no extra cast between them.
- Reverse (union → atom) requires **match** (or later narrowing); no free
  downcast.
- **`if` / control-flow with expect:** when the outside provides an expected
  type (`-> Outcome`, `let x: Outcome = if …`), each branch is checked against
  that expect (inject allowed). Unannotated `if` arms: unify true types to a
  member-set union (`Done | Quit`) or require agreement — do **not** search for
  a named alias.

---

## 3. Fallibility marks: `!` and `?` on types

Marks identify **what is extracted**, not only “which arm exists.”

| Mark | Role |
|------|------|
| **`!`** | **Success extract** — type of the value after a successful value-level `?` |
| **`?`** | **Residual extract** — type early-returned on the residual path |

### 3.1 Placement

```thx
## Usual May / Trial ergonomics (unwrap payloads)
type May<T> = Has<T!> | Not?
type Trial<P, F> = Pass<P!> | Fail<F?>

## Whole-arm extract (if desired)
type Trial<P, F> = Pass<P>! | Fail<F>?
```

| Syntax | Extract |
|--------|---------|
| `Has<T!>` | success → **`T`** |
| `Has<T>!` | success → **`Has<T>`** |
| `Fail<F?>` | residual → **`F`** |
| `Fail<F>?` | residual → **`Fail<F>`** |
| `Not?` | residual nullary → unit / that arm |

- At most **one** residual extract (`?`) and at most **one** success extract
  (`!`) in a given union definition (tighten in impl if multi-`!` needed later).
- Value-level postfix `?` remains the operator; type-level `?` / `!` appear in
  **type definitions** (and type apps inside them).

### 3.2 Glyphs

- **`!`** = success extract (accepted for freeze; also used on formals as
  `name!: Type` — different position).
- **`?`** = residual extract / fallibility family.

### 3.3 Value-level `x?`

Given `x` of a union type that defines residual/success extracts:

1. If residual arm → **return** the residual extract value from the enclosing
   function (enclosing return type must accept it; rewrap rules at impl time).
2. Else → yield the **success extract** type as above.

**Who may use `?`:** only types that define a residual mark.  
**Non-residual type:** error.

### 3.4 Multi non-residual arms (no single unwrap)

```thx
type Multi = Foo | Bar | Baz?    ## residual Baz; success Foo | Bar
```

Success type of `m?` is **`Foo | Bar`** (union of non-residual members).  
If a single success arm is marked for payload extract (`Pass<P!>`), unwrap to
**`P`**. That is the W2/U2 combination:

| Success shape | `?` yields |
|---------------|------------|
| One arm, extract on payload type arg (`T!`) | **`T`** |
| One arm, extract on whole arm (`Has<T>!`) | **`Has<T>`** |
| Several non-residual arms | **union of those arms** |
| Nullary success extract | **Void** / unit |

### 3.5 May / Trial

- **Not compiler builtins** — **`std/Fallible`** unions using this surface
  (**import required**, D123; session Shell inject still loads Fallible for
  the product Shell API).
- Special only as std + marks; same IR as user unions.
- **Full cutover** of conformance and samples landed with D118 then D123.
- **Effects (D2)** may later subsume or replace parts of this; freeze does not
  depend on effects.

---

## 4. Match

- Constructor patterns: bare name = **tag-only** (nullary **or** discard payload
  without destructure — **D130**); payload binds **positional** (function rules)
  via `Name(x)` / `Name(_)`.
- Arm body: **`=> expr`** (single expression) or **`{ … }`** (block); optional
  `if` guard before either form (**D130** / D111 / D120).
- **Exhaustiveness:** `else` required unless every member of the scrutinee
  union is covered (**E1**).
- Residual/`!` marks do not change pattern syntax.
- Value-literal match (existing D87+) remains for non-union / literal scrutinees;
  unify under one `match` expression in the language.

---

## 5. Explicit non-goals (this freeze)

- Partial / curry types (`Point(x: 0)` as a type)
- Redefining May/Trial before user unions exist in code (order is ladder §10)
- Effects integration
- Full dependent types beyond literal types + marks
- **Structural products** (two products with the same fields are still
  different types — atoms are nominal)
- Opaque/newtype union brands (optional later; default aliases stay transparent)

---

## 6. Representation summary

| Union kind | Runtime sketch |
|------------|----------------|
| Pure literals only | Narrow scalar (Byte-up chain fits all members) |
| Otherwise | Tagged: tag + payload; nullary = tag only |
| Active arm RC | Dup/drop only live arm (generalize Trial/May RC) |

---

## 7. Worked Fallible sketch (shipped shape)

Product source: **`std/Fallible.thx`** (import required — D123).

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"

## Defined in std/Fallible (exported):
## type Has<T>(value: T)
## type Not
## type May<T> = Has<T!> | Not?
## type Pass<T>(value: T)
## type Fail<E>(error: E)
## type Trial<P, F> = Pass<P!> | Fail<F?>

fn half(x: Int64) -> Trial<Int64, String> {
    if x % 2 == 0 { Pass(x / 2) } else { Fail("odd") }
}

fn demo() -> Trial<Int64, String> {
    let n = half(10)?    ## n: Int64; Fail → return String residual as F?
    Pass(n)
}
```

Exact formal field names (`value` / `error`) are product-chosen; marks and
union shape are the lock.

---

## 8. Interaction with product (historical → shipped)

| Was | Shipped |
|-----|---------|
| Builtin `May` / `Trial` / `?` | `std/Fallible` unions + general `?` (import, D123) |
| Value match literals only | + constructor match on unions |
| OpenMode Int32 lets (D81) | OpenMode as nullary union sample (`file_openmode`) |
| Nested Trial payloads (D114) | Layout via general sum monomorphs |

---

## 9. Decisions minted when shipping (record)

| Topic | D-id |
|-------|------|
| User unions + marks + general `?` + May/Trial as generics | **D118** |
| Union identity (nominal atoms · transparent aliases) | **D119** |
| Union-match guards | **D120** |
| May/Trial not ambient — import from `std/Fallible` | **D123** |

---

## 10. Implementation ladder (when CURRENT says go)

1. **Parser / typespace:** `type A`, `type U = A | B`, literals in type position,
   `Has<T>` in aliases; bare name expr error reserved.
2. **Tagged sums + nullary + product arms;** construct/match; widen into union.
3. **Pure-literal unions** + Byte-up storage policy.
4. **Marks `!` / `?` on types** + value `?` semantics (W2/U2).
5. **Transparent aliases / member-set equality** (`Outcome` ≡ `Done | Quit`);
   expect-type on `if` / annotated lets; unannotated infer set union.
6. **Prelude May/Trial** rewrite; delete builtin special cases; full test cutover.
7. **Polish:** OpenMode-as-union samples, exhaustiveness quality, generics edge
   cases.

Each step: conformance + progress docs + D-id as appropriate.

---

## 11. Acceptance (design)

This freeze is **accepted product intent** when:

- [x] Brainstorm locks recorded (this document)
- [x] `CURRENT.md` points here for sum/fallible target
- [x] Capability row(s) set to **spec’d** (not shipped); D118 intent logged
- [x] Implementation only under CURRENT priority (not auto-started)

**Code acceptance** is later: ladder steps green under `cargo make test`.

---

## 12. Changelog (design only)

| Date | Note |
|------|------|
| 2026-08-07 | Initial freeze from design brainstorm (TS oracle, marks, layout, prelude fallibility) |
| 2026-08-07 | Partial impl: nullary + one-field scalar unions, inject, match (`sums_nullary`, `sums_payload`) |
| 2026-08-07 | Multi-field product patterns (`sums_product`); pattern binds are a field-order list |
| 2026-08-07 | Pure-literal union members (`0 \| 1`, `"r" \| "w"`, bools); inject + lit patterns (`sums_literal`) |
| 2026-08-07 | Residual arm mark + general value `?` with W2 unwrap (`sums_try`) |
| 2026-08-08 | **Nominal atoms · transparent union aliases** locked (§2.3); infer set union, no alias search |
| 2026-08-08 | Member-set equality + expect on `if` / alias remap (`sums_alias`) |
| 2026-08-08 | Unannotated if set-union lub via synthetic `$union$…` (`sums_if_lub`) |
| 2026-08-08 | Monomorphic type-arg extract `Hold<Int64!>` vs whole-arm `Hold!` (`sums_extract_mark`) |
| 2026-08-08 | Heap String/Bytes payloads in sum members + tag-aware sum RC (`sums_heap`) |
| 2026-08-08 | Nested union aliases flatten; subset inject into outer (`sums_nested`) |
| 2026-08-08 | Pure-literal unions as narrow scalar (Byte-up typedef; `sums_literal`) |
| 2026-08-08 | Generic products/unions + monomorphization (`sums_generic`) |
| 2026-08-08 | May/Trial prelude cutover (ambient Fallible generics; full conformance green) |
| 2026-08-08 | Dead `Ty::May`/`Trial` removed; methods on generic products (`sums_generic_methods`) |
| 2026-08-08 | Exhaustiveness names missing arms; OpenMode union sample (`file_openmode`) |
| 2026-08-09 | May/Trial **import required** from `std/Fallible` (D123; no ambient inject) |
