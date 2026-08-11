# Freeze: Multi-clause functions (type-driven match sugar)

**Status:** spec’d (v0 — not implemented)  
**Decision:** D131  
**Gate:** `multi_clause` (plus typespace coverage under existing `sums_*` as needed)  
**As of:** 2026-08-11  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D131 in
[`decisions.md`](../decisions.md)

---

## Intent

Several `fn` definitions with the **same name** in one scope are **one
function**: definition sugar for an ordered `match` on formals whose types
differ. Formals stay **type annotations** (`name: Type`) — not pattern
formals. Dispatch types may be **any** types, including **singleton literal
types** (`"home"`, `0`, `true`).

```thx
type Request

type Router {
    fn get(path: "home", req: Request) {
        ## path refined to "home"
    }

    fn get(path: "admin", req: Request) {
        ## path refined to "admin"
    }
}
```

Conceptual desugar (illustrative):

```thx
fn get(path: "home" | "admin", req: Request) {
    match path {
        "home" => { … }
        "admin" => { … }
    }
}
```

One runtime entry; call sites and typing behave like that single function.
Not C++/Java overload rank; not first-class multi-clause values.

---

## Locks (v0)

### 1. Scopes and names

| Scope | Merge key |
|-------|-----------|
| **Module** (free functions) | module + function name |
| **Type body** (methods) | type + method name |

Free functions are **module-scoped members** (the old “import module as
instance” idea is conceptual only — this freeze does not change import
surface). Methods merge only within their type.

- **Non-adjacent** clauses allowed; order = **source order** of clause
  definitions in that scope.
- **Equal arity** across clauses (cross-arity is out of v0).
- **`extern` multi-clause:** out of v0 (no bodies to sugar).

### 2. Formal shape (A-core)

- Formals are always **`name` / `name: Type` / `!` / defaults grammar as today**.
- **No pattern formals** (`fn get("home", …)`). Destructuring in the param
  list is a possible later add-on; type annotation remains core.
- **Defaults:** **forbidden** on multi-clause functions in v0 (avoids
  “which clause owns the default”). Single-clause fns unchanged (D98/D106/D116).
- **`!` positional:** same rules as today; must be **consistent** across
  clauses (same indices marked).

### 3. Typespace substrate (required for the sample)

Literal types already exist as **union members** (D118 / `sums_literal`).
Multi-clause needs them as **first-class types**:

1. **Bare singleton literal types** are valid types: `"home"`, `0`, `true`
   (Int/Bool/String literals as today in typespace).
2. They are valid in **all type positions**: formals, returns, `let` annotations,
   type-alias RHS, etc.
3. **Member-set unions** (`"home" | "admin"`, and general `A | B` where already
   allowed) are valid in those positions when written — not only on
   `type Name = …` RHS. (Today inline `|` in formal position does not parse;
   this freeze requires that gap closed as part of typespace work.)
4. Singleton alias `type Home = "home"` is a **transparent alias** to that
   literal type (D119 family), not a two-member union requirement.

True type of a string/int/bool **literal expression** remains as specific as
today’s inject-into-union rules; assignability into a singleton or member-set
follows D118/D119 (inject / membership), not silent widen to `String`/`Int64`
unless the expected type is that wide type.

### 4. Clause set → one function (sugar)

1. Collect all clauses with the same merge key.
2. **Source order** = match arm order (**first match wins**).
3. Desugar to **one** function:
   - Domain: for each parameter index, if all clause types at that index are
     **equivalent** (after alias normalize / member-set eq), that formal is
     **shared**; otherwise it is a **dispatch position** whose domain is the
     **member-set join** of the clause types at that index (as specific as
     possible — **do not overgeneralize** to `String` / top type).
   - Body: `match` (or equivalent decision tree) on dispatch formals in clause
     order; each clause body is the arm body under **refinement** (formals
     have that clause’s parameter types).
4. One ABI / one runtime entry. No first-class “overload set” value.

### 5. Discrimination

- **Any types** may differ across clauses (literals, nominal products, union
  arms, generics monomorphs, …).
- A parameter index is a dispatch position when clause types there are **not**
  all equivalent.
- Shared positions must agree (error if they conflict without being intended
  as dispatch — i.e. incompatible types that are not merged via join for
  dispatch; implement as: non-equivalent ⇒ dispatch join; if join is
  intractable → annotate / error).

### 6. Overlap, unreachable, duplicates (**errors**)

These **fail** `thoxa check` / compile (not soft warnings):

| Situation | Error |
|-----------|--------|
| Two clauses with **equivalent** full domain (same param types after normalize) | duplicate clause |
| Later clause **unreachable** because an earlier clause’s domain already covers it (e.g. `String` before `"home"`, or identical arm) | unreachable clause |
| General-before-specific on a dispatch position | unreachable later arm |

Align diagnostics with existing duplicate match-arm errors where practical.

### 7. Domain at calls / exhaustiveness

- Inferred domain of the merged function = join described above (**closed** to
  those members when only singletons/narrow types appear).
- Call arguments must be **assignable into** that domain (normal rules).
- There is **no** requirement that multi-clause cover `String` or other wide
  types. A catch-all is opt-in via a clause with a wide type (e.g. `path: String`).
- Exhaustiveness of the **internal** match is ensured by construction when
  the domain is exactly the join of clause domains.

### 8. Return types (one function, match-like join)

Sugar: type the merged function like a normal function whose body is a match.

| Case | Rule |
|------|------|
| No clause `->` | Free functions: **D129** on the **merged** body (join of all clause exits + residual `?` laws). Methods: still default **`Void`** in v0 unless a clause annotates (same method gap as D129). |
| One or more clause `-> T` | There is still **one** overall return type for the merged function. Each clause-level `-> T` is a **local expect** on that clause’s body (`T` must accept the clause exits). Overall return = **join** of all clause exit contributions (and any overall annotation if present), same laws as match / D129 — **returns may vary across clauses** and form a member-set union when needed. Do **not** invent a named alias. Intractable join → require a single explicit overall strategy (annotate every clause with compatible `->` or error asking for annotation). |
| `main` | Unchanged (`Int64` process status); multi-clause `main` is not a goal. |

Per-clause `->` is **not** a separate ABI per clause.

### 9. Export

- If **any** clause is `export`, **all** clauses for that name in the scope
  must be `export`.
- Mismatch → **error**.
- The merged function is exported iff clauses are.

### 10. Relation to other decisions

| Decision | Relation |
|----------|----------|
| **D16** (`Bytes.from` overload rejected) | **Unchanged.** Multi-clause is the deliberate language feature; reopening std factories is a **separate** decision after this ships. |
| **D118 / D119** | Literal types + member-set identity; this freeze extends **where** literals/`\|` may appear. |
| **D129** | Merged free-fn return inference. |
| **D130** | Internal match may use `=>` / bare tag arms like any match. |
| Pattern formals / composition of clause sets from multiple types | **Out of v0.** Non-adjacent same-scope merge is enough; cross-type composition order is future. |

---

## Explicit non-goals (v0)

- Pattern formals / destructuring in the parameter list  
- Cross-arity overloads  
- Clause composition from traits / multiple type sources (order rules TBD)  
- Guards on clauses (`fn f(x: T) if …`) — use `match` / `if` in the body  
- `extern` multi-clause  
- First-class function values / method references of multi-clause sets  
- Soft warnings for unreachable or export mismatch  
- Changing product `std/` APIs to multi-clause (D16 remains)

---

## Implementation notes (non-normative shape)

1. **Parse / resolve:** stop treating same-name `fn` as immediate duplicate;
   collect clause groups per scope.  
2. **Typespace:** resolve bare `TypeLit` formals; allow `|` in general type
   positions (or equivalent desugar to synthetic member-set types).  
3. **Lower:** one MIR function; match (or cascade) on dispatch params; RC
   same as hand-written match.  
4. **LSP:** one logical symbol; hover shows merged domain + return.  
5. **Conformance:** gate `multi_clause` (name flexible); cover free + method,
   string/int/bool literals, non-literal type split (`Pass`/`Fail` or user
   products), unreachable error, export mismatch error, closed-domain call
   reject for wide `String`, varying returns join, non-adjacent clauses.

---

## Acceptance

**Design acceptance (this freeze):**

- [x] Locks recorded (this document + D131)  
- [x] Capability row **spec’d**  
- [x] `CURRENT.md` points here as next language implement slice  

**Code acceptance (later):**

- [ ] `cargo make test` green with multi-clause conformance  
- [ ] Bare singleton + formal `|` typespace gaps closed as needed  
- [ ] Handbook updated only when **shipped**

---

## Changelog (design only)

| Date | Note |
|------|------|
| 2026-08-11 | Initial freeze from design lock (A-core type formals, any-type dispatch, match-order sugar, errors for overlap/export, match-like return join) |
