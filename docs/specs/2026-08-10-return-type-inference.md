# Freeze: Function return-type inference (D129)

**Status:** shipped (v0 — free functions; methods still default `Void`)  
**Decision:** D129  
**Gate:** `return_infer`  
**As of:** 2026-08-11

## Intent

Unannotated **free functions** get a return type from their body exits when
inference succeeds. An explicit `-> T` remains the authoritative contract and
is checked against the body (lint / type error when the body does not match).
This replaces the old default “missing `->` ⇒ `Void`” for free fns (except
`main` ⇒ `Int64`). **Unannotated methods** still default to `Void` in v0.

## Rules (v0)

### 1. Annotation wins

| Source | Behavior |
|--------|----------|
| `fn f() -> T { … }` | Use `T`; body must match |
| `fn main()` unannotated | Still **`Int64`** (process status) |
| `fn f()` unannotated | Infer, or error if intractable |

### 2. Exit set

Collect every way a value leaves the function:

| Exit | Contribution |
|------|----------------|
| `return e` | type of `e` |
| bare `return` | `Void` |
| block tail | type of tail (`Never` from diverging `loop` adds no value exit) |
| `e?` residual path | **collapsed residual-marked type** of `e` (below) |

**Combine:** unify / form one return type from contributions. If that fails →
diagnostic requiring `-> …`.

### 3. Residual `?` collapse (structural, not name-based)

`Trial` / `May` are ordinary user generics (`std/Fallible`). The compiler does
**not** special-case those names.

Given `e : U` where `U` is residual-marked:

1. Residual extract type `F` from `U`’s residual arm.
2. If `U` is a monomorph of a **generic residual union** `G<…>`:
   - Type parameters that appear under success/`!` marks → **`Void`** for this
     exit contribution.
   - Type parameters that appear under residual/`?` marks → **`F`**.
   - Contribution = monomorph `G` with those args (e.g. operand
     `Trial<Socket, String>` contributes `Trial<Void, String>`).
3. If `U` is a non-generic residual union → contribution is **`U`** as a whole
   (refine later if needed).

Callee return types already materialize monomorphs; analysis does not require
the local module to `import { Trial }` to *infer* a Trial monomorph that
appears on an expression. Writing `Trial` / `Pass` / `Fail` in source still
needs those names in scope.

### 4. Refining success from value exits

Collapsed residual contributions use `Void` on the success side only as a
placeholder. Non-residual value exits refine that slot:

- residuals → `Trial<Void, String>`
- value exits `Int64` and `String`
- combined → `Trial<Int64 | String, String>` when that sum is formable

Exact lub/sum laws may grow (D119-style member sets). Intractable → annotate.

### 5. Bare `Pass` / `Fail` (and similar)

`Pass` and `Fail` are independent product types; many unions may use them.
Seeing only `Pass(…)` / `Fail(…)` does **not** uniquely determine `Trial`.

**v0:** unannotated function whose exits require bare `Pass`/`Fail` without an
expected residual union → **error: annotate `-> …`**.

Annotated `-> Trial<…>` (or other residual union) still selects monomorphs as
today.

### 6. Recursion (minimal)

If the body depends on this function’s own return type (self-call) or on a
cycle of unannotated functions (mutual recursion) → **error: annotate `-> …`**.
No fixed-point solver in v0.

### 7. Hover / UX

- Explicit `-> T` → show `T`.
- Inferred → show the inferred type (compiler + LSP: last successful lower
  on open/change caches free-fn and `Type.method` returns for hover).
- `main` unannotated → `Int64`.
- If lower fails, hover falls back to `Void` / `main`→`Int64`.

## Non-goals (v0)

- Full Hindley–Milner / polymorphic let generalization
- Inferring union *names* from constructors alone (`Pass`/`Fail` → `Trial`)
- Recursive return inference without annotation

## Worked example

```thx
fn run() {                    ## inferred Trial<Void, String>
    let server = net.createSocket()?
    server.reuseAddr()?
    server.bind(port)?
    server.listen(16)?
    loop { … }                ## Never (D128)
}
```

Each `?` residual collapses to `Trial<Void, String>`; no success value →
inferred return `Trial<Void, String>`.

```thx
fn half(x: Int64) {           ## error: annotate
    if x % 2 == 0 { Pass(x / 2) } else { Fail("odd") }
}
```

## Implementation notes

- Infer **free-function** return types after signature tables exist and before
  body MIR lower, so callees with known/`->` types are available.
- Update `FnSig.ret` with the inferred type; body lower uses that `fn_ret`
  (existing `?` rewrap / D128). Methods: unannotated still `Void` until a
  follow-up.
- Ensure collapsed monomorphs exist (seed / monomorphize on demand).
- LSP: `check_and_returns_with_overlays` caches free-fn / `Type.method` displays
  on publish; hover uses them (local + remote).

## Related

- D118 / D123 — sum types + Fallible import  
- D128 — diverging `loop`  
- D42 — Pass/Fail expect typing (expression-level; not return inference from bare ctors)  
- D63 — early `return`  
- Freeze: [`2026-08-07-sum-types-and-fallible.md`](2026-08-07-sum-types-and-fallible.md)
