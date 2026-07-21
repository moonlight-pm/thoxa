# Export on constructor parameters — design

**Status:** implemented 2026-07-18.  
**Driver:** HTTP `Request` / `Response` / `Header` (and any record-like type) had to
rename ctor params (`method0`) and re-export with `export let method = method0`.  
**Files:** `thoxa/ast.py`, `parser.py`, `interp.py`, `checker.py`, `resolver.py`;
`spec/SPEC.md`; tests; migrate `examples/http/Http.thx`.

## Problem

Constructor parameters become fields, but those fields are **always private**. To
expose them under the same name you need a body re-export:

```thoxa
export type Request(method0!: String, path0!: String) {
    export let method = method0
    export let path = path0
}
```

That is pure ceremony for the common case: “this input *is* a public field.”

Body `export let` / `export fn` remain essential for derived fields, renamed
projections (`Match(r, c) { export let row = r }`), and methods. This design only
adds a shorter path when the public name **is** the param name.

## Decision

Allow `export` on a **type constructor parameter**:

```thoxa
export type Request(
    export method: String,
    export path: String,
    export version: String,
    export headers: Array<Header>,
    export body: Bytes,
) {}
```

- The param is still a ctor argument and a field, same storage model as today.
- `export` on the param publishes that field for external access (`req.method`).
- Keyword parameters (`name: T`) stay the preferred call-site style; positional
  (`name!: T`) remains available and may also be exported (`export fd!: Int32`)
  when a tiny internal type wants both.

### Still valid (unchanged)

```thoxa
export type Match(r!: Int64, c!: Int64) {
    export let row = r
    export let col = c
}

export type File(path: String) {
    mut fd = -1
    export fn open(mode: OpenMode) { ... }
}
```

Bodies can still export anything: methods, derived fields, renames — not only
ctor params.

### Not in scope

- `export` on **function** / `extern` parameters — parse error if written.
  Methods and free functions have no “field visibility”; only type ctors do.
- Making all ctor params public by default — opt-in `export` keeps encapsulation
  for `Socket(fd)`, `File(path)`, etc.
- Changing named-vs-positional rules.

## Semantics

| Form | Field exists? | Externally visible? |
|------|---------------|---------------------|
| `type T(x: Int64)` | yes | no |
| `type T(export x: Int64)` | yes | yes (`t.x`) |
| `type T(x: Int64) { export let x = x }` | yes (one slot) | yes (body export) |
| `type T(export x: Int64) { export let y = x }` | `x` and `y` | both public |

Same-name ctor param + body field continue to unify to one storage slot (existing
resolver rule). If either site marks export, the member is exported.

## Implementation

1. **AST:** `Param.exported: bool` (default `False`).
2. **Parser:** `parse_params(allow_export=…)`. Type ctors pass `True`; functions /
   externs pass `False`. Leading `export` before a param name when disallowed →
   clear error: *"export is only valid on type constructor parameters"*.
3. **Interpreter:** `ThoxaType.exported` includes ctor params with `exported`.
4. **Checker:** `type_exported` includes those names (member-access checks).
5. **Resolver:** `_declare_members` passes `exported=p.exported` for ctor params.
6. **SPEC.md** §5: document the form; keyword args preferred at call sites.
7. **Tests:** parse, public access, still-private without export, export forbidden
   on fn params; migrate `examples/http/Http.thx` as the proving ground.

## Style note (not enforced)

Prefer keyword parameters on exported records for self-documenting call sites:

```thoxa
Request(method: "GET", path: "/", version: "HTTP/1.1", headers: [], body: Bytes(0))
```

Reserve positional (`!`) for small internal types and prelude payloads where
labels add noise (`Pass(x)`, `Has(i)`).
