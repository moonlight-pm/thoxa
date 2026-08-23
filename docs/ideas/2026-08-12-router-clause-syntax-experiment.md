# Experiment: router clause syntax (type-body `GET` / `PUT` …)

**Status:** **shipped** — 2026-08-23 (**D150**). Freeze
[`../specs/2026-08-23-route-clauses.md`](../specs/2026-08-23-route-clauses.md).
Handbook [`../handbook/language.md`](../handbook/language.md).  
**Date:** 2026-08-12 (updated 2026-08-23)

---

## Goal

Thin desugar for route-looking clauses inside a type body, with **minimal
compiler knowledge of `std/`**:

```thx
type MyRouter {
    GET "/home" {
        "Home"
    }

    PUT "/user/:userId" (userId: Int64) -> String {
        "user {userId}"
    }
}

let router = MyRouter()
let a = router.route(req)  ## a: Response (LSP)
```

Compiler (as built):

1. Parse route clauses in a **type body** only.  
2. Lower each clause to a **module free-function** handler.  
3. Emit **`route(req: Request) -> Response`** on the type (reads
   `req.method` / `req.path`; miss is `notFound()`).  
4. String clause bodies wrap via `ok(Bytes.fromString(s), "text/plain")`;
   `-> Response` is passed through.  
5. Path captures `String` or `Int64` in any segment; optional `req: Request`.
   Bad `Int64` digits → `badRequest("invalid {name}")`.  
6. Resolve/LSP desugars the same way so hover/outline see generated symbols.

`Request` / `Response` / `ok` / `notFound` / `badRequest` are userland
(usually `std/Http`). Segment split and int parse are generated helpers.
No lang-items. JSON / form bodies are **not** compiler work.

---

## Non-goals (this branch)

- General macros / attribute macros  
- Compiler-hardcoded `Request<T>` / `Response<T>` / JSON / form codecs  
- Lang-items for path matching  
- Shipping handbook / capabilities “shipped” / freeze — **done (D150)**  
- First-class function values for a runtime route table  

---

## As-built desugar (v0)

```thx
type MyRouter {
    export fn route(req: Request) -> Response {
        ## if req.method/path match { return ok(handler) or handler }
        notFound()
    }
}

fn __MyRouter_route_0() -> String { "Home" }
fn __MyRouter_route_1(userId: Int64) -> String { "user {userId}" }
```

Module must import `Request`, `Response`, `ok`, `notFound`, and `badRequest`
if any `Int64` capture exists (no ambient inject).

| Choice | Why |
|--------|-----|
| Free-fn handlers, not private methods | Private methods cannot be called via `Type().m()` from another method |
| No Array table field | Ladder is the table (`emit-c` shows it) |
| Exact path or **segment captures** | `:name` in any segment; `__routeSegments` helper |
| Capture types `String` \| `Int64` | Parse via generated `__routeParseInt64`; fail → 400 |
| Optional `req: Request` formal | Passed through; not a path capture |
| `route(req: Request) -> Response` | Dispatcher takes the request; only `.method` / `.path` |
| String → `ok(…, "text/plain")` | Framework wrap so clauses can return text |
| `-> Response` | Pass through (status / headers / body) |
| Miss → `notFound()` | No `May` at the call site (no static fallthrough) |
| Nullary product types only | No ctor params / body fields on the router type |

**Hooks:** `mir::lower_package_with` and `resolver::resolve_module` both run
`route_desugar::desugar_module` (idempotent once `routes` cleared).

**Editor:** tree-sitter colors HTTP verbs as keywords; LSP hover shows
`route` / handlers and `let a: Response` when ty_hint is known.

---

## Inspecting generated code

```bash
cargo run -p thoxa -- run scratch/router_clause.thx   # local; scratch/ gitignored
# curl -s localhost:9092/   /home   /health   /user/42   /user/42/edit
# curl -s -X PUT localhost:9092/user/42
# curl -s localhost:9092/user/abc      # 400
# curl -s localhost:9092/nope          # 404
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
| 4 Scratch + `emit-c` | **done** | `scratch/router_clause.thx` — one-file HTTP demo (gitignored) |
| 5 LSP hover/outline for desugared symbols | **done** | resolve desugar + synthetic hover |
| 6 Typed local hover (`let a: Response`) | **done** | general LSP; helps this sample |
| 7 Typed miss (`May<String>`) | **done** | `Has` / `Not`; user imports Fallible |
| 8 `route(req: Request)` | **done** | reads `.method` / `.path` only |
| 9 String/`Response` wrap | **done** | `ok` / pass-through / `notFound` |
| 10 `Int64` + multi-segment + `req` | **done** | generated helpers; 400 on bad int |
| 11 Path-match helper in std | **n/a** | generated `__routeSegments` instead |
| 12 Compiler surface | **done** | merged to master |
| 13 Crema dogfood / freeze | **done (D150)** | `examples/http` uses type-body `GET`/`POST`; gate `route_clauses` |

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

## Open questions / next (not compiler)

1. ~~Crema dogfood~~ — `examples/http` `type Routes` (example-only)  
2. Promote freeze + handbook only if keep as product  
3. JSON / form / `Request<T>` — library later, not this desugar  

---

## Commits on this branch (checkpoint)

- `dbf2a19` parse + idea note  
- `9df5760` desugar  
- `e344190` LSP resolve desugar + tree-sitter HTTP keywords  
- `8f857a7` let hover shows inferred type  
- `509675c` docs checkpoint (no product claim)  
- `34c2170` typed miss `May<String>`  
- `7cded8e` scratch is one-file HTTP demo  
- `cff31be` `route(req: Request)`  
- `fce7561` String/`Response` wrap + `notFound` miss  
- `07aa788` route-clause mismatch mentions Response  
- *(this slice)* Int64 + multi-segment + optional `req`  

Base: master at FCF park note (`969b6d7` lineage).

---

## Pickup

Freeze [`../specs/2026-08-23-route-clauses.md`](../specs/2026-08-23-route-clauses.md)
· D150 · capability `route-clauses`.
