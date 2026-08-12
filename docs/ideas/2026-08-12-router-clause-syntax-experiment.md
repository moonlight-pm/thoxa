# Experiment: router clause syntax (type-body `GET` / `PUT` …)

**Status:** **active experiment** — branch `experiment/router-clause-syntax` only.  
**Date:** 2026-08-12  
**Not** a freeze, **not** CURRENT product work, **not** handbook surface.  
**Toss:** delete the branch if the tact fails.

---

## Goal

Try a **thin** desugar for route-looking clauses inside a type body, with **minimal
compiler knowledge of `std/`**:

```thx
type MyRouter {
    GET "/home" {
        "Home"
    }

    PUT "/user/:userId" (userId: Int64) {
        "user {userId}"
    }
}

let router = MyRouter()
## later: router.route(method, path) or similar
```

Compiler responsibilities (target end state of this experiment):

1. Parse route clauses in a **type body** only.  
2. Lower each clause to a **private method**.  
3. Build a **private table** (method string + pattern string + clause id) on the type.  
4. Emit a **`route` method** that walks the table and calls the private methods.

Everything else (`Request`, body codecs, HTTP server) stays **normal Thoxa** in
userland or std — not lang-items in v0 of this experiment.

---

## Non-goals (this branch)

- General macros / attribute macros  
- Compiler-hardcoded `Request<T>` / `Response<T>`  
- Lang-items for path matching  
- Shipping handbook / capabilities “shipped” / product `examples/http` rewrite  
- First-class function values for the table (ladder or simple data is enough)

---

## Target desugar (as-built v0)

```thx
type MyRouter {
    export fn route(method: String, path: String) -> String {
        ## ordered if method == … { if path match { return __MyRouter_route_N(…) } }
        ""
    }
}

fn __MyRouter_route_0() -> String { "Home" }
fn __MyRouter_route_1(userId: String) -> String { "user {userId}" }
```

Notes:

- Handlers are **module free functions** (not type methods): private methods cannot
  be called via `Type().m()` from another method.
- **No runtime table field** — the ordered if-ladder *is* the table (visible in
  `emit-c`).
- Path match: exact `path == pattern`, or trailing `:capture` via `Bytes` prefix
  compare + `slice`/`toString` (capture formal must be `String`).
- Handler / `route` return type: **String** only in v0.

---

## Inspecting generated code

```bash
cargo make emit-c path.thx
# → path under target/thoxa/c/
```

No `dump-ast` / expanded-`.thx` CLI yet. After lowering works, **`emit-c`** is the
source of truth for “what the compiler produced.” Parser step uses unit tests.

---

## Steps

| Step | Status | Notes |
|------|--------|--------|
| 0 Branch `experiment/router-clause-syntax` | **done** | same worktree |
| 1 This idea note | **done** | |
| 2 Parse + AST only | **done** | `RouteClause` on `TypeDef`; unit tests `parse_route_*` |
| 3 Desugar → handlers + `route` | **done** | `route_desugar.rs`; free-fn handlers + `route` ladder; exact + trailing `:cap` |
| 4 Scratch sample + `emit-c` | **done** (with 3) | `scratch/router_clause.thx` |
| 5 Optional path-match helper (userland/std) | pending | only if needed |

---

## Surface (parse v0)

Inside `type Name { … }` (not top-level, not free functions):

```text
METHOD string_lit [ ( params ) ] [ -> type ] block
```

- `METHOD` is an **identifier** in the fixed set:  
  `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS`  
  (not reserved keywords elsewhere — only recognized in this position.)
- `string_lit` is the path pattern (e.g. `"/user/:userId"`). Captures are
  **not** validated against formals until a later step. Formals use normal
  Thoxa idents (no `_` — prefer camelCase).
- Optional param list and return type use the same grammar as methods.
- Body is a normal block.

Mix with normal `fn` methods and `let`/`mut` fields is allowed by the parser;
semantics of mixing are TBD at lower time.

---

## Open questions (later steps)

- Table representation without fn values  
- How `path_match` is called (generated `if`s vs helper)  
- 404 / first-match / HEAD≡GET policy  
- Whether `type MyRouter: Router` marker is needed (v0: **no** — any type body)  
- Multi-clause interaction if user also writes `fn route` by hand  

---

## Pickup

Branch + this file. Do **not** promote to `docs/specs/` until the experiment
earns a freeze.
