# Thoxa LSP Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hover, go-to-definition/declaration, find-all-references, document-symbol outline, and rename to the Thoxa Zed LSP, backed by a shared lexical resolver extracted from the compiler frontend.

**Architecture:** A new `thoxa/resolver.py` performs lexical name resolution over the parsed AST, producing a position-queryable `Resolution` (bindings + references). The LSP consumes it directly for navigation (Phase 1). The checker is then refactored to consume the same `Resolution` (Phase 2), which unlocks inferred-type hover and member (`.field`/`.method`) navigation gated on a provable receiver type. Precise name spans come from matching names against the token stream — no parser/AST changes.

**Tech Stack:** Python 3.13, `pytest`, zero third-party runtime dependencies (hand-rolled stdio JSON-RPC LSP). Package under `thoxa/`, tests under `tests/`.

## Global Constraints

- **Zero runtime dependencies.** `pyproject.toml` declares `dependencies = []`. Do not add any. Standard library only.
- **Reuse the frontend.** Resolution/inference logic must not be re-implemented; the resolver reuses `Checker._collect` for module-level names and the checker (Phase 2) consumes the resolver. No second, divergent scope implementation.
- **Behavior-preserving checker refactor.** The existing suite (`pytest`, ~214 tests) MUST stay green through Phase 2. Diagnostics must not change. The known gap — entry/module-file `TypeDef` *method bodies* are not type-inferred (`Checker._check_stmt` early-returns on `TypeDef`) — is *preserved*, not closed.
- **Spans are 1-based** `(line, col, length)`, single-line (`thoxa/errors.py:Span`). **LSP positions are 0-based** `(line, character)`, half-open ranges. All resolver/checker code works in 1-based spans; `thoxa/lsp/convert.py` is the only place that crosses to 0-based.
- **Single-file (v1).** An imported name's definition is its own `import` statement. Members resolve only when the receiver's concrete type is provable in this file; otherwise the occurrence is inert (no def/refs/hover/rename).
- **Naming rule (rename validation).** Value-ish names (fn, extern, method, param, field, let, mut, forvar) are camelCase; types (type, union) are PascalCase. The parser/lexer enforce this at load time, so rename must reject a new name of the wrong case rather than emit an invalid edit.
- **Handler safety.** Every LSP request handler catches exceptions, logs to stderr, and returns the empty/null result for its method (never crash the serve loop) — matching the existing `semantic_tokens_full`/`publish` idiom in `thoxa/lsp/server.py`.
- **Run tests with:** `python -m pytest` (from the repo root; the venv is `.venv`, `uv`-managed). Single test: `python -m pytest tests/test_resolver.py::test_name -v`.

## File Structure

- **Create `thoxa/resolver.py`** — the shared lexical resolver: `Binding`, `Reference`, `Resolution`, `resolve(module, source)`. One responsibility: turn a parsed module into a position-queryable binding/reference table.
- **Create `thoxa/lsp/index.py`** — `DocumentIndex` + `build_index(text, path)` + signature rendering. Bridges a `Resolution` (+ checker types in Phase 2) to position queries the handlers use.
- **Modify `thoxa/lsp/convert.py`** — add pure LSP-structure mappings (position lookup, Location, DocumentSymbol, Hover, WorkspaceEdit, SymbolKind, `doc_above`).
- **Modify `thoxa/lsp/server.py`** — add handlers + advertise capabilities.
- **Modify `thoxa/checker.py`** (Phase 2 only) — consume the `Resolution`; key inferred types by binding id; write member resolutions back.
- **Create `tests/test_resolver.py`**, **`tests/test_lsp_navigation.py`** — new tests. Extend **`tests/test_lsp.py`** for convert additions.

---

## Phase 1 — Resolver + resolver-backed navigation

### Task 1: Resolver data model and query helpers

**Files:**
- Create: `thoxa/resolver.py`
- Test: `tests/test_resolver.py`

**Interfaces:**
- Produces:
  - `Binding(id: int, name: str, kind: str, def_span: Optional[Span], node: object, exported: bool = False, owner: Optional[str] = None)` — `owner` is the enclosing type name for `field`/`method` bindings, else `None`.
  - `Reference(span: Span, binding_id: Optional[int], ctx: str)`
  - `Resolution` with: `bindings: dict[int, Binding]`, `references: list[Reference]`, `module_binding_ids: list[int]`, `member_index: dict[tuple[str, str], int]`, `member_refs: dict[int, Reference]`; methods `ref_at(line, col) -> Optional[Reference]`, `binding_of(ref) -> Optional[Binding]`, `refs_to(binding_id) -> list[Reference]`, `module_definitions() -> list[Binding]`, `members_of(type_name) -> list[Binding]`, `set_member_ref(fa_node_id, binding_id) -> None`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_resolver.py
from thoxa.errors import Span
from thoxa import resolver as R


def test_ref_at_prefers_narrowest_covering_span():
    res = R.Resolution()
    b = R.Binding(id=0, name="x", kind="let", def_span=Span(1, 5, 1), node=None)
    res.bindings[0] = b
    wide = R.Reference(Span(1, 1, 10), 0, "value")
    narrow = R.Reference(Span(1, 5, 1), 0, "value")
    res.references += [wide, narrow]
    assert res.ref_at(1, 5) is narrow          # 1-based line/col
    assert res.ref_at(1, 2) is wide
    assert res.ref_at(2, 1) is None


def test_refs_to_and_binding_of():
    res = R.Resolution()
    res.bindings[0] = R.Binding(0, "x", "let", Span(1, 5, 1), None)
    r = R.Reference(Span(2, 3, 1), 0, "value")
    res.references.append(r)
    assert res.binding_of(r).name == "x"
    assert res.refs_to(0) == [r]
    assert res.binding_of(R.Reference(Span(9, 9, 1), None, "value")) is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_resolver.py -v`
Expected: FAIL — `ModuleNotFoundError: thoxa.resolver` / attributes undefined.

- [ ] **Step 3: Write minimal implementation**

```python
# thoxa/resolver.py
"""Shared lexical name resolver for Thoxa.

Turns a parsed `ast.Module` into a position-queryable table of definitions
(`Binding`) and occurrences (`Reference`). Resolution is purely lexical —
independent of types — so both the checker and the LSP can consume it. Member
access (`obj.field`) is emitted as a *pending* reference and only linked to a
binding by the checker when the receiver's type is provable.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from . import ast
from .errors import Span, Source


@dataclass
class Binding:
    id: int
    name: str
    kind: str                      # fn|extern|type|union|field|method|param|let|mut|forvar|pattern|import|namespace|builtin
    def_span: Optional[Span]       # name span; None for external (builtin/prelude/ffi/imported)
    node: object                   # defining AST node, for signature/outline rendering
    exported: bool = False
    owner: Optional[str] = None    # enclosing type name for field/method, else None


@dataclass
class Reference:
    span: Span
    binding_id: Optional[int]
    ctx: str                       # value|type|call|member|label|pattern|def


class Resolution:
    def __init__(self) -> None:
        self.bindings: dict[int, Binding] = {}
        self.references: list[Reference] = []
        self.module_binding_ids: list[int] = []
        self.member_index: dict[tuple[str, str], int] = {}
        self.member_refs: dict[int, Reference] = {}   # id(FieldAccess node) -> pending member Reference

    def ref_at(self, line: int, col: int) -> Optional[Reference]:
        """The narrowest reference whose 1-based span covers (line, col)."""
        best: Optional[Reference] = None
        for r in self.references:
            s = r.span
            length = max(1, s.length)
            if s.line == line and s.col <= col < s.col + length:
                if best is None or length < max(1, best.span.length):
                    best = r
        return best

    def binding_of(self, ref: Optional[Reference]) -> Optional[Binding]:
        if ref is None or ref.binding_id is None:
            return None
        return self.bindings.get(ref.binding_id)

    def refs_to(self, binding_id: int) -> list[Reference]:
        return [r for r in self.references if r.binding_id == binding_id]

    def module_definitions(self) -> list[Binding]:
        return [self.bindings[i] for i in self.module_binding_ids]

    def members_of(self, type_name: str) -> list[Binding]:
        return [b for b in self.bindings.values() if b.owner == type_name]

    def set_member_ref(self, fa_node_id: int, binding_id: int) -> None:
        ref = self.member_refs.get(fa_node_id)
        if ref is not None:
            ref.binding_id = binding_id
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_resolver.py -v`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add thoxa/resolver.py tests/test_resolver.py
git commit -m "resolver: data model + position query helpers"
```

---

### Task 2: Module-level bindings (reuse the checker's collection)

**Files:**
- Modify: `thoxa/resolver.py`
- Test: `tests/test_resolver.py`

**Interfaces:**
- Produces: `resolve(module: ast.Module, source: Source) -> Resolution`. After this task it populates module-level bindings only (top-level `fn`/`extern`/`type`/`union`/`let`/`mut`/`import`/`namespace`) plus builtins and prelude names as external (`def_span=None`). The reference walk arrives in Task 3.
- Consumes: `thoxa.checker.Checker._collect`, `thoxa.checker._BUILTINS`, `thoxa.lexer.Lexer`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_resolver.py (append)
from thoxa.parser import parse
from thoxa.errors import Source


def _resolve(src, filename="<t>"):
    source = Source(src, filename)
    return R.resolve(parse(src, filename), source)


def test_module_definitions_cover_top_level():
    src = (
        "fn add(a!: Int64, b!: Int64) -> Int64 { a }\n"
        "type Point(x: Int64, y: Int64) {}\n"
        "let base = 10\n"
    )
    res = _resolve(src)
    defs = {b.name: b.kind for b in res.module_definitions()}
    assert defs == {"add": "fn", "Point": "type", "base": "let"}


def test_function_name_span_is_precise():
    res = _resolve("fn greet() -> Int64 { 0 }\n")
    b = next(b for b in res.module_definitions() if b.name == "greet")
    # 'greet' starts at column 4 (1-based) on line 1
    assert b.def_span.line == 1 and b.def_span.col == 4 and b.def_span.length == 5
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_resolver.py -k module_definitions -v`
Expected: FAIL — `resolve` undefined.

- [ ] **Step 3: Write minimal implementation**

Append to `thoxa/resolver.py`:

```python
from .lexer import Lexer
from .checker import Checker, _BUILTINS


class _Resolver:
    def __init__(self, module: ast.Module, source: Source) -> None:
        self.module = module
        self.source = source
        self.res = Resolution()
        self._next_id = 0
        self.tokens = Lexer(source).tokenize()   # for precise name spans

    # -- binding/reference construction ----------------------------------
    def _new_binding(self, name, kind, def_span, node, *, exported=False,
                     owner=None, module_level=False) -> int:
        bid = self._next_id
        self._next_id += 1
        self.res.bindings[bid] = Binding(bid, name, kind, def_span, node,
                                         exported=exported, owner=owner)
        if module_level:
            self.res.module_binding_ids.append(bid)
        return bid

    def _name_span(self, start: Span, name: str) -> Span:
        """Span of the first token at/after `start` whose text is `name`."""
        key = (start.line, start.col)
        for t in self.tokens:
            s = t.span
            if (s.line, s.col) >= key and t.value == name:
                return Span(s.line, s.col, len(name))
        return Span(start.line, start.col, len(name))   # fallback: never None

    # -- module scope seeding --------------------------------------------
    def _seed_module(self, scope: "Scope") -> None:
        # Names the checker knows about (funcs/types/unions/imports/namespaces),
        # reusing its collection so imports/exports are not re-implemented.
        chk = Checker(self.source)
        try:
            from .loader import load_prelude_source
            from .parser import parse as _parse
            prelude = _parse(*_prelude_args(load_prelude_source()))
            chk._collect(prelude.body)
        except Exception:
            pass
        chk._collect(self.module.body)

        # Prelude / builtins / imported names: external (no in-file def).
        for name in list(chk.funcs) + list(chk.type_kind) + list(chk.global_values) \
                + list(chk.imported) + list(_BUILTINS):
            if name not in scope.names:
                bid = self._new_binding(name, "builtin", None, None)
                scope.define(name, bid)

        # This file's own top-level definitions: precise spans + real nodes,
        # overriding the external placeholder so they are navigable/renameable.
        for stmt in self.module.body:
            self._declare_top_level(stmt, scope)

    def _declare_top_level(self, stmt, scope) -> None:
        kind = {ast.FunctionDef: "fn", ast.ExternFn: "extern",
                ast.TypeDef: "type", ast.TypeAlias: "union"}.get(type(stmt))
        if kind:
            span = self._name_span(stmt.span, stmt.name)
            bid = self._new_binding(stmt.name, kind, span, stmt,
                                    exported=getattr(stmt, "exported", False),
                                    module_level=True)
            scope.define(stmt.name, bid)
            if isinstance(stmt, ast.TypeDef):
                self._declare_members(stmt)
        elif isinstance(stmt, ast.LetBinding):
            span = self._name_span(stmt.span, stmt.name)
            bid = self._new_binding(stmt.name, "mut" if stmt.mutable else "let",
                                    span, stmt, exported=stmt.exported,
                                    module_level=True)
            scope.define(stmt.name, bid)
        elif isinstance(stmt, ast.Import):
            for local in _import_locals(stmt):
                bid = self._new_binding(local, "import", stmt.span, stmt,
                                        module_level=True)
                scope.define(local, bid)

    def _declare_members(self, td: ast.TypeDef) -> None:
        for p in td.ctor_params:
            self._member_binding(td.name, p.name, "field", p)
        for m in td.members:
            if isinstance(m, ast.Field):
                self._member_binding(td.name, m.name, "field", m,
                                     exported=m.exported)
            else:  # FunctionDef method
                self._member_binding(td.name, m.name, "method", m,
                                     exported=m.exported)

    def _member_binding(self, type_name, name, kind, node, *, exported=False) -> int:
        span = self._name_span(node.span, name)
        bid = self._new_binding(name, kind, span, node, exported=exported,
                                owner=type_name)
        self.res.member_index[(type_name, name)] = bid
        return bid


class Scope:
    def __init__(self, parent: Optional["Scope"] = None) -> None:
        self.parent = parent
        self.names: dict[str, int] = {}

    def define(self, name: str, binding_id: int) -> None:
        self.names[name] = binding_id

    def lookup(self, name: str) -> Optional[int]:
        s: Optional[Scope] = self
        while s is not None:
            if name in s.names:
                return s.names[name]
            s = s.parent
        return None


def _prelude_args(src) -> tuple[str, str]:
    return src.text, src.filename


def _import_locals(stmt: ast.Import) -> list[str]:
    if stmt.names is not None:
        return [local for _imported, local in stmt.names]
    return [stmt.alias or stmt.namespace]


def resolve(module: ast.Module, source: Source) -> Resolution:
    r = _Resolver(module, source)
    root = Scope()
    r._seed_module(root)
    r._root = root          # Task 3 walks the body under this scope
    return r.res
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_resolver.py -k "module_definitions or function_name_span" -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/resolver.py tests/test_resolver.py
git commit -m "resolver: module-level bindings via checker collection reuse"
```

---

### Task 3: Reference walk — expressions, statements, local scopes

**Files:**
- Modify: `thoxa/resolver.py`
- Test: `tests/test_resolver.py`

**Interfaces:**
- Produces: after this task, `resolve()` records a `Reference` for every `Name`, `TypeValue`, `TypeRef` name, and `FieldAccess` member (pending), and introduces local bindings for params, block `let`/`mut`, `for` vars, and function/method bodies. `ref_at` on a use returns a `Reference` whose `binding_id` points at the defining `Binding`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_resolver.py (append)
def _def_of_use(res, line, col):
    ref = res.ref_at(line, col)
    return res.binding_of(ref)


def test_local_use_resolves_to_param():
    src = "fn id(x!: Int64) -> Int64 {\n    x\n}\n"
    res = _resolve(src)
    b = _def_of_use(res, 2, 5)          # the `x` on line 2
    assert b is not None and b.kind == "param" and b.name == "x"


def test_shadowing_prefers_inner_binding():
    src = (
        "let x = 1\n"
        "fn f() -> Int64 {\n"
        "    let x = 2\n"
        "    x\n"
        "}\n"
    )
    res = _resolve(src)
    inner = _def_of_use(res, 4, 5)       # `x` on line 4 -> inner let (line 3)
    assert inner.def_span.line == 3


def test_call_resolves_callee():
    src = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    res = _resolve(src)
    b = _def_of_use(res, 2, 22)          # `g` in the call on line 2
    assert b is not None and b.name == "g" and b.kind == "fn"


def test_member_reference_is_pending():
    src = "fn f(s!: String) -> Int64 { s.count }\n"
    res = _resolve(src)
    ref = res.ref_at(1, 31)              # `count` after the dot
    assert ref is not None and ref.ctx == "member" and ref.binding_id is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_resolver.py -k "local_use or shadowing or call_resolves or member_reference" -v`
Expected: FAIL — references not recorded yet (`ref_at` returns None).

- [ ] **Step 3: Write minimal implementation**

Add the walk to `_Resolver` and call it from `resolve()`. Replace the body of `resolve()` and add these methods:

```python
    # -- the walk --------------------------------------------------------
    def run(self) -> Resolution:
        root = Scope()
        self._seed_module(root)
        for stmt in self.module.body:
            self._walk(stmt, root)
        return self.res

    def _walk(self, node, scope) -> None:
        if node is None:
            return
        handler = getattr(self, "_w_" + type(node).__name__, None)
        if handler is not None:
            handler(node, scope)
        else:
            self._walk_children(node, scope)

    def _walk_children(self, node, scope) -> None:
        import dataclasses
        if not dataclasses.is_dataclass(node):
            return
        for f in dataclasses.fields(node):
            self._walk_value(getattr(node, f.name), scope)

    def _walk_value(self, v, scope) -> None:
        import dataclasses
        if isinstance(v, (list, tuple)):
            for it in v:
                self._walk_value(it, scope)
        elif dataclasses.is_dataclass(v) and not isinstance(v, type):
            self._walk(v, scope)

    # -- references ------------------------------------------------------
    def _w_Name(self, n: ast.Name, scope) -> None:
        self.res.references.append(Reference(n.span, scope.lookup(n.ident), "value"))

    def _w_TypeValue(self, n: ast.TypeValue, scope) -> None:
        self.res.references.append(Reference(n.span, scope.lookup(n.name), "value"))

    def _w_TypeRef(self, n: ast.TypeRef, scope) -> None:
        if n.span is not None:
            span = Span(n.span.line, n.span.col, len(n.name))
            self.res.references.append(Reference(span, scope.lookup(n.name), "type"))
        for a in n.args:
            self._w_TypeRef(a, scope)
        if n.ret is not None:
            self._w_TypeRef(n.ret, scope)

    def _w_FieldAccess(self, n: ast.FieldAccess, scope) -> None:
        self._walk(n.obj, scope)                       # resolve the receiver
        span = self._name_span(n.span, n.name)         # n.span is the dot
        ref = Reference(span, None, "member")          # pending; checker links it
        self.res.references.append(ref)
        self.res.member_refs[id(n)] = ref

    # -- scoped constructs ----------------------------------------------
    def _w_FunctionDef(self, n: ast.FunctionDef, scope) -> None:
        inner = Scope(scope)
        self._bind_params(n.params, inner)
        self._walk(n.body, inner)

    def _bind_params(self, params, scope) -> None:
        for p in params:
            if p.type_ann is not None:
                self._w_TypeRef(p.type_ann, scope)
            if p.default is not None:
                self._walk(p.default, scope)
            span = self._name_span(p.span, p.name)
            bid = self._new_binding(p.name, "param", span, p)
            scope.define(p.name, bid)

    def _w_TypeDef(self, n: ast.TypeDef, scope) -> None:
        for p in n.ctor_params:
            if p.type_ann is not None:
                self._w_TypeRef(p.type_ann, scope)
        for m in n.members:
            if isinstance(m, ast.Field):
                if m.type_ann is not None:
                    self._w_TypeRef(m.type_ann, scope)
                if m.default is not None:
                    self._walk(m.default, scope)
            else:  # method: `self` gets the enclosing type's members in scope
                inner = Scope(scope)
                for mb in self.res.members_of(n.name):
                    inner.define(mb.name, mb.id)
                self._bind_params(m.params, inner)
                self._walk(m.body, inner)

    def _w_ExternFn(self, n: ast.ExternFn, scope) -> None:
        for p in n.params:
            if p.type_ann is not None:
                self._w_TypeRef(p.type_ann, scope)

    def _w_Block(self, n: ast.Block, scope) -> None:
        inner = Scope(scope)
        for stmt in n.stmts:
            self._walk(stmt, inner)

    def _w_LetBinding(self, n: ast.LetBinding, scope) -> None:
        self._walk(n.value, scope)
        if n.type_ann is not None:
            self._w_TypeRef(n.type_ann, scope)
        span = self._name_span(n.span, n.name)
        bid = self._new_binding(n.name, "mut" if n.mutable else "let", span, n)
        scope.define(n.name, bid)                       # in scope for later stmts

    def _w_For(self, n: ast.For, scope) -> None:
        self._walk(n.iterable, scope)
        inner = Scope(scope)
        span = self._name_span(n.span, n.var)
        bid = self._new_binding(n.var, "forvar", span, n)
        inner.define(n.var, bid)
        self._walk(n.body, inner)

    def _w_Cast(self, n: ast.Cast, scope) -> None:
        self._walk(n.operand, scope)
        self._w_TypeRef(n.type_ref, scope)

    def _w_CanCheck(self, n: ast.CanCheck, scope) -> None:
        self._walk(n.operand, scope)
        self._w_TypeRef(n.type_ref, scope)

    def _w_IsCheck(self, n: ast.IsCheck, scope) -> None:
        self._walk(n.operand, scope)
        self._w_TypeRef(n.type_ref, scope)
```

Notes for the implementer:
- `_w_FunctionDef` handles top-level *and* nested function definitions; the top-level *binding* was already created in Task 2's `_declare_top_level`, so this method only opens the param/body scope. Nested `fn` statements inside a block are handled by `_check_stmt`-style walking — a nested `FunctionDef` also needs a binding; add it in `_w_Block` before walking (see next bullet).
- In `_w_Block`, before `self._walk(stmt, inner)`, if `stmt` is a `FunctionDef`, create a binding for it so siblings can call it:
  ```python
  if isinstance(stmt, ast.FunctionDef):
      span = self._name_span(stmt.span, stmt.name)
      bid = self._new_binding(stmt.name, "fn", span, stmt)
      inner.define(stmt.name, bid)
  ```
- Replace `resolve()` to call the walk:
  ```python
  def resolve(module: ast.Module, source: Source) -> Resolution:
      return _Resolver(module, source).run()
  ```
- Match/patterns are handled in Task 4 (`_w_Match`); until then the generic `_walk_children` recurses into arms without binding pattern names (a pattern use may resolve to `None`, which Task 4 fixes).

- [ ] **Step 4: Run test to verify it passes, and the whole resolver suite**

Run: `python -m pytest tests/test_resolver.py -v`
Expected: PASS (all resolver tests so far).

- [ ] **Step 5: Commit**

```bash
git add thoxa/resolver.py tests/test_resolver.py
git commit -m "resolver: reference walk + local scopes (params/blocks/for/functions)"
```

---

### Task 4: Pattern binding and match scoping

**Files:**
- Modify: `thoxa/resolver.py`
- Test: `tests/test_resolver.py`

**Interfaces:**
- Produces: `match` arms open a scope; `BindingPattern` introduces a `pattern` binding; `ConstructorPattern` records a `Reference` (ctx `pattern`) for the constructor name to its type binding and recurses into subpatterns.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_resolver.py (append)
def test_match_binding_is_scoped_to_arm():
    src = (
        "type Box(v: Int64) {}\n"
        "fn f(b!: Box) -> Int64 {\n"
        "    match b {\n"
        "        Box(v) { v }\n"
        "    }\n"
        "}\n"
    )
    res = _resolve(src)
    b = _def_of_use(res, 4, 18)          # `v` in the arm body
    assert b is not None and b.kind == "pattern" and b.name == "v"


def test_constructor_pattern_references_type():
    src = (
        "type Box(v: Int64) {}\n"
        "fn f(b!: Box) -> Int64 { match b { Box(v) { v } } }\n"
    )
    res = _resolve(src)
    ref = res.ref_at(2, 36)              # `Box` constructor in the pattern
    assert ref is not None and ref.ctx == "pattern"
    assert res.binding_of(ref).name == "Box"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_resolver.py -k "match_binding or constructor_pattern" -v`
Expected: FAIL — pattern names resolve to `None`.

- [ ] **Step 3: Write minimal implementation**

Add to `_Resolver`:

```python
    def _w_Match(self, n: ast.Match, scope) -> None:
        self._walk(n.scrutinee, scope)
        for arm in n.arms:
            arm_scope = Scope(scope)
            self._bind_pattern(arm.pattern, arm_scope)
            self._walk(arm.body, arm_scope)
        if n.else_body is not None:
            self._walk(n.else_body, scope)

    def _bind_pattern(self, pat, scope) -> None:
        if isinstance(pat, ast.BindingPattern):
            span = Span(pat.span.line, pat.span.col, len(pat.name))
            bid = self._new_binding(pat.name, "pattern", span, pat)
            scope.define(pat.name, bid)
        elif isinstance(pat, ast.ConstructorPattern):
            span = Span(pat.span.line, pat.span.col, len(pat.name))
            self.res.references.append(
                Reference(span, scope.lookup(pat.name), "pattern"))
            for sub in pat.subpatterns:
                self._bind_pattern(sub, scope)
        elif isinstance(pat, ast.LiteralPattern):
            self._walk(pat.value, scope)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_resolver.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/resolver.py tests/test_resolver.py
git commit -m "resolver: match arms + pattern bindings and constructor references"
```

---

### Task 5: Named-argument label → parameter linking

**Files:**
- Modify: `thoxa/resolver.py`
- Test: `tests/test_resolver.py`

**Interfaces:**
- Produces: a `Call` whose callee resolves to a `fn`/`extern`/`method` binding links each labelled `Arg` to the matching parameter binding under that callee (ctx `label`). Best-effort: if the callee or a matching param is not found, the label is skipped (no reference).

- [ ] **Step 1: Write the failing test**

```python
# tests/test_resolver.py (append)
def test_named_arg_label_links_to_param():
    src = (
        "fn box(width!: Int64, height!: Int64) -> Int64 { width }\n"
        "fn main() -> Int64 { box(width: 3, height: 4) }\n"
    )
    res = _resolve(src)
    ref = res.ref_at(2, 26)              # the `width` label in the call
    assert ref is not None and ref.ctx == "label"
    b = res.binding_of(ref)
    assert b is not None and b.kind == "param" and b.name == "width"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_resolver.py -k named_arg_label -v`
Expected: FAIL — label has no reference.

- [ ] **Step 3: Write minimal implementation**

The generic walk already recurses into `Call.args`; add an explicit `_w_Call` that also links labels. Params are bound in a child scope of the fn, so build a one-shot lookup from the callee node's params:

```python
    def _w_Call(self, n: ast.Call, scope) -> None:
        self._walk(n.callee, scope)
        for ta in n.type_args:
            self._w_TypeRef(ta, scope)
        callee_binding = self._callee_binding(n.callee, scope)
        params = self._params_of(callee_binding)
        for a in n.args:
            if a.label is not None and a.label in params:
                span = self._name_span(a.span, a.label)
                self.res.references.append(
                    Reference(span, params[a.label], "label"))
            self._walk(a.value, scope)

    def _callee_binding(self, callee, scope):
        if isinstance(callee, ast.Name):
            return self.res.bindings.get(scope.lookup(callee.ident) or -1)
        if isinstance(callee, ast.FieldAccess):
            return None      # method-label linking needs the receiver type (Phase 2+); skip
        return None

    def _params_of(self, binding) -> dict[str, int]:
        """Map param name -> param binding id for a fn/extern binding's node."""
        out: dict[str, int] = {}
        if binding is None or not hasattr(binding.node, "params"):
            return out
        for p in binding.node.params:
            span = self._name_span(p.span, p.name)
            # Re-find the param binding created during the callee's own walk.
            for b in self.res.bindings.values():
                if b.kind == "param" and b.name == p.name and b.def_span == span:
                    out[p.name] = b.id
                    break
        return out
```

Note: `_params_of` matches on the param's precise `def_span`, so it finds the exact binding created when the callee function was walked (the callee is a top-level/sibling fn, already walked or walked later — param bindings are created during `_w_FunctionDef`; if the callee is walked after the call site, `out` is empty and labels are skipped, which is acceptable best-effort). Because top-level functions are all declared in Task 2 but their *bodies/params* are walked in body order, prefer resolving labels in a second pass is out of scope; skipping is fine.

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_resolver.py -v`
Expected: PASS. (If `test_named_arg_label_links_to_param` is order-sensitive because `box` is defined before `main`, it passes; document the best-effort limitation in the resolver module docstring.)

- [ ] **Step 5: Commit**

```bash
git add thoxa/resolver.py tests/test_resolver.py
git commit -m "resolver: best-effort named-arg label to parameter linking"
```

---

### Task 6: convert.py — LSP structure mappings

**Files:**
- Modify: `thoxa/lsp/convert.py`
- Test: `tests/test_lsp.py`

**Interfaces:**
- Produces (all pure, no compiler internals except `Span`/`Binding.kind` strings):
  - `position_covers(span: Span, line0: int, char0: int) -> bool`
  - `location(uri: str, span: Span) -> dict`
  - `hover(markdown: str, span: Optional[Span]) -> dict`
  - `text_edit(span: Span, new_text: str) -> dict`
  - `workspace_edit(uri: str, edits: list[dict]) -> dict`
  - `document_symbol(name: str, kind_str: str, range_span: Span, selection_span: Span, children: list[dict]) -> dict`
  - `symbol_kind(kind_str: str) -> int`
  - `doc_above(text: str, def_line: int) -> Optional[str]` — 1-based `def_line`; contiguous `##` block directly above.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp.py (append)
def test_position_covers_is_zero_based_half_open():
    sp = Span(line=3, col=5, length=4)     # 1-based cols 5..8 -> 0-based chars 4..7
    assert convert.position_covers(sp, 2, 4) is True
    assert convert.position_covers(sp, 2, 7) is True
    assert convert.position_covers(sp, 2, 8) is False
    assert convert.position_covers(sp, 1, 4) is False


def test_symbol_kind_maps_known_kinds():
    assert convert.symbol_kind("fn") == 12       # Function
    assert convert.symbol_kind("type") == 23      # Struct
    assert convert.symbol_kind("field") == 8      # Field
    assert convert.symbol_kind("let") == 13       # Variable


def test_doc_above_collects_contiguous_double_hash():
    text = (
        "## first line\n"
        "## second line\n"
        "fn f() -> Int64 { 0 }\n"
    )
    assert convert.doc_above(text, 3) == "first line\nsecond line"


def test_doc_above_stops_at_blank_or_single_hash():
    text = "# not a doc\n\n## real doc\nfn f() -> Int64 { 0 }\n"
    assert convert.doc_above(text, 4) == "real doc"
    assert convert.doc_above(text, 1) is None


def test_workspace_edit_shape():
    we = convert.workspace_edit("file:///m.thx", [convert.text_edit(Span(1, 1, 3), "abc")])
    assert we == {"changes": {"file:///m.thx": [
        {"range": {"start": {"line": 0, "character": 0},
                   "end": {"line": 0, "character": 3}}, "newText": "abc"}]}}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp.py -k "position_covers or symbol_kind or doc_above or workspace_edit" -v`
Expected: FAIL — functions undefined.

- [ ] **Step 3: Write minimal implementation**

Append to `thoxa/lsp/convert.py`:

```python
from typing import Optional

# LSP SymbolKind (subset we emit)
_SYMBOL_KIND = {
    "fn": 12, "extern": 12, "method": 6,       # Function / Method
    "type": 23, "union": 10,                    # Struct / Enum
    "field": 8, "param": 13, "let": 13,         # Field / Variable
    "mut": 13, "forvar": 13, "pattern": 13,
    "import": 2, "namespace": 3,                # Module / Namespace
}


def position_covers(span: Span, line0: int, char0: int) -> bool:
    """True if the 0-based LSP (line0, char0) falls within the 1-based span."""
    start = max(0, span.col - 1)
    return span.line - 1 == line0 and start <= char0 < start + max(1, span.length)


def location(uri: str, span: Span) -> dict:
    return {"uri": uri, "range": span_to_range(span)}


def hover(markdown: str, span: Optional[Span]) -> dict:
    out = {"contents": {"kind": "markdown", "value": markdown}}
    if span is not None:
        out["range"] = span_to_range(span)
    return out


def text_edit(span: Span, new_text: str) -> dict:
    return {"range": span_to_range(span), "newText": new_text}


def workspace_edit(uri: str, edits: list) -> dict:
    return {"changes": {uri: edits}}


def symbol_kind(kind_str: str) -> int:
    return _SYMBOL_KIND.get(kind_str, 13)       # default Variable


def document_symbol(name: str, kind_str: str, range_span: Span,
                    selection_span: Span, children: list) -> dict:
    sym = {
        "name": name,
        "kind": symbol_kind(kind_str),
        "range": span_to_range(range_span),
        "selectionRange": span_to_range(selection_span),
    }
    if children:
        sym["children"] = children
    return sym


def doc_above(text: str, def_line: int) -> Optional[str]:
    """The contiguous `##` doc block immediately above 1-based `def_line`."""
    lines = text.splitlines()
    collected: list[str] = []
    i = def_line - 2                            # 0-based line just above the def
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("##"):
            collected.append(stripped[2:].strip())
            i -= 1
        else:
            break
    if not collected:
        return None
    return "\n".join(reversed(collected))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp.py -v`
Expected: PASS (new + existing convert tests).

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/convert.py tests/test_lsp.py
git commit -m "lsp/convert: Location/Hover/WorkspaceEdit/DocumentSymbol/doc_above mappings"
```

---

### Task 7: index.py — DocumentIndex, build_index, signature rendering

**Files:**
- Create: `thoxa/lsp/index.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces:
  - `build_index(text: str, path: str = "<input>") -> DocumentIndex`
  - `DocumentIndex` with `resolution: Resolution`, `types: dict[int, object]`, `text: str`, and methods `reference_at(line0, char0) -> Optional[Reference]`, `binding_at(line0, char0) -> Optional[Binding]`, `type_of(binding) -> Optional[object]`.
  - `render_signature(binding: Binding, ty=None) -> str` — the fenced-block body (no fences) for hover.
- Consumes: `thoxa.resolver.resolve`, `thoxa.parser.parse`, `thoxa.errors.Source`, `thoxa.checker.ty_str`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py
from thoxa.lsp import index as I


def test_build_index_binding_at_finds_definition():
    src = "fn add(a!: Int64, b!: Int64) -> Int64 { a }\n"
    idx = I.build_index(src)
    b = idx.binding_at(0, 4)                 # 0-based: the `a` param name? no -> the fn name 'add'
    assert b is not None and b.name == "add" and b.kind == "fn"


def test_build_index_is_empty_on_unparseable():
    idx = I.build_index('fn main() { "unterminated ')
    assert idx.binding_at(0, 3) is None       # no crash, empty index


def test_render_signature_function():
    src = "fn serve(port!: Int32) -> Int64 { 0 }\n"
    idx = I.build_index(src)
    b = idx.binding_at(0, 3)                   # 'serve'
    assert I.render_signature(b) == "fn serve(port!: Int32) -> Int64"


def test_render_signature_type_and_field():
    src = "type Point(x: Int64, y: Int64) {}\n"
    idx = I.build_index(src)
    t = idx.binding_at(0, 5)                   # 'Point'
    assert I.render_signature(t) == "type Point(x: Int64, y: Int64)"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -v`
Expected: FAIL — `thoxa.lsp.index` missing.

- [ ] **Step 3: Write minimal implementation**

```python
# thoxa/lsp/index.py
"""Bridges a resolver `Resolution` (+ optional checker types) to the position
queries the LSP handlers need, and renders signatures for hover."""
from __future__ import annotations

from typing import Optional

from .. import ast
from ..errors import Source
from ..resolver import Binding, Reference, Resolution, resolve


class DocumentIndex:
    def __init__(self, resolution: Resolution, types: dict, text: str) -> None:
        self.resolution = resolution
        self.types = types
        self.text = text

    def reference_at(self, line0: int, char0: int) -> Optional[Reference]:
        return self.resolution.ref_at(line0 + 1, char0 + 1)

    def binding_at(self, line0: int, char0: int) -> Optional[Binding]:
        return self.resolution.binding_of(self.reference_at(line0, char0))

    def type_of(self, binding: Optional[Binding]):
        if binding is None:
            return None
        return self.types.get(binding.id)


def build_index(text: str, path: str = "<input>") -> DocumentIndex:
    from ..parser import parse
    source = Source(text, path)
    try:
        module = parse(text, path)
        resolution = resolve(module, source)
    except Exception:                      # mid-edit / unparseable buffer
        return DocumentIndex(Resolution(), {}, text)
    types: dict = {}                        # Phase 2 fills this
    return DocumentIndex(resolution, types, text)


# -- signature rendering -------------------------------------------------

def _type_str(ann: Optional[ast.TypeRef]) -> str:
    return str(ann) if ann is not None else ""


def _params_str(params) -> str:
    out = []
    for p in params:
        bang = "!" if p.positional else ""
        ann = f": {p.type_ann}" if p.type_ann is not None else ""
        out.append(f"{p.name}{bang}{ann}")
    return ", ".join(out)


def render_signature(binding: Binding, ty=None) -> str:
    n = binding.node
    k = binding.kind
    if k in ("fn", "method") and isinstance(n, ast.FunctionDef):
        ret = f" -> {n.return_type}" if n.return_type is not None else ""
        return f"fn {n.name}({_params_str(n.params)}){ret}"
    if k == "extern" and isinstance(n, ast.ExternFn):
        ret = f" -> {n.return_type}" if n.return_type is not None else ""
        return f"extern fn {n.name}({_params_str(n.params)}){ret}"
    if k == "type" and isinstance(n, ast.TypeDef):
        return f"type {n.name}({_params_str(n.ctor_params)})"
    if k == "union" and isinstance(n, ast.TypeAlias):
        return f"type {n.name} = {' | '.join(str(v) for v in n.variants)}"
    if k == "field" and isinstance(n, ast.Field):
        return f"{n.name}: {_type_str(n.type_ann)}"
    if k == "param" and isinstance(n, ast.Param):
        return f"{n.name}: {_type_str(n.type_ann)}"
    if k in ("let", "mut"):
        from ..checker import ty_str
        ann = getattr(n, "type_ann", None)
        shown = str(ann) if ann is not None else (ty_str(ty) if ty is not None else "")
        head = "mut" if k == "mut" else "let"
        return f"{head} {binding.name}: {shown}".rstrip(": ").rstrip()
    return f"{binding.name}"
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp_navigation.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/index.py tests/test_lsp_navigation.py
git commit -m "lsp/index: DocumentIndex, build_index, signature rendering"
```

---

### Task 8: Handler — go-to-definition / declaration

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Consumes: `index.build_index`, `convert.location`, `convert.uri_to_path`.
- Produces: `Server.definition(params) -> Optional[dict]` (a `Location` or `None`), registered for both `textDocument/definition` and `textDocument/declaration`; capabilities `definitionProvider: True`, `declarationProvider: True`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
from thoxa.lsp import server as S
from thoxa.lsp import protocol


class _FakeConn:
    def __init__(self): self.sent = []
    def send(self, msg): self.sent.append(msg)
    def read(self): return None


def _server_with(uri, text):
    srv = S.Server(_FakeConn())
    srv.docs[uri] = text
    return srv


def _pos(uri, line0, char0):
    return {"textDocument": {"uri": uri}, "position": {"line": line0, "character": char0}}


def test_definition_jumps_to_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    srv = _server_with(uri, text)
    loc = srv.definition(_pos(uri, 1, 21))       # `g` in the call (0-based)
    assert loc["uri"] == uri
    assert loc["range"]["start"] == {"line": 0, "character": 3}   # 'g' at col 4 (1-based)


def test_definition_returns_none_off_symbol():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn g() -> Int64 { 0 }\n")
    assert srv.definition(_pos(uri, 0, 0)) is None   # the `fn` keyword
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k definition -v`
Expected: FAIL — `Server` has no `definition`.

- [ ] **Step 3: Write minimal implementation**

In `thoxa/lsp/server.py`, add imports if missing (`from . import index`) and add the method + registrations. Add to the `initialize` capabilities dict:

```python
                "definitionProvider": True,
                "declarationProvider": True,
```

Add the handler method to `Server`:

```python
    def definition(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            binding = idx.binding_at(pos["line"], pos["character"])
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: definition failed: {exc}", file=sys.stderr)
            return None
        if binding is None or binding.def_span is None:
            return None
        return convert.location(uri, binding.def_span)
```

Register in `serve()`'s `requests` dict:

```python
            "textDocument/definition": self.definition,
            "textDocument/declaration": self.definition,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp_navigation.py -k definition -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: go-to-definition and declaration handler"
```

---

### Task 9: Handler — find-all-references

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: `Server.references(params) -> list[dict]` (list of `Location`), registered for `textDocument/references`; capability `referencesProvider: True`. Honors `params["context"]["includeDeclaration"]`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_references_lists_all_uses_and_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 3)                       # on the declaration of `g`
    params["context"] = {"includeDeclaration": True}
    locs = srv.references(params)
    starts = sorted((l["range"]["start"]["line"], l["range"]["start"]["character"]) for l in locs)
    # declaration at (0,3) plus two calls on line 1
    assert (0, 3) in starts
    assert sum(1 for s in starts if s[0] == 1) == 2


def test_references_can_exclude_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 3)
    params["context"] = {"includeDeclaration": False}
    locs = srv.references(params)
    assert all(l["range"]["start"]["line"] != 0 for l in locs)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k references -v`
Expected: FAIL — no `references` method.

- [ ] **Step 3: Write minimal implementation**

Add capability `"referencesProvider": True,` to `initialize`, register `"textDocument/references": self.references,` in `serve()`, and add:

```python
    def references(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        include_decl = (params.get("context") or {}).get("includeDeclaration", True)
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            binding = idx.binding_at(pos["line"], pos["character"])
            if binding is None:
                return []
            spans = []
            for ref in idx.resolution.refs_to(binding.id):
                if not include_decl and ref.span == binding.def_span:
                    continue
                spans.append(ref.span)
            if include_decl and binding.def_span is not None \
                    and binding.def_span not in spans:
                spans.append(binding.def_span)
            return [convert.location(uri, s) for s in spans]
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: references failed: {exc}", file=sys.stderr)
            return []
```

Note: the resolver records a `Reference` at each *use*; the definition's own name span is a `Binding.def_span`. `refs_to` returns uses only, so the declaration is appended explicitly when `includeDeclaration`. (A use whose span equals `def_span` — e.g. a self-reference — is de-duplicated by the `not in spans` guard.)

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp_navigation.py -k references -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: find-all-references handler"
```

---

### Task 10: Handler — document symbols (outline)

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: `Server.document_symbol(params) -> list[dict]` (hierarchical `DocumentSymbol[]`), registered for `textDocument/documentSymbol`; capability `documentSymbolProvider: True`. A `type` nests its `field`/`method` bindings.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_document_symbol_outline_with_nested_members():
    uri = "file:///m.thx"
    text = (
        "fn top() -> Int64 { 0 }\n"
        "type Point(x: Int64, y: Int64) {\n"
        "    fn mag() -> Int64 { 0 }\n"
        "}\n"
    )
    srv = _server_with(uri, text)
    syms = srv.document_symbol({"textDocument": {"uri": uri}})
    names = {s["name"]: s for s in syms}
    assert set(names) == {"top", "Point"}
    child_names = {c["name"] for c in names["Point"].get("children", [])}
    assert {"x", "y", "mag"} <= child_names
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k document_symbol -v`
Expected: FAIL — no `document_symbol` method.

- [ ] **Step 3: Write minimal implementation**

Add capability `"documentSymbolProvider": True,`, register `"textDocument/documentSymbol": self.document_symbol,`, and add:

```python
    def document_symbol(self, params: dict):
        uri = params["textDocument"]["uri"]
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            out = []
            for b in idx.resolution.module_definitions():
                if b.def_span is None:
                    continue
                children = []
                if b.kind == "type":
                    for m in idx.resolution.members_of(b.name):
                        if m.def_span is None:
                            continue
                        children.append(convert.document_symbol(
                            m.name, m.kind, m.def_span, m.def_span, []))
                out.append(convert.document_symbol(
                    b.name, b.kind, b.def_span, b.def_span, children))
            return out
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: documentSymbol failed: {exc}", file=sys.stderr)
            return []
```

Note: `range` and `selectionRange` both use the name span here (a valid `DocumentSymbol` requires `selectionRange ⊆ range`; equal is allowed). Widening `range` to the whole definition is a deferred refinement.

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp_navigation.py -k document_symbol -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: document-symbol outline with nested type members"
```

---

### Task 11: Handler — hover (signature + doc comment)

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: `Server.hover(params) -> Optional[dict]` (a `Hover` or `None`), registered for `textDocument/hover`; capability `hoverProvider: True`. Markdown = a fenced ```` ```thoxa ```` block with `render_signature`, then the `doc_above` block (if any). A binding with no in-file definition (`def_span is None`) still hovers its signature if renderable; builtins → `None`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_hover_shows_signature_and_doc():
    uri = "file:///m.thx"
    text = (
        "## Starts the echo server.\n"
        "## Threads failures as Trial.\n"
        "fn serve(port!: Int32) -> Int64 { 0 }\n"
        "fn main() -> Int64 { serve(port: 8080) }\n"
    )
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 3, 21))              # `serve` in the call
    val = h["contents"]["value"]
    assert "```thoxa" in val
    assert "fn serve(port!: Int32) -> Int64" in val
    assert "Starts the echo server." in val
    assert "Threads failures as Trial." in val


def test_hover_none_off_symbol():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f() -> Int64 { 0 }\n")
    assert srv.hover(_pos(uri, 0, 0)) is None    # `fn` keyword
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k hover -v`
Expected: FAIL — no `hover` method.

- [ ] **Step 3: Write minimal implementation**

Add capability `"hoverProvider": True,`, register `"textDocument/hover": self.hover,`, and add:

```python
    def hover(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        text = self.docs.get(uri, "")
        try:
            idx = index.build_index(text, convert.uri_to_path(uri))
            ref = idx.reference_at(pos["line"], pos["character"])
            binding = idx.resolution.binding_of(ref)
            if binding is None or binding.kind == "builtin":
                return None
            sig = index.render_signature(binding, idx.type_of(binding))
            if not sig:
                return None
            parts = [f"```thoxa\n{sig}\n```"]
            if binding.def_span is not None:
                doc = convert.doc_above(text, binding.def_span.line)
                if doc:
                    parts.append(doc)
            return convert.hover("\n\n".join(parts), ref.span if ref else None)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: hover failed: {exc}", file=sys.stderr)
            return None
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_lsp_navigation.py -k hover -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: hover with signature + ## doc comment"
```

---

### Task 12: Handler — rename (lexical bindings) + prepareRename

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces:
  - `Server.prepare_rename(params) -> Optional[dict]` — a `Range` (of the identifier) or `None`, registered for `textDocument/prepareRename`.
  - `Server.rename(params) -> Optional[dict]` — a `WorkspaceEdit` or `None`, registered for `textDocument/rename`.
  - Capability `renameProvider: {"prepareProvider": True}`.
- Rename is refused (returns `None`) when the binding is a `builtin`/`import` (no in-file def) or an unresolved `member` (Phase 2 gates provable members). New name must match the binding's casing class (see Global Constraints); a mismatch raises an LSP error via the existing `_error` path — implement by returning `None` for v1 simplicity plus a stderr log (a hard error dialog is a deferred refinement).

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_prepare_rename_on_local_returns_range():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f(x!: Int64) -> Int64 { x }\n")
    r = srv.prepare_rename(_pos(uri, 0, 26))     # the `x` use
    assert r == {"start": {"line": 0, "character": 26}, "end": {"line": 0, "character": 27}}


def test_prepare_rename_refuses_builtin():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f() -> Int64 { println(\"hi\")  0 }\n")
    assert srv.prepare_rename(_pos(uri, 0, 18)) is None   # `println`


def test_rename_edits_declaration_and_uses():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 1, 21)                     # a `g` call
    params["newName"] = "h"
    we = srv.rename(params)
    edits = we["changes"][uri]
    assert len(edits) == 3                        # decl + two calls
    assert all(e["newText"] == "h" for e in edits)


def test_rename_rejects_wrong_case():
    uri = "file:///m.thx"
    srv = _server_with(uri, "type Point(x: Int64) {}\nfn main() -> Int64 { 0 }\n")
    params = _pos(uri, 0, 5)                       # `Point`
    params["newName"] = "point"                    # not PascalCase
    assert srv.rename(params) is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k "rename" -v`
Expected: FAIL — no `rename`/`prepare_rename`.

- [ ] **Step 3: Write minimal implementation**

Add capability `"renameProvider": {"prepareProvider": True},`, register both methods in `serve()`:

```python
            "textDocument/prepareRename": self.prepare_rename,
            "textDocument/rename": self.rename,
```

Add a casing helper and the two methods to `Server`:

```python
    _VALUE_KINDS = {"fn", "extern", "method", "param", "field", "let", "mut",
                    "forvar", "pattern"}
    _TYPE_KINDS = {"type", "union"}

    def _renameable(self, binding):
        if binding is None or binding.def_span is None:
            return False
        return binding.kind in self._VALUE_KINDS or binding.kind in self._TYPE_KINDS

    def _valid_new_name(self, binding, new_name: str) -> bool:
        if not new_name or not (new_name[0].isalpha() or new_name[0] == "_"):
            return False
        if any(not (c.isalnum() or c == "_") for c in new_name):
            return False
        first = new_name[0]
        if binding.kind in self._TYPE_KINDS:
            return first.isupper()
        return first.islower() or first == "_"

    def prepare_rename(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        try:
            idx = index.build_index(self.docs.get(uri, ""), convert.uri_to_path(uri))
            ref = idx.reference_at(pos["line"], pos["character"])
            binding = idx.resolution.binding_of(ref)
            if not self._renameable(binding):
                return None
            return convert.span_to_range(ref.span)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: prepareRename failed: {exc}", file=sys.stderr)
            return None

    def rename(self, params: dict):
        uri = params["textDocument"]["uri"]
        pos = params["position"]
        new_name = params.get("newName", "")
        try:
            idx = index.build_index(self.docs.get(uri, ""), convert.uri_to_path(uri))
            binding = idx.binding_at(pos["line"], pos["character"])
            if not self._renameable(binding) or not self._valid_new_name(binding, new_name):
                return None
            spans = [r.span for r in idx.resolution.refs_to(binding.id)]
            if binding.def_span not in spans:
                spans.append(binding.def_span)
            edits = [convert.text_edit(s, new_name) for s in spans]
            return convert.workspace_edit(uri, edits)
        except Exception as exc:  # noqa: BLE001
            print(f"thoxa-lsp: rename failed: {exc}", file=sys.stderr)
            return None
```

- [ ] **Step 4: Run test to verify it passes, then the full suite**

Run: `python -m pytest tests/test_lsp_navigation.py -v && python -m pytest`
Expected: PASS. The full suite confirms Phase 1 added no regressions (the checker is untouched so far).

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: rename + prepareRename for lexical bindings (casing-validated)"
```

**Phase 1 checkpoint — shippable.** Def, refs, outline, hover (annotation signatures + docs), and rename work for lexical bindings. Member navigation and inferred-type hover arrive in Phase 2.

---

## Phase 2 — Checker consumes the resolver (inferred hover + members)

> Every task in this phase ends by running the **full suite** (`python -m pytest`) and confirming it stays green. Diagnostics must not change.

### Task 13: Checker builds and consumes the Resolution

**Files:**
- Modify: `thoxa/checker.py`
- Test: `tests/test_checker.py` (regression only)

**Interfaces:**
- Produces: `Checker` gains `self.resolution: Optional[Resolution]` and `self.types: dict[int, object]`. `check_module` builds a `Resolution` from the module and stores it. `_infer_Name` resolves via the resolution first, falling back to the old `env` lookup. `check(text, filename)` behavior is unchanged.
- Consumes: `thoxa.resolver.resolve`.

- [ ] **Step 1: Write the failing test (a characterization test that must keep passing)**

```python
# tests/test_checker.py (append)
def test_checker_exposes_resolution_after_check():
    from thoxa.checker import Checker
    from thoxa.errors import Source
    from thoxa.parser import parse
    src = "fn f(x!: Int64) -> Int64 { x }\n"
    source = Source(src, "<t>")
    chk = Checker(source)
    chk.check_module(parse(src, "<t>"))
    assert chk.resolution is not None
    assert any(b.name == "x" and b.kind == "param"
               for b in chk.resolution.bindings.values())
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_checker.py -k exposes_resolution -v`
Expected: FAIL — `Checker` has no `resolution`.

- [ ] **Step 3: Write minimal implementation**

In `thoxa/checker.py`:
- In `__init__`, add `self.resolution = None` and `self.types = {}`.
- In `check_module`, after parsing/collecting and before/around the body walk, build the resolution:
  ```python
  from .resolver import resolve as _resolve_names
  self.resolution = _resolve_names(module, self.src)
  ```
  Place this right after `self._collect(module.body)` so the module AST is the same object the resolver walks.
- Change `_infer_Name` to consult the resolution first, then fall back:
  ```python
  def _infer_Name(self, n: ast.Name, env):
      if self.resolution is not None:
          ref = self.resolution.ref_at(n.span.line, n.span.col)
          b = self.resolution.binding_of(ref)
          if b is not None and b.id in self.types:
              return self.types[b.id]
      if n.ident in env:
          val = env[n.ident]
          if isinstance(val, tuple) and val and val[0] == "namespace":
              self.err("a module namespace cannot be used as a value", n.span)
              return UNKNOWN
          return val
      self.err(f"'{n.ident}' is not defined", n.span)
      return UNKNOWN
  ```

Because `self.types` is still empty at this point, `_infer_Name` falls through to the existing `env` path — **behavior is identical**. Task 14 starts populating `self.types`.

- [ ] **Step 4: Run the full suite**

Run: `python -m pytest`
Expected: PASS — all ~214 tests, plus the new characterization test.

- [ ] **Step 5: Commit**

```bash
git add thoxa/checker.py tests/test_checker.py
git commit -m "checker: build and store the shared Resolution (behavior-preserving)"
```

---

### Task 14: Key inferred types by binding id

**Files:**
- Modify: `thoxa/checker.py`
- Test: `tests/test_checker.py` (regression) + `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: at each definition site the checker infers (`let`/`mut`, params, `for` var, pattern binds), it records `self.types[binding_id] = ty` (binding id looked up via `self.resolution.ref_at(name_span)`). The `env` path remains as the fallback, so diagnostics are unchanged.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_checker_records_inferred_let_type():
    from thoxa.checker import Checker, ty_str
    from thoxa.errors import Source
    from thoxa.parser import parse
    src = "fn f() -> Int64 {\n    let n = 41\n    n\n}\n"
    source = Source(src, "<t>")
    chk = Checker(source)
    chk.check_module(parse(src, "<t>"))
    # find the `let n` binding and confirm its inferred type was captured
    b = next(b for b in chk.resolution.bindings.values()
             if b.name == "n" and b.kind == "let")
    assert ty_str(chk.types[b.id]) == "Int64"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k records_inferred -v`
Expected: FAIL — `self.types` empty for `n`.

- [ ] **Step 3: Write minimal implementation**

Add a helper and record types at definition sites. In `thoxa/checker.py`:

```python
    def _record_type(self, name_span, ty) -> None:
        if self.resolution is None:
            return
        ref = self.resolution.ref_at(name_span.line, name_span.col)
        b = self.resolution.binding_of(ref)
        if b is None:
            # the def name may not itself be a Reference; find the binding whose
            # def_span matches this position instead.
            for cand in self.resolution.bindings.values():
                if cand.def_span is not None \
                        and cand.def_span.line == name_span.line \
                        and cand.def_span.col == name_span.col:
                    b = cand
                    break
        if b is not None:
            self.types[b.id] = ty
```

Then record at each site (the `env` writes stay; add a `_record_type` next to each):
- In `_check_stmt` `LetBinding` branch, after computing `env[stmt.name]`:
  ```python
  self._record_type(stmt.span, env[stmt.name])
  ```
  (`stmt.span` matches the def; `_record_type` falls back to `def_span` matching, and the resolver's `let` `def_span` is the name span found from `stmt.span`, so the fallback finds it.)
- In `_check_function`, after `env[p.name] = ty` for each param: `self._record_type(p.span, ty)`.
- In `_infer_For`, after `scope[n.var] = elem`: `self._record_type(n.span, elem)`.
- In `_bind_pattern`, after `scope[sub.name] = fty` and `scope[pat.name] = sty`: record with the sub/pattern span.

Because `_record_type` only *adds* to `self.types` and `_infer_Name` returns those types only when present (and they equal what `env` held), diagnostics are unchanged.

- [ ] **Step 4: Run the failing test, then the full suite**

Run: `python -m pytest tests/test_lsp_navigation.py -k records_inferred -v && python -m pytest`
Expected: PASS everywhere.

- [ ] **Step 5: Commit**

```bash
git add thoxa/checker.py tests/test_lsp_navigation.py
git commit -m "checker: record inferred types keyed by binding id"
```

---

### Task 15: Member write-back (provable receiver types)

**Files:**
- Modify: `thoxa/checker.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: when `_member` resolves `obj.name` on a provable concrete type (`("named", TypeName)` with `name` in that type's members), the checker links the pending member reference: `self.resolution.set_member_ref(id(fa_node), self.resolution.member_index[(TypeName, name)])`. Requires threading the `FieldAccess` node into `_member`. Unprovable receivers leave the reference pending (`binding_id=None`) → inert.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_member_resolves_on_provable_type():
    uri = "file:///m.thx"
    text = (
        "type Point(x: Int64, y: Int64) {\n"
        "    fn mag() -> Int64 { 0 }\n"
        "}\n"
        "fn f(p!: Point) -> Int64 { p.mag() }\n"
    )
    srv = _server_with(uri, text)
    loc = srv.definition(_pos(uri, 3, 29))       # `mag` after the dot
    assert loc is not None
    assert loc["range"]["start"]["line"] == 1     # the method decl on line 2


def test_member_inert_when_type_unprovable():
    uri = "file:///m.thx"
    text = "fn f(p!: Any) -> Int64 { p.mystery() }\n"
    srv = _server_with(uri, text)
    assert srv.definition(_pos(uri, 0, 27)) is None   # `mystery` — no provable type
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_lsp_navigation.py -k "member_resolves or member_inert" -v`
Expected: FAIL — member reference stays pending; `definition` returns None for `mag`.

- [ ] **Step 3: Write minimal implementation**

The checker must build its own resolution *and share it with the LSP index* (Task 16 wires the index). For write-back, thread the `FieldAccess` node into `_member`:

- In `_infer_FieldAccess`, change the member call to pass the node:
  ```python
  return self._member(objty, n.name, n.span, node=n)
  ```
- In `_infer_Call`'s `FieldAccess` branch, likewise pass `node=n.callee`:
  ```python
  member = self._member(objty, n.callee.name, n.callee.span, node=n.callee)
  ```
- Change `_member`'s signature and add the write-back where a member is found on a `("named", X)` type:
  ```python
  def _member(self, objty, name, span, node=None):
      if isinstance(objty, tuple) and objty[0] == "named":
          if objty[1] not in self.type_members:
              return UNKNOWN
          members = self.type_members[objty[1]]
          if name not in members:
              self.err(f"'{objty[1]}' has no member '{name}'", span)
              return UNKNOWN
          if name not in self.type_exported.get(objty[1], set()):
              self.err(f"member '{name}' of '{objty[1]}' is private", span)
          self._link_member(node, objty[1], name)
          return members[name]
      # ... unchanged array/String branches ...
  ```
  Add:
  ```python
  def _link_member(self, node, type_name, name) -> None:
      if node is None or self.resolution is None:
          return
      bid = self.resolution.member_index.get((type_name, name))
      if bid is not None:
          self.resolution.set_member_ref(id(node), bid)
  ```

Note: the private-member check still emits its diagnostic (behavior preserved); write-back happens regardless so navigation works even to a private member within the same file. `Any`/unknown receivers never reach the `("named", …)` branch, so their members stay inert.

- [ ] **Step 4: Run the full suite (definition test still uses Phase-1 index — expect the member tests to still fail until Task 16)**

Run: `python -m pytest`
Expected: the existing ~214 + Phase-1 nav tests PASS; `test_member_resolves_on_provable_type`/`test_member_inert_when_type_unprovable` **still fail** because `index.build_index` does not yet run the checker. That is expected — they pass after Task 16. Commit the write-back now.

- [ ] **Step 5: Commit**

```bash
git add thoxa/checker.py
git commit -m "checker: link member references on provable receiver types"
```

---

### Task 16: Wire checker analysis into the index (inferred hover + members)

**Files:**
- Modify: `thoxa/lsp/index.py`, `thoxa/checker.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: `build_index` runs the checker so the `DocumentIndex` carries the shared `Resolution` (with member write-back) and `self.types`. Add `Checker.analyze(module) -> tuple[Resolution, dict]` (or reuse `check_module` then read `chk.resolution`/`chk.types`). Hover fills inferred `let` types and member signatures; the two member tests from Task 15 pass.

- [ ] **Step 1: Update `build_index` and re-point the member tests**

```python
# thoxa/lsp/index.py — replace build_index
def build_index(text: str, path: str = "<input>") -> DocumentIndex:
    from ..parser import parse
    from ..checker import Checker
    source = Source(text, path)
    try:
        module = parse(text, path)
    except Exception:
        return DocumentIndex(Resolution(), {}, text)
    chk = Checker(source)
    try:
        chk.check_module(module)                 # builds resolution + types, links members
        resolution = chk.resolution or resolve(module, source)
        types = chk.types
    except Exception:
        # analysis failed mid-edit: fall back to a resolver-only index
        try:
            resolution = resolve(module, source)
        except Exception:
            return DocumentIndex(Resolution(), {}, text)
        types = {}
    return DocumentIndex(resolution, types, text)
```

- [ ] **Step 2: Run the member + inferred-hover tests to verify they now pass**

```python
# tests/test_lsp_navigation.py (append)
def test_hover_shows_inferred_let_type():
    uri = "file:///m.thx"
    text = "fn f() -> Int64 {\n    let n = 41\n    n\n}\n"
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 2, 4))               # the `n` use on line 3
    assert "let n: Int64" in h["contents"]["value"]
```

Run: `python -m pytest tests/test_lsp_navigation.py -k "member_resolves or member_inert or inferred_let_type" -v`
Expected: PASS.

- [ ] **Step 3: Run the full suite**

Run: `python -m pytest`
Expected: PASS everywhere. `collect_diagnostics` in `analysis.py` still calls `check()` — unaffected — so diagnostics behavior is unchanged; only the navigation index now also runs the checker.

- [ ] **Step 4: Confirm no double-reporting / performance regression**

Run: `python -m pytest tests/test_lsp.py -v`
Expected: PASS — the diagnostics path (`analysis.collect_diagnostics`) is independent of `build_index`; both parse once per call, which is acceptable for editor latency.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/index.py tests/test_lsp_navigation.py
git commit -m "lsp/index: run the checker so hover shows inferred types and members resolve"
```

---

### Task 17: Extend rename to provable members

**Files:**
- Modify: `thoxa/lsp/server.py`
- Test: `tests/test_lsp_navigation.py`

**Interfaces:**
- Produces: `rename`/`prepare_rename` now accept `field`/`method` bindings, because a resolved member reference points at a real in-file member binding. Unprovable members remain inert (their references never link, so `binding_at` returns `None` → refused). No handler changes are needed beyond confirming `field`/`method` are in `_VALUE_KINDS` (they are) and that member references carry the linked `binding_id`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_lsp_navigation.py (append)
def test_rename_member_updates_declaration_and_uses():
    uri = "file:///m.thx"
    text = (
        "type Point(x: Int64, y: Int64) {\n"
        "    fn mag() -> Int64 { 0 }\n"
        "}\n"
        "fn f(p!: Point) -> Int64 { p.mag() }\n"
    )
    srv = _server_with(uri, text)
    params = _pos(uri, 3, 29)                     # `mag` use
    params["newName"] = "magnitude"
    we = srv.rename(params)
    edits = we["changes"][uri]
    assert len(edits) == 2                        # method decl + the call use
    assert all(e["newText"] == "magnitude" for e in edits)


def test_rename_member_refused_when_unprovable():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f(p!: Any) -> Int64 { p.mystery() }\n")
    params = _pos(uri, 0, 27)
    params["newName"] = "clarity"
    assert srv.rename(params) is None
```

- [ ] **Step 2: Run test to verify it fails or passes**

Run: `python -m pytest tests/test_lsp_navigation.py -k "rename_member" -v`
Expected: the *unprovable* case already passes (inert); the *provable* case may already pass if Task 15/16 linked the member. If the provable case fails, verify the member reference is linked (`ref.binding_id` set) and that `refs_to(member_binding.id)` includes both the decl and the use.

- [ ] **Step 3: Adjust if needed**

If `refs_to` misses the declaration, ensure the member binding's `def_span` is appended (the existing `rename` already appends `binding.def_span` when absent). No code change is expected; if a change is needed it is confined to `rename`'s span-gathering.

- [ ] **Step 4: Run the full suite**

Run: `python -m pytest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: rename provable members; leave unprovable members inert"
```

---

### Task 18: Capability review, integration smoke, and manual Zed checklist

**Files:**
- Modify: `thoxa/lsp/server.py` (only if a capability is missing)
- Test: `tests/test_lsp_navigation.py`

- [ ] **Step 1: Write an integration test asserting advertised capabilities**

```python
# tests/test_lsp_navigation.py (append)
def test_initialize_advertises_navigation_capabilities():
    srv = S.Server(_FakeConn())
    caps = srv.initialize({})["capabilities"]
    for key in ("hoverProvider", "definitionProvider", "declarationProvider",
                "referencesProvider", "documentSymbolProvider"):
        assert caps.get(key) is True
    assert caps.get("renameProvider") == {"prepareProvider": True}
```

- [ ] **Step 2: Run it**

Run: `python -m pytest tests/test_lsp_navigation.py -k initialize_advertises -v`
Expected: PASS (add any missing capability to `initialize`).

- [ ] **Step 3: Run the entire suite one final time**

Run: `python -m pytest`
Expected: PASS — all existing tests plus the new resolver, convert, and navigation tests.

- [ ] **Step 4: Manual Zed smoke test (record results in the commit message)**

The dev extension is already side-loaded (`editor/zed/`). In Zed, open a `.thx` file (e.g. `examples/http/Main.thx`) and confirm:
- Hover a function/param/type → signature + any `##` doc.
- Cmd/Ctrl-click a call → jumps to its definition; `g()` in another spot → declaration.
- Right-click → Find All References highlights every use.
- Outline panel shows top-level defs with type members nested.
- Rename a local and a provable method → all sites update; renaming `println` or an `Any`-typed member is refused.

- [ ] **Step 5: Commit**

```bash
git add thoxa/lsp/server.py tests/test_lsp_navigation.py
git commit -m "lsp: verify navigation capabilities + integration smoke"
```

---

## Self-Review

**Spec coverage** (against `docs/specs/2026-07-13-thoxa-lsp-navigation-design.md`):
- Shared resolver (`thoxa/resolver.py`) — Tasks 1–5. ✓
- Checker consumes it (types by binding id; member write-back) — Tasks 13–15. ✓
- `index.py` + `convert.py` additions — Tasks 6, 7, 16. ✓
- Handlers (def/decl, refs, documentSymbol, hover, rename+prepare) + capabilities — Tasks 8–12, 17, 18. ✓
- Members gated on provable type — Tasks 15–17 (inert path tested in 15 and 17). ✓
- Hover = signature + `##` doc via source pre-scan — Tasks 6 (`doc_above`), 11. ✓
- Single-file: imported name → its `import` statement — Task 2 (`import` binding `def_span = stmt.span`); Task 8 returns that Location. ✓
- Behavior-preserving refactor gated by the suite — Tasks 13–16 each run `python -m pytest`. ✓
- Named-arg labels (best-effort) — Task 5. ✓
- Precise name spans without AST surgery — Task 2 (`_name_span` token match). ✓

**Placeholder scan:** no TBD/TODO; every code step carries complete code; tests are concrete with expected positions. The one deliberately-deferred refinement (hard-error dialog on wrong-case rename; widening `documentSymbol.range`) is called out explicitly, not left as a gap.

**Type consistency:** `Binding`/`Reference`/`Resolution` field and method names match across resolver, checker, index, and handlers (`ref_at`, `binding_of`, `refs_to`, `module_definitions`, `members_of`, `member_index`, `set_member_ref`, `render_signature`, `build_index`, `type_of`). `_member(objty, name, span, node=None)` signature is consistent between its definition (Task 15) and both call sites.

**Known cross-task ordering caveat (documented, not a defect):** the two member tests written in Task 15 pass only after Task 16 wires the checker into `build_index`; Task 15 Step 4 states this explicitly.
