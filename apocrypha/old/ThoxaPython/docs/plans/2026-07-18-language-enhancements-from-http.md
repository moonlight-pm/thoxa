# Language enhancements — backlog from the HTTP filesystem router

**Date:** 2026-07-18  
**Source:** Building `examples/http/` (Request/Response, Router over `web/`).  
**Status:** backlog — review one item at a time; promote to a design spec when accepted.

These are not commitments. They are friction notes ordered by how hard they hit while
writing the static file server, and how well they fit Thoxa’s direction.

---

## High value (felt immediately)

### 1. Public record / ctor fields — **done 2026-07-18**

**Shipped:** `export` on type constructor params:
`export type Header(export name: String, export value: String) {}`.
Body `export let` / `export fn` kept for renames, derived fields, methods.
Keyword params preferred at call sites; `export` on fn params is a parse error.
See `docs/specs/2026-07-18-export-ctor-params-design.md`.

---

### 2. `String` methods that mirror useful `Bytes` ops

**Pain:** strings have `.count` and indexing, then everything else bounced through
`Bytes.fromString` → op → `.toString()`.

**Wanted (minimum):**

- `slice(start, end)`
- `find(needle) -> May<Int64>` (and/or `contains` / `startsWith` / `endsWith`)
- later: `split(delim) -> Array<String>`

Path handling and header parsing should not require Bytes round-trips for ASCII text.

---

### 3. `Bytes.find` from an offset (or `findAfter`)

**Pain:** parsing lines repeatedly was “slice remaining → find → add offset.”

**Sketch:** `hay.find(needle, from: i)` or `hay.findAfter(needle, i) -> May<Int64>`.

Avoids allocation-y slice loops in hot parsers.

---

### 4. Small string/bytes “text” vocabulary in the prelude

Even without a full stdlib, protocol examples re-hand-roll the same helpers:

| Op | Why (from the example) |
|----|------------------------|
| `startsWith` / `endsWith` | path must start with `/`; extension check |
| `split` on a delimiter | request-line tokens, path segments, headers |
| `trim` / trim leading space | header values after `:` |

`splitBytes` in `Http.thx` is the kind of thing that belongs once, not in every example.

---

## Medium value (systems story, next layer of HTTP)

### 5. `Io.stat` / file kind

**Pain:** Router v1 does “open file; on fail try `…/index.html`.” Can’t distinguish
directory from missing.

**Sketch:** `stat` → size + `isFile` / `isDir`, or a small `FileKind` union.

Makes static serving honest; unlocks directory listings later without guesswork.

---

### 6. Growable `String` builder — or document that `Buffer` is the way

**Pain:** interpolation is the only concat; loops use `path = "{path}/{s}"`. Fine for a
few segments, painful for large assembly.

**Options:**

- `StringBuf` / append-string that freezes to `String`, or
- explicit convention: build text as `Buffer`, then `freeze().toString()`.

Right now the story is incomplete.

---

### 7. `match` ergonomics for discarding payloads

**Note:** `Fail { }` worked for `Trial<_, String>`. Lock in that wildcard / ignore-binding
is intentional (`Fail(_)`, `Fail { }`) and document it — parsers often don’t care about
the error detail at the match site.

---

### 8. Hex / ASCII escapes investment

`b' '` and `"\r\n"` worked well. Protocol code is mostly delimiters and control bytes —
keep investing here (already better than kilo-style `b'\r'` loops).

---

## Design-level (don’t rush)

### 9. Type-associated / static functions

**Sketch:** `Request.parse(raw) -> Trial<Request, String>` instead of (or in addition to)
free functions on the module instance. You already have `Bytes.fromString`; generalizing
type-associated functions reduces `http.parseRequest` namespace noise.

---

### 10. Prefix patterns on strings/bytes

```thoxa
match path {
    "/api/" + rest { ... }   // or starts-with patterns
    ...
}
```

Optional long-term; `startsWith` + slice covers 90% first.

---

### 11. Streaming / partial reads — example pressure first

`receive()` is one 4 KiB chunk. Incomplete headers → 400. Before language changes, a
loop-until-`\r\n\r\n` in `Net`/`Client` teaches the right I/O shape.

Language-wise that argues for better buffer growth (have `Buffer`) and maybe non-copying
`Bytes` views later — not async yet. Synchronous read-loop is enough.

---

### 12. Richer error types than `String`

`Fail("malformed request line")` is demo-grade. When ready: small union errors
(`Incomplete | MalformedLine | BadHeader`) so `match` can branch without string compares.
Fits May/Trial better than English payloads everywhere.

---

## Explicitly deprioritized (from this exercise)

- Full regex
- URL percent-decoding as a language built-in (stdlib later)
- HTTP in the language (keep it an example)
- Implicit string `+` if it fights interpolation-only — prefer builders + methods instead

---

## Suggested review order

When iterating this backlog, walk in this order unless a design driver says otherwise:

1. Public ctor fields  
2. `String.slice` + `startsWith` / `endsWith`  
3. `Bytes.find` from offset  
4. `split` for Bytes or String  
5. `Io` metadata (`stat`)  
6. String builder story  
7. Then medium / design-level items as interest allows  

For each item: discuss → accept/reject/defer → if accept, write a short design under
`docs/specs/` before implementing.
