# Freeze: Map and value collections (D133, D134)

**Status:** shipped (v0); user keys, structural `==` (Map's too), and wider keys shipped later  
**Decisions:** D133 (collection mutation), D134 (`Map<K, V>`); follow-ups
D175 / D177 / D178 (`==`), D179 (user keys), D180 (collection writes reach a
place), D181 (`remove` keeps insertion order), D203 (method parameters are
positional), D205 (`insert(item, at:)` replaces `insertAt`), D207 (a field
indexes as a local), D212 (`==` on maps), D213 (map literals take the
expected type), D214 (arrays and Float64 in keys)  
**Gates:** `maps` (Map); `arrays` / `array_user` / `array_heap_elems` / `splice`
(Array surface after D133); `map_user_keys`, `map_user_keys_more`,
`map_keys_arrays_floats`, `map_remove_order`, `map_eq`, `map_literal_expect`,
`field_index`, `array_eq`, `collection_element_writes`  
**As of:** 2026-10-18  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D133/D134 in
[`decisions.md`](../decisions.md)

---

## Intent

`Array` and `Map` are **value collections** in the Swift / `Buffer` sense.

- `mut` on a place means that place may be written (methods and `a[i] =` /
  `m[k] =`).
- `let` aliases do **not** observe later writes.
- `let ys = xs` shares storage until a write; a write through a `mut` place
  copies if `rc > 1`, then updates **only that place**.

This amends **D54** (functional `append` + shared index-set). It does **not**
add Rust moves or a borrow checker. Assignment of a heap collection remains
an RC retain.

`Map<K, V>` is a language builtin (sibling of `Array<T>`), not a `std/` type.

---

## Locks

### 1. Mutation model (D133) — Array and Map

Writes go through a **place** (`mut` local, `mut` module binding, or a field
reached from a `mut` receiver — same write-back rule as `Buffer` and method
field assign).

| Binding | `xs.append(4)` / `m.set(k, v)` / `xs[i] =` / `m[k] =` |
|---------|--------------------------------------------------------|
| `mut xs` | Allowed. Unique (`rc == 1`) mutates in place. Shared copies, then updates this place only. |
| `let xs` | **Error** — place is not writable. |
| Temporary (`foo().append(x)`) | Allowed (unique throwaway), same as Buffer. |

```thx
mut xs = [1, 2, 3]
let ys = xs
xs.append(4)
xs[1] = 99
# xs is [1, 99, 3, 4]; ys is still [1, 2, 3]
```

Mutating methods return **`Void`**. They do **not** return a new collection.

```thx
mut xs = [1, 2]
xs.append(3)           # not xs = xs.append(3)
# let zs = xs.append(4)  # error: Void
```

To copy-then-edit: bind a `mut` alias (share until first write).

```thx
let frozen = xs
mut edit = xs
edit.append(4)         # frozen unchanged
```

**Array methods after D133:** `append`, `dropLast`, `insertAt`, `removeAt` are
write-back `Void` (same bounds panics as D55). Since **D205** (2026-10-18)
`insertAt(index, item)` is `insert(item, at: index)`. Index get `a[i]` is unchanged
(element, OOB panic). Index set `a[i] = x` write-back / in-place per the table
above — **not** a store into a shared `ThoxaArray*` that aliases would see.

**Bytes** `insertAt` / `removeAt` stay as D55 (functional; `insert(byte, at:)`
since D205). Out of this freeze.

**Fixed `Array<T, N>`** (D27 / D117) is inline storage, not a heap collection.
Element set is unchanged.

### 2. `Map<K, V>` (D134)

Builtin heap RC value (`ThoxaMap*`). Two type arguments. Empty value: `Map()`
at a typed slot (annotation or expected type), same idea as `[]`.

```thx
mut m: Map<String, String> = Map()
```

### 3. Keys and values (v0)

| | Allowed |
|--|---------|
| **Keys** | `String`, `Int64`, `Bool` (already have `==` and a trivial hash) |
| **Values** | Same as dynamic Array elems: `Int64`, `Bool`, `String`, `Bytes`, User, `Array<…>`, `Map<…>`, `Node` |

**Not v0 keys or values:** User keys (see below), function types (D132 / D48),
`Buffer`, `Address`. Nested `Map` / `Array` **values** are in. User types as
**values** (`Map<String, Header>`) are in.

**User keys (shipped as D179, 2026-10-16 — not traits):** a *User key* is a constructor-param
product (`type Header(name: String, …)`) used as `K` in `Map<K, V>`. v0
omits them because products have no `==` and no hash, not because Map is
String-only forever. **Do not invent Eq/Hash traits for this.** The intended
slice is:

1. Structural `==` / `!=` on constructor-param products whose fields are
   already eq-able (`Int64`, `Bool`, `String`, `Bytes`, nested such products).
   **Shipped as D175** (2026-10-16), also covering `Float64`, refined, and
   literal-union fields.
2. A private runtime hash of those same fields (not a user `.hash()`).
3. Then those types are legal Map keys.

   **Shipped (D179):** products and unions whose contents hash by value
   (integers, Bool, String, Bytes, refined and literal-union fields, nested
   such types); cgen generates the hash and equality per key type. Keys
   holding Float64, Array, or Map are refused. A literal key may be a call,
   `{Point(x: 1, y: 2): v}`.

Keys compare **fields**, never RC pointer identity (COW copies would miss).
Out of that slice: Array/Map fields as keys (needs collection `==`);
C-layout; unions; function types. `==` on User is independently useful
(`if h1 == h2`) even without Map.

A `set` on an existing key **overwrites the value** and **keeps insertion
order**. `remove` then `set` of the same key is a new insert (goes to the end).

### 4. Read

`m.get(k)` and `m[k]` are the same: **`May<V>`**. Missing is normal, not a
panic. `m[k]?` unwraps (`std/Fallible` in scope, same as `Bytes.find`).

`m.has(k) -> Bool`. Not `contains`. Distinct from the `Has` constructor.

`m.count` is `Int64` (entry count), same spelling as Array / String.

### 5. Write

| Form | Meaning |
|------|---------|
| `m.set(k, v)` | Insert or overwrite. Write-back `Void`. Requires writable place. |
| `m.remove(k)` | Drop the entry if present; **no-op** if missing. Write-back `Void`. |
| `m[k] = v` | Same as `m.set(k, v)`. |

`m[k] = v` is index **assign**, not assign-into-`May`. The get form `m[k]`
stays `May<V>` in value position.

### 6. Literals

`{ }` is still a **block** expression. Empty `{}` is an empty block (Void).
Empty map is `Map()`.

In **expression** position, `{` starts a map if the body is colon-pairs:

```thx
let m = { "Cookie": "a=1", "Host": "localhost" }
let n = { 1: "one", 2: "two" }
let p = { Cookie: "a=1", Host: "localhost" }
```

- **Ident keys** are the string of that name (`Cookie` → `"Cookie"`). They are
  **not** variable keys. A computed key is `m.set(name, v)` (no `{(name): v}`
  in v0).
- Other keys: string / int / bool literals (typed as `String` / `Int64` /
  `Bool`).
- Trailing comma allowed (same as Array lits).
- Values are ordinary expressions (including nested map lits).
- Duplicate keys in one literal are a **type/check error** (not last-wins).

Disambiguation: `{ x = 1 }` is a block; `{ x: 1 }` is a map. Parser lookahead
after `{` for `key :`.

Heterogeneous keys in one literal are a type error.

### 7. Iteration

No tuples. Map iteration is a **two-binder** loop, insertion order:

```thx
for k, v in m {
    info("{k}={v}")
}
```

- `for x in m` is an **error** (say use `for k, v` or `.keys()` / `.values()`).
- `for k, v in xs` on an Array is an **error**.
- `.keys() -> Array<K>` and `.values() -> Array<V>` are snapshots (new arrays,
  insertion order). Mutating the map afterwards does not change those arrays.

`for-in` on Array is unchanged (`for x in xs`).

### 8. Display and interpolation

A Map interpolates / displays as `{k: v, k: v}` in insertion order, values
using the same element rendering as Array. String keys that are ident-safe
print bare; others quote. No `==` / `!=` on Map (Array has `==` since D178).

### 9. Where a Map may live

| Place | v0 |
|-------|----|
| Argument / return / local `let` / `mut` | **yes** |
| Module `let` / `mut` | **yes** (D122 — not dropped at end of the assigning fn) |
| Constructor-param field | **yes** — D48/D69 allowlist grows by `Map<K, V>` |
| C-layout body field | **no** |
| Dynamic Array element | **yes** |
| Nested Map value | **yes** |
| Fixed `Array<T, N>` element | **no** (ABI scalars only, D117) |

### 10. Representation (normative enough to lock RC)

`ThoxaMap` is a heap RC object: open-addressed table plus an **insertion-order**
walk (order list or equivalent). Keys and values are retained on insert and
dropped on remove / map drop.

Copy-on-write: if `rc > 1`, a write allocates a new map, dups live keys and
values, then mutates the new object and write-backs the pointer. `rc == 1`
mutates in place.

Hash: stable per process for the three key types (string bytes; i64 bit
pattern; bool as 0/1). Not a user-visible hash API.

### 11. Relation to other decisions

| Decision | Relation |
|----------|----------|
| **D54** | **Amended by D133.** Functional `append` / shared index-set are out. |
| **D55** | Array splice *names* stay; return becomes `Void` + write-back. Bytes splice unchanged. |
| **D50** | Buffer already is this model. Collections join it. |
| **D48 / D69** | Ctor fields may be `Map<K, V>`. |
| **D85 / D123** | `m[k]` / `.get` produce `May<V>`; import Fallible to name/construct May. |
| **D132** | Function types still not collection elems or Map keys/values. |
| **D69** | `Request.headers` stays `Array<Header>` in this freeze. Crema may use Map for query/form locally. |

---

## Explicit non-goals (v0)

- User keys in this ship (trajectory above; shipped later as D179)  
- Eq/Hash traits / protocols  
- `==` on Array or Map (`==` on Array shipped later as D178; Map still has none)  
- Persistent HAMT / structural sharing beyond whole-table COW  
- Rust moves / borrow checker  
- Functional `appended` / `set` that returns a new collection  
- `{ (expr): v }` computed keys  
- `for x in m` as values-only  
- Bytes mutation-model change  
- Migrating `std/Http` headers to Map  
- Map / Array as FFI / `Address`  
- `std/collections`

---

## Implementation notes (non-normative shape)

1. **Parse:** map lit vs block by lookahead; `for name, name in expr`; type
   `Map<K, V>`; `Map()` call as empty ctor (like `Buffer()`).  
2. **Ty:** `Ty::Map { key, val }`.  
3. **Lower:** Map methods + index get/set parallel Buffer write-back (and
   Array after the flip). `May` for get via existing Fallible monomorphs.  
4. **Runtime:** `thoxa_map_*` in `thoxa_rt` — new, count, get, set, remove,
   has, keys, values, iter cursor, dup/drop, to_string.  
5. **Array flip:** `append` / `dropLast` / `insertAt` / `removeAt` and dyn
   index-set become write-back; stop returning a new `ThoxaArray*` to the
   caller. Update `arrays.thx`, `array_user`, `array_heap_elems`, `splice`,
   and every `x = x.append(…)` sample (Crema headers especially).  
6. **LSP:** hover `Map<K, V>`; `.has` / `.get` / `.set` / `.remove` signatures.  
7. **Conformance:** gate `maps` — empty + typed `Map()`; literal + ident keys;
   get/`May`; `has`; set overwrite keeps order; remove missing is no-op;
   `for k, v` insertion order; COW alias (`let` snapshot survives `mut` write);
   User values; `let m.set` is an error. Array gates keep existing stories
   under the new `Void` write-back API.

**Suggested order:** Map on the new rule first (nothing to break), then Array
flip + sample updates, then handbook. Do not handbook Map while Array still
returns new arrays.

---

## Acceptance

**Design acceptance (this freeze):**

- [x] Locks recorded (this document + D133/D134)
- [x] Capability rows **spec’d**
- [x] `CURRENT.md` lists this as an implement slice

**Code acceptance:**

- [x] `cargo make test` green with `maps` + updated Array gates
- [x] Crema compiles (append sites updated); local Map dogfood (query/form/cookies)
- [x] Handbook: Map + amended Array mutation (only when shipped)

---

## Changelog

| Date | Note |
|------|------|
| 2026-08-14 | Initial freeze: Swift/Buffer COW for Array+Map; `Map<K,V>`; `.has`; `{k:v}` lits; `for k, v in m` |
| 2026-08-14 | **Shipped:** `Ty::Map` + `thoxa_map_*`; Array append/index-set write-back; gates `maps`, `maps_user` |
| 2026-08-14 | User-key follow-up locked: structural product `==` + private hash; no Eq/Hash traits |
| 2026-08-14 | Crema dogfood: `parseQuery` / `parseForm` / `parseCookies` → `Map<String, String>` |
| 2026-08-14 | MapGet of heap values retains (so `foo()[k]` is safe after the temp drops) |
