# Type-directed hover — design

Date: 2026-07-14
Status: approved (scope + ctor-return depth chosen by Joshua)
Follows: `docs/specs/2026-07-13-thoxa-cross-module-hover-design.md`

## Problem

Hovering names in `examples/http/Main.thx` exposed that navigation is only as
smart as the *conservative* checker's type tracking, which is thin:

| Cursor | Today | Should be |
| --- | --- | --- |
| `server` in `let server = Server(port: 9091)?` | `let server: Unknown` | `let server: Server` |
| `Server` (the ctor) | `type Server(host, port)` | defaults + `-> Trial<Server, String>` |
| `client` in `let client = server.accept()?` | `let client: Unknown` | `let client: Client` |
| `server.accept` | *(nothing)* | the method signature |
| `client.receive` | *(nothing)* | the method signature |
| `socket` field in `let socket = net.createSocket()?` | `socket: ` (empty) | `socket: Socket` |
| `socket.bind` | *(nothing)* | `bind`'s signature, jump into `Net.thx` |
| `net` on the `import net from "./Net"` line | *(nothing)* | the module summary (as at use sites) |

Three root causes in `thoxa/checker.py`, all "the checker discards a type it could
keep", plus two pure LSP-rendering bugs:

1. **`?` drops the payload type.** `_infer_Try` (checker.py:707) returns `UNKNOWN`.
   So a `let` bound to `expr?` is `Unknown`, and its members can't resolve.
2. **Generic args are dropped.** `_resolve` (checker.py:379) turns
   `Trial<Socket, String>` into `("union","Trial")` — the `Socket` payload is gone
   before `?` could recover it.
3. **Unannotated returns aren't inferred from bodies.** `accept()` returns
   `Client(...)` but has no annotation, so `type_members["Server"]["accept"].ret`
   is `UNKNOWN`; and `main` is checked *before* `Server`, so it never learns
   `accept -> Client`.
4. **Rendering:** `render_signature`'s field branch ignores inferred types
   (index.py:96); the import binding's `def_span` is the `import` keyword, not the
   name, so hovering the name finds nothing.

Empirically confirmed (throwaway monkeypatch, no files touched): a one-line
`_infer_Try` passthrough alone fixes exactly 2 of 8 (`server: Server` and the
`server.accept` link); the rest need #2, #3, and the cross-module member lookup.

## Scope (approved)

All eight rows above. Coverage chosen "full type-directed hover"; the `Server`
ctor row uses **full error-type (`E`) inference** (chosen over a marker-only form).

Out of scope: cross-module find-all-references / rename (single-file, as today);
generic *user* types beyond retaining their args for display; flow-sensitive
narrowing.

## Guiding constraint: precision without new diagnostics

The checker is conservative by design. Every change here must only ever turn
`UNKNOWN` into a concrete type — never introduce a *false* error. The risk is that
a newly-concrete local type lets `_member` fire a real "has no member" / "is
private" check (correct when the code is wrong, but a regression if our inference
is wrong). Mitigations, baked into the design:

- **Quiet inference.** The return pre-pass and field-init inference run with
  `err()` suppressed (a `self._quiet` flag). They *learn types*; they never
  *report*. The normal check pass still walks every body and emits each
  diagnostic exactly once.
- **Cross-module types stay `UNKNOWN` in `_member`.** A `("named", X)` whose `X`
  isn't in `self.type_members` already returns `UNKNOWN` without erroring
  (checker.py:672) — so `socket: Socket` (Socket from Net.thx) never triggers a
  false "has no member". Its hover is served by the LSP cross-module path instead.
- **Full-suite gate.** All 315 tests plus a re-probe of all 8 positions must pass;
  any new diagnostic on existing code is treated as a regression to investigate.

## Mechanisms

### ① Retain generic payloads (`_resolve`)

Extend the union type-rep to carry resolved type args:
`("union", name)` → `("union", name, args)` where `args` is a tuple of resolved
arg reps (`()` when none). `Trial<Socket, String>` becomes
`("union", "Trial", (("named","Socket"), "String"))`.

Audit every consumer of `("union", …)` and of union-tuple unpacking/equality.
Known-safe: `ty_str` and `_infer_Match` read `ty[1]`. The hazard is any
`ty == ("union", X)` exact-equality or `_, name = ty` two-element unpack — grep
and convert to index/`ty[1]` access. Match-exhaustiveness (checker.py:745) reads
`sty[0]`/`sty[1]` and `union_variants[sty[1]]`, so it is preserved; a test asserts
this explicitly.

Same treatment reaches Trial/May via the module-export tables: `_module_exports`
builds an imported module's export `_FnInfo`s with the same `_resolve`, so a
namespace member's return keeps its payload too (this is what makes `socket`
resolve through `net.createSocket()`).

### ② Unwrap in `_infer_Try`

```python
def _infer_Try(self, n, env):
    ty = self._infer(n.operand, env)
    return _trial_payload(ty)

def _trial_payload(ty):
    if isinstance(ty, tuple) and ty[0] == "union" and len(ty) > 2:
        if ty[1] == "Trial" and ty[2]:   # Trial<T, E> -> T
            return ty[2][0]
        if ty[1] == "May" and ty[2]:     # May<T> -> T
            return ty[2][0]
    return ty     # ctor calls etc. already model their success type
```

### ③ Quiet method/fn-return pre-pass

After `self.resolution` is built and before the statement-checking loop in
`check_module`, run `_infer_returns()`:

- For every `fn`/method with **no** return annotation, infer its body's result
  type (reuse `_infer` on the body block) in the correct env — for a method, the
  instance-member env `_check_function` already constructs — and write the result
  into the corresponding `_FnInfo.ret` (in `self.funcs` and in
  `self.type_members[owner][name]`).
- Iterate to a fixpoint (cap at a small N, e.g. 3): `fn A` whose tail is `B()`
  learns its type once `B`'s return is known.
- Runs under `self._quiet = True`.

This makes `accept -> ("named","Client")` visible when `main` is later checked, so
`client: Client` and the `client.receive` link both fall out.

### ④ Quiet field-init inference (`_check_type`)

`_check_type` (checker.py:459) currently walks only methods. Add: for each
`ast.Field` with an initializer and no annotation, infer the initializer type in
the type's member env and `_record_type(field.span, field.name, ty)` — under
`self._quiet`, so no new field-init diagnostics. This records `socket: Socket`
against the field binding id, which `_infer_Name` and the LSP both read.

### ⑤ Cross-module member on a typed value (`crossmodule.py`)

Extend `_origin`/`resolve_remote`: when the cursor is a member ref
(`ref.ctx == "member"`, `binding is None`) whose receiver `Name` has an inferred
type `("named", X)` in `idx.types` (not a namespace — that branch already exists):

1. Find the import in `idx.module` whose target module *exports a type named `X`*
   (scan namespace and selective imports; resolve each spec and check its
   `member_index`/exported type bindings — reuse `_resolve_module`).
2. In that module's `Resolution`, look up `member_index[(X, member)]`; render its
   signature + `doc_above`, and jump to its `def_span`.

New origin form `("typed_member", spec, type_name, member_name)`. Purely additive,
LSP-only. Falls back to `None` (today's behaviour) whenever the receiver type is
unknown or `X` isn't found among imports.

### ⑥ Rendering fixes

- `render_signature` field branch (index.py:96): when `type_ann is None`, show the
  inferred type passed in (`ty`), mirroring the `let`/`mut` branch, so a
  type-body `let` field renders `socket: Socket` (empty type ⇒ bare name, no
  dangling `: `).
- Import-binding `def_span`: in the resolver's `_declare_top_level` Import branch
  (resolver.py:159), set the binding's `def_span` to the namespace name's span
  (via `_name_span`) instead of `stmt.span`, so hovering `net` on the import line
  finds the binding — then the existing cross-module namespace-summary path serves
  it. (Selective imports bind multiple locals; give each its own name span.)

### ⑦ `Server` ctor hover — full `E` inference

Render ctor param defaults and types:
`type Server(host = "127.0.0.1", port = 8000)`. When the ctor **threads Trial**
(reuse the interpreter's existing `?`/variant-construction threading analysis),
append `-> Trial<Server, E>`:

- `E` is the unification of every error type flowing out of the ctor body: the
  `args[1]` of each `?`-operand's `Trial<_, E>` type, and the payload type of each
  `Fail(x)` construction. `May`/`Not` contribute no `E`.
- If all agree, show that `E` (here `String`). If they conflict, fall back to the
  bare success (`-> Trial<Server>`); never fabricate an `E`.

This same success/error computation is what a future "ctor returns Trial" checker
pass would want; here it is used only for rendering.

## Handler integration

- `hover` and `definition` already try `crossmodule.resolve_remote` first; the new
  `("typed_member", …)` origin plugs into that call with no handler change beyond
  passing what it already passes (`idx` carries `types`).
- The field/import rendering changes are internal to `index.render_signature` and
  the resolver; handlers are untouched.

## Testing

Extend `tests/test_lsp_crossmodule.py` and add checker unit tests:

- **Checker (`tests/test_checker*.py`):** `Trial<T,E>` resolves to a 3-tuple
  carrying `T`,`E`; `?` unwraps `Trial`/`May`/passes-through named; match
  exhaustiveness on a `Trial`/`May` scrutinee still fires; a method's unannotated
  return is inferred from its body; a field init records its type; **no new
  diagnostic** appears on the existing example/corpus files (snapshot the error
  count on `examples/http/*`).
- **LSP:** each of the 8 positions in `examples/http/Main.thx` renders the target
  row; `socket.bind` jumps into `Net.thx:58`; `client.receive` into `Main.thx`;
  the `net` import line yields the module summary; a receiver whose type is
  genuinely unknown still returns `None` (no crash, graceful fallback).
- **Regression:** the full suite (315) stays green; re-run `scripts/lsp_probe.py`
  on all 8 positions as an integration check.

## Task breakdown (for the plan)

1. `_resolve` union-args representation + consumer audit; tests (incl. match
   exhaustiveness preserved).
2. `_infer_Try` unwrap + `_trial_payload`; tests.
3. `self._quiet` flag + quiet method/fn-return pre-pass (`_infer_returns`,
   fixpoint); tests.
4. Quiet field-init inference in `_check_type`; tests.
5. `render_signature` field-type rendering + import `def_span` fix; tests.
6. `crossmodule` typed-member origin (`("typed_member", …)`); tests + probe.
7. `Server` ctor render with full `E` inference; tests.
8. Full-suite gate + re-probe all 8; update memory/goal ledger.
