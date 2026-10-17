# Freeze: Typed codecs (D182)

**Date:** 2026-10-16
**Status:** target (freeze)
**Implementation:** §1, §6 shipped
**Decision:** D182 (amends D149: reads report every issue)
**Gates:** per section below
**Gaps:** see Non-goals
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D182 in
[`decisions.md`](../decisions.md)
**Builds on:** [`2026-08-23-type-read.md`](2026-08-23-type-read.md) (D149),
tree codec D141

---

## Intent

D149 made a type its own schema: the compiler attaches `read` / `fromJson` /
`encode` / `toJson` to a product. This freeze finishes that idea. Every value
whose type can be written as JSON encodes. A read reports every problem, not
the first. A form or query string reads into the same type as JSON. A `Json`
value can be written as a literal. Still no traits and no schema values: the
compiler sees the type and generates the code.

```thx
type Qty = Int64 where >= 1
type DrinkSlug = "espresso" | "mocha"
type CreateOrder(slug: DrinkSlug, qty: Qty)

match CreateOrder.readForm(request.body.toString()) {   # or .read(json)
    Fail(issues) => page(form, issues)                   # every bad field
    Pass(order) => place(order)
}

fn ticketOk(ticket!: Ticket) -> Json {
    { ok: true, ...ticket }
}
Pass(rows) => jsonOk(rows.toJson())
```

---

## Locks

### 1. Every encodable value has `toJson` / `encode`

`x.toJson() -> Json` and `x.encode() -> String` (compact) for `x` of an
encodable type:

| Type | JSON |
|------|------|
| `String` | string |
| `Int64`, `Int32`, `UInt32`, `UInt64`, `Byte` | integer (`UInt64` above `Int64` max panics, D167) |
| `Float64` | number |
| `Bool` | `true` / `false` |
| `Array<T>` | array |
| `Map<String, T>` | object, insertion order |
| `May<T>` | the value, or `null` |
| `Json` and its members | itself |
| refined type (`Qty`) | its base value |
| literal union (`DrinkSlug`) | the literal |
| product | object of its fields in declaration order; a `May` field that is `Not` is left out |
| union of products | the member's object with `"type": "<Member>"` first; a member with no fields is `{"type": "<Member>"}` |

`"type"` cannot collide with a field: `type` is a keyword, so no field has that
name.

Not encodable (compile error naming the type, and the field when there is
one): `Bytes`, `Buffer`, `Node`, functions, `Address`, types with `drop`,
C-layout types, `Map` with non-`String` keys, unions mixing literals and
products, generic types and `Trial`. Types in `std/` modules get no generated
codec (v0); `std/json` writes `Issue`'s JSON by hand.

The compiler generates the encoder for every encodable named type in every
module once `std/json` is in the package (some module imports it). A module
need not import `std/json` itself to encode its types; calling `toJson`
somewhere needs `std/json` in the package.

**Gate:** `json_encode`.

### 2. A read reports every issue

`T.read(s)` and `T.fromJson(v)` return `Trial<T, Array<Issue>>`. A `Fail`
holds at least one issue:

- bad JSON syntax: one `Rejected("", "bad json: …")`;
- not an object: one `Mismatch` at that path;
- otherwise every field is checked, in declaration order. Each missing or
  mismatched field adds its issue; a nested product or an array element adds
  the issues inside it (`items.1.qty`). A refined field adds one issue (its
  first failing conjunct).

`T.from(x)` checks one value and keeps `Trial<T, Issue>`.

`std/json` adds `messages(issues) -> String` (joined with `; `) and
`errorsObject(issues) -> Json` (`{"ok": false, "errors": [...]}`, one object
per issue as `errorObject` writes it, without `ok`).

**Gates:** `json_read`, `where_refine` (updated), `json_read_all`.

### 3. Forms read into the same types

`T.readForm(text) -> Trial<T, Array<Issue>>` reads
`application/x-www-form-urlencoded` text: a POST body
(`request.body.toString()`) or a query string (`request.query`). `T` is a
product whose fields a form can hold:

| Field type | Value | Absent or empty |
|------------|-------|-----------------|
| `String` | the text | absent → `Missing`; empty → `""` |
| `Int64` / `Float64` | parsed | `Missing`; unparsable → `Mismatch` |
| `Bool` | present → `true` (a checked box) | `false` |
| refined / literal union | base value, then `from`'s check | as its base |
| `May<T>` | `Has(value)` | `Not` |
| `Array<T>` | one element per repeated key, in order | `[]` |
| nested product | its fields as `field.sub` names | each field by its own rule |

Empty means absent for every type except `String`. A repeated key for a
non-array field takes the last value. Keys the type does not name (a CSRF
token) are ignored. An issue's path is the input's `name`. Issues are reported
as in §2. Other field types (`Map`, `Json`, unions) are a compile error at the
`readForm` call, naming the field.

**Gate:** `form_read`.

### 4. `Json` literals

Where a `Json` is expected (a parameter, a return, an annotated `let`, a value
inside another `Json` literal), a map literal `{ key: value, … }` is an
object and an array literal `[ … ]` is an array. Each value is lowered
expecting `Json`; a value of another type converts with `toJson` (§1).
`...value` inside an object literal spreads a product's or a
`Map<String, T>`'s entries; a later key replaces an earlier one. Outside a
`Json` literal nothing converts implicitly: write `x.toJson()`.

**Gate:** `json_literal`.

### 5. Reads cover what encodes

`read` / `fromJson` (and `readForm` where §3 allows) also take:

- unions of products, chosen by the `"type"` key (missing → `Missing` at
  `path.type`; unknown → `Mismatch` listing the members);
- `Map<String, T>` fields and `Json` fields (any value);
- constructor defaults: a missing key takes the field's default
  (`qty: Qty = 1`) instead of a `Missing` issue.

**Gate:** `json_read_more`.

### 6. A type's own `toJson` wins

A type that defines `fn toJson() -> Json` is encoded by it, everywhere,
including inside other types' encoders; the compiler generates no encoder for
it. (Today defining `read` / `fromJson` is an error; that stays until custom
decoding has a shape.)

**Gate:** `json_custom_encode`.

---

## Order

One slice per section, in order §1, §2, §3, §4, §5, §6, each with its gate and
progress docs. §3 builds the field walk that §2 and §5 share, so the decode
features land once for JSON and forms.

---

## Non-goals (punt)

| Item | Why |
|------|-----|
| Field renaming (`rename_all`) | No annotation syntax; wait for an external snake_case API |
| Formats beyond JSON and forms | Wait for a third customer |
| Top-level `Array<T>` / `Map` reads | No static-method syntax on structural types; wrap in a product |
| Generic types (`Page<T>`) | Codecs are generated before monomorphization |
| Generated codecs for `std/` types | Every program would compile them; `Issue` is written by hand |
| Custom decoding | No static methods to hang it on |
| Encoding into a stream | Encode builds the `Json` tree, then the string |
| `t.encodeForm()` | Wait for a use (redirects, links) |
| Route clauses binding a body by `Content-Type` | Later, on top of §2 / §3 |
