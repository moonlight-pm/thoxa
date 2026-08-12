# Experiment: router clause syntax (type-body `GET` / `PUT` …)

**Status:** **active experiment** — branch `experiment/router-clause-syntax` only.  
**Date:** 2026-08-12  
**Not** a freeze, **not** CURRENT product work, **not** handbook / capabilities
“shipped.”  
**Toss:** delete the branch if the tact fails.

---

## Goal

Thin desugar for route-looking clauses inside a type body, with **minimal
compiler knowledge of `std/`**:

```thx
type MyRouter {
    GET "/home" {
        "Home"
    }

    PUT "/user/:userId" (userId: String) -> String {
        "user {userId}"
    }
}

let router = MyRouter()
let a = router.route(method: "GET", path: "/home")  ## a: May<String> (LSP)
```

Compiler (as built):

1. Parse route clauses in a **type body** only.  
2. Lower each clause to a **module free-function** handler.  
3. Emit **`route(method: String, path: String) -> May<String>`** on the type
   (ordered if-ladder = logical table; miss is `Not()`).  
4. Resolve/LSP desugars the same way so hover/outline see generated symbols.

HTTP types stay userland. No lang-items.

---

## Non-goals (this branch)

- General macros / attribute macros  
- Compiler-hardcoded `Request<T>` / `Response<T>`  
- Lang-items for path matching  
- Shipping handbook / capabilities / product `examples/http` rewrite  
- First-class function values for a runtime route table  

---

## As-built desugar (v0)

```thx
type MyRouter {
    export fn route(method: String, path: String) -> May<String> {
        ## ordered if method == … { if path match { return Has(__MyRouter_route_N(…)) } }
        Not()
    }
}

fn __MyRouter_route_0() -> String { "Home" }
fn __MyRouter_route_1(userId: String) -> String { "user {userId}" }
```

Module must `import { Has, Not, May } from "std/Fallible"` (no ambient inject).

| Choice | Why |
|--------|-----|
| Free-fn handlers, not private methods | Private methods cannot be called via `Type().m()` from another method |
| No Array table field | Ladder is the table (`emit-c` shows it) |
| Exact path or **trailing** `:capture` only | Capture formal must be `String` |
| `route` returns **`May<String>`** | Hit `Has(…)`; miss `Not()` — handlers still `String` |
| Nullary product types only | No ctor params / body fields on the router type |

**Hooks:** `mir::lower_package_with` and `resolver::resolve_module` both run
`route_desugar::desugar_module` (idempotent once `routes` cleared).

**Editor:** tree-sitter colors HTTP verbs as keywords; LSP hover shows
`route` / handlers and `let a: May<String>` when ty_hint is known.

---

## Inspecting generated code

```bash
cargo run -p thoxa -- run scratch/router_clause.thx   # local; scratch/ gitignored
cargo run -p thoxa -- emit-c scratch/router_clause.thx
# → target/thoxa/c/router_clause.c
```

---

## Steps

| Step | Status | Notes |
|------|--------|--------|
| 0 Branch `experiment/router-clause-syntax` | **done** | same worktree |
| 1 Idea note | **done** | this file |
| 2 Parse + AST | **done** | `RouteClause` on `TypeDef` |
| 3 Desugar → handlers + `route` | **done** | `route_desugar.rs` |
| 4 Scratch + `emit-c` | **done** | `scratch/router_clause.thx` (gitignored) |
| 5 LSP hover/outline for desugared symbols | **done** | resolve desugar + synthetic hover |
| 6 Typed local hover (`let a: May<String>`) | **done** | general LSP; helps this sample |
| 7 Typed miss (`May<String>`) | **done** | `Has` / `Not`; user imports Fallible |
| 8 Path-match helper in std | **pending** | only if Bytes ladder hurts |
| 9 Decision: keep / reshape / toss | **open** | not a freeze yet |

---

## Surface (parse v0)

```text
METHOD string_lit [ ( params ) ] [ -> type ] block
```

- `METHOD` ∈ `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS` (idents in
  type body only; also tree-sitter keywords).  
- Formals: normal Thoxa idents (**no `_`** — use camelCase).  
- Mix with `fn` methods / fields: parse OK; desugar rejects fields/ctor params.

---

## Open questions / next experiment slices

1. `Int64` captures / multi-segment `:id`  
2. Optional request formal (still userland types)  
3. Crema dogfood one route (example-only)  
4. Promote freeze only if keep  

---

## Commits on this branch (checkpoint)

- `dbf2a19` parse + idea note  
- `9df5760` desugar  
- `e344190` LSP resolve desugar + tree-sitter HTTP keywords  
- `8f857a7` let hover shows inferred type  
- `509675c` docs checkpoint (no product claim)  
- *(this slice)* typed miss `May<String>`  

Base: master at FCF park note (`969b6d7` lineage).

---

## Pickup

Branch + this file + `docs/ideas/README.md` row. Do **not** promote to
`docs/specs/` or capabilities until a keep decision.
