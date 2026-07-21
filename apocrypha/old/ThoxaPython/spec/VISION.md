# Thoxa — Vision

> The doc that never existed. Every prior attempt shipped with an empty README or
> "aims to [goals to be determined]". This is the "why it exists / who it's for."

## What Thoxa is

Thoxa is an **expression-oriented systems programming language** that aims for the safety and
expressiveness people want from Rust **without the borrow checker or the learning cliff**.

Memory safety comes from **automatic memory management** — no manual `free`, no ownership,
no lifetimes. Ergonomics come from a deliberately small, teachable surface:

- **everything is an expression** (implicit trailing returns),
- **static typing with local inference**,
- **named parameters by default**,
- **string interpolation**,
- and a friendlier algebraic-error vocabulary: `May<T>` (`Has` / `Not`) for optionals and
  `Trial<T,E>` (`Pass` / `Fail`) for results, with a `?` propagation operator.

A single `type Name(ctorParams) { export let fields, export fn methods }` construct unifies
structs and modules. An opinionated `camelCase` / `PascalCase` style is enforced by the lexer.

## Who it's for

People writing systems-adjacent programs — servers, tools, parsers, small runtimes — who want:

- **native performance eventually**, without hand-managing memory or fighting a borrow checker;
- **an error model that reads like prose** (`Pass`/`Fail`, `Has`/`Not`) rather than `Ok`/`Err`/`Some`/`None`;
- **a language you can learn in an afternoon**, taught through a live, literate notebook.

## The novelty worth betting on

1. **The `May` / `Trial` vocabulary** — the single most durable idea; it survived all three rewrites verbatim.
2. **A genuinely good interactive teaching notebook** as a first-class part of the language.
3. **Automatic memory without a borrow checker**, targeting Perceus-style precise reference counting
   (immutable-by-default makes reference cycles a tiny, statically-known set).

## Non-goals (for now)

- Not a borrow-checked language. Not manual-memory. Not garbage-collected in the long run
  (a tracing GC is temporary scaffolding only).
- Not chasing a native backend yet. The language is **unfrozen**; we perfect the surface in an
  interpreter first, then add QBE / transpile-to-C once the syntax stabilizes.
- Not promising features that don't run. **A feature is "done" only when its conformance test passes.**

## Principles

1. **Everything is an expression.** Only bindings and type definitions are statements.
2. **Safety without ceremony.** The type system catches mistakes; the memory model is invisible.
3. **Say less, mean more.** Named parameters, interpolation, implicit returns, no `self`.
4. **Honest by construction.** The spec is executable; docs cannot drift from behavior.
5. **Small core, iterate hard.** Freeze a deliberately small surface; keep changing the
   implementation underneath a loud "unfrozen" banner.
