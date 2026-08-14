# Parked: first-class functions, closures, and ORC

**Status:** **sleeping** — full FCF / closures / ORC still parked.  
**Date:** 2026-08-11  
**Promoted from here:** alternative **C** (code pointers + bound methods, no
capture) is now freeze **D132**
([`../specs/2026-08-13-function-pointers.md`](../specs/2026-08-13-function-pointers.md)).  
**Pickup:** this file for the parked bundle · D132 freeze (**shipped**) ·
root [`CURRENT.md`](../../CURRENT.md)  
**Related:** Crema framework↔app boundary
([`2026-08-10-examples-http-crema-experiment.md`](2026-08-10-examples-http-crema-experiment.md));
value model D4 / simple RC D8; Perceus still separate deferred megawork.

---

## Why this exists

Session brainstorm (2026-08-11) explored full first-class functions for Thoxa:

- Values: pass / return / store `Fn…`
- Anonymous nested functions
- JS/TS-like **shared lexical capture** (no deep-copy of heap; shared `let`/`mut` slots)
- **Must not leak** owning cycles → whole-heap **cycle reclamation**
- Closest industrial prior art: **Nim ORC** (ARC + cycle collector), Python RC+cyclic GC

**Decision for now: park the whole bundle.** Prefer getting as far as possible
with patterns that need **no** closures, **no** heap function values, and
**no** cycle collector. Revisit when a sample forces it or when runtime maturity
is an explicit CURRENT line.

---

## What “full FCF” would have meant

| Piece | Intent |
|-------|--------|
| Function type | e.g. oracle-ish `Fn(A, B) -> R` as a real type |
| Fat value | code pointer + environment (shared scope record) |
| Capture | JS-like shared bindings; RC retain, not structural copy |
| Cycles | Owning cycles (fn↔env↔object) must not leak |
| Collector | Whole RC heap cycle collector (ORC-shaped), not Perceus |

Perceus remains **orthogonal** (precision / reuse). It does **not** replace a
cycle story for general graphs + stored functions.

### Honest naming

RC + automatic cycle reclamation **is hybrid GC** on the cycle path. Saying
“only refcounting” while reclaiming cycles is marketing. Parked design would
document: *RC-first, cycle collector for unreachable cyclic subgraphs.*

### Latent cycle hole today

Runtime is **pure RC** (`thoxa_dup` / `thoxa_drop`; free at zero). Cycles would
**leak** if formed. Current surface mostly prevents rich owning graphs (user
values are field-walking aggregates; nested User / free graph shapes limited).
Normal array/string programs are acyclic in practice — the hole is **latent**,
not a pile of known production leaks. Full FCF would make retain cycles **easy
and common**.

---

## Alternatives shortlist (brainstorm ratings)

Catalog of “callbacks / injection / routing without full FCF.” Ratings from
session owner (not decisions):

| | Technique | Rating | Notes |
|---|-----------|--------|--------|
| **A** | Interfaces / traits / protocols | **Should implement** | Type-shaped injection; formalize beyond ad-hoc `App` type import |
| **B** | Defunctionalization (enums / names + match) | **Already can; like it** | Crema string route names + `dispatch` match; typed ids better later |
| **C** | Free function pointers (no capture) | **Promoted — D132** | Freeze [`../specs/2026-08-13-function-pointers.md`](../specs/2026-08-13-function-pointers.md): `(a: T) -> R`; free + unbound + bound methods; no closures |
| **D** | SAM / single-method objects | Can do; **don’t like** | Verbose stand-in for lambdas; state on fields |
| **E** | Generics / monomorphized callable params | **Clarify existing generics** | User `type Box<T>` monomorphs exist; “`impl Fn` param” style not the same as heap `Fn` |
| F | Restricted escaping/non-escaping blocks | Nah (for now) | |
| G | Dynamic message send (ObjC/Smalltalk) | Nah | |
| H | Algebraic effects | Not yet | Roadmap megawork |
| I | Partial application as a feature | Nah | Needs FCF or desugars to wrappers |

Also discussed and not primary here:

- Call-site-only / non-escaping lambdas and route DSLs that **look** like lambdas
  but desugar to static fns (may fold into A/C later).
- ObjC-style **selectors** — close to B (typed names) or C (method/fn refs).

**Active direction:** **C** is **D132 shipped**. Push **A** (traits) later;
**B** already works. Avoid **D** as the preferred pattern; leave FCF+ORC
sleeping in this file.

---

## Non-goals while parked

- No `Fn` heap values, env records, or anonymous capturing nested fns
- No cycle collector / ORC implementation work
- No handbook claim of first-class functions
- No CURRENT “Now” line for this bundle unless explicitly promoted

---

## Open questions (if unparked later)

1. JS-like shared env vs copy-in vs ban `mut` capture — session leaned JS-like + ORC.
2. Whole-heap cycle collector algorithm (trial deletion vs Python-style, when it runs).
3. Syntax: `Fn(A)->R` vs `type F = (a: A) -> R` vs both.
4. Interaction with methods, defaults/`!`, modules as static environments.
5. Whether Crema’s ideal `serve(port, handle)` is better solved by **C** or **A** without ever unparking full FCF.

---

## Promotion path

If this wakes up: freeze under `docs/specs/`, D-id(s) for surface + runtime,
capability rows for `fn-values` / `closures` / `cycle-rc` (names TBD), handbook
only when shipped. Prefer a **forced sample** or explicit runtime milestone over
speculation.
