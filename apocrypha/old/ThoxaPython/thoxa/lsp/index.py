# thoxa/lsp/index.py
"""Bridges a resolver `Resolution` (+ optional checker types) to the position
queries the LSP handlers need, and renders signatures for hover."""
from __future__ import annotations

from typing import Optional

from .. import ast
from ..errors import Source, Span
from ..resolver import Binding, Reference, Resolution, resolve


class DocumentIndex:
    def __init__(self, resolution: Resolution, types: dict, text: str,
                 module: Optional[ast.Module] = None,
                 ctor_returns: Optional[dict] = None,
                 fn_returns: Optional[dict] = None) -> None:
        self.resolution = resolution
        self.types = types
        self.text = text
        self.module = module           # parsed AST (None if the buffer wouldn't parse)
        self.ctor_returns = ctor_returns or {}   # type name -> inferred Trial<Self,E>
        self.fn_returns = fn_returns or {}       # binding id -> inferred fn/method return

    def reference_at(self, line0: int, char0: int) -> Optional[Reference]:
        return self.resolution.ref_at(line0 + 1, char0 + 1)

    def binding_at(self, line0: int, char0: int) -> Optional[Binding]:
        # The resolver only records a `Reference` at *uses*, not at definition
        # names, so a lookup on a definition site (e.g. the `add` in
        # `fn add(...)`) finds nothing via reference_at. Fall back to scanning
        # bindings whose `def_span` covers this position.
        b = self.resolution.binding_of(self.reference_at(line0, char0))
        if b is not None:
            return b
        line1, col1 = line0 + 1, char0 + 1
        for cand in self.resolution.bindings.values():
            s = cand.def_span
            if s is not None and s.line == line1 and s.col <= col1 < s.col + max(1, s.length):
                return cand
        return None

    def type_of(self, binding: Optional[Binding]):
        if binding is None:
            return None
        return self.types.get(binding.id)


def build_index(text: str, path: str = "<input>") -> DocumentIndex:
    from ..checker import Checker
    from ..parser import parse
    source = Source(text, path)
    try:
        module = parse(text, path)
    except Exception:                      # mid-edit / unparseable buffer
        return DocumentIndex(Resolution(), {}, text)
    chk = Checker(source)
    try:
        chk.check_module(module)           # builds resolution + types, links members
        resolution = chk.resolution or resolve(module, source)
        types = chk.types
    except Exception:
        # analysis failed mid-edit: fall back to a resolver-only index
        try:
            resolution = resolve(module, source)
        except Exception:
            return DocumentIndex(Resolution(), {}, text, module)
        types = {}
    return DocumentIndex(resolution, types, text, module,
                         ctor_returns=getattr(chk, "ctor_returns", {}),
                         fn_returns=getattr(chk, "fn_returns", {}))


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


def _ctor_params_str(params, text, param_types=None) -> str:
    """Constructor params as written, defaults included (sliced from source).

    An unannotated param falls back to its inferred type (from `param_types`, a
    name -> type map) when one was recorded, so `host = "127.0.0.1"` renders as
    `host: String = "127.0.0.1"`."""
    from . import convert
    from ..checker import ty_str
    param_types = param_types or {}
    out = []
    for p in params:
        exp = "export " if getattr(p, "exported", False) else ""
        bang = "!" if p.positional else ""
        if p.type_ann is not None:
            ann = f": {p.type_ann}"
        elif param_types.get(p.name) is not None:
            ann = f": {ty_str(param_types[p.name])}"
        else:
            ann = ""
        default = ""
        if p.default is not None and text is not None:
            slice_ = convert.span_text(text, p.default.span)
            if slice_:
                default = f" = {slice_}"
        out.append(f"{exp}{p.name}{bang}{ann}{default}")
    return ", ".join(out)


def _render_generic(ty) -> str:
    """`("union", "Trial", (T, E))` -> `Trial<T, E>`."""
    from ..checker import ty_str
    return f"{ty[1]}<{', '.join(ty_str(a) for a in ty[2])}>"


def _ret_str(ty) -> str:
    """Render an inferred fn/method return: a `Trial<T,E>`/`May<T>` union with its
    generic args, anything else via the plain type string."""
    from ..checker import ty_str
    if isinstance(ty, tuple) and ty[0] == "union" and len(ty) > 2:
        return _render_generic(ty)
    return ty_str(ty)


def _literal_str(node) -> str:
    """Render a valued type's literal from the AST alone. Real hover threads the
    source and uses the exact slice via `span_text`; this is the no-text fallback."""
    if isinstance(node, ast.IntLit):
        return str(node.value)
    if isinstance(node, ast.FloatLit):
        return str(node.value)
    if isinstance(node, ast.BoolLit):
        return "true" if node.value else "false"
    if isinstance(node, ast.CharLit):
        return f"'{node.value}'"
    if isinstance(node, ast.ByteLit):
        v = node.value
        if 0x20 <= v < 0x7F and chr(v) not in "'\\":
            return f"b'{chr(v)}'"
        return f"b'\\x{v:02x}'"
    if isinstance(node, ast.StringLit):
        return '"' + "".join(t if k == "str" else "{…}" for k, t in node.parts) + '"'
    if isinstance(node, ast.UnaryOp):
        return f"{node.op}{_literal_str(node.operand)}"
    return "?"


def render_signature(binding: Binding, ty=None, text=None, param_types=None) -> str:
    n = binding.node
    k = binding.kind
    if k in ("fn", "method") and isinstance(n, ast.FunctionDef):
        if n.return_type is not None:
            ret = f" -> {n.return_type}"
        elif ty is not None:                 # inferred return, threaded for hover
            ret = f" -> {_ret_str(ty)}"
        else:
            ret = ""
        return f"fn {n.name}({_params_str(n.params)}){ret}"
    if k == "extern" and isinstance(n, ast.ExternFn):
        ret = f" -> {n.return_type}" if n.return_type is not None else ""
        return f"extern fn {n.name}({_params_str(n.params)}){ret}"
    if k == "type" and isinstance(n, ast.TypeDef):
        if n.value is not None:                  # valued nominal type: `type ReadOnly 0`
            from . import convert
            val = convert.span_text(text, n.value.span) if text is not None else None
            return f"type {n.name} {val or _literal_str(n.value)}"
        ret = ""
        if (isinstance(ty, tuple) and ty[0] == "union"
                and ty[1] in ("Trial", "May") and len(ty) > 2):
            ret = f" -> {_render_generic(ty)}"
        return f"type {n.name}({_ctor_params_str(n.ctor_params, text, param_types)}){ret}"
    if k == "union" and isinstance(n, ast.TypeAlias):
        return f"type {n.name} = {' | '.join(str(v) for v in n.variants)}"
    if k == "field" and isinstance(n, (ast.Field, ast.Param)):
        from ..checker import ty_str
        shown = str(n.type_ann) if n.type_ann is not None else (
            ty_str(ty) if ty is not None else "")
        return f"{n.name}: {shown}".rstrip(": ").rstrip()
    if k == "param" and isinstance(n, ast.Param):
        return f"{n.name}: {_type_str(n.type_ann)}"
    if k in ("let", "mut"):
        from ..checker import ty_str
        ann = getattr(n, "type_ann", None)
        shown = str(ann) if ann is not None else (ty_str(ty) if ty is not None else "")
        head = "mut" if k == "mut" else "let"
        return f"{head} {binding.name}: {shown}".rstrip(": ").rstrip()
    return f"{binding.name}"


def ctor_param_types(idx: "DocumentIndex", binding: Binding) -> dict:
    """Map each ctor-param name of a `type` binding to its recorded type (declared
    or inferred-from-default), for rendering the type's own signature. Empty for a
    non-type binding or when nothing was recorded (e.g. a mid-edit index)."""
    out: dict = {}
    n = binding.node
    if binding.kind != "type" or not isinstance(n, ast.TypeDef):
        return out
    for p in n.ctor_params:
        bid = idx.resolution.member_index.get((binding.name, p.name))
        if bid is not None:
            ty = idx.types.get(bid)
            if ty is not None:
                out[p.name] = ty
    return out


def voidlit_at(idx: DocumentIndex, line0: int, char0: int):
    """The span of a bare `Void` literal at this 0-based position, or None.

    `Void` parses to a `VoidLit`, not a name reference, so hover finds no binding
    for it; this lets the handler recognise the built-in unit value and describe
    it."""
    module = idx.module
    if module is None:
        return None
    line1, col1 = line0 + 1, char0 + 1
    for node in _walk_ast(module):
        if isinstance(node, ast.VoidLit):
            s = node.span
            if s is not None and s.line == line1 and s.col <= col1 < s.col + 4:
                return Span(s.line, s.col, 4)
    return None


def _walk_ast(node):
    """Yield every dataclass AST node in the tree rooted at `node`."""
    import dataclasses
    if isinstance(node, (list, tuple)):
        for it in node:
            yield from _walk_ast(it)
        return
    if not dataclasses.is_dataclass(node) or isinstance(node, type):
        return
    yield node
    for f in dataclasses.fields(node):
        yield from _walk_ast(getattr(node, f.name))
