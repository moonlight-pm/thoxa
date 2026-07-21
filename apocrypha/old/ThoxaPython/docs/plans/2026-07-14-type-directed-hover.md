# Type-Directed Hover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make LSP hover / go-to-definition type-aware — `let` bindings show inferred types, members on typed receivers resolve (including into other modules), and the `Server` ctor shows its inferred `Trial<…>` return — by teaching the conservative checker to keep types it currently discards, without emitting any new diagnostics.

**Architecture:** The checker keeps generic payloads (`Trial<Socket,String>` no longer collapses to bare `Trial`), unwraps `?`, and gains a *quiet* (error-suppressed) pre-pass that infers unannotated method returns and field-initializer types. The LSP layer renders the newly-available types and resolves cross-module members on typed values. Follows `docs/specs/2026-07-14-thoxa-type-directed-hover-design.md`.

**Tech Stack:** Python 3, hand-written checker/resolver/LSP (zero deps). Tests via pytest in the uv venv: `.venv/bin/python -m pytest`.

## Global Constraints

- **Precision without new diagnostics.** Every change may only turn `UNKNOWN` into a concrete type. The two new inference passes run under `self._quiet` and emit nothing; the normal check pass still emits each diagnostic exactly once. Cross-module types stay `UNKNOWN` in `_member` (checker.py:672) — no false "has no member".
- **Type-rep union becomes a 3-tuple `("union", name, args)`** where `args` is a tuple of resolved arg reps (`()` when none). ONLY `ty_str` (checker.py:51) and `_infer_Match` (checker.py:745) consume type-rep unions, both via `ty[1]` — 3-tuple-safe. DO NOT touch the unrelated export-info tuple `("union", variants)` (checker.py:309/324) or the binding-kind string `"union"` (index.py:93).
- **The `Server` ctor's inferred `Trial<…>` return must NOT be stored in `self.types`** (that dict feeds `_infer_Name`, which would break `Server(...)` construction). It lives in a separate `self.ctor_returns` map that only the renderer reads.
- **Spans are 1-based `(line, col, length)`** in Thoxa; LSP positions are 0-based. `_record_type` keys inferred types by resolver binding id.
- **Commit directly to `master`** (project convention). Every commit ends with the trailers:
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6
  ```
- **Baseline:** 315 tests green. Any new failure or new diagnostic on existing code is a regression to investigate, not to suppress.

---

### Task 1: Retain generic payloads in `_resolve`

**Files:**
- Modify: `thoxa/checker.py` (`_resolve`, lines 377-383)
- Test: `tests/test_checker.py`

**Interfaces:**
- Produces: `_resolve(TypeRef)` returns `("union", name, tuple_of_arg_reps)` for union types (was `("union", name)`). `("named", name)` and scalars unchanged.

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_checker.py`:

```python
from thoxa.lsp import index as _index


def _binding_named(idx, name, kind=None):
    for b in idx.resolution.bindings.values():
        if b.name == name and b.def_span is not None and (kind is None or b.kind == kind):
            return b
    raise AssertionError(f"no binding named {name!r}")


def test_trial_type_retains_payload_args():
    # A param annotated Trial<Int64,String> records a 3-tuple carrying the args.
    idx = _index.build_index("fn f(x!: Trial<Int64, String>) -> Int64 {\n  0\n}\n")
    b = _binding_named(idx, "x", "param")
    assert idx.type_of(b) == ("union", "Trial", ("Int64", "String"))


def test_may_type_retains_single_payload_arg():
    idx = _index.build_index("fn f(x!: May<Int64>) -> Int64 {\n  0\n}\n")
    b = _binding_named(idx, "x", "param")
    assert idx.type_of(b) == ("union", "May", ("Int64",))


def test_match_exhaustiveness_still_fires_on_trial():
    from thoxa.checker import check
    diags = check("fn f(t!: Trial<Int64, String>) -> Int64 {\n"
                  "  match t {\n    Pass { 1 }\n  }\n}\n")
    assert any("non-exhaustive match on 'Trial'" in d.message for d in diags)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "retains_payload or single_payload or exhaustiveness_still" -v`
Expected: the two `retains_payload`/`single_payload` tests FAIL (get `("union","Trial")` / `("union","May")`); `exhaustiveness_still` PASSES already (guards against regression).

- [ ] **Step 3: Implement the payload-retaining resolve**

In `thoxa/checker.py`, replace the tail of `_resolve` (currently):
```python
        for a in ann.args:
            self._resolve(a)
        if self.type_kind.get(name) == "union":
            return ("union", name)
        if name in self.type_kind:
            return ("named", name)
        return UNKNOWN
```
with:
```python
        resolved_args = tuple(self._resolve(a) for a in ann.args)
        if self.type_kind.get(name) == "union":
            return ("union", name, resolved_args)
        if name in self.type_kind:
            return ("named", name)
        return UNKNOWN
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "retains_payload or single_payload or exhaustiveness_still" -v`
Expected: PASS (all three).

- [ ] **Step 5: Run the full checker + match suites (union-rep audit)**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_match.py tests/test_interp.py -q`
Expected: PASS. If any test comparing a union type-rep breaks, it is a real consumer the audit missed — fix it to read `ty[1]`, not to unpack a 2-tuple.

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py tests/test_checker.py
git commit  # message: "checker: retain generic type args in union type-reps"  + trailers
```

---

### Task 2: Unwrap `?` in `_infer_Try`

**Files:**
- Modify: `thoxa/checker.py` (`_infer_Try`, lines 707-709; add module-level `_trial_payload`)
- Test: `tests/test_checker.py`

**Interfaces:**
- Consumes: the `("union", name, args)` rep from Task 1.
- Produces: `_infer_Try` returns the operand's success payload: `Trial<T,E>`/`May<T>` → `T`; anything else (e.g. a ctor call's `("named", X)`) → the operand type unchanged.

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_checker.py`:

```python
def test_try_unwraps_trial_payload():
    idx = _index.build_index(
        "fn f() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "fn g() -> Int64 {\n  let x = f()?\n  x\n}\n")
    assert idx.type_of(_binding_named(idx, "x", "let")) == "Int64"


def test_try_unwraps_may_payload():
    idx = _index.build_index(
        "fn h() -> May<Int64> {\n  Has(0)\n}\n"
        "fn g() -> Int64 {\n  let y = h()?\n  y\n}\n")
    assert idx.type_of(_binding_named(idx, "y", "let")) == "Int64"


def test_try_passthrough_on_ctor_call():
    idx = _index.build_index(
        "type Box(n: Int64) {\n  let bad = maybe()?\n}\n"
        "fn maybe() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "fn g() -> Int64 {\n  let b = Box(n: 1)?\n  0\n}\n")
    assert idx.type_of(_binding_named(idx, "b", "let")) == ("named", "Box")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "try_unwraps or try_passthrough" -v`
Expected: FAIL (all three get `Unknown`).

- [ ] **Step 3: Implement the unwrap**

In `thoxa/checker.py`, add a module-level helper near `ty_str` (after line 59):
```python
def _trial_payload(ty):
    """The success payload of a `?` operand: Trial<T,E>/May<T> -> T; else `ty`."""
    if isinstance(ty, tuple) and ty[0] == "union" and len(ty) > 2 and ty[2]:
        if ty[1] in ("Trial", "May"):
            return ty[2][0]
    return ty
```
Replace `_infer_Try` (lines 707-709):
```python
    def _infer_Try(self, n: ast.Try, env):
        return _trial_payload(self._infer(n.operand, env))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "try_unwraps or try_passthrough" -v`
Expected: PASS.

- [ ] **Step 5: Regression check**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_interp.py tests/test_modules.py -q`
Expected: PASS. A new "has no member"/"is private" error here means a `?`-derived type is now concrete and exposed a real-or-false member access — inspect; if the inference is correct and the error is correct, update the offending test; if the inference is wrong, fix `_trial_payload`.

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py tests/test_checker.py
git commit  # "checker: unwrap ?'s payload type in _infer_Try"  + trailers
```

---

### Task 3: Quiet method/fn-return pre-pass

**Files:**
- Modify: `thoxa/checker.py` (`__init__` add `_quiet`/`ctor_returns`; `err` guard; `check_module` call `_prepass`; add `_prepass`, `_infer_returns`, `_type_member_env`)
- Test: `tests/test_checker.py`

**Interfaces:**
- Produces: after `check_module`, an unannotated `fn`/method's `_FnInfo.ret` in `self.funcs` / `self.type_members[owner][name]` holds its inferred body type. `self._quiet: bool` suppresses `err`. `self.ctor_returns: dict[str, tuple]` (populated in Task 7; declared here).

- [ ] **Step 1: Write the failing test**

Add to `tests/test_checker.py`:

```python
def test_unannotated_method_return_inferred_from_body():
    # accept() has no annotation and returns Client(...); a caller's `?` then
    # yields Client, so the caller binding is typed.
    src = (
        "type Client(socket!: Int64) {\n"
        "  export fn id() -> Int64 { socket }\n"
        "}\n"
        "type Server(fd: Int64) {\n"
        "  export fn accept() {\n"
        "    Client(fd)\n"
        "  }\n"
        "}\n"
        "fn run(s!: Server) -> Int64 {\n"
        "  let c = s.accept()\n"
        "  c.id()\n"
        "}\n")
    idx = _index.build_index(src)
    assert idx.type_of(_binding_named(idx, "c", "let")) == ("named", "Client")
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k unannotated_method_return -v`
Expected: FAIL (`c` is `Unknown` — `accept`'s return is `UNKNOWN` when `run` is checked).

- [ ] **Step 3: Implement the quiet pre-pass**

In `thoxa/checker.py` `__init__`, after `self.types: dict[int, object] = {}` (line 82) add:
```python
        self._quiet = False
        self.ctor_returns: dict[str, tuple] = {}   # type name -> inferred Trial<…> (Task 7)
```
At the top of `err` (line 97, first line of body) add:
```python
        if self._quiet:
            return
```
In `check_module`, after `self.resolution = _resolve_names(...)` / the `except` block that sets it, and before `genv = self._global_env()`... actually insert the pre-pass call right after `genv` is built and before the check loops:
```python
        genv = self._global_env()
        self._prepass(module, genv)
        for stmt in prelude.body:
            self._check_decl_bodies(stmt, genv)
        for stmt in module.body:
            self._check_stmt(stmt, dict(genv))
        return self.errors
```
Add these methods (near `_check_type`):
```python
    def _prepass(self, module: ast.Module, genv: dict) -> None:
        """Quietly learn types the normal (error-emitting) pass needs up front:
        unannotated fn/method returns (Task 3). Field-init types (Task 4) and
        ctor Trial-returns (Task 7) hook in here too. Emits no diagnostics."""
        self._quiet = True
        try:
            self._infer_returns(module, genv)
        finally:
            self._quiet = False

    def _type_member_env(self, td: ast.TypeDef, genv: dict) -> dict:
        env = dict(genv)
        for mname, mty in self.type_members.get(td.name, {}).items():
            env.setdefault(mname, mty)
        return env

    def _infer_returns(self, module: ast.Module, genv: dict) -> None:
        for _ in range(3):                       # fixpoint, capped
            changed = False
            for stmt in module.body:
                if isinstance(stmt, ast.FunctionDef) and stmt.return_type is None:
                    ty = self._infer(stmt.body, dict(genv))
                    info = self.funcs.get(stmt.name)
                    if info is not None and ty is not UNKNOWN and info.ret != ty:
                        info.ret = ty
                        changed = True
                elif isinstance(stmt, ast.TypeDef):
                    menv = self._type_member_env(stmt, genv)
                    for m in stmt.members:
                        if isinstance(m, ast.FunctionDef) and m.return_type is None:
                            ty = self._infer(m.body, dict(menv))
                            info = self.type_members[stmt.name].get(m.name)
                            if (isinstance(info, tuple) and info[0] == "fn"
                                    and ty is not UNKNOWN and info[1].ret != ty):
                                info[1].ret = ty
                                changed = True
            if not changed:
                break
```

- [ ] **Step 4: Run test to verify it passes**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k unannotated_method_return -v`
Expected: PASS.

- [ ] **Step 5: Regression check**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_interp.py tests/test_modules.py tests/test_conformance.py -q`
Expected: PASS. Investigate any new diagnostic per the Task-2 note.

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py tests/test_checker.py
git commit  # "checker: quietly infer unannotated fn/method returns (pre-pass)"  + trailers
```

---

### Task 4: Quiet field-initializer inference

**Files:**
- Modify: `thoxa/checker.py` (`_infer_returns` or a sibling called from `_prepass`)
- Test: `tests/test_checker.py`

**Interfaces:**
- Produces: an `ast.Field` with an initializer and no annotation records its inferred type under its field binding id (`self.types`) AND updates `self.type_members[owner][field]`, so both hover and later method-envs see it.

- [ ] **Step 1: Write the failing test**

Add to `tests/test_checker.py`:

```python
def test_field_initializer_type_recorded():
    src = (
        "fn make() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "type Server(host = 1) {\n"
        "  let fd = make()?\n"
        "}\n")
    idx = _index.build_index(src)
    b = _binding_named(idx, "fd", "field")
    assert idx.type_of(b) == "Int64"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k field_initializer_type -v`
Expected: FAIL (`None` — field inits are never inferred).

- [ ] **Step 3: Implement quiet field-init inference**

In `thoxa/checker.py`, add a method and call it from `_prepass` BEFORE `_infer_returns` (so method-envs see field types):
```python
    def _infer_field_inits(self, module: ast.Module, genv: dict) -> None:
        for stmt in module.body:
            if not isinstance(stmt, ast.TypeDef):
                continue
            menv = self._type_member_env(stmt, genv)
            for m in stmt.members:
                if isinstance(m, ast.Field) and m.default is not None and m.type_ann is None:
                    ty = self._infer(m.default, dict(menv))
                    if ty is not UNKNOWN:
                        self.type_members[stmt.name][m.name] = ty
                        self._record_type(m.span, m.name, ty)
```
Update `_prepass`:
```python
        self._quiet = True
        try:
            self._infer_field_inits(module, genv)
            self._infer_returns(module, genv)
        finally:
            self._quiet = False
```

- [ ] **Step 4: Run test to verify it passes**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k field_initializer_type -v`
Expected: PASS.

- [ ] **Step 5: Regression check**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_interp.py tests/test_modules.py -q`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py tests/test_checker.py
git commit  # "checker: quietly record field-initializer types for hover"  + trailers
```

---

### Task 5: Cross-module member on a typed value

**Files:**
- Modify: `thoxa/lsp/crossmodule.py` (`_resolve_remote` → pass `idx` to `_origin`; `_origin` new `typed_member` branch; add `_spec_exporting_type`, `_lookup_typed_member`; handle the new origin in `_resolve_remote`)
- Test: `tests/test_lsp_crossmodule.py`

**Interfaces:**
- Consumes: `idx.types` (Task 4 populated field/let types) and `idx.module`.
- Produces: origin `("typed_member", spec, type_name, member_name)`; `resolve_remote` returns a `Remote` pointing into the type's defining module for a member on a value whose inferred type is `("named", X)`.

- [ ] **Step 1: Write the failing test**

Add to `tests/test_lsp_crossmodule.py` (uses `tmp_path` so `./Lib` resolves):

```python
def test_typed_member_resolves_into_defining_module(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text(
        "export type Widget(id: Int64) {\n"
        "  export fn spin() -> Int64 { id }\n"
        "}\n"
        "export fn make() -> Trial<Widget, String> {\n  Pass(Widget(id: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = (
        'import lib from "./Lib"\n'
        "fn go() -> Int64 {\n"
        "  let w = lib.make()?\n"
        "  w.spin()\n"
        "}\n")
    main.write_text(main_text)
    path = str(main)
    from thoxa.lsp import crossmodule
    idx = index.build_index(main_text, path)
    # cursor on `spin` in `w.spin()` (line 4, col 5 1-based -> 0-based char 4)
    ref = idx.reference_at(3, 4)
    r = crossmodule.resolve_remote(idx, ref, idx.resolution.binding_of(ref), {}, path)
    assert r is not None
    assert r.signature == "fn spin() -> Int64"
    assert r.path.endswith("Lib.thx")
    assert r.def_span is not None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/python -m pytest tests/test_lsp_crossmodule.py -k typed_member_resolves -v`
Expected: FAIL (`None` — receiver `w` typed but the member branch only handles namespace receivers).

- [ ] **Step 3: Implement typed-member resolution**

In `thoxa/lsp/crossmodule.py`, change `_resolve_remote` to pass `idx` into `_origin` and handle the new origin:
```python
def _resolve_remote(idx, ref, binding, docs, current_path):
    origin = _origin(binding, ref, idx, docs, current_path)
    if origin is None:
        return None
    kind = origin[0]
    if kind == "member":
        _, spec, imported = origin
        return _lookup_member(spec, imported, docs, current_path)
    if kind == "module":
        _, spec = origin
        return _lookup_module(spec, docs, current_path)
    if kind == "typed_member":
        _, spec, type_name, member = origin
        return _lookup_typed_member(spec, type_name, member, docs, current_path)
    return None
```
Update `_origin`'s signature and its branch (2). Replace the branch-(2) body so that, when the receiver is NOT a namespace, it tries the receiver's inferred type:
```python
def _origin(binding, ref, idx, docs, current_path):
    resolution, module = idx.resolution, idx.module
    # (1) unchanged — imported name / namespace name
    if (binding is not None and binding.kind == "import"
            and isinstance(binding.node, ast.Import)):
        node = binding.node
        if node.names is not None:
            for imported, local in node.names:
                if local == binding.name:
                    return ("member", node.module, imported)
            return None
        return ("module", node.module)
    # (2) a member `recv.member`
    if (binding is None and ref is not None and ref.ctx == "member"
            and module is not None and resolution is not None):
        fa = _field_access_for(ref, module, resolution)
        if fa is not None and isinstance(fa.obj, ast.Name):
            recv = resolution.binding_of(
                resolution.ref_at(fa.obj.span.line, fa.obj.span.col))
            # (2a) receiver is a namespace import -> ns.member
            if (recv is not None and recv.kind == "import"
                    and isinstance(recv.node, ast.Import)
                    and recv.node.names is None):
                return ("member", recv.node.module, fa.name)
            # (2b) receiver has an inferred cross-module type -> typed member
            if recv is not None:
                rty = idx.types.get(recv.id)
                if isinstance(rty, tuple) and rty[0] == "named":
                    spec = _spec_exporting_type(module, rty[1], docs, current_path)
                    if spec is not None:
                        return ("typed_member", spec, rty[1], fa.name)
    # (3) prelude builtin — unchanged
    if binding is not None and binding.kind == "builtin":
        return ("member", "std/Prelude", binding.name)
    return None
```
Add the two helpers:
```python
def _spec_exporting_type(module, type_name, docs, current_path):
    """The import specifier of the module that exports a type named `type_name`."""
    for stmt in module.body:
        if not isinstance(stmt, ast.Import):
            continue
        path = _resolve_spec(stmt.module, current_path)
        if path is None:
            continue
        text = _module_text(path, docs)
        if text is None:
            continue
        res = _resolve_module(path, text)
        b = _exported_binding(res, type_name)
        if b is not None and b.kind == "type":
            return stmt.module
    return None


def _lookup_typed_member(spec, type_name, member, docs, current_path) -> Optional[Remote]:
    path = _resolve_spec(spec, current_path)
    if path is None:
        return None
    text = _module_text(path, docs)
    if text is None:
        return None
    res = _resolve_module(path, text)
    bid = res.member_index.get((type_name, member))
    if bid is None:
        return None
    b = res.bindings.get(bid)
    if b is None or b.def_span is None:
        return None
    doc = convert.doc_above(text, b.def_span.line)
    return Remote(render_signature(b), doc, path, b.def_span)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `.venv/bin/python -m pytest tests/test_lsp_crossmodule.py -k typed_member_resolves -v`
Expected: PASS.

- [ ] **Step 5: Full LSP suite (existing origins unregressed)**

Run: `.venv/bin/python -m pytest tests/test_lsp_crossmodule.py tests/test_lsp.py tests/test_lsp_navigation.py -q`
Expected: PASS (namespace-member `s.count`-style still returns `None`; a receiver of unknown type still returns `None`).

- [ ] **Step 6: Commit**

```bash
git add thoxa/lsp/crossmodule.py tests/test_lsp_crossmodule.py
git commit  # "lsp/crossmodule: resolve members on a value of an imported type"  + trailers
```

---

### Task 6: Rendering fixes — field type + import def_span + import-site hover

**Files:**
- Modify: `thoxa/lsp/index.py` (`render_signature` field branch)
- Modify: `thoxa/resolver.py` (`_declare_top_level` Import branch)
- Modify: `thoxa/lsp/server.py` (`hover`: run the `binding_at` fallback before `resolve_remote`)
- Test: `tests/test_resolver.py`, `tests/test_lsp_crossmodule.py`

**Interfaces:**
- Consumes: field inferred types (Task 4); the namespace-summary origin (existing).
- Produces: field hover shows `name: <inferred>`; an import binding's `def_span` is the local name's span; hovering an import-site name yields the module summary.

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_resolver.py`:
```python
def test_import_binding_def_span_is_on_the_name():
    from thoxa.resolver import resolve
    from thoxa.parser import parse
    from thoxa.errors import Source
    src = 'import net from "./Net"\n'
    res = resolve(parse(src, "m.thx"), Source(src, "m.thx"))
    b = next(b for b in res.bindings.values() if b.kind == "import" and b.name == "net")
    # "net" starts at column 8 (1-based) on line 1
    assert (b.def_span.line, b.def_span.col) == (1, 8)
```
Add to `tests/test_lsp_crossmodule.py`:
```python
def test_hover_on_import_site_namespace_name(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("## A little library.\nexport fn f() -> Int64 { 0 }\n")
    main = tmp_path / "Main.thx"
    main_text = 'import lib from "./Lib"\nfn g() -> Int64 { lib.f() }\n'
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 0, 7))            # `lib` on the import line (0-based char 7)
    assert h is not None
    assert "module ./Lib" in h["contents"]["value"]


def test_field_hover_shows_inferred_type(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n  export fn f() -> Int64 { fd }\n}\n"
                   "export fn open() -> Trial<Sock, String> {\n  Pass(Sock(fd: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import net from "./Lib"\n'
                 "type Server(x = 1) {\n  let sock = net.open()?\n}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 2, 6))            # `sock` field (0-based char 6)
    assert h is not None
    assert "sock: Sock" in h["contents"]["value"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_resolver.py -k import_binding_def_span tests/test_lsp_crossmodule.py -k "import_site or field_hover_shows" -v`
Expected: FAIL (def_span at the `import` keyword; import-site hover `None`/bare; field hover empty).

- [ ] **Step 3a: Fix the import binding def_span**

In `thoxa/resolver.py`, `_declare_top_level`, replace the Import branch (lines 159-163):
```python
        elif isinstance(stmt, ast.Import):
            for local in _import_locals(stmt):
                span = self._name_span(stmt.span, local)
                bid = self._new_binding(local, "import", span, stmt,
                                        module_level=True)
                scope.define(local, bid)
```

- [ ] **Step 3b: Show the inferred type in the field branch**

In `thoxa/lsp/index.py`, replace the field branch of `render_signature` (lines 96-97):
```python
    if k == "field" and isinstance(n, ast.Field):
        from ..checker import ty_str
        shown = str(n.type_ann) if n.type_ann is not None else (
            ty_str(ty) if ty is not None else "")
        return f"{n.name}: {shown}".rstrip(": ").rstrip()
```

- [ ] **Step 3c: Run the `binding_at` fallback before `resolve_remote` in hover**

In `thoxa/lsp/server.py`, `hover`, replace the block from `ref = idx.reference_at(...)` through the `remote = crossmodule.resolve_remote(...)` guard so the fallback runs first:
```python
            ref = idx.reference_at(pos["line"], pos["character"])
            binding = idx.resolution.binding_of(ref)
            span = ref.span if ref is not None else None
            if binding is None:                        # cursor may be ON a definition name
                binding = idx.binding_at(pos["line"], pos["character"])
                if span is None and binding is not None:
                    span = binding.def_span
            # cross-module: an imported name, a namespace member/name, a prelude
            # type, or a member on a value of an imported type
            remote = crossmodule.resolve_remote(idx, ref, binding, self.docs, path)
            if remote is not None:
                parts = [f"```thoxa\n{remote.signature}\n```"]
                if remote.doc:
                    parts.append(remote.doc)
                return convert.hover("\n\n".join(parts), span)
            if binding is None or binding.kind == "builtin":
                return None
            sig = index.render_signature(binding, idx.type_of(binding))
```
(Delete the now-duplicate `if binding is None:` fallback that previously sat after the `remote` guard.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_resolver.py -k import_binding_def_span tests/test_lsp_crossmodule.py -k "import_site or field_hover_shows" -v`
Expected: PASS.

- [ ] **Step 5: Full LSP + resolver suite**

Run: `.venv/bin/python -m pytest tests/test_resolver.py tests/test_lsp.py tests/test_lsp_navigation.py tests/test_lsp_crossmodule.py -q`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add thoxa/lsp/index.py thoxa/resolver.py thoxa/lsp/server.py tests/test_resolver.py tests/test_lsp_crossmodule.py
git commit  # "lsp: render field types, fix import def_span, hover import-site names"  + trailers
```

---

### Task 7: `Server` ctor hover — full `Trial<Server, E>` inference

**Files:**
- Modify: `thoxa/checker.py` (add `_infer_ctor_returns` to `_prepass`; `_collect_error_types`)
- Modify: `thoxa/lsp/index.py` (`DocumentIndex.ctor_returns`; `build_index`; `render_signature` type branch + `_ctor_params_str`, `_render_generic`)
- Modify: `thoxa/lsp/server.py` (`hover` passes the ctor return + text for a `type` binding)
- Test: `tests/test_checker.py`, `tests/test_lsp_crossmodule.py`

**Interfaces:**
- Consumes: fallibility via `interp._ctor_is_fallible`; the union payload rep (Task 1); field/return types (Tasks 3-4).
- Produces: `self.ctor_returns[type_name] = ("union","Trial",(("named",type_name), E))` for a fallible ctor (else absent). `DocumentIndex.ctor_returns` exposes it. `render_signature(binding, ty, text)` renders `type Name(<verbatim ctor params>) -> Trial<Name, E>`.

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_checker.py`:
```python
def test_fallible_ctor_return_inferred():
    src = (
        "fn make() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "type Server(host = 1) {\n"
        "  let fd = make()?\n"
        "}\n")
    idx = _index.build_index(src)
    assert idx.ctor_returns.get("Server") == ("union", "Trial", (("named", "Server"), "String"))


def test_infallible_ctor_has_no_trial_return():
    idx = _index.build_index("type Point(x = 0, y = 0) {\n}\n")
    assert "Point" not in idx.ctor_returns
```
Add to `tests/test_lsp_crossmodule.py`:
```python
def test_hover_fallible_ctor_shows_trial_return(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {}\n"
                   "export fn open() -> Trial<Sock, String> {\n  Pass(Sock(fd: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import net from "./Lib"\n'
                 'type Server(host = "127.0.0.1", port = 8000) {\n'
                 "  let sock = net.open()?\n}\n"
                 "fn run() -> Int64 {\n  let s = Server()?\n  0\n}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 5, 10))          # `Server` in `Server()?` (0-based char 10)
    assert h is not None
    val = h["contents"]["value"]
    assert 'type Server(host = "127.0.0.1", port = 8000) -> Trial<Server, String>' in val
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "ctor_return or infallible_ctor" tests/test_lsp_crossmodule.py -k hover_fallible_ctor -v`
Expected: FAIL.

- [ ] **Step 3a: Infer ctor Trial-returns in the checker**

In `thoxa/checker.py`, extend `_prepass` (inside the `try`, after `_infer_returns`):
```python
            self._infer_ctor_returns(module, genv)
```
Add the methods:
```python
    def _infer_ctor_returns(self, module: ast.Module, genv: dict) -> None:
        from .interp import _ctor_is_fallible
        for stmt in module.body:
            if not isinstance(stmt, ast.TypeDef) or not _ctor_is_fallible(stmt):
                continue
            menv = self._type_member_env(stmt, genv)
            errs: list = []
            roots = [p.default for p in stmt.ctor_params if p.default is not None]
            roots += [m.default for m in stmt.members
                      if isinstance(m, ast.Field) and m.default is not None]
            for r in roots:
                self._collect_error_types(r, dict(menv), errs)
            e = errs[0] if errs and all(x == errs[0] for x in errs) else UNKNOWN
            self.ctor_returns[stmt.name] = ("union", "Trial", (("named", stmt.name), e))

    def _collect_error_types(self, node, env: dict, out: list) -> None:
        """Error types flowing out of a ctor body: each `?`-operand's Trial<_, E>
        and each `Fail(x)` payload. Does not descend into nested function defs."""
        if isinstance(node, ast.Try):
            ty = self._infer(node.operand, env)
            if isinstance(ty, tuple) and ty[0] == "union" and ty[1] == "Trial" \
                    and len(ty) > 2 and len(ty[2]) > 1:
                out.append(ty[2][1])
        if isinstance(node, ast.Call) and isinstance(node.callee, ast.TypeValue) \
                and node.callee.name == "Fail" and node.args:
            out.append(self._infer(node.args[0].value, env))
        if isinstance(node, ast.FunctionDef):
            return
        if isinstance(node, (list, tuple)):
            for it in node:
                self._collect_error_types(it, env, out)
        elif dataclasses.is_dataclass(node) and not isinstance(node, type):
            for f in dataclasses.fields(node):
                self._collect_error_types(getattr(node, f.name), env, out)
```
Add `import dataclasses` at the top of `thoxa/checker.py` if not already present.

- [ ] **Step 3b: Expose `ctor_returns` on the index**

In `thoxa/lsp/index.py`, `DocumentIndex.__init__`, add a field:
```python
    def __init__(self, resolution, types, text, module=None, ctor_returns=None):
        self.resolution = resolution
        self.types = types
        self.text = text
        self.module = module
        self.ctor_returns = ctor_returns or {}
```
In `build_index`, thread it through both success returns (the checker path):
```python
    return DocumentIndex(resolution, types, text, module,
                         ctor_returns=getattr(chk, "ctor_returns", {}))
```
(The parse-failure / resolver-only fallbacks pass no `ctor_returns` — they default to `{}`.)

- [ ] **Step 3c: Render ctor params verbatim + Trial return**

In `thoxa/lsp/index.py`, add helpers and extend the type branch of `render_signature` (accept a `text` param):
```python
def _ctor_params_str(params, text) -> str:
    from . import convert
    out = []
    for p in params:
        verbatim = convert.span_text(text, p.span) if text is not None else None
        if verbatim:
            out.append(verbatim.strip())
        else:
            ann = f": {p.type_ann}" if p.type_ann is not None else ""
            out.append(f"{p.name}{ann}")
    return ", ".join(out)


def _render_generic(ty) -> str:
    from ..checker import ty_str
    return f"{ty[1]}<{', '.join(ty_str(a) for a in ty[2])}>"
```
```python
def render_signature(binding: Binding, ty=None, text=None) -> str:
```
Type branch:
```python
    if k == "type" and isinstance(n, ast.TypeDef):
        ret = ""
        if (isinstance(ty, tuple) and ty[0] == "union"
                and ty[1] in ("Trial", "May") and len(ty) > 2):
            ret = f" -> {_render_generic(ty)}"
        return f"type {n.name}({_ctor_params_str(n.ctor_params, text)}){ret}"
```
(First implementation step in this task: verify `Param.span` covers the whole `host = "127.0.0.1"` — run `convert.span_text` on a parsed ctor param in a scratch REPL; if it covers only the name, widen the slice to span from `p.span` start through `p.default.span` end. Adjust `_ctor_params_str` accordingly.)

- [ ] **Step 3d: Pass the ctor return + text from hover**

In `thoxa/lsp/server.py`, `hover`, change the local-render line:
```python
            ty = idx.type_of(binding)
            if binding.kind == "type":
                ty = idx.ctor_returns.get(binding.name)
            sig = index.render_signature(binding, ty, idx.text)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_checker.py -k "ctor_return or infallible_ctor" tests/test_lsp_crossmodule.py -k hover_fallible_ctor -v`
Expected: PASS.

- [ ] **Step 5: Regression check**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_lsp.py tests/test_lsp_navigation.py tests/test_lsp_crossmodule.py -q`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py thoxa/lsp/index.py thoxa/lsp/server.py tests/test_checker.py tests/test_lsp_crossmodule.py
git commit  # "checker+lsp: infer and render a fallible ctor's Trial<T,E> return"  + trailers
```

---

### Task 8: Full-suite gate + re-probe + ledger

**Files:**
- Verify only (no product code); update `MEMORY.md` / memory files.

- [ ] **Step 1: Run the entire suite**

Run: `.venv/bin/python -m pytest -q`
Expected: PASS, count ≥ 315 + the new tests. Investigate any regression per the Task-2 note (a *correct* new diagnostic on a test's fixture means updating that test; a *wrong* inference means fixing the mechanism).

- [ ] **Step 2: Re-probe all eight positions on the real example**

Run each and confirm against the spec's target column:
```bash
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 11 8    # net  -> module ./Net summary
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 15 9    # server -> let server: Server
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 15 18   # Server -> type Server(...) -> Trial<Server, String>
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 18 13   # client -> let client: Client
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 18 29   # accept -> fn accept() ...
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 19 22   # receive -> fn receive() ...
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 28 9    # socket -> socket: Socket
.venv/bin/python scripts/lsp_probe.py examples/http/Main.thx 32 16   # bind -> fn bind(port: Int32) -> Trial<Void, String>, def -> Net.thx:58
```
Expected: each shows its target row; `bind` and `socket`/`accept` navigate into the right file/line. Any `(nothing)` or `Unknown` is a miss to fix before closing.

- [ ] **Step 3: Update the goal ledger / memory**

Mark the type-directed-hover goal complete in the memory index; note any deliberately-deferred edge (e.g. conflicting-`E` ctors rendering `-> Trial<Server>`). No code commit needed unless memory files live in-repo.

- [ ] **Step 4: Final commit (if any docs/memory in repo changed)**

```bash
git add -A && git commit  # "docs: close out type-directed hover"  + trailers   (skip if nothing staged)
```

---

## Self-Review

**Spec coverage:** ① Task 1; ② Task 2; ③ Task 3; ④ Task 4; ⑤ Task 5; ⑥ Task 6; ⑦ Task 7; safety spine (quiet inference, cross-module `UNKNOWN`, suite gate) in Global Constraints + Tasks 3/4/8; all 8 symptom rows re-probed in Task 8. No spec section is unimplemented.

**Placeholder scan:** every code step shows real code; the one verification note (Task 7 Step 3c, `Param.span` extent) is a concrete check with a defined fallback, not a TODO.

**Type consistency:** `("union", name, args)` 3-tuple is produced in Task 1 and consumed by `_trial_payload` (Task 2), `_collect_error_types` (Task 7), and `_render_generic` (Task 7) with matching indexing. `self.ctor_returns` (declared Task 3, filled Task 7) → `DocumentIndex.ctor_returns` (Task 7) → `hover` (Task 7). `resolve_remote(idx, ref, binding, docs, current_path)` unchanged at the boundary; only the private `_origin` gains `idx`. `render_signature(binding, ty=None, text=None)` — the new `text` is optional, so Task 5/6 callers that omit it stay valid.
