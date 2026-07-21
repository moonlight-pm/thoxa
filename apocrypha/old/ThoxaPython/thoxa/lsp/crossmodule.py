# thoxa/lsp/crossmodule.py
"""Lazy, hover-time resolution of names that live in *another* module.

When hover / go-to-definition lands on an imported name (or a namespace-qualified
member, or a prelude type), this resolves the origin module, finds the exported
binding there, and renders its signature + doc pointed at that file. Everything is
best-effort: any failure returns ``None`` so the handlers fall back to today's
single-file behaviour.
"""
from __future__ import annotations

import dataclasses
import os
from dataclasses import dataclass
from typing import Optional

from .. import ast
from ..errors import Source, Span
from . import convert
from .index import render_signature


@dataclass
class Remote:
    signature: str
    doc: Optional[str]
    path: str              # absolute path of the defining module
    def_span: Optional[Span]   # name span in that module (1-based); None => no jump target


# Resolved target modules, cached per path (invalidated when the text changes).
# Repeated hovers on the same imported module — and every prelude-type hover,
# which all hit the fixed std/Prelude — then cost one dict lookup, not a reparse.
_RES_CACHE: dict = {}


def resolve_remote(idx, ref, binding, docs, current_path) -> Optional[Remote]:
    """Resolve the cursor's target in another module, or ``None`` to fall back.

    ``idx`` is the current document's :class:`DocumentIndex` (its resolution and
    parsed AST), ``ref`` / ``binding`` the local resolution at the cursor, ``docs``
    the server's ``uri -> text`` buffers (so an open target file's unsaved edits
    win over disk), and ``current_path`` the current document's filesystem path
    (for relative-import resolution).
    """
    try:
        return _resolve_remote(idx, ref, binding, docs, current_path)
    except Exception:          # never let a cross-module probe break the handler
        return None


# ---------------------------------------------------------------- internals
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


def _origin(binding, ref, idx, docs, current_path):
    """Where does the cursor point?

    - ``("member", spec, imported_name)`` — an imported definition (a selective
      import's local name, or a ``ns.member`` off a namespace import), or a
      prelude type used bare.
    - ``("module", spec)`` — a namespace name itself (→ a module summary).
    - ``("typed_member", spec, type_name, member)`` — a member on a value whose
      inferred type is a type imported from ``spec``.
    - ``None`` — not a cross-module reference; the handler falls back to local.
    """
    resolution, module = idx.resolution, idx.module
    # (1) a use of an imported name (kind == "import")
    if (binding is not None and binding.kind == "import"
            and isinstance(binding.node, ast.Import)):
        node = binding.node
        if node.names is not None:
            # selective import: recover the *imported* name behind any `as` alias
            for imported, local in node.names:
                if local == binding.name:
                    return ("member", node.module, imported)
            return None
        # namespace import: the cursor is on the namespace name itself
        return ("module", node.module)
    # (2) a member `recv.member`. Members are never linked to a local binding
    #     when the receiver isn't a locally-known named type, so `binding is None`
    #     skips this walk for ordinary typed-member hovers like `s.count`.
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
            # (2b) receiver has an inferred type imported from another module
            if recv is not None:
                rty = idx.types.get(recv.id)
                if isinstance(rty, tuple) and rty[0] == "named":
                    spec = _spec_exporting_type(module, rty[1], docs, current_path)
                    if spec is not None:
                        return ("typed_member", spec, rty[1], fa.name)
    # (3) a prelude name used bare (kind == "builtin"): resolve into std/Prelude.
    #     This now covers `print`/`println` too — they are real prelude functions,
    #     so hover finds and renders their signatures like any other prelude name.
    if binding is not None and binding.kind == "builtin":
        return ("member", "std/Prelude", binding.name)
    return None


def _spec_exporting_type(module, type_name, docs, current_path):
    """The import specifier of the module that exports a type named ``type_name``.

    Scans this file's imports, resolving and parsing each target only until one is
    found to export a matching top-level type; ``None`` if none does."""
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
    """A member on a value of an imported type → the member's signature/doc/jump
    inside the type's defining module."""
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


def _lookup_member(spec, imported, docs, current_path) -> Optional[Remote]:
    path = _resolve_spec(spec, current_path)
    if path is None:
        return None
    text = _module_text(path, docs)
    if text is None:
        return None
    res = _resolve_module(path, text)
    b = _exported_binding(res, imported)
    if b is None:
        return None
    doc = convert.doc_above(text, b.def_span.line) if b.def_span is not None else None
    return Remote(render_signature(b), doc, path, b.def_span)


def _resolve_spec(spec, current_path) -> Optional[str]:
    """Map an import specifier to the absolute path of a Thoxa source file."""
    from ..loader import Loader
    importer_dir = (os.path.dirname(current_path)
                    if current_path and current_path.startswith("/") else None)
    resolved = Loader(project_root=None).resolve(spec, importer_dir, Span(1, 1))
    if resolved.kind != "file":     # the built-in `ffi` root has no Thoxa source
        return None
    return resolved.path


def _module_text(path, docs) -> Optional[str]:
    """The target module's text — an open buffer (unsaved edits win) or disk.

    Buffers are matched by resolved path, not raw URI string, so a client that
    percent-encodes the URI differently than we would still hits its open buffer."""
    if docs:
        for uri, buffered in docs.items():
            if convert.uri_to_path(uri) == path:
                return buffered
    try:
        with open(path, encoding="utf-8") as f:
            return f.read()
    except (OSError, ValueError):   # ValueError: embedded NUL in the path
        return None


def _resolve_module(path, text):
    hit = _RES_CACHE.get(path)
    if hit is not None and hit[0] == text:
        return hit[1]
    from ..parser import parse
    from ..resolver import resolve
    res = resolve(parse(text, path), Source(text, path))
    _RES_CACHE[path] = (text, res)
    return res


def _exported_binding(res, name):
    """The exported top-level definition (with a real name span) of ``name``.

    ``owner is None`` restricts to module-level defs: an imported / namespace /
    prelude name always names a top-level export, never a type's field or method,
    so a same-named field can never shadow the intended definition."""
    for b in res.bindings.values():
        if (b.name == name and b.exported and b.owner is None
                and b.def_span is not None):
            return b
    return None


def _field_access_for(ref, module, resolution):
    """The `FieldAccess` node whose pending member reference *is* ``ref``.

    Member refs are recorded by node identity in ``member_refs``; walk the AST to
    recover the node so we can inspect its receiver (`ns` in `ns.member`)."""
    for node in _walk(module):
        if (isinstance(node, ast.FieldAccess)
                and resolution.member_refs.get(id(node)) is ref):
            return node
    return None


def _walk(node):
    """Yield every dataclass AST node in the tree rooted at ``node``."""
    if isinstance(node, (list, tuple)):
        for it in node:
            yield from _walk(it)
        return
    if not dataclasses.is_dataclass(node) or isinstance(node, type):
        return
    yield node
    for f in dataclasses.fields(node):
        yield from _walk(getattr(node, f.name))


def _lookup_module(spec, docs, current_path) -> Optional[Remote]:
    """A namespace name → a `module <spec>` summary with the target's lead doc."""
    path = _resolve_spec(spec, current_path)
    if path is None:
        return None
    text = _module_text(path, docs)
    if text is None:
        return None
    return Remote(f"module {spec}", _module_doc(text), path, Span(1, 1, 1))


def _module_doc(text) -> Optional[str]:
    """The contiguous ``##`` doc block at the very top of the file, if any."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("##"):
            out.append(s[2:].strip())
        else:
            break
    return "\n".join(out) if out else None
