# Idea: `std/json` encode/decode (dynamic tree)

**Status:** **shipped** — 2026-08-16 (**D141**). Product `std/json` +
conformance `json`. Not a freeze.  
**Sparked by:** Crema Phase D+ JSON POST APIs.  
**Related:** Crema charter
[`2026-08-10-examples-http-crema-experiment.md`](2026-08-10-examples-http-crema-experiment.md);
route-clauses
[`2026-08-12-router-clause-syntax-experiment.md`](2026-08-12-router-clause-syntax-experiment.md)
(JSON / form is library work, not desugar); **D140** `Float64`.

---

## What this is

Product JSON encode/decode in `std/json`, written in Thoxa like `std/http`
parse. No compiler derive. No traits.

Approved v0 face: a **dynamic `Json` tree**. Callers map to domain types by
hand. Crema dogfoods with something like `POST /api/orders`.

```thx
import json, { Json, JNull, JBool, JInt, JFloat, JStr, JArr, JObj } from "std/json"
import { Pass, Fail, Trial } from "std/fallible"

fn parse(s: String) -> Trial<Json, String>
fn stringify(v: Json) -> String
```

`Json` is a user union. Arrays / objects recurse through `Array<Json>` /
`Map<String, Json>` (D135 allows that; direct recursive User fields do not).

## Numbers

Integers without `.` / exponent become `JInt(Int64)`. Anything else that is a
JSON number becomes `JFloat(Float64)`. Out-of-range integers fail parse.
Stringify emits JSON number syntax (ints unquoted, floats shortest
round-trip). Do not collapse everything to `Float64` — coffee-shop `qty: 2`
should stay an int.

## Non-goals (v0)

- Auto encode/decode of user types (`Order` ↔ JSON)
- `Request<T>` / route-clause JSON binding
- Comments, trailing commas, NaN / Infinity
- Pretty-print knobs
- HTTP helpers in this module (`jsonResponse` only if Crema forces it)

## Hard rules

1. Sample-driven `std/` (D97). Domain types stay in `examples/http/`.
2. Fail payloads are `String`.
3. Conformance `.thx` + `.out` is done. Then Crema POST.

**Pickup:** this file + root [`CURRENT.md`](../../CURRENT.md).
