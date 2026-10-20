# Generated codecs for generic types — maybe later

**Status:** sleeping. **Date:** 2026-10-19. **Decision:** D244 (Joshua:
"maybe future"). Today's surface: D240.

## As built

A declared non-generic type gets a generated `toJson` and reader from a
body-less `conform Point to Encodable, Decodable` (D182). A generic type
does not: `conform Box<T: Encodable> to Encodable` must write `toJson`, and
`Decodable` needs a `from(json: Json)` clause (D240). std's own generic
types (`May`, `Trial`, `Entry`) are handled by the compiler.

## The idea

A body-less conformance of a generic type would generate both, field by
field, as for a declared type: `Box<T: Encodable>` writes `value` through
`T`'s `toJson`, and `Box<T: Decodable>` reads it through `T`'s reader.

## Why it waits

- Reading needs `json as T` inside generic code: the reader of the type a
  parameter stands for, reached through the `Decodable` bound. Generic code
  has no way to call it yet.
- A reader reports every issue as `Array<Issue>` with paths (D182 §2); a
  generic reader would collect them through `T`'s reader the same way.
- Unions (`"type"` tags) would need the instance's member names, which
  differ per instance (`Box<Int64>` and `Box<String>`).

## Open questions

- Static requirements in traits (`fn from(json: Json) -> Trial<Self, …>`),
  or a compiler-known path for `Decodable` only.
- Whether instances that fail their bounds report at the conformance or at
  the use.
