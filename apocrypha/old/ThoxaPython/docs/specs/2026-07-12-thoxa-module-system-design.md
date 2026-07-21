# Thoxa Module System — Design

**Date:** 2026-07-12
**Status:** Implemented as M8 across milestones M-MOD-1..4 (see CHANGELOG); this is the as-built
design. Resolves every open module question parked in the FFI spec's §8.

## 1. Motivation

A real standard library needs a module system. Today Thoxa is single-file: FFI
externs are file-local, `export` is parsed but does nothing across files, and the
only import that resolves is the built-in `"ffi"` pseudo-module. This spec makes
`.thx` files composable so a stdlib — written in Thoxa itself — becomes possible.

The design is driven by one question Joshua flagged from the start: **is a module
a plain namespace of names, or is it an *instantiable value* the way the two
prior attempts modeled it?** Both `old/Thoxa` and `old/ThoxaFail` landed on
"a module *is* a singleton unit; a type is a parameterized module." §3 works
through why we keep that as the *implementation* model but not the *user-facing*
semantics.

### Non-negotiables carried in from the language

- **Zero runtime dependencies.** Resolution and loading are pure interpreter
  logic; `pyproject.toml` stays `dependencies = []`.
- **Casing rules hold.** A module namespace is value-side → `camelCase`. Its
  members keep their own casing (`strings.split` value, `strings.Builder` type).
- **Declare all names.** The rule that made `errno` an explicit import is
  preserved, and *sharpened* (§4): every name in scope is either the language's
  closed prelude or an explicit import — nothing else is ambient.
- **Immutability by default.** Module-level mutable state is allowed but
  *contained* (§9): private, reached only through exported functions.

## 2. Scope

**In scope:**

- Every `.thx` file is a module; `export` publishes top-level `let`/`fn`/`type`.
- Resolution: `./` (local), `~/` (project root), and named roots (`std/…`, `ffi`).
- The prelude, delivered as a real auto-imported `std/prelude` module.
- Import surface: selective `{ … }`, namespace, `as` aliasing; no user-facing glob.
- Module top level is declarations-only; two-phase load with defined init order.
- Circular imports: allowed for `fn`/`type`, an error only for value-init cycles.

**Out of scope (deferred, §13):** module-as-first-class-value / functors,
re-exports, a package manager, hot reload, and the concurrency model that gates
*direct* external mutation of module state.

## 3. The binding model — namespace to the user, "instantiate once" underneath

Two real rationales pull toward the prior "module is a value" model:

1. **Easy module-level mutable state.** Rust's global-mutable story is painful
   (`static mut` + `unsafe`, or `Mutex`/`OnceLock` ceremony). Thoxa should make a
   module-scoped mutable trivial.
2. **Types are values.** Thoxa already has first-class types (`let k = Int64`). If
   modules are types and types are values, modules "should" be values too.

Neither actually forces module-as-value:

- **Rationale 1 is satisfied without it.** A module is evaluated *once*; a
  top-level `mut` is a single cell the module's functions close over. That is a
  first-class global mutable with no ceremony — and it needs only "instantiated
  once," not "is a value." See §9.
- **Rationale 2 equivocates.** `Int64`-as-a-value is an *opaque token* the checker
  types as `Type`. A module-as-value, to be useful, needs member access with
  statically-known members and their types — i.e. a **structural signature** per
  module. That is a much larger commitment (ML-style signatures / structural
  typing) than "types are values," and its payoff (functors, dependency
  injection, runtime module selection) has no driver today.

**Decision: modules are namespaces to the user; "a module is a paramless type,
instantiated once" is the *implementation* model, never surfaced as a value.**
No `Module()`, no passing a module, no mutable module singleton exposed. The
namespace name is valid only to the left of a `.` — you cannot bind or pass it.

This is **forward-compatible** with the value model: because a module already
*is* a paramless type internally, reifying it as a value (plus signatures) later
is purely additive and breaks no existing import. We bank rationale 2; we don't
build it until a functor/DI driver appears.

## 4. Files, resolution, and the prelude

### 4.1 Resolution — three origins, never climb

The `"…"` in `from "…"` is a **logical specifier**, not a filesystem path. Its
leading form names one of three origins:

```thoxa
import { connect } from "./sockets"       # local — sibling file
import { Codec }   from "./codec/json"     # local — child directory
import { config }  from "~/config"         # project — from the project root
import { sqrt }    from "std/math"         # stdlib — a named root
import { errno }   from "ffi"              # built-in — a named root (already shipped)
```

- **`./…` → local.** Relative to *this file's* directory; reaches siblings and
  children only. A `..` segment anywhere is a **compile error**:
  *"upward imports aren't allowed — address it from the project root with `~/…`."*
- **`~/…` → project.** Always resolves from the project root, wherever the
  importing file sits. This is the replacement for `../`: you never climb, you
  start from the top.
- **bare first segment → a named root.** `std/…` is the standard library; `ffi`
  is the built-in FFI module; future *package* names slot in here. `.` and `~` are
  the two relative roots; everything else is a named root you cannot reach by
  walking the filesystem. A bare specifier whose first segment is not a known root
  is an error — no guessing.

The **project root** is the directory of the entry file today; it becomes a
configured/anchored root when a package manager lands. File basenames are
`camelCase` (they name a value-side namespace): `sockets.thx`, `httpClient.thx`.
The `.thx` extension is implicit in specifiers.

### 4.2 The prelude — closed, and delivered as a module

Thoxa's "declare all names" rule is **sharpened**, not weakened:

> **Every name in scope is either part of the language's prelude — a small,
> closed, spec-enumerated set — or explicitly imported. Nothing else is ambient.**

This is *more* disciplined than today, where the ambient set is whatever happens
to sit in the interpreter's builtin table. The prelude becomes a fixed list:

- **Algebraic vocabulary:** `May`, `Trial`, `Has`, `Not`, `Pass`, `Fail`
  (load-bearing for `?` and `match`).
- **`Array<T>`** and its literal methods (`.count`, `append`, …).
- **`print` / `println`** — the single I/O carve-out, blessed for the teaching
  notebook's sake. *All other* I/O (files, sockets, `errno`) is imported.

The distinction is **language vs. capability**: the prelude is core vocabulary
with no hidden state or capability; anything that reaches the outside world is
named at an import boundary. Mechanically the prelude is a real `std/prelude`
module the compiler **auto-imports** into every file — no separate "magic
builtin" concept, self-hostable, and later openable to an opt-out. The prelude
set is enumerated in `SPEC.md`; growing it is a deliberate spec change.

## 5. Import surface

```thoxa
import { sqrt, pi }     from "std/math"      # selective
import { sqrt as root } from "std/math"      # selective + alias
import strings          from "std/strings"   # namespace → strings.split(…)
import strings as str   from "std/strings"   # namespace + alias
```

- **Selective `{ … }` vs. namespace (no braces).** Braces bind names directly; a
  bare identifier binds a **namespace**. (This is the shape already shipped for
  `from "ffi"`.)
- **Namespace binding.** `camelCase`; its members keep their own casing. The
  binding identifier must equal the module's canonical name (the specifier's last
  segment) unless renamed with `as`. A namespace is **not a value** — valid only
  left of a `.`; you cannot `let x = strings` or pass it (that is the deferred
  module-as-value line, §13). Note a *type* imported from a module *is* a
  first-class value as usual; only the namespace qualifier is not.
- **`as` aliasing** in both forms, reusing the existing keyword (unambiguous
  inside an import clause). An alias cannot cross casing kinds: `{ foo as bar }`
  stays `camelCase`, `{ Foo as Bar }` stays `PascalCase`.
- **No user-facing `import *`.** A glob you cannot read fights "declare all
  names." The prelude's auto-import is the one spec-blessed exception, permitted
  precisely because its contents are enumerated.
- **Exportable:** top-level `let` / `fn` / `type` via `export`; `extern fn` too
  (§10). A top-level **`mut` is always private** — module-internal state reached
  through exported functions, never exported directly (§9). Members remain
  private by default.
- **Re-exports** (`export { x } from "…"`) — deferred unless assembling
  `std/prelude` forces them; added then if needed.

## 6. Module top level & initialization

**Imported modules are declarations-only.** A module's top level is
`let` / `mut` / `fn` / `type` / `import` / `export` — no free-floating executable
statements. The **entry file** (the one you run) keeps its script body: top-level
statements plus an optional `fn main() -> Int64`.

Rationale: an imported module is a namespace. Letting it run arbitrary code the
first time it is imported makes import order silently become execution order —
the prior art's `println("module loaded")` footgun. Python has this hazard and
papers over it with `if __name__ == "__main__"`; Thoxa makes the clean cut. The
asymmetry is honest: the entry file *is* the program; a library is a bag of
declarations.

**Loading is two-phase:**

1. **Register.** Resolve and parse each module; register its declaration *names*
   plus `fn`/`type` bodies. Mutually-referential types and functions across files
   resolve here.
2. **Initialize.** Evaluate top-level `let`/`mut` initializers in dependency
   order. Each module initializes **exactly once**; subsequent imports reuse it.

## 7. Circular imports

Cycles are allowed for the common, legitimate case and rejected only when truly
unresolvable:

- **`fn`/`type` mutual recursion across files** — allowed. A type in `a.thx` may
  reference a type in `b.thx` and vice versa; ditto functions. These resolve in
  phase 1 (register), which needs no evaluation order.
- **Value-initializer cycles** — a compile error. If `a`'s top-level `let` needs
  `b`'s top-level `let` which needs `a`'s, there is no valid init order; the error
  names the cycle path.

A blanket "any cycle is an error" would forbid legitimate cross-file mutual
recursion, so we do not do that.

## 8. Interaction with existing language rules

- **Casing.** Namespaces are `camelCase` (value-side); imported types/functions
  keep their declared casing; selective imports self-document (`{ split, Builder }`
  → value, type).
- **Declare all names.** Preserved and sharpened (§4.2). The checker's existing
  undefined-name path enforces it: a name that is neither prelude nor imported is
  an error.
- **Types are values.** Unchanged for types; the *namespace* is the one new
  non-value binding, which keeps the door open to module-as-value later (§3, §13).
- **`?` / `match` / conversions.** Unaffected — they operate on values and types,
  which cross module boundaries as ordinary imported names.

## 9. Module-level mutable state — the "global mutable" answer

This is rationale 1 delivered without module-as-value:

```thoxa
# counter.thx
mut count = 0                                    # module-level, initialized once
export fn bump() -> Int64 { count = count + 1  count }
export fn current() -> Int64 { count }
```

`count` is a single mutable cell created once when the module first loads; the
exported functions close over it. No `unsafe`, no `Mutex` — a first-class global
mutable, which Rust makes you hand-roll.

**Access is contained, not (yet) direct.** Module globals are **private**; you
mutate them only through exported functions. Direct external mutation
(`counter.count = 5` from another file) is **deferred** — it sprays the mutation
surface across every call site and is the hardest thing to retrofit any
concurrency discipline onto. Containment is not a safety guarantee (even
single-threaded coroutines have reentrancy) but a *retrofit* guarantee: the whole
mutation surface is a set of named functions we can later constrain however the
concurrency model demands. The interpreter is single-threaded today, so there is
no live hazard regardless. **Concurrency is its own future spec** (blank slate —
no design exists in the current repo or the prior attempts); the direct-mutation
question is gated on it.

## 10. How externs and `ffi` migrate

No change to the `extern fn` surface. Two adjustments:

- **`extern fn` is exportable.** It is a top-level declaration like any other, so
  a module may `export extern fn …` — or, more idiomatically, keep the raw extern
  private and export a safe wrapper that lifts `-1`/`errno` into `Trial`. A
  future `std/sys` (or similar) module is where the raw libc layer would live.
- **`ffi` becomes a named root.** The built-in `import { errno, strerror } from
  "ffi"` is already a logical name, not a path; under §4.1 it is simply a
  reserved named root alongside `std/…`. Zero surface churn.

## 11. Errors

- Unknown specifier / unresolvable module → located error naming the specifier.
- `..` in a `./` specifier → error with the `~/` fix-it (§4.1).
- Bare specifier with an unknown first root → error listing known roots.
- Importing a non-`export`ed name → error ("`x` is not exported by `…`").
- Namespace binding name ≠ canonical name without `as` → error.
- Value-initializer cycle → error naming the cycle (§7).
- Using a name that is neither prelude nor imported → the existing
  undefined-name error (§8).

## 12. Milestones

Each milestone = interpreter/checker changes + at least one conformance `.thx`
program + tests. A feature is "done" only when its `spec/conformance/` test passes.

| ID | Milestone | Proves |
|----|-----------|--------|
| **M-MOD-1** | Cross-file resolution (`./`, `~/`, `std/`) + selective import; two-phase load; `export`-gating enforced. | A multi-file program runs; only exported names cross files. |
| **M-MOD-2** | Namespace import + `as` aliasing; namespace-is-not-a-value checker rule; no glob. | `strings.split` qualified access; collisions resolved with `as`. |
| **M-MOD-3** | The prelude as an auto-imported `std/prelude` module; "closed prelude or import" enforced. | Prelude-only files need no imports; non-prelude use requires an import. |
| **M-MOD-4** | Circular imports (`fn`/`type` mutual recursion works; value-init cycles error); module-private `mut` via exported functions. | Cross-file mutual recursion; a stateful counter module. |
| **M-MOD-5** | `ffi` as a named root; seed one or two real stdlib modules written in Thoxa (e.g. a `std/sys` wrapper over externs lifting `-1`/`errno` into `Trial`). | A Thoxa-written stdlib module that itself uses `extern`. |

## 13. Deferred / roadmap

- **Module as a first-class value / functors** — passing a module, runtime module
  selection, dependency injection. Needs module *signatures* (structural typing).
  The namespace-first design is a non-breaking upgrade path (§3).
- **Re-exports** (`export { x } from "…"`) — add when the prelude or a facade
  module needs them.
- **Direct external mutation** of module state — gated on the concurrency model.
- **The concurrency model itself** — async/await vs. structured concurrency vs.
  actors vs. CSP; deeply entangled with Perceus RC. Its own spec, when a driver
  appears.
- **Package manager** — more named roots, versioning, resolution config.
- **Hot reload** — reload modules during notebook/dev sessions.

## 14. Resolved

- **Binding model:** namespace to the user; paramless-type-instantiated-once
  internally; forward-compatible with module-as-value (§3).
- **Resolution:** `./` local (no `..`), `~/` project, bare named roots (§4.1).
- **Prelude:** closed set incl. `print`/`println`; auto-imported `std/prelude`;
  "closed prelude or explicit import" (§4.2).
- **Import surface:** selective + namespace, `as`, no glob; export `let`/`fn`/
  `type`/`extern fn`; top-level `mut` always private (§5, §9).
- **Top level:** imported modules declarations-only; entry file keeps its script
  body; two-phase load (§6).
- **Cycles:** allowed for `fn`/`type`; value-init cycles are errors (§7).

## 15. Known limitations (as-built, after adversarial review)

The runtime (`thoxa run`) catches all three of these cleanly; they are gaps in the
static checker's coverage or in cross-cell reuse, left for later. They sit within
the checker's stated **advisory** posture (SPEC §3: the interpreter is the runtime
source of truth).

- **Cross-module types are keyed by bare name.** The checker cannot tell a
  dependency's `Point` from the entry's `Point`, so a same-name collision across
  modules can produce a false accept (a member access that fails only at run
  time). The fix is module-qualified type identity — a larger checker change.
- **Transitive value-init cycles are invisible to `check`.** The checker detects
  only a direct self-import; a real cross-file value cycle is caught by the
  interpreter (`value-initializer cycle: …`), not statically. A static check would
  need a cross-module initialization graph.
- **Imported-module `mut` state resets between REPL/notebook cells.** `run_module`
  rebuilds its registry each call, so a stateful imported module re-initializes
  per cell while entry-level globals persist. Latent through the shipped CLI
  (in-memory cells can't `./`/`~/`-import), but it bites an embedder that reuses
  one `Interpreter`. Whether module state *should* persist across cells is a
  design call tied to the eventual notebook/session semantics.
