# Thoxa PascalCase Modules & `Main.thx` Entrypoint — Design

**Date:** 2026-07-13
**Status:** Approved; to be implemented on `master`. Revises the naming decision in
[`2026-07-12-thoxa-module-system-design.md`](2026-07-12-thoxa-module-system-design.md)
(§ "Casing rules hold") and adds a project entrypoint convention.

## 1. Motivation

The module system landed with one aesthetic left half-decided. The as-built design
reasoned that "a module *is* a singleton unit; a type is a parameterized module,"
yet named module **files** in `camelCase` (`mathUtils.thx`, `geometry.thx`) and bound
the namespace with a `camelCase` name required to *equal* the file's last segment.

That conflates two things the singleton model actually separates:

- **The file describes a type.** `Http.thx` is the definition of the `Http` module —
  a type, in the "a type is a parameterized module" sense. Types are `PascalCase`.
- **The import is an instance of that type.** `import http from "./Http"` binds a
  *singleton instance*. Instances are values, and values are `camelCase`.

So the naming falls straight out of the casing rules already in the language:

```thoxa
import http from "./Http"                 # PascalCase file → camelCase instance
import { ok200, notFound } from "./Http"  # selective: members keep their own casing
```

This revision makes module files `PascalCase` and frees the namespace binding to be
any `camelCase` name the importer likes (JS-style), because an instance's local name
is the importer's business.

Separately, the entrypoint is standardized: a runnable project is a **directory with
a `Main.thx`** that `export`s a `main` function, and `thoxa run <dir>` runs it.

## 2. Naming rule

### 2.1 File modules are PascalCase

Every **file** module's name — the last segment of an import specifier — must be
`PascalCase`:

| Specifier            | Module name | OK? |
| -------------------- | ----------- | --- |
| `./Http`             | `Http`      | ✅  |
| `~/modlib/Geometry`  | `Geometry`  | ✅  |
| `std/Math`           | `Math`      | ✅  |
| `./httpResponse`     | `httpResponse` | ❌ not PascalCase |
| `./http-response`    | `http-response`| ❌ not an identifier |

A non-PascalCase file module is a compile error at the import site. The rule is about
the *module the specifier resolves to*, so it applies identically to selective and
namespace imports.

**Named roots stay lowercase.** `std` and `ffi` are *roots*, not type-modules: `std`
is a package root (a namespace of modules), `ffi` is the built-in FFI pseudo-module.
They are exempt. Std's *member* modules are ordinary file modules and so become
`PascalCase`: `std/Math`, `std/Prelude`.

### 2.2 Namespace bindings are camelCase, freely named

A namespace import binds a singleton **instance**, so the binding is a value name —
`camelCase`. Unlike the old rule, it need **not** match the file name:

```thoxa
import http   from "./Http"    # conventional
import client from "./Http"    # also fine — the instance name is the importer's call
```

A `PascalCase` namespace binding (`import Http from "./Http"`) is an error: *"a module
import binds an instance; use a camelCase name."* The old "the namespace name must be
the module's canonical name" rule is **removed**.

Because the binding is now chosen freely, the namespace `as` form is **removed too** —
it existed only to override the forced canonical name, and there is nothing left to
override. The namespace form is exactly `import <name> from "..."`. (`as` still applies
to *selective* imports, where it renames an individual imported member.)

### 2.3 Selective imports

`import { add, Point } from "./Geometry"` — only §2.1 (the file is `PascalCase`)
applies. The imported member names keep their own casing kind (`add` value,
`Point` type), unchanged.

## 3. Enforcement

One new check in the checker's import handling, plus a parser guard:

- **Checker:** after resolving a specifier to a `file` module, require its last
  segment to be `PascalCase` (first char upper). `builtin` roots (`ffi`) and the
  auto-injected prelude are exempt. This one check covers both import forms.
- **Checker:** for a namespace import, require the binding to be `camelCase` (first
  char lower). Drop the canonical-name equality check entirely.
- **Parser:** the namespace binding token must be `LOWER` (it was `LOWER`-or-`UPPER`);
  an `UPPER` binding is rejected with the instance message. The namespace `as` clause
  is dropped (§2.2): after the binding name the parser expects `from`.
  `_check_alias_casing` stays, now used by selective `as` only.

"PascalCase" / "camelCase" here means exactly what the lexer already encodes: an
`UPPER`-initial identifier vs a `LOWER`-initial one.

## 4. `Main.thx` project entrypoint

### 4.1 The contract

**A runnable project is a directory containing `Main.thx`, and `Main.thx` must
`export fn main`.** The runtime invokes that `main` from *outside* the module, so it
must be part of the module's public surface — which is *why* it is exported, in the
same module-as-type model. The entry's directory is the project source root, so `~/…`
imports resolve against it (this is already how `_entry_location` derives the root;
the convention just names it).

The contract is **name-based**: the requirement fires whenever the entry file's
basename is `Main.thx`. A `Main.thx` that lacks an exported `main` is an error:
*"Main.thx must export a `main` function."*

### 4.2 Running

- `thoxa run <dir>` → resolve `<dir>/Main.thx`; if absent, error: *"no Main.thx in
  <dir> — a runnable Thoxa project needs a Main.thx that exports main."* Project root
  = `<dir>`.
- `thoxa run <file.thx>` → unchanged. Runs that file directly, calling its `main` if
  present, with **no** export requirement. This preserves single-file scripting and
  every existing conformance case (which use a bare `fn main`). If the file *is* named
  `Main.thx`, the §4.1 export contract applies.
- `thoxa check <dir>` → same folder→`Main.thx` resolution, for symmetry.

Resolution lives in a small shared helper in `cli.py` (`_resolve_entry(path)` →
returns the concrete `.thx` path, or a located CLI error). The export check lives
where the entry module's exports are known (interpreter/checker), gated on the entry
basename being `Main.thx`.

### 4.3 Why not require exported `main` everywhere

Requiring `export fn main` for *every* run would break all conformance programs and
single-file examples for no user benefit — a one-file script has no "project." The
export contract is a property of a *project* (a `Main.thx`), not of running code.

## 5. The example — `examples/http/`

Replaces `examples/http_server.thx`. Run with `thoxa run examples/http`.

| File       | Role                                                             | Demonstrates                    |
| ---------- | --------------------------------------------------------------- | ------------------------------- |
| `Main.thx` | `export fn main()`; the `accept` loop wiring `Net` + `Http`      | project entrypoint, instances   |
| `Net.thx`  | socket/syscall layer: `extern fn`s, `sockaddrIn`, `die`; `import { errno, strerror } from "ffi"` | selective import from a builtin |
| `Http.thx` | protocol layer: `ok200()`, `notFound()` response builders       | a pure PascalCase module        |

`Main.thx` uses `import net from "./Net"` and `import http from "./Http"` —
`camelCase` instances of `PascalCase` modules — then calls `net.socket(…)`,
`http.ok200()`, etc. (If a namespace-qualified `extern fn` call proves unsupported,
`Net`'s syscalls fall back to a selective import; verified during implementation.)

## 6. Migration (repo-wide)

The rule is *required*, so the repository must obey it:

- **Rename** module files to `PascalCase`: `modlib/{MathUtils,Geometry}`,
  `cyclib/{Evens,Odds,Ping,Pong}`, `modstate/Counter`, `thoxa/std/{Math,Prelude}`.
- **Update** every import specifier and namespace binding in `spec/conformance/*`.
- **`loader.py`:** `PRELUDE_PATH` → `Prelude.thx`, prelude `Source` name → `std/Prelude`;
  `std/math` resolution unaffected (path math is generic; only the file renames).
- **Tests (`tests/test_modules.py`):** rename the temp-file fixtures to `PascalCase`,
  update specifiers, replace the two "the namespace name must be '…'" assertions and
  the dropped canonical-name test with tests for the new rules (PascalCase-file
  required; camelCase free binding; PascalCase binding rejected).
- **Docs:** `SPEC.md §12` example snippets and prose; this doc; `CHANGELOG.md`.
- **Untouched:** `examples/tcp_echo.thx`, `examples/ffi_hello.thx` (import only `ffi`).

## 7. Non-goals

- No change to *how* modules load (two-phase init, cycle rules) or to selective/`as`
  semantics beyond the namespace binding.
- No package manifest, versioning, or multi-root projects — a project is still just a
  directory with a `Main.thx`.
- `ffi`/`std` root casing is not revisited; they remain lowercase roots.

## 8. Risks

- **Blast radius.** Many files rename at once; mitigated by the conformance suite and
  `test_modules.py` — a missed specifier fails a test immediately.
- **Namespace-qualified `extern fn` calls.** Unverified until implementation; the
  example has a selective-import fallback (§5) if needed.
