# Growable `Buffer` — design & the thought line

**Status:** implemented 2026-07-15 (interpreter + checker + conformance).
**Files:** `thoxa/ffi.py` (`Buffer`, `_Alloc`, `Bytes.from_bytes`), `thoxa/interp.py`
(dispatch + the aliasing hook), `thoxa/checker.py` (`_resolve`), `spec/conformance/buffer.thx`,
`tests/test_buffer.py`.

## Why this exists, and why it's not just `bytearray`

We are prototyping Thoxa in Python, but Thoxa is a **systems language** that will leave
Python for a native backend (typed IR → QBE / transpile-to-C) with **Perceus reference
counting** before 1.0. So a growable byte buffer cannot be a thin wrapper over Python's
`bytearray`: that would silently paper over exactly the things a systems language must get
right, and let programs that are memory bugs in the real language "work" in the prototype.

A growable buffer is also the **first place Thoxa's central promise is tested**: *Rust-like
safety without a borrow checker.* A fixed `Bytes` dodges the question — you can't grow it, so
a pointer into it stays valid. The instant a buffer can **reallocate**, every alias into it can
dangle. That is precisely what Rust's borrow checker exists to prevent and what we've said we
will *not* have. So the buffer's design *is* our first concrete answer to "how do we stay safe
without that checker."

## The model we chose (and the two we didn't)

Three coherent ways to make aliasing + mutation safe without a borrow checker:

1. **Refcount + in-place-iff-unique, copy-on-write when shared** *(chosen)* — values are
   immutable-by-default; a uniquely-referenced buffer mutates/grows in place, a shared one
   copies before it's written, so **you never mutate shared memory** → nothing dangles → safe
   *by construction*. This is exactly what Koka and Lean 4 get from **Perceus** statically, and
   what **Swift** (`isKnownUniquelyReferenced` + COW on `Array`/`String`) and **Rust**
   (`Rc::make_mut`) do with a runtime uniqueness check. It is continuous with our committed
   endgame (immutable-by-default + Perceus RC), so we model it now rather than rip something out
   later.
2. **Value semantics + moves (Rust minus the checker)** — assignment moves, use-after-move
   traps, slices borrow and are invalidated by growth. Faithful, but it reintroduces the
   move/borrow friction we're explicitly avoiding.
3. **GC sharing + runtime validity checks** — keep sharing, trap on stale slices. Simplest, but
   it *detects* the hazard instead of *preventing* it, and its lowering story is the least honest.

We chose **①**.

## How it's built

A `Buffer` is a small **handle** over a separately reference-counted **allocation** — the same
split Swift's `Array` uses:

```
Buffer  = { alloc, len, consumed }          # the handle (value-semantic, cheap to copy)
_Alloc  = { store: bytearray, cap, rc,       # the heap block: refcounted, malloc/realloc/free
            reallocs }                       #   maps to  struct { u8* ptr; usize len, cap; }
```

- **Allocation + geometric growth.** `append` doubles `cap` and copies on overflow (amortised
  O(1)); each growth bumps `_Alloc.reallocs`. Capacity is explicit and observable (`.capacity`)
  so a test can *see* the `0 → 8 → 16 → 32` doubling rather than trust it. This *is* the
  allocator: a new, larger block + a copy = `realloc`, which is why a raw pointer into the old
  block would dangle.
- **dup / copy-on-write.** Binding a buffer from an existing owner (`let a = b`, a call argument,
  a field init) is a **share**: a fresh handle over the *same* allocation, `rc += 1`
  (`Buffer.share`, hooked in the interpreter's `_maybe_alias` at let / assign / `bind_args`). Any
  mutation first calls `_ensure_unique`: `rc == 1` → write in place; `rc > 1` → copy this
  handle's allocation out first (and `rc -= 1` on the one it left), so aliases never observe the
  write. Because the handle bound to the variable is mutated in place (its `alloc` pointer is
  swapped), no environment rebind is needed — the variable sees its own private copy, the alias
  keeps the original.
- **Ownership transfer at the FFI boundary.** `freeze()` **consumes** the buffer and returns a
  fixed `Bytes` (stable pointer, FFI-safe) — the `BytesMut → Bytes` handoff. Using a buffer after
  `freeze` is a use-after-move error. `Buffer` is deliberately **absent from
  `MARSHALABLE_PARAMS`**, so the checker rejects it at any `extern` boundary; `ffi._to_c` is the
  runtime backstop with a clear "freeze() it first" message. A growable buffer must never reach C,
  because a later `append` could `realloc` the block out from under a pointer C still holds.

## Language surface

```
Buffer()              -> Buffer            # empty, capacity 0
Buffer(capacity)      -> Buffer            # pre-reserved
b.count               -> Int64             # logical length
b.capacity            -> Int64             # allocated size (grows geometrically)
b.append(byte)        -> Void              # amortised O(1); COW if shared
b.appendBytes(bytes)  -> Void              # append a whole fixed Bytes (this is our concat)
b.reserve(extra)      -> Void              # pre-grow for `extra` more bytes, no realloc
b.get(i) / b.set(i,b)                      # indexed read / write (set is COW if shared)
b.freeze()            -> Bytes             # consume -> fixed, FFI-safe Bytes
```

## What the prototype approximates (and the real backend will sharpen)

Honesty about the gap, so we don't mistake the prototype for the target:

- **No precise `drop` at last-use yet.** We hook `dup` at binding sites; we do not decrement a
  buffer's refcount when a binding dies at scope exit (the tree-walker has no dup/drop-insertion
  pass). Copy-on-write still decrements precisely when it splits a share, so drift is bounded —
  but a buffer whose alias has died may copy *once* where Perceus would have reused it in place.
  The cost is purely **conservative**: we only ever copy too much, **never too little**, so it is
  never unsafe. The real backend's Perceus pass inserts exact `dup`/`drop` and reclaims these
  cases (garbage-free reuse).
- **`freeze` copies the live prefix.** With a unique buffer, the native impl transfers the block
  with **zero copy**; here we copy.

## Deliberately deferred

- **Slices / views** into a buffer (a borrow with a stable pointer). These reintroduce the
  invalidation hazard directly; when we add them they'll carry a generation and trap on
  use-after-realloc, or be gated behind uniqueness. Not needed yet.
- **Combinable bit-flags** (the `OpenMode` enum in `examples/http/Io.thx` is an access-mode sum
  type; real `open(2)` flags are an OR-able bitset) — waits on byte/bit ops.
- **Precise `dup`/`drop` insertion + static Perceus reuse** — a backend concern.
- **Concurrency interaction** — a shared, mutable, refcounted buffer is entangled with the
  (blank-slate) concurrency + Perceus-RC model; direct cross-thread sharing is out of scope until
  that lands.

## The payoff for the demo

`examples/http` previously sent a response as **two writes** (header, then body) precisely
*because `Bytes` had no concat*. `Buffer` closes that: build one response —
`appendBytes(header)`, `appendBytes(body)`, `freeze()` — and send it as a single stable `Bytes`.
The demo was the forcing function; the primitive is the point.
