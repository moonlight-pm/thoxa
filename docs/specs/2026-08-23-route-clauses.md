# Freeze: Type-body route clauses (D150)

**Date:** 2026-08-23
**Status:** target (freeze)
**Implementation:** shipped
**Decision:** D150
**Gate:** `route_clauses`
**Gaps:**
- ~~No JSON / form auto-bind~~: typed body parameters since D191 §5
- Miss is `notFound()` — no fallthrough / `May`
- Router type must be nullary (no ctor params / body fields)
- No compiler lang-items for `Request` / `Response`
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D150 in
[`decisions.md`](../decisions.md)
**Promoted from:** [`ideas/2026-08-12-router-clause-syntax-experiment.md`](../ideas/2026-08-12-router-clause-syntax-experiment.md)
**Related:** HTTP types **D136**; file-based routing stays parked
([`ideas/2026-08-15-file-based-routing.md`](../ideas/2026-08-15-file-based-routing.md))

---

## Intent

HTTP routes look like routes. Clauses live on a **nullary type**; the compiler
desugars them to free-function handlers plus `route(req) -> Response`. Minimal
knowledge of `std/`: it only **names** userland `Request` / `Response` / `ok` /
`notFound` / `badRequest`.

```thx
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

---

## Locks (v0)

### 1. Surface

In a **type body** only:

```text
METHOD string_lit [ ( params ) ] [ -> type ] block
```

`METHOD` ∈ `GET` `POST` `PUT` `PATCH` `DELETE` `HEAD` `OPTIONS` (idents in
type body; also Tree-sitter keywords). Mix with `fn` methods is parse-ok;
desugar **rejects** constructor params and body fields on that type.

### 2. Generated `route`

Each clause becomes a module free function `__{Type}_route_N`. The type gets
`route(req: Request) -> Response` that reads `req.method` / `req.path` only.

The module must import the names the desugar calls (`Request`, `Response`,
`ok`, `notFound`; `badRequest` if any `Int64` capture). No ambient inject.

### 3. Returns

| Clause | Meaning |
|--------|---------|
| omitted / `-> String` | Wrap `ok(Bytes.fromString(s), "text/plain")` |
| `-> Response` | Pass through |
| anything else | Error |

Miss → `notFound()`. No static fallthrough.

### 4. Path

Must start with `/`. Exact path, or **segment captures** `:name` (alphanumeric;
no `_`). Captures: `String` or `Int64` (bad digits → `badRequest("invalid {name}")`).
Optional one `Request` formal (any name; not a capture). Capture formals must
match the `:name`s.

### 5. Editor

Resolve/LSP desugar the same way. Hover on `GET "/…"` shows a synthetic
`route` signature. `let a = routes.route(req)` can show `let a: Response`.

---

## Non-goals (punt)

| Item | Why |
|------|-----|
| JSON / form / `Request<T>` codecs | Call `T.read` in the handler (**D149**) |
| Fallthrough / `May<Response>` | Miss is `notFound()` |
| Router types with fields | Nullary v0 |
| Runtime route table / fn-pointer list | Ladder of `if`s is the table |
| File-based routing | Parked idea |
| Macros | Desugar only this surface |

---

## Dogfood

`examples/http` `type Routes`. Conformance is `route_clauses`, not the cafe.
