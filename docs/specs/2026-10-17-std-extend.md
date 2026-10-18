# Freeze: std writes built-in types' methods in Thoxa (D196)

**Date:** 2026-10-17
**Status:** target (freeze)
**Implementation:** shipped
**Decision:** D196 (reworks how D195's String methods are built)
**Gates:** `string_methods`, `string_scalars`, `stdlib`, `fail/extend_outside_std`, `lsp::analysis::tests::string_methods_without_imports`
**Gaps:** no generic `extend` (`Array<T>`); an extension method cannot change its receiver; every program initializes the std/unicode tables (globals are all kept)
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D196 in
[`decisions.md`](../decisions.md)
**Builds on:** [`2026-10-17-string-methods.md`](2026-10-17-string-methods.md) (D195)

---

## Intent

Thoxa aims to host itself, so its library is written in Thoxa. Joshua,
2026-10-17: "we should be writing our code in thoxa when possible … write in
thoxa unless you can't. If we need to add a feature to write something in
thoxa, we can look into that."

D195 first shipped String's methods as C in `thoxa_rt` behind a table in the
compiler. That put library code on the wrong side of the line, and it showed:
a built-in method could not return a std type, so `splitOnce` stayed a free
function. This freeze moves the line. The C runtime keeps what needs the
machine: allocation, reference counts, the String representation (bytes,
cached scalar count, UTF-8 repair at creation), and a few byte primitives.
Everything else a String does is Thoxa in `std/string.thx`.

```thx
## std/string.thx
export type Split(before: String, after: String)

extend String as text {
    export fn splitOnce(separator!: String) -> May<Split> {
        match text.byteIndexOf(separator) {
            Has(at) => Has(Split(before: text.byteSlice(0, at), after: text.byteSlice(at + separator.byteCount, text.byteCount)))
            Not => Not()
        }
    }
}

extend Array<String> as parts {
    export fn join(separator!: String) -> String { … }
}
```

## §1 `extend`

- `extend Type as receiver { [export] fn … }` declares methods on a built-in
  type: `String`, `Bytes`, `Buffer`, `Int64`, `Float64`, `Bool`, or a concrete
  `Array<…>` / `Map<…, …>` of those. A declared type's methods stay in its
  body.
- There is no `self`, and a built-in value has no fields to see by name, so
  the header names the receiver (`as text`). Inside, it is an ordinary
  immutable parameter.
- **Only a std module may `extend`.** A type's methods are then the same in
  every program, whatever it imports, and two libraries cannot disagree about
  them. Anywhere else, `extend` is an error ("only a std module can extend a
  built-in type").
- An `export fn` is callable everywhere; a method without `export` only inside
  its own module.
- `extend_desugar` turns each method into a free function
  `__extend_{Type}_{method}` (`__extend_Array_String_join`) whose first
  parameter is the receiver. A call `value.method(…)` on a built-in receiver
  looks for that function first, then for a runtime primitive. A refined
  String takes String's methods.

## §2 Core modules

`std/string` is in every package, imported or not: the loader adds it
(`include_core_modules`), including for the packages `thoxa check` and the
language server build by hand. It brings `std/fallible` and
`std/unicode` with it. Loading it adds no names to a program (consistent with
D123): `Split` and `parseUInt` are still imported by name.

## §3 What stays in C

| Primitive | Meaning |
|-----------|---------|
| `.count`, `.byteCount` | Unicode scalars (cached when the String is made), bytes |
| `byteAt(offset)` | the byte at `offset`; out of range panics |
| `byteSlice(start, end)` | the bytes from `start` up to `end`, clamped; a cut inside a character becomes U+FFFD |
| `byteIndexOf(needle, from: 0)` | `May<Int64>`, the byte offset of the first `needle` at or after `from` |

They are public: they are also the fast path for code that works in bytes.
The rest of D195's methods are Thoxa: search and split over `byteIndexOf`,
positions by walking UTF-8 from the start (O(1) in ASCII text), case mapping
by binary search over the tables `cargo make unicode` writes to
`std/unicode.thx`, building through `Buffer`.

## §4 Constant arrays

An array literal whose elements are all integer constants
(`[0x41, 0x5A, 32, 1]`) lowers to one C array (`RValue::IntArray`), not a
temporary per element. The Unicode tables are about 3,150 numbers; before
this they would have added a statement each to every program.

## Cost

Measured 2026-10-17 on hello world: 4,673 lines of C and 0.33 s for `cc -O2`
before; 9,912 lines and 0.60 s after, because the compiler emitted every
function of every loaded module and std/string is now always loaded. **D197**
emits only reachable functions: 2,824 lines and 0.19 s.
Runtime speed of the methods is not tuned (API first; Joshua, 2026-10-17).

## Non-goals

- `extend` outside std; traits; generic `extend Array<T>`.
- Methods that change their receiver (`mut` receivers for `extend`).
