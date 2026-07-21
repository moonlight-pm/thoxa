# thoxa/resolver.py
"""Shared lexical name resolver for Thoxa.

Turns a parsed `ast.Module` into a position-queryable table of definitions
(`Binding`) and occurrences (`Reference`). Resolution is purely lexical —
independent of types — so both the checker and the LSP can consume it. Member
access (`obj.field`) is emitted as a *pending* reference and only linked to a
binding by the checker when the receiver's type is provable.

Named-argument labels (`f(label: value)`) are linked to the callee's matching
param binding on a best-effort basis: `_params_of` re-finds the param binding
by scanning already-created bindings for the callee's param name/span, which
only succeeds if the callee's body/params were already walked (functions are
walked in body order, not resolved in a separate pass). If the callee is
walked after the call site, or the callee can't be resolved (e.g. a method
call via `obj.method(...)`, which needs the receiver's type), the label is
simply skipped — no reference is produced. This is intentional, not a bug.
"""
from __future__ import annotations

import dataclasses
from dataclasses import dataclass
from typing import Optional

from . import ast
from .errors import Span, Source


@dataclass
class Binding:
    id: int
    name: str
    kind: str                      # fn|extern|type|union|field|method|param|let|mut|forvar|pattern|import|builtin
    def_span: Optional[Span]       # name span; None for external (builtin/prelude/ffi/imported)
    node: object                   # defining AST node, for signature/outline rendering
    exported: bool = False
    owner: Optional[str] = None    # enclosing type name for field/method, else None


@dataclass
class Reference:
    span: Span
    binding_id: Optional[int]
    ctx: str                       # value|type|member|label|pattern


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
            from .loader import parse_prelude
            prelude, _ = parse_prelude()
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
                # def_span on the local name (not the `import` keyword) so hovering
                # the name at its own import site finds this binding.
                span = self._name_span(stmt.span, local)
                bid = self._new_binding(local, "import", span, stmt,
                                        module_level=True)
                scope.define(local, bid)

    def _declare_members(self, td: ast.TypeDef) -> None:
        for p in td.ctor_params:
            self._member_binding(td.name, p.name, "field", p, exported=p.exported)
        for m in td.members:
            if isinstance(m, ast.Field):
                self._member_binding(td.name, m.name, "field", m,
                                     exported=m.exported)
            else:  # FunctionDef method
                self._member_binding(td.name, m.name, "method", m,
                                     exported=m.exported)

    def _member_binding(self, type_name, name, kind, node, *, exported=False) -> int:
        span = self._name_span(node.span, name)
        existing_id = self.res.member_index.get((type_name, name))
        if existing_id is not None:
            existing = self.res.bindings[existing_id]
            if existing.kind == "field" and kind == "field":
                # `type Box(n: Int64) { mut n = n }`: the ctor param and the
                # same-name field are ONE storage slot at runtime (construct()
                # copies the ctor arg into inst.fields[name], then field
                # defaults read from inst.fields). Unify into a single
                # binding -- keep the ctor param's binding (created first, so
                # its def_span is the natural value-source definition) -- and
                # record this declaration site as a reference to it so
                # find-references/rename cover both spots.
                if exported:
                    existing.exported = True
                self.res.references.append(Reference(span, existing_id, "member"))
                return existing_id
        bid = self._new_binding(name, kind, span, node, exported=exported,
                                owner=type_name)
        self.res.member_index[(type_name, name)] = bid
        return bid

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
        if not dataclasses.is_dataclass(node):
            return
        for f in dataclasses.fields(node):
            self._walk_value(getattr(node, f.name), scope)

    def _walk_value(self, v, scope) -> None:
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

    # -- scoped constructs ----------------------------------------------
    def _w_FunctionDef(self, n: ast.FunctionDef, scope) -> None:
        inner = Scope(scope)
        self._bind_params(n.params, inner)
        if n.return_type is not None:
            self._w_TypeRef(n.return_type, scope)   # outer scope: refers to module-level types
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
        # Members (fields/methods) are visible while walking ctor-param defaults,
        # field defaults, and method bodies -- matching runtime `construct()`
        # semantics where field defaults see instance members.
        body_scope = Scope(scope)
        for mb in self.res.members_of(n.name):
            body_scope.define(mb.name, mb.id)
        for p in n.ctor_params:
            if p.type_ann is not None:
                self._w_TypeRef(p.type_ann, scope)
            if p.default is not None:
                self._walk(p.default, body_scope)
        for m in n.members:
            if isinstance(m, ast.Field):
                if m.type_ann is not None:
                    self._w_TypeRef(m.type_ann, scope)
                if m.default is not None:
                    self._walk(m.default, body_scope)
            else:  # method: `self` gets the enclosing type's members in scope
                inner = Scope(body_scope)
                self._bind_params(m.params, inner)
                self._walk(m.body, inner)

    def _w_ExternFn(self, n: ast.ExternFn, scope) -> None:
        for p in n.params:
            if p.type_ann is not None:
                self._w_TypeRef(p.type_ann, scope)
        if n.return_type is not None:
            self._w_TypeRef(n.return_type, scope)

    def _w_Block(self, n: ast.Block, scope) -> None:
        inner = Scope(scope)
        for stmt in n.stmts:
            if isinstance(stmt, ast.FunctionDef):
                span = self._name_span(stmt.span, stmt.name)
                bid = self._new_binding(stmt.name, "fn", span, stmt)
                inner.define(stmt.name, bid)
            self._walk(stmt, inner)

    def _w_LetBinding(self, n: ast.LetBinding, scope) -> None:
        self._walk(n.value, scope)
        if n.type_ann is not None:
            self._w_TypeRef(n.type_ann, scope)
        # Top-level let/mut already got a module-level binding in
        # _declare_top_level; reuse it (matched by AST-node identity) so that
        # uses, def, refs, and rename all share one binding id. Nested
        # (block-local) lets fall through and get a fresh binding.
        existing = next((bid for bid in self.res.module_binding_ids
                         if self.res.bindings[bid].node is n), None)
        if existing is not None:
            scope.define(n.name, existing)
            return
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





def _import_locals(stmt: ast.Import) -> list[str]:
    if stmt.names is not None:
        return [local for _imported, local in stmt.names]
    return [stmt.alias or stmt.namespace]


def resolve(module: ast.Module, source: Source) -> Resolution:
    return _Resolver(module, source).run()
