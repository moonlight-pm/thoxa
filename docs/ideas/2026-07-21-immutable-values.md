# Idea: Thoxa as an “immutable” language (values only, structural sharing)

**Status:** sleeping — speculation only; do not implement yet.  
**Sparked by:** discussion of no references / immutable values only, with shared structure on “updates” (e.g. append → new array, most memory shared).  
**Related:** D4 (value model hybrid A), D8 (Perceus later), D50 (Buffer COW), D53–D54 (Array + mutation model), D26 (`Address<T>`), D2 (effects deferred).

---

## Motivation

What would it mean for Thoxa to be **immutable by default (or only)** — no user-visible references, values only — while keeping memory reasonable via **structural sharing** / COW / uniqueness?

Thoxa already sits halfway toward that story and halfway against it. That tension is the interesting part.

---

## Where Thoxa sits now

| Piece | Today | “Fully immutable” would mean |
|--------|--------|------------------------------|
| Scalars | Unboxed values | Same |
| `String` | RC heap, immutable content | Same (already value-ish) |
| `Array` | Functional `append`/`dropLast`, but **index set mutates shared storage** (D54) | Every “change” returns a new array; no shared mutability |
| `Buffer` | RC + **COW** write-back (D50) | Builder that freezes, or pure rope/chunked seq |
| `Bytes` | Mutable FFI buffer | Immutable bytes + explicit mutable builder for FFI |
| `mut` / assign | First-class | Gone, or only for uniquely owned locals |
| `Address<T>` / C-layout | For FFI | Escape hatch only, or out of the pure surface |

So “append gives a new array, but most memory is shared” is exactly **persistent / structurally shared** collections. `Array.append` already *looks* functional; the footgun is **D54**: two names can still share storage and `a[i] = …` mutates both. Full immutability kills that class of bug and forces real sharing under the hood (or full copies, which we don’t want).

---

## Languages that do this (or live there)

### Pure / default-immutable with real structural sharing

| Language | Style | How sharing works |
|----------|--------|-------------------|
| **Clojure** | Immutable by default | Vectors (trie), maps/sets (HAMT), lists (cons cells). Classic persistent story. |
| **Haskell** | Pure | Lazy sharing + immutable cons lists, maps, etc. |
| **Elm** | Pure | Immutable records/lists/dicts; no refs in the language. |
| **Elixir / Erlang** | Everything immutable | Process-local sharing; messages copy (no shared heap between processes). |
| **Gleam** | Immutable on BEAM | Same family as Elixir for values. |
| **PureScript** | Pure (JS backend) | Immutable by construction. |
| **Roc** | Pure + ownership | Functional surface; unique values mutated under the hood. |
| **Koka** | Functional + Perceus RC | Same spirit as deferred **D8**: reuse when unique. |

### Mostly immutable, mutation explicit

| Language | Notes |
|----------|--------|
| **OCaml / F# / Scala** | Immutable by default; `ref` / `mutable` / `var` when needed. Persistent collections in ecosystem. |
| **Swift** | Value types + **COW** for `Array`/`Dictionary`/`String` — industrial “feels immutable, shares until write.” |
| **Rust** | Immutable by default + ownership; not persistent sharing by default (clone often deep). |
| **Racket / Scheme** | Both immutable and mutable variants of structures. |

### Libraries into mutable languages

- **Immutable.js**, **Immer** (JS)
- **Pyrsistent** (Python)
- **paguro** / Vavr (Java)

**Best product references for Thoxa’s memory story:** Clojure (pure persistent DS) and Swift (COW + value semantics without full purity).

---

## Techniques (beyond “append shares a tail”)

1. **Structural sharing (persistent DS)**  
   - Lists: cons — share the tail.  
   - Vectors: wide trees (32-way tries) — path copy O(log₃₂ n) nodes, rest shared.  
   - Maps: **HAMT** (Clojure) — same idea for keys.  
   - Ropes / chunked sequences for big strings/bytes.

2. **Copy-on-write (COW)** — what Thoxa `Buffer` already does  
   Share until a write; if RC==1, mutate in place; else clone.  
   Feels mutable at the API, immutable at the semantics *if* aliasing is never exposed. Swift’s model.

3. **Uniqueness / ownership / Perceus**  
   If the compiler knows a value is unique, “update” is free in-place mutation.  
   Koka/Roc/Perceus: pure *surface* without pure *implementation cost*. Roadmap already points here (**D8**).

4. **Builders / freeze**  
   Mutable under the hood (`Buffer`), then `freeze()` → immutable `Bytes`/`Array`.  
   Industry: `StringBuilder`, Clojure transients, Rust `Vec` then freeze.

5. **Ephemeral / transient**  
   Clojure transients: drop persistence for a batch of updates, then freeze. Big win for bulk construction.

Thoxa already has pieces of (2) and (4). Full immutability pushes toward (1)+(3) for collections people actually use (maps, vectors, request trees).

---

## Impact if Thoxa went “immutable only”

### Programming model

**Wins**

- No aliasing surprises (the real cost of **D54** shared array mutation).
- Easier reasoning; safer concurrency *if* threads appear later (immutable data is freely shareable).
- Matches functional surface already liked: `append` returns new, `match` + `May`/`Trial`, pure-ish `handle(req) -> Response`.

**Costs**

- Naturally in-place algorithms (sort, fill buffer, parse into fixed slots, ring buffers) get uglier or need builders.
- Nested record “update field” needs lenses/with-syntax, reconstruction, or compiler smarts.
- Teaching: people expect `a[i] = x` to mutate; pure languages often use `assoc` / `set` / `a.with(i, x)`.

### Performance & memory

| Area | Effect |
|------|--------|
| **Allocation rate** | Higher without uniqueness opts; every “edit” allocates a path of nodes or a new header. |
| **Peak memory** | Often *better* when many versions coexist (undo, branching configs, parallel snapshots) because of sharing. |
| **Steady-state single-version** | Can be *worse* than a dense mutable array (pointer-chasing trees vs contiguous `T[]`). |
| **Cache** | Contiguous arrays win; persistent vectors are good-but-not-C. |
| **RC pressure** | More short-lived nodes → more retain/release. Perceus / coalescing helps a lot. |
| **HTTP hot path** | Parsing into immutable trees is fine; *rewriting* large bodies without builders hurts. Builders + freeze stay essential. |

Rule of thumb: **immutability is cheap for sharing history, expensive for single-threaded bulk mutation** unless you have COW / uniqueness / transients.

### Runtime & compiler (Thoxa-specific)

Today: simple RC, C codegen, contiguous-ish arrays, Buffer COW, C-layout structs for FFI.

Full immutable would push toward:

1. **Persistent `Array` / `Map` in `thoxa_rt`** — not a small change; bigger than current `ThoxaArray`.
2. **No true shared mutability** — ban index-set on shared arrays, or make set always return a new array (path-copy).
3. **Kill or quarantine `mut`** — module-level mut (**D34**) and method field write-back (deferred File type) become design problems.
4. **FFI friction** — C wants `char*`, mutable structs, `Address<T>`. Pure languages usually have an **impure edge**: `Bytes` builder, `extern`, maybe `Unsafe` or an `Io` effect. R6–R7 is already that edge.
5. **Perceus becomes more valuable** — pure surface + unique in-place updates (Koka, Roc, etc.).
6. **Effects** (deferred **D2**) pair well: pure values + effectful IO, instead of mutable globals for “the world.”

### Product / ecosystem

- **HTTP ladder** can stay mostly pure: `Request`/`Response` are value-shaped; Router `handle` is pure-ish. The socket loop is the impurity.
- **Interop identity** — “native, C-friendly, systems-capable” fights total purity unless immutability is **only** for the high-level surface and FFI stays impure.
- **Oracle / SPEC** — list semantics already lean functional append; tightening immutability is more “close the hole on shared mutation” than inventing a new language.

---

## Design spectra (not a mandate)

Realistic points on a line:

1. **Status quo** — hybrid; functional append; shared array mutation allowed (**D54**). Fast, C-friendly, alias bugs possible.

2. **Value semantics + COW** (Swift-like)  
   - No user-visible references.  
   - Assignments copy headers; storage shared until write.  
   - `a[i]=x` is logically a new array; unique → in-place.  
   - Fits current Buffer + RC mindset; smaller leap than full persistent tries.

3. **Immutable collections + builders** (Clojure-ish surface)  
   - `Array`/`Map` persistent.  
   - `Buffer` / transients for construction.  
   - `mut` only for locals the compiler proves unique, or removed.

4. **Pure language + effects + Perceus** (Roc/Koka-ish)  
   - No refs in the language.  
   - IO/net as effects.  
   - Aggressive reuse when unique.  
   - Biggest rewrite of model; best long-term story for “immutable *and* fast.”

For phase-A goals (native → C, HTTP, FFI, conformance ladder), **(2)** is the high-leverage middle: immutable mental model without throwing away C-layout and sockets. **(3)** pays off for cheap snapshots, undo, concurrent readers, map-heavy code. **(4)** is a language-identity choice, not a one-rung feature.

---

## Concrete vignettes

**Append-heavy log**  
Immutable vector with sharing: many versions, low extra memory. Mutable array: one version, denser. COW/unique: often as fast as mutable for the single-owner case.

**`req.headers` with one header changed**  
Persistent map/array: O(log n) new nodes. Deep copy of whole request: painful. Today with shared mutability: accidental cross-request bugs if header arrays are ever shared.

**Fill a 64 KiB read buffer from `recv`**  
Pure immutable `Bytes` every 4 KiB chunk without a builder → death by allocation. Need `Buffer` / transient / `Bytes(n)` then freeze — already half-built.

**FFI `tcgetattr` into a C struct**  
Immutable language either: (a) opaque mutable blob at the edge, or (b) copy in/out to immutable fields. C-layout + `mut` fields is (a). Pure-only makes (a) an explicit unsafe/edge feature.

**Concurrency later**  
Immutable wins hard: share `Router` config and parsed templates across threads with no locks. Mutable shared arrays become a minefield.

---

## Bottom line

- The technique meant here is standard: **persistent data structures** and/or **COW + uniqueness**. Clojure and Swift are the clearest industrial poles; Haskell/Elm for purity; Koka/Roc/Perceus for “pure looks, fast under the hood.”
- **Impact** is not free memory forever — trade allocation and indirection for safety, sharing, and simpler concurrency, then claw performance back with builders, COW, and uniqueness (Perceus).
- **Near-term form** (if ever promoted) is not “delete all references tomorrow,” but:
  1. Make collection updates **semantically** immutable (fix **D54** shared mutation),
  2. Implement with **COW or path-copy + RC**,
  3. Keep **Buffer / freeze** and **FFI mutability** as the impure edge,
  4. Let **Perceus** later turn unique updates into real in-place mutation.

Useful forks to design against later: **Swift-style values only** vs **Clojure-style persistent collections** vs **pure + effects** — they look similar in demos and diverge hard at FFI and hot loops.

---

## Open questions (if ever woken)

- Close D54 only (semantic immutability of Array set) without full language purity?
- Persistent vector runtime vs COW contiguous arrays first?
- Does `mut` stay as “unique local rebinding” or vanish from the surface?
- How hard a line between pure Thoxa values and C-layout / `Address` / `Bytes`?
- Pair with effects (D2) or stay with free IO + impure edge?
