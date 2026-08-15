# Parked: `toString` / `repr` / interpolation — not traits-first

**Status:** **sleeping** — do not implement yet.  
**Date:** 2026-08-15  
**Sparked by:** Crema `n.html()` / `htmlResponse` plus the wish for an
automatic string form (Rust `Display` / `ToString`, Python `str`/`repr`).  
**Pickup:** this file ·
[`2026-08-13-jsx-node-syntax-experiment.md`](2026-08-13-jsx-node-syntax-experiment.md)
(`.html()` as-built) ·
[`2026-08-11-first-class-functions-and-orc.md`](2026-08-11-first-class-functions-and-orc.md)
(traits as **A**, later) · D134 (no Eq/Hash traits)  
**Not** a freeze, **not** a CURRENT Now line.

---

## Why this exists

`.html()` is a special-case method on builtin `Node`. The obvious generalization
is “every value has a string form,” which *looks* like a first trait
(`ToString` / `Display`). Session 2026-08-15: **park**. Do not let Display
pull traits forward. Traits stay motivated by type-shaped injection (**A** in
the FCF note), not by interpolation.

## Three operations, one name

Do not collapse these. They already disagree in the language:

| Operation | Meaning today | Example |
|-----------|---------------|---------|
| **Decode / convert** | Encoding change, not a pretty-print | `Bytes.toString()` (UTF-8) |
| **Interpolate / dump** | Closed compiler table | `"{n}"` for Int64 / Bool / String, and Arrays/Maps of those |
| **Serialize a format** | Destination codec | `Node.html()` — escaped markup, void tags, boolean attrs |

`.html()` is a **format**. Decimal `42` is a value’s natural text. Markup is a
codec for a tree. If `Node.toString()` *is* HTML, logs and `htmlResponse` get
nicer; if it is a debug dump (`Element("div", …)`), logs get better and HTTP
gets worse. Rust splits `Display` / `Debug` / `From<String>`; Python splits
`str` / `repr`. Reusing `toString` as the protocol name collides with
`Bytes.toString()`.

**Do not replace `.html()` with a generic `toString`.** Interpolation may later
call the same render. Keep the method as “this tree as markup.”

## As-built (do not re-derive)

- `"{x}"` is a monomorphic match on `Ty` → `RValue::ToString` (Int64, Bool,
  String; Array/Map of those). User types and `Node` error.
- JSX `{e}` children: String / Int64 / Bool / Node / `Array<Node>` (scalars
  share `stringify_place` with interp). Attrs: String / Int64 / Bool.
- `logging.info` takes a `String`. Call sites interpolate first.
- `Node.html()` → runtime `thoxa_html_render`. Product
  `std/Http.htmlResponse` (**D137**) wraps doctype + `root.html()`.
- Components are types that **produce** `Node` via `.view()`, not subtypes of
  `Node`.

Every interp / JSX embed site already knows the concrete type. There is no
sample for `fn f(x: impl Show)` or `Array<dyn Show>`.

## Does this force traits?

**No** — not for the itches that actually hurt.

Traits would buy: a bound without knowing `x` at the definition, erased
“anything printable,” coherence / orphans / default impls / trait objects.
Interpolation and JSX do not need those. Same reason D134 refused Eq/Hash
traits for Map keys: the compiler can see the type.

Traits remain on the **injection** list (`serve(app)`, multi-method objects).
Display is a bad first trait (tutorial-shaped, not sample-forced).

## Ways that are not traits (when this wakes up)

Pick by itch, not by Rust nostalgia.

| | Path | What it is | When |
|---|------|------------|------|
| **A** | Grow the closed table | `Node` interpolates as `html_render` (maybe JSX text too) | `.html()` noise at one or two sites |
| **B** | Derived dump | Structural `repr` for products/sums — no override | Logs / REPL “any value prints” |
| **C** | Magic method | If the concrete type has `fn show(self) -> String`, interp uses it | A sample wants a custom label |
| **D** | Nominal traits | `impl Show`, several capabilities on one param | Real `fn f(x: impl Show)` or multi-method injection — **not** this idea’s first customer |

Also considered and **rejected as the path**:

- Module-level multi-clause `show` (D131) — who owns `show(Drink)` across
  modules; becomes a global open function.
- Auto-`show` of user types as JSX children — components already go through
  `.view() → Node`. `{drink}` as text is how you get accidental
  `[object Object]`. Keep JSX embeds closed; user types as children stay
  components or explicit fields.
- Naming the hook `toString` — `Bytes.toString()` is decode.

Session bias (not a lock): **A** if `.html()` noise bothers; **B** when
session/REPL is unparked; **C** only if a sample wants a custom label; **D**
not for strings. If a hook exists, prefer `show` / `repr` over `toString`.
A structural `repr` can always work; `show` is opt-in; `"{x}"` uses `show`
if present else `repr`. Node’s `show` can be HTML; its `repr` can be the
tree. `.html()` stays.

## Itches (so a later session does not mix them)

1. **`n.html()` is noisy** at `htmlResponse` / `logging.info(n.html())` — **A**,
   tiny, not traits.
2. **Every value should be printable** in logs/REPL — **B** or **C**. Not
   Crema-forced. Session/REPL is parked; `info` already wants a `String`.

## Non-goals while sleeping

- No compiler/runtime work from this file
- No handbook / D-id / capability row
- No CURRENT promotion unless a sample forces A/B/C or traits wake up for **A**
  (injection) and someone tries to sneak Display in as the first impl
- Do not invent Eq/Hash/Show traits as a bundle

## Promotion path

If this wakes: one slice, named (A, B, or C). Freeze + D-id only if the
surface is user-facing and lasting. Capability might fold into interpolation /
`jsx-node` gaps rather than a new row. Handbook only when shipped.

If someone wants **D** (bounds / trait objects), that is the FCF **A**
surface — implement traits for injection, and only then decide whether Show
is a std trait. Do not start traits *because* of this file.
