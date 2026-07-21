# Thoxa Revival — Design

**Date:** 2026-07-12
**Status:** Approved; implementation in progress
**Author:** Joshua + Claude (brainstormed)

## 1. Background

Thoxa is a novel systems programming language. It has been attempted three times
(all preserved read-only under `old/`):

| Tree | Host / Backend | Outcome |
|------|----------------|---------|
| `old/ThoxaFail`  | Rust → Cranelift native (JIT+AOT) | Named "Fail"; abandoned. Drowned in codegen: 128-bit fat pointers dead-ended under Cranelift, two string ABIs shipped at once, generics/variants/match/`?` were docs-only. |
| `old/Thoxa`      | Rust → QBE native | The high-water mark: working native binaries, ARC, arrays, interpolation, `May`/`Trial` matching. But user generics never worked (monomorphization substituted signatures, not bodies) and inference was stubbed. |
| `old/ThoxaNotebook` | Python tree-walker + web notebook | The latest. Fast syntax iteration via a Lark grammar + a genuinely good teaching notebook — but the type checker, `match`, `?`, and string interpolation were unimplemented; ~40 tests parked as `.disabled`. |

**The one durable lesson:** across all three rewrites the *language spec barely changed* while
the *backend was thrown away every time*. The recurring failure was a fully-designed spec that
always outran a half-built implementation, plus docs/CHANGELOGs that claimed "100% complete"
for features that returned literal `[Any value]` or always printed `42`.

A comparative study of ~12 other new systems languages (Rust, Zig, Nim, V, Crystal, Hare, Roc,
Koka, Vale, Gleam, Mojo, Odin, Nelua/C3) reinforced the plan below. Full archaeology and wisdom
dossiers live in the scratchpad notes referenced by this session.

## 2. Decisions

### 2.1 Strategy

- **Identity:** a novel systems language, but currently **UNFROZEN** (pre-alpha). We iterate on
  syntax/semantics in a fast interpreter + literate notebook; native compilation is **deferred**.
- **Host language:** **Python 3.13** (maximum iteration velocity; we already have a working
  interpreter and a proven QBE backend in `old/`). A one-time port to a WASM-friendly static host
  (OCaml is the front-runner) is deferred to the freeze — *never* re-host mid-churn (Roc/Gleam's
  most expensive mistake).
- **Backends:** interpreter-only for now. A **stable typed-AST seam** separates frontend from
  evaluator so a lowering→IR→QBE/transpile-to-C path can slot in later without touching the
  frontend. This is the concrete fix for "the backend got thrown away every time."
- **Memory:** **immutable by default.** The interpreter uses host memory as *explicitly-labelled
  scaffolding*; the design commits to **Perceus-style precise reference counting** (Koka) before
  1.0. Immutability confines reference cycles to a small, statically-known set (explicit mutable
  reference types only), which nearly dissolves the cycle problem all three prior attempts left open.
- **Parser:** hand-written **recursive-descent + Pratt** (control over `<…>` disambiguation and
  no-separator statement boundaries; best-in-class error messages, which are core to the pitch).
- **Type inference:** local/bidirectional, **never global** (Crystal's decade-long regret).
- **Errors:** friendly, source-spanned diagnostics from day one.

### 2.2 Syntax (the reconciled canonical surface)

Every decision below was chosen deliberately (previous surfaces disagreed with each other):

- **Comments:** `#` line, `##` doc.
- **Identifiers:** HARD-enforced at the lexer — `camelCase` for values/functions, `PascalCase`
  for types; underscores are illegal. (Distinctive signature feature.)
- **Statement separators:** none required (no significant newlines, no semicolons). Revisit only
  if genuine ambiguity appears.
- **Primitives:** PascalCase — `Int8…Int64`, `UInt8…UInt64`, `Int`(=`Int64`), `Byte`(=`UInt8`),
  `Float32`/`Float64`, `Float`(=`Float64`), `Bool`, `Char`, `String`, `Void`.
- **Types are first-class values (hybrid):** you can bind/pass/inspect types (`let T = Int`), but
  primitives are compiler-provided (no `@intType` bootstrapping).
- **Strings:** interpolation-only — `"{expr}"`, escape with `{{` / `}}`, multi-line auto-dedent.
  No `+`/`++` concatenation operator.
- **Bindings:** `let x` immutable, `mut x` mutable (standalone `mut`). Optional `: Type`.
- **Aggregates:** `type Name(ctorParams) { members }`; members default **private**, `export` to
  expose; fields via `let`/`mut`; methods via `fn`; **implicit field access, no `self`**.
- **Sum types:** separate constructor types + union alias —
  `type Has<T>(value: T) {}` … `type May<T> = Has<T> | Not`.
- **`match`:** block-only arms `Pattern { body }`; catch-all is a final **`else { body }` arm**,
  **required unless the match is provably exhaustive**.
- **Parameters:** named by default; **call-site order enforced** (labels required, in declaration
  order); defaulted params may be omitted; **`!` marks a positional param** (no label, no default);
  positionals precede named in the signature; `...name: T` variadic (last).
- **Generics:** angle brackets `<T>`; explicit and inferred instantiation; `Array<T>`.
- **Conversions:** the full `as` / `can` / `is` triad is retained in the vision (deferred to the
  type-system milestone). No implicit coercion.
- **Logical operators:** keywords `and` / `or` / `not`.
- **Error handling:** `May<T> = Has<T> | Not`, `Trial<T,E> = Pass<T> | Fail<E>`, `?` propagation.

### 2.3 The three cures (for how Thoxa died before)

1. **Executable spec.** `spec/conformance/*.thx` with expected output *is* the spec; a feature is
   "done" only when its conformance test passes. Replaces the `.thx.disabled` graveyard.
2. **Friendly diagnostics from day one** — one `errors` module with spans, carets, suggestions.
3. **Local inference, never global** — keeps iteration incremental and fast.

## 3. Architecture

```
source → lexer → parser (recursive-descent + Pratt) → typed AST ──▶ interpreter (tree-walk)
                                                          │
                                                          └────────▶ [ lower → IR → QBE / C ]  (deferred)
```

Layout:

```
thoxa/     lexer, tokens, ast, parser/, checker/, interp/, prelude, builtins, errors, cli, repl
spec/      VISION.md · SPEC.md · conformance/*.thx (+ expected output)
notebook/  literate interactive tool — local kernel over the real interpreter
examples/  demos + future/ north-star apps
docs/      checked against the spec
old/       the three prior trees, read-only (git-ignored)
```

## 4. Milestones

Each milestone is end-to-end and gated on conformance tests — no parsing features we haven't
implemented.

- **M0** — clean base: scaffold + VISION + SPEC + conformance harness + honest CHANGELOG + unfrozen README.
- **M1** — core: lexer/parser/AST/interpreter for `let`/`mut`, `fn`, `type` w/ fields+methods,
  `if`/`loop`/`while`/`for`, arrays, blocks, string interpolation, hard-enforced naming, diagnostics.
- **M2** — `May`/`Trial` as real generic unions + `match` (block arms, `else`, exhaustiveness) + `?`.
- **M3** — named-argument dispatch (order-enforced, defaults, `!` positional, variadics).
- **M4** — type checker + local inference + `as`/`can`/`is`.
- **M5** — notebook (local kernel over the interpreter) + literate docs sharing the engine.
- **M6** — first reference app: a web server via interpreter + native host functions.
- **Deferred (post-freeze):** typed IR + QBE/transpile-C backends · Perceus RC · OCaml port +
  WASM playground · editions.

## 5. Risks & mitigations

- **Spec outruns impl** → conformance-gated "done"; build strictly milestone by milestone.
- **`<…>` + no-separators ambiguity** → hand-written parser with explicit disambiguation; if a real
  ambiguity survives, reintroduce optional separators (pre-agreed escape hatch).
- **Memory model becomes the project (Vale)** → scaffolding memory now; Perceus is a later, opt-in
  optimization, not a prerequisite for language iteration.
- **Dishonest status (V, past Thoxa)** → CHANGELOG entries cite the conformance test that proves them.
