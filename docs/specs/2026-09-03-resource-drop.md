# Freeze: Resource `drop` (last RC)

**Date:** 2026-09-03
**Status:** target (freeze)
**Implementation:** shipped (v0)
**Decision:** D155
**Gates:** `file_drop`, `file_type`, `http_net_err`, `sqlite`
**Gaps:**
- No `drop` on unions, C-layout types, or multi-clause methods
- `drop` takes no formals and returns `Void` (not `Trial`)
- No language `defer` / linear types / Perceus
- Cycle collector still parked
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D155 in
[`decisions.md`](../decisions.md)
**Related:** simple-RC [`2026-09-02-simple-rc-correctness.md`](2026-09-02-simple-rc-correctness.md)
(class E); kernel D4 / D8; File/Socket/Db explicit `close` (D107 write-back)

---

## Intent

A constructor-param product may define `export fn drop()`. The compiler heap-boxes
values of that type (header + payload, like `String`). Last RC calls `drop()`,
then drops heap fields, then frees. Explicit `close` stays; it must leave the
value inert so a later `drop` is a no-op.

Timing is **last reference** (RC / deinit), not lexical unique/move. Copies
`let g = f` share the box. Returning or storing keeps the resource alive.
`?` / `return` drop live boxed locals the same as other `needs_drop` places
(**D154** / **D63**).

## v0 rules

- Name is **`drop`**. No extra formals. Return **`Void`**. Not multi-clause.
- Product User only (not unions, not C-layout `TypeName()` body fields).
- Payload stays the C struct; locals / params / returns / fields of that type
  are **pointers**. Method `_self` is still `Type *` (D107).
- `thoxa_alloc` + `thoxa_set_dtor`; tag `THOXA_TAG_USER`. Generic `thoxa_drop`
  runs the dtor at refcount 0 (so `Array`/`Map` heap elems work).
- `File` / `Socket` / `Db` ship `drop()` as `close()`.
- Bound method values of a Drop type are out of v0 if they would memcpy the
  payload; treat the box like other heap pointers.

## Non-goals

- Traits / `Drop` word / linear types
- Cycle collector
- Auto-close without a `drop` method
- Session/libtcc sanitizer
