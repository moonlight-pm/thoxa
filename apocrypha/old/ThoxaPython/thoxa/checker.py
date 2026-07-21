"""A conservative, local static checker for Thoxa.

Design goal: **no false positives on valid programs.** Anything the checker cannot
resolve becomes ``UNKNOWN``, which is compatible with everything, so the checker only
ever reports errors it can *prove*. It catches the high-value mistakes:

* undefined value names,
* non-Bool `if`/`while` conditions and non-numeric arithmetic operands (when types are known),
* member access on a known concrete type that has no such (exported) member,
* non-exhaustive `match` over a known union (the M4 headline: static exhaustiveness),
* string `+` (Thoxa concatenates via interpolation).

It is intentionally incomplete: full inference (generics, let-polymorphism) is future work.
The interpreter remains the source of truth for runtime behaviour; `thoxa check` is advisory.
"""

from __future__ import annotations

import dataclasses
import os

from . import ast
from . import ffi
from .errors import Diagnostic, Source, Span
from .loader import (
    Loader,
    LoaderError,
    is_pascal_case,
    module_name,
    pascal_module_error,
)

UNKNOWN = "Unknown"
_INT_TYPES = {
    "Int8", "Int16", "Int32", "Int64",
    "UInt8", "UInt16", "UInt32", "UInt64", "Byte",
}
_FLOAT_TYPES = {"Float32", "Float64"}
# print/println are now ordinary prelude functions (over a libc `write` extern),
# collected into `funcs` like any other declaration; no ambient builtins remain.
_BUILTINS: frozenset[str] = frozenset()

# Type-annotation names that name a built-in yet resolve to UNKNOWN, so they must
# NOT be synthesized into a ("named", X) field type for cross-module hover.
_NON_USER_TYPE_NAMES = frozenset({"Any", "Type", "Fn"})
# Variant constructors — a fallible body whose success tail is a bare `Pass`/`Has`
# (or error `Fail`/`Not`) construction carries no recoverable payload type, so the
# inferred Trial/May return is dropped rather than rendered as `Trial<Pass>`.
_VARIANT_CTOR_NAMES = frozenset({"Pass", "Fail", "Has", "Not"})

# The checker treats all integer types as one numeric class and all float types
# as another; every sized annotation resolves to one of these two tags. `Int`
# and `Float` themselves are rejected (see `_resolve`) — no ambiguous widths.
_INT_TAG = "Int64"
_FLOAT_TAG = "Float64"


def is_numeric(ty) -> bool:
    return ty in _INT_TYPES or ty in _FLOAT_TYPES


def ty_str(ty) -> str:
    if isinstance(ty, tuple):
        if ty[0] == "array":
            return f"Array<{ty_str(ty[1])}>"
        if ty[0] in ("named", "union", "typeval"):
            return ty[1]
        if ty[0] == "fn":
            return "function"
    return str(ty)


def _trial_payload(ty):
    """The success payload of a `?` operand: Trial<T,E>/May<T> -> T; else `ty`.

    A constructor call already models its success type as ("named", X), so the
    passthrough is correct there too — only a real Trial/May wrapper is unwrapped."""
    if isinstance(ty, tuple) and ty[0] == "union" and len(ty) > 2 and ty[2]:
        if ty[1] in ("Trial", "May"):
            return ty[2][0]
    return ty


def _is_error_variant(ty) -> bool:
    """True for a `Fail(...)`/`Not` construction's inferred type — the error or
    absence channel of a fallible body, skipped when computing a success type."""
    return isinstance(ty, tuple) and ty[0] == "named" and ty[1] in ("Fail", "Not")


class Checker:
    def __init__(self, source: Source) -> None:
        self.src = source
        self.errors: list[Diagnostic] = []
        self.type_members: dict[str, dict[str, object]] = {}
        self.type_exported: dict[str, set[str]] = {}
        self.union_variants: dict[str, list[str]] = {}
        self.type_kind: dict[str, str] = {}  # name -> 'type' | 'union'
        self.abi_structs: set[str] = set()
        self.funcs: dict[str, object] = {}
        # Top-level `let`/`mut` values are module-global: sibling functions and
        # type methods close over them at runtime, so the checker must see them
        # in scope too (parity with the interpreter; cf. modstate conformance).
        self.global_values: dict[str, object] = {}
        self.imported: dict[str, object] = {}
        # Namespace imports: local binding name -> (declared, exports, spec).
        self.namespaces: dict[str, tuple] = {}
        # Shared name resolution (built in `check_module`) plus inferred types
        # keyed by binding id. `_infer_Name` consults these before the env; while
        # `self.types` is empty this branch never fires (behavior-preserving).
        self.resolution = None
        self.types: dict[int, object] = {}
        # Set while the pre-pass quietly learns types (see `_prepass`); suppresses
        # `err` so learning types never emits a diagnostic the real pass wouldn't.
        self._quiet = False
        # Fallible types' inferred ctor returns (Trial<Self, E>), keyed by type
        # name, for hover only — deliberately NOT in `self.types` (that feeds
        # `_infer_Name`, which must keep seeing a type as a constructor).
        self.ctor_returns: dict[str, tuple] = {}
        # Unannotated fn/method returns as hover should render them (fallible
        # bodies wrapped Trial<T,E>/May<T>, like the interpreter), keyed by
        # binding id — hover only, deliberately NOT in `self.types`/`funcs`, so
        # inference keeps seeing the raw body type and diagnostics never move.
        self.fn_returns: dict[int, object] = {}
        # Namespace-imported types' member tables (type name -> members) and their
        # exported-member sets, indexed for the quiet return-inference pre-pass so a
        # value of an imported type can resolve its members cross-module. Never
        # consulted by the error-emitting pass (see `_member`), so cross-module
        # member access never produces a "has no member" diagnostic.
        self.ns_type_members: dict[str, dict] = {}
        self.ns_type_exported: dict[str, set] = {}
        # Cross-file resolution: derive the project root from the entry's path,
        # mirroring the interpreter (a `<...>` filename is in-memory → no root).
        fn = source.filename
        if fn and not fn.startswith("<"):
            self._entry_path = os.path.abspath(fn)
            self._entry_dir = os.path.dirname(self._entry_path)
            self._project_root = self._entry_dir
        else:
            self._entry_path = None
            self._entry_dir = None
            self._project_root = None
        self._loader = Loader(self._project_root)  # names brought in via `import`

    # -- reporting --------------------------------------------------------
    def err(self, msg: str, span: Span | None, notes=None) -> None:
        if self._quiet:                 # pre-pass is learning types, not reporting
            return
        span = span or Span(1, 1)
        # A few annotations (notably function return types) are resolved on more
        # than one pass; collapse identical diagnostics so the user sees one.
        if any(d.message == msg and d.span == span for d in self.errors):
            return
        self.errors.append(Diagnostic(msg, span, self.src, notes or []))

    # -- entry ------------------------------------------------------------
    def check_module(self, module: ast.Module) -> list[Diagnostic]:
        from .loader import parse_prelude
        from .resolver import resolve as _resolve_names

        prelude, _ = parse_prelude()
        self._collect(prelude.body)
        self._collect(module.body)

        # Build the shared name resolution over the SAME module AST the checker
        # walks. The resolver's cross-file `_collect` can raise on some modules;
        # a failure must leave `self.resolution = None` so `_infer_Name` stays on
        # the pure env path and checking proceeds exactly as before.
        try:
            self.resolution = _resolve_names(module, self.src)
        except Exception:
            self.resolution = None

        genv = self._global_env()
        self._prepass(module, genv)
        for stmt in prelude.body:
            self._check_decl_bodies(stmt, genv)
        for stmt in module.body:
            self._check_stmt(stmt, dict(genv))
        return self.errors

    def _collect(self, body) -> None:
        for stmt in body:
            if isinstance(stmt, ast.TypeDef) and ffi.abi_struct_specs(stmt) is not None:
                self.abi_structs.add(stmt.name)
        for stmt in body:
            if isinstance(stmt, ast.TypeDef):
                self.type_kind[stmt.name] = "type"
                members: dict[str, object] = {}
                exported: set[str] = set()
                for p in stmt.ctor_params:
                    members[p.name] = self._resolve(p.type_ann)
                    if p.exported:
                        exported.add(p.name)
                for m in stmt.members:
                    if isinstance(m, ast.Field):
                        members[m.name] = self._resolve(m.type_ann)
                        if m.exported:
                            exported.add(m.name)
                    else:  # method
                        members[m.name] = ("fn", _FnInfo(m.params, self._resolve(m.return_type)))
                        if m.exported:
                            exported.add(m.name)
                self.type_members[stmt.name] = members
                self.type_exported[stmt.name] = exported
            elif isinstance(stmt, ast.TypeAlias):
                self.type_kind[stmt.name] = "union"
                self.union_variants[stmt.name] = [v.name for v in stmt.variants]
            elif isinstance(stmt, ast.FunctionDef):
                self.funcs[stmt.name] = _FnInfo(stmt.params, self._resolve(stmt.return_type))
            elif isinstance(stmt, ast.ExternFn):
                self._check_extern(stmt)
                self.funcs[stmt.name] = _FnInfo(stmt.params, self._resolve(stmt.return_type))
            elif isinstance(stmt, ast.LetBinding):
                # Register the name (with its annotated type, else UNKNOWN) so it
                # resolves inside sibling function/method bodies regardless of
                # textual order — the value is re-inferred in _check_stmt.
                self.global_values[stmt.name] = (
                    self._resolve(stmt.type_ann) if stmt.type_ann else UNKNOWN
                )
            elif isinstance(stmt, ast.Import):
                self._collect_import(stmt)

    def _check_extern(self, node: ast.ExternFn) -> None:
        if node.library not in ffi.SUPPORTED_LIBRARIES:
            self.err(
                f'unsupported FFI library "{node.library}"; only "c" is available in v1',
                node.span,
            )
        for p in node.params:
            if not p.positional:
                self.err(f"extern parameter '{p.name}' must be positional (add '!')", p.span)
            if p.variadic:
                self.err(
                    f"extern parameter '{p.name}' cannot be variadic (C varargs are not supported)",
                    p.span,
                )
            if p.default is not None:
                self.err(f"extern parameter '{p.name}' cannot have a default value", p.span)
            self._check_marshalable(p.type_ann, p.name, node.span, is_return=False)
        self._check_marshalable(node.return_type, None, node.span, is_return=True)

    def _check_marshalable(self, ann, pname, span, *, is_return: bool) -> None:
        allowed = ffi.MARSHALABLE_RETURNS if is_return else ffi.MARSHALABLE_PARAMS
        if ann is None:
            if not is_return:  # an omitted return type means Void, which is fine
                self.err(f"extern parameter '{pname}' needs a type", span)
            return
        if ann.name == "Address":
            if is_return:
                self.err(
                    "a C function cannot return an Address<T> (pointer returns are deferred)",
                    ann.span or span,
                )
                return
            if len(ann.args) != 1:
                self.err("Address<T> takes exactly one type argument", ann.span or span)
                return
            pointee = ann.args[0].name
            if pointee not in self.abi_structs:
                self.err(
                    f"Address<{pointee}>: '{pointee}' is not a C-ABI struct "
                    "(needs only fixed-width scalar or Array<scalar, N> fields, no methods)",
                    ann.span or span,
                )
            return
        if ann.name in allowed:
            return
        kind = "return type" if is_return else "parameter type"
        self.err(
            f"'{ann.name}' cannot cross the C FFI as a {kind}{self._marshal_hint(ann.name)}",
            ann.span or span,
        )

    @staticmethod
    def _marshal_hint(name: str) -> str:
        if name in ("Int", "Float"):
            return "; name a concrete width like Int64"
        if name == "Bool":
            return "; pass an Int32 or UInt8 and compare"
        if name == "Char":
            return "; use Byte or Bytes for C character data"
        if name == "String":
            return "; C cannot return a String (pointer return) — use a scalar"
        return ""

    def _collect_import(self, node: ast.Import) -> None:
        try:
            resolved = self._loader.resolve(node.module, self._entry_dir, node.span)
        except LoaderError as exc:
            self.err(exc.message, exc.span, notes=exc.notes)
            return

        if resolved.kind == "builtin":  # the built-in `ffi` root
            self._collect_ffi(node)
            return

        # A module importing itself is legal (a `fn`/`type` self-import): the
        # imported names are the entry's own top-level declarations, already in
        # scope — nothing to register, and the interpreter accepts it too.
        if resolved.key == self._entry_path:
            return

        # A file module describes a type, so its name is PascalCase (design §2.1).
        # The entry above is exempt (it is a program, not an importable module).
        if not is_pascal_case(module_name(node.module)):
            self.err(pascal_module_error(node.module), node.span)
            return

        try:
            module, source = self._loader.module_ast(resolved, node.module, node.span)
        except LoaderError as exc:
            self.err(exc.message, exc.span, notes=exc.notes)
            return

        # A dependency's top level is declarations only (design §6). Locate the
        # diagnostic at the import statement, since the checker renders its
        # diagnostics against the entry source, not the imported file.
        for stmt in module.body:
            if not isinstance(stmt, (ast.LetBinding, ast.FunctionDef, ast.ExternFn,
                                     ast.TypeDef, ast.TypeAlias, ast.Import)):
                self.err(
                    f"imported module '{node.module}' has a non-declaration at its "
                    "top level (a module's top level is declarations only)",
                    node.span,
                )
                break

        declared, exports = self._module_exports(module, source)

        if node.names is None:
            # Namespace import binds a singleton instance under the freely-chosen
            # camelCase name (the parser enforces the casing); there is no
            # canonical-name constraint on the binding.
            self.namespaces[node.namespace] = (declared, exports, node.module)
            # Index the module's exported *types* by name so a value of such a type
            # (e.g. a `Socket` obtained via `net.createSocket()`) can resolve its
            # members during the quiet return-inference pre-pass (see `_member`).
            for ename, info in exports.items():
                if info[0] == "type":
                    self.ns_type_members.setdefault(ename, info[1])
                    self.ns_type_exported.setdefault(ename, info[2])
            return

        for imported, local in node.names:
            if imported not in declared:
                self.err(f"'{imported}' is not defined in '{node.module}'", node.span)
                continue
            if imported not in exports:
                self.err(f"'{imported}' is not exported by '{node.module}'", node.span)
                continue
            self._register_export(local, exports[imported])

    def _collect_ffi(self, node: ast.Import) -> None:
        if node.names is None:
            self.err(
                'import from "ffi" must name its members, '
                'e.g. import { errno, strerror } from "ffi"',
                node.span,
            )
            return
        for imported, local in node.names:
            if imported not in ffi.FFI_EXPORTS:
                self.err(f'"ffi" has no export \'{imported}\'', node.span)
                continue
            self.imported[local] = UNKNOWN

    def _module_exports(self, module: ast.Module, source: Source):
        """A target module's (declared-names, exported-name→info) tables.

        A throwaway sub-checker resolves the target's *own* declarations; its
        imports are skipped, so a cycle can never recurse back into us. The
        prelude is collected first so an exported signature's `Trial<T, E>` /
        `May<T>` return resolves to a union rep carrying its payload (else it
        would fall to UNKNOWN and the caller's `?` could not recover the type).
        """
        from .loader import parse_prelude
        sub = Checker(source)
        try:
            prelude, _ = parse_prelude()
            sub._collect(prelude.body)
        except Exception:
            pass
        sub._collect([s for s in module.body if isinstance(
            s, (ast.TypeDef, ast.TypeAlias, ast.FunctionDef, ast.ExternFn))])
        declared: set[str] = set()
        exports: dict[str, tuple] = {}
        for stmt in module.body:
            if isinstance(stmt, (ast.FunctionDef, ast.ExternFn)):
                declared.add(stmt.name)
                if stmt.exported:
                    exports[stmt.name] = ("fn", sub.funcs.get(stmt.name))
            elif isinstance(stmt, ast.TypeDef):
                declared.add(stmt.name)
                if stmt.exported:
                    exports[stmt.name] = ("type", sub.type_members.get(stmt.name, {}),
                                          sub.type_exported.get(stmt.name, set()))
            elif isinstance(stmt, ast.TypeAlias):
                declared.add(stmt.name)
                if stmt.exported:
                    exports[stmt.name] = ("union", sub.union_variants.get(stmt.name, []))
            elif isinstance(stmt, ast.LetBinding):
                declared.add(stmt.name)
                if stmt.exported and not stmt.mutable:
                    exports[stmt.name] = ("value",)
        return declared, exports

    def _register_export(self, name: str, info: tuple) -> None:
        kind = info[0]
        if kind == "fn":
            self.funcs[name] = info[1] if info[1] is not None else _FnInfo([], UNKNOWN)
        elif kind == "type":
            self.type_kind[name] = "type"
            self.type_members[name] = info[1]
            self.type_exported[name] = info[2]
        elif kind == "union":
            self.type_kind[name] = "union"
            self.union_variants[name] = info[1]
        else:  # value
            self.imported[name] = UNKNOWN

    def _global_env(self) -> dict:
        env: dict = {}
        for name, info in self.funcs.items():
            env[name] = ("fn", info)
        for name, ty in self.global_values.items():
            env[name] = ty
        for name in self.type_kind:
            env[name] = ("typeval", name)
        for name in _BUILTINS:
            env[name] = UNKNOWN
        for name in self.imported:
            env[name] = UNKNOWN
        for name, info in self.namespaces.items():
            env[name] = ("namespace",) + info  # ("namespace", declared, exports, spec)
        return env

    # -- type resolution --------------------------------------------------
    def _resolve(self, ann: ast.TypeRef | None):
        if ann is None:
            return UNKNOWN
        name = ann.name
        if name in ("Int", "Float"):
            suggested = _INT_TAG if name == "Int" else _FLOAT_TAG
            self.err(
                f"'{name}' is not a valid type; use a sized type like {suggested}",
                ann.span,
            )
            return suggested
        if name in _INT_TYPES:
            return _INT_TAG
        if name in _FLOAT_TYPES:
            return _FLOAT_TAG
        if name in ("Bool", "Char", "String", "Void", "Bytes", "Buffer"):
            return name
        if name == "Array":
            return ("array", self._resolve(ann.args[0]) if ann.args else UNKNOWN)
        if name == "Fn":
            for a in ann.args:
                self._resolve(a)
            if ann.ret is not None:
                self._resolve(ann.ret)
            return UNKNOWN
        if name in ("Any", "Type"):
            return UNKNOWN
        # Generic type arguments (May<T>, Trial<T, E>, user generics, or an unknown
        # name) are resolved both for their side effects — a nested invalid type,
        # a bare Int inside Trial<Int, String>, is still reported — and to retain
        # the payload reps on the union, so `?` can unwrap and hover can display.
        resolved_args = tuple(self._resolve(a) for a in ann.args)
        if self.type_kind.get(name) == "union":
            return ("union", name, resolved_args)
        if name in self.type_kind:
            return ("named", name)
        return UNKNOWN  # generic type parameter or unknown — stay conservative  # generic type parameter or unknown — stay conservative  # generic type parameter or unknown — stay conservative  # generic type parameter or unknown — stay conservative

    # -- statement checking ----------------------------------------------
    def _check_decl_bodies(self, stmt, genv: dict) -> None:
        if isinstance(stmt, ast.FunctionDef):
            self._check_function(stmt, genv, instance_type=None)
        elif isinstance(stmt, ast.TypeDef):
            self._check_type(stmt, genv)

    def _record_type(self, node_span, name, ty) -> None:
        # Key an inferred type by binding id, for later LSP consumers (hover).
        # We can't match by exact `(line, col)` against `def_span`: the def
        # site's node span (e.g. a `LetBinding`'s span is the `let` keyword)
        # does not equal the resolver's binding `def_span` (the name's own
        # span). Instead, replicate the resolver's `_name_span` search: find
        # the binding with this `name` whose `def_span` is the earliest
        # name-position at/after the node span.
        if self.resolution is None:
            return
        key = (node_span.line, node_span.col)
        best = None
        for cand in self.resolution.bindings.values():
            s = cand.def_span
            if s is not None and cand.name == name and (s.line, s.col) >= key:
                if best is None or (s.line, s.col) < (best.def_span.line, best.def_span.col):
                    best = cand
        # Do NOT record a type for a module-global (`let`/`mut`) binding.
        # Module-global values are UNKNOWN across functions by the checker's
        # design (a preserved known gap); recording their concrete type would
        # make it visible to sibling functions and change diagnostics. Local
        # let/param/for-var/pattern bindings are not module-level, so they are
        # still recorded.
        if best is not None and best.id not in self.resolution.module_binding_ids:
            self.types[best.id] = ty

    def _check_stmt(self, stmt, env: dict):
        if isinstance(stmt, ast.LetBinding):
            vty = self._infer(stmt.value, env)
            env[stmt.name] = self._resolve(stmt.type_ann) if stmt.type_ann else vty
            self._record_type(stmt.span, stmt.name, env[stmt.name])
            return "Void"
        if isinstance(stmt, ast.Assign):
            self._infer(stmt.value, env)
            self._infer(stmt.target, env)
            return "Void"
        if isinstance(stmt, ast.FunctionDef):
            env[stmt.name] = ("fn", _FnInfo(stmt.params, self._resolve(stmt.return_type)))
            self._check_function(stmt, env, instance_type=None)
            return "Void"
        if isinstance(stmt, (ast.TypeDef, ast.TypeAlias)):
            return "Void"
        if isinstance(stmt, ast.Return):
            if stmt.value is not None:
                self._infer(stmt.value, env)
            return "Void"
        if isinstance(stmt, (ast.Break, ast.Continue, ast.Import, ast.ExternFn)):
            return "Void"
        return self._infer(stmt, env)

    def _check_function(self, fn: ast.FunctionDef, genv: dict, instance_type) -> None:
        env = dict(genv)
        if instance_type is not None:
            for mname, mty in self.type_members.get(instance_type, {}).items():
                env.setdefault(mname, mty if not (isinstance(mty, tuple) and mty[0] == "fn") else mty)
        for p in fn.params:
            ty = self._resolve(p.type_ann)
            if p.variadic:
                ty = ("array", ty)
            env[p.name] = ty
            self._record_type(p.span, p.name, ty)
            # A parameter default is an expression too — check it (e.g. an
            # undefined name in a default was silently accepted before).
            if p.default is not None:
                self._infer(p.default, env)
        self._infer(fn.body, env)

    def _check_type(self, td: ast.TypeDef, genv: dict) -> None:
        for m in td.members:
            if isinstance(m, ast.FunctionDef):
                self._check_function(m, genv, instance_type=td.name)

    # -- quiet pre-pass: learn types the real pass needs up front ---------
    def _prepass(self, module: ast.Module, genv: dict) -> None:
        """Quietly (no diagnostics) learn types the normal error-emitting pass
        needs before it runs: declared field types (ctor params) first, then
        field-initializer types (so method-envs see them), then unannotated
        fn/method returns, then fallible ctors' inferred Trial<Self, E> returns,
        then unannotated fn/method returns as hover renders them (Trial-wrapped).
        The real pass re-walks every body."""
        self._quiet = True
        try:
            self._record_ctor_field_types(module, genv)
            self._infer_field_inits(module, genv)
            self._infer_returns(module, genv)
            self._infer_ctor_returns(module, genv)
            self._infer_fn_returns(module, genv)
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

    def _infer_field_inits(self, module: ast.Module, genv: dict) -> None:
        """Record the inferred type of each unannotated field initializer, both
        into `type_members` (so method-envs and members see it) and via
        `_record_type` (so hover shows it). Quiet: no field-init diagnostics."""
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

    def _binding_for_node(self, node):
        """The resolver binding whose def node *is* `node` (by identity)."""
        if self.resolution is None:
            return None
        for b in self.resolution.bindings.values():
            if b.node is node:
                return b
        return None

    def _declared_field_type(self, ann):
        """The type to surface (LSP) for a field/ctor-param annotation. Prefer
        the checker's own resolution; if that is UNKNOWN but the annotation is a
        bare user-type name only reachable through a namespace import (e.g.
        `socket!: Socket`, with `Socket` behind `import net`), synthesize
        ("named", name) so cross-module member resolution can find its module."""
        ty = self._resolve(ann)
        if ty is UNKNOWN and ann is not None and not ann.args and ann.ret is None:
            name = ann.name
            if name and name[0].isupper() and name not in _NON_USER_TYPE_NAMES:
                return ("named", name)
        return ty

    def _record_ctor_field_types(self, module: ast.Module, genv: dict) -> None:
        """Record each ctor-param field's type into `self.types`, so hover shows
        `socket: Socket` and a member on it (`socket.receive`) resolves into the
        type's defining module. An annotated param uses its declared type; an
        unannotated one with a default takes the default's inferred type (so
        `host = "127.0.0.1"` hovers as `host: String`). Hover-facing only: a field
        name never occurs in a top-level function's scope and type-method bodies
        aren't error-checked, so `_infer_Name` reading this changes no diagnostic."""
        for stmt in module.body:
            if not isinstance(stmt, ast.TypeDef):
                continue
            menv = self._type_member_env(stmt, genv)
            for p in stmt.ctor_params:
                if p.type_ann is not None:
                    ty = self._declared_field_type(p.type_ann)
                elif p.default is not None:
                    ty = self._infer(p.default, dict(menv))
                else:
                    continue
                if ty is not UNKNOWN:
                    self._record_type(p.span, p.name, ty)

    def _infer_ctor_returns(self, module: ast.Module, genv: dict) -> None:
        """For each fallible type (its ctor body uses `?`), record the ctor's
        inferred return `Trial<Self, E>` for hover. `E` is the unification of the
        error types flowing out of the body — each `?`-operand's `Trial<_, E>` and
        each `Fail(x)` payload. A `Trial` always shows BOTH args: if `E` can't be
        determined (the error types disagree or are unknown) the return is left
        undeclared rather than rendered as a partial `Trial<Self>`. Kept in
        `ctor_returns` (not `self.types`), so a type is still a constructor."""
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
            if not (errs and all(x == errs[0] for x in errs) and errs[0] != UNKNOWN):
                continue    # E undeterminable — never a one-arg Trial<Self>.
            self.ctor_returns[stmt.name] = (
                "union", "Trial", (("named", stmt.name), errs[0]))

    def _infer_fn_returns(self, module: ast.Module, genv: dict) -> None:
        """Record each unannotated fn/method's return type as hover renders it,
        keyed by binding id. A fallible body (uses `?` or constructs a variant)
        is Trial<T,E>/May<T>-returning, exactly as the interpreter threads it: T
        is the body's success type and E the unification of its error types. A
        `Trial` always shows BOTH args, so when E can't be determined the return
        is dropped (left undeclared) rather than rendered as a partial `Trial<T>`;
        `May<T>` legitimately carries one. The whole return is also dropped when T
        is unknown or a bare variant construction, so hover never shows
        `Trial<Unknown>`/`Trial<Pass>`. Kept in `fn_returns` alone — `funcs`/
        `self.types` keep the raw type the checker's own inference depends on."""
        from .interp import _fn_trial_family

        def record(fn: ast.FunctionDef, env: dict) -> None:
            if fn.return_type is not None:
                return
            b = self._binding_for_node(fn)
            if b is None:
                return
            body_ty = self._infer(fn.body, dict(env))
            family = _fn_trial_family(fn)
            if family is None:
                if body_ty is not UNKNOWN:
                    self.fn_returns[b.id] = body_ty
                return
            payload = _trial_payload(body_ty)
            if payload is UNKNOWN or (isinstance(payload, tuple)
                    and payload[0] == "named" and payload[1] in _VARIANT_CTOR_NAMES):
                return
            if family == "may":
                self.fn_returns[b.id] = ("union", "May", (payload,))
                return
            errs: list = []
            self._collect_error_types(fn.body, dict(env), errs)
            if not (errs and all(x == errs[0] for x in errs) and errs[0] != UNKNOWN):
                return    # E undeterminable — never a one-arg Trial<T>.
            self.fn_returns[b.id] = ("union", "Trial", (payload, errs[0]))

        for stmt in module.body:
            if isinstance(stmt, ast.FunctionDef):
                record(stmt, genv)
            elif isinstance(stmt, ast.TypeDef):
                menv = self._type_member_env(stmt, genv)
                for m in stmt.members:
                    if isinstance(m, ast.FunctionDef):
                        record(m, menv)

    def _collect_error_types(self, node, env: dict, out: list) -> None:
        """Error types flowing out of a ctor body: each `?`-operand's Trial<_, E>
        and each `Fail(x)` payload. Does not descend into nested function defs."""
        if isinstance(node, ast.Try):
            ty = self._infer(node.operand, env)
            if (isinstance(ty, tuple) and ty[0] == "union" and ty[1] == "Trial"
                    and len(ty) > 2 and len(ty[2]) > 1):
                out.append(ty[2][1])
        if (isinstance(node, ast.Call) and isinstance(node.callee, ast.TypeValue)
                and node.callee.name == "Fail"):
            # `Fail(x)` contributes `x`'s type; a bare `Fail()` carries the prelude
            # default `error!: E = Void`, so its error type is `Void`.
            out.append(self._infer(node.args[0].value, env) if node.args else "Void")
        if isinstance(node, ast.FunctionDef):
            return
        if isinstance(node, (list, tuple)):
            for it in node:
                self._collect_error_types(it, env, out)
        elif dataclasses.is_dataclass(node) and not isinstance(node, type):
            for f in dataclasses.fields(node):
                self._collect_error_types(getattr(node, f.name), env, out)

    # -- expression inference --------------------------------------------
    def _infer(self, node, env: dict):
        m = getattr(self, "_infer_" + type(node).__name__, None)
        if m is None:
            return UNKNOWN
        return m(node, env)

    def _infer_IntLit(self, n, env):
        return _INT_TAG

    def _infer_FloatLit(self, n, env):
        return _FLOAT_TAG

    def _infer_BoolLit(self, n, env):
        return "Bool"

    def _infer_CharLit(self, n, env):
        return "Char"

    def _infer_ByteLit(self, n, env):
        return _INT_TAG  # a byte literal is an integer literal in character form

    def _infer_StringLit(self, n, env):
        for part in n.parts:
            if part[0] == "expr":
                self._infer(part[1], env)
        return "String"

    def _infer_VoidLit(self, n, env):
        return "Void"

    def _infer_Name(self, n: ast.Name, env):
        if self.resolution is not None:
            ref = self.resolution.ref_at(n.span.line, n.span.col)
            b = self.resolution.binding_of(ref)
            if b is not None and b.id in self.types:
                return self.types[b.id]
        if n.ident in env:
            val = env[n.ident]
            if isinstance(val, tuple) and val and val[0] == "namespace":
                # Reaching a namespace here means it is used as a value; a field
                # access resolves its base before calling `_infer`.
                self.err("a module namespace cannot be used as a value", n.span)
                return UNKNOWN
            return val
        self.err(f"'{n.ident}' is not defined", n.span)
        return UNKNOWN

    def _infer_TypeValue(self, n: ast.TypeValue, env):
        return ("typeval", n.name)

    def _infer_ArrayLit(self, n: ast.ArrayLit, env):
        elem = UNKNOWN
        for e in n.elements:
            elem = self._infer(e, env)
            break
        for e in n.elements:
            self._infer(e, env)
        return ("array", elem)

    def _infer_Block(self, n: ast.Block, env):
        scope = dict(env)
        result = "Void"
        for stmt in n.stmts:
            result = self._check_stmt(stmt, scope)
        return result

    def _infer_BinaryOp(self, n: ast.BinaryOp, env):
        lt = self._infer(n.left, env)
        rt = self._infer(n.right, env)
        if n.op in ("==", "!="):
            return "Bool"
        if n.op in ("<", "<=", ">", ">="):
            self._require_numeric(lt, n.left, n.op)
            self._require_numeric(rt, n.right, n.op)
            return "Bool"
        if n.op in ("&", "|", "^", "<<", ">>"):
            for ty, sub in ((lt, n.left), (rt, n.right)):
                if ty is not UNKNOWN and ty not in _INT_TYPES:
                    self.err(f"'{n.op}' expects integer operands, found {ty_str(ty)}", sub.span)
            return _INT_TAG
        # arithmetic
        if n.op == "+" and (lt == "String" or rt == "String"):
            self.err(
                "strings do not support '+'; use interpolation like \"{a}{b}\"",
                n.span,
            )
            return "String"
        self._require_numeric(lt, n.left, n.op)
        self._require_numeric(rt, n.right, n.op)
        if lt in _FLOAT_TYPES or rt in _FLOAT_TYPES:
            return _FLOAT_TAG
        if lt in _INT_TYPES and rt in _INT_TYPES:
            return _INT_TAG
        return UNKNOWN

    def _require_numeric(self, ty, node, op) -> None:
        if ty is not UNKNOWN and not is_numeric(ty):
            self.err(f"operator '{op}' expects numbers, found {ty_str(ty)}", getattr(node, "span", None))

    def _infer_Logical(self, n: ast.Logical, env):
        for side in (n.left, n.right):
            ty = self._infer(side, env)
            if ty is not UNKNOWN and ty != "Bool":
                self.err(f"'{n.op}' expects Bool operands, found {ty_str(ty)}", getattr(side, "span", None))
        return "Bool"

    def _infer_UnaryOp(self, n: ast.UnaryOp, env):
        ty = self._infer(n.operand, env)
        if n.op == "not":
            if ty is not UNKNOWN and ty != "Bool":
                self.err(f"'not' expects a Bool, found {ty_str(ty)}", n.span)
            return "Bool"
        if n.op == "~":
            if ty is not UNKNOWN and ty not in _INT_TYPES:
                self.err(f"'~' expects an integer, found {ty_str(ty)}", n.span)
            return ty if ty in _INT_TYPES else _INT_TAG
        self._require_numeric(ty, n.operand, "-")
        return ty if is_numeric(ty) else UNKNOWN

    def _infer_RangeExpr(self, n: ast.RangeExpr, env):
        self._infer(n.start, env)
        self._infer(n.end, env)
        return ("array", _INT_TAG)

    def _infer_If(self, n: ast.If, env):
        self._check_cond(n.cond, env, "if")
        tt = self._infer(n.then, env)
        if n.otherwise is None:
            return "Void"
        et = self._infer(n.otherwise, env)
        if tt == et:
            return tt
        # In the quiet inference pre-pass only: a `Fail(...)`/`Not` branch is the
        # error/absence channel of a fallible body, so the if's *success* type is
        # the other branch. This lets an unannotated `if c { Fail() } else { Void }`
        # infer success `Void` (Trial-wrapped by the caller). The error-emitting
        # pass keeps the conservative UNKNOWN, so no new diagnostic can fire.
        if self._quiet:
            if _is_error_variant(et):
                return tt
            if _is_error_variant(tt):
                return et
        return UNKNOWN

    def _infer_While(self, n: ast.While, env):
        self._check_cond(n.cond, env, "while")
        self._infer(n.body, env)
        return "Void"

    def _infer_Loop(self, n: ast.Loop, env):
        self._infer(n.body, env)
        return "Void"

    def _infer_For(self, n: ast.For, env):
        it = self._infer(n.iterable, env)
        elem = it[1] if isinstance(it, tuple) and it[0] == "array" else UNKNOWN
        scope = dict(env)
        scope[n.var] = elem
        self._record_type(n.span, n.var, elem)
        self._infer(n.body, scope)
        return "Void"

    def _check_cond(self, cond, env, kw) -> None:
        ty = self._infer(cond, env)
        if ty is not UNKNOWN and ty != "Bool":
            self.err(f"the {kw} condition must be a Bool, found {ty_str(ty)}", getattr(cond, "span", None))

    def _infer_Call(self, n: ast.Call, env):
        if isinstance(n.callee, ast.FieldAccess):
            ns = self._ns_base(n.callee.obj, env)
            if ns is not None:
                member = self._namespace_member(ns, n.callee.name, n.callee.span)
            else:
                objty = self._infer(n.callee.obj, env)
                member = self._member(objty, n.callee.name, n.callee.span, node=n.callee)
            for a in n.args:
                self._infer(a.value, env)
            if isinstance(member, tuple) and member[0] == "fn":
                return member[1].ret
            return UNKNOWN
        cty = self._infer(n.callee, env)
        for a in n.args:
            self._infer(a.value, env)
        if isinstance(cty, tuple):
            if cty[0] == "typeval":
                nm = cty[1]
                if self.type_kind.get(nm) == "type":
                    return ("named", nm)
                return UNKNOWN
            if cty[0] == "fn":
                return cty[1].ret
        return UNKNOWN

    def _infer_FieldAccess(self, n: ast.FieldAccess, env):
        ns = self._ns_base(n.obj, env)
        if ns is not None:
            return self._namespace_member(ns, n.name, n.span)
        objty = self._infer(n.obj, env)
        return self._member(objty, n.name, n.span, node=n)

    def _ns_base(self, node, env):
        """If `node` is a bare name bound to a namespace, return its tuple."""
        if isinstance(node, ast.Name) and node.ident in env:
            val = env[node.ident]
            if isinstance(val, tuple) and val and val[0] == "namespace":
                return val  # ("namespace", declared, exports, spec)
        return None

    def _namespace_member(self, ns, name: str, span: Span):
        """Resolve `ns.name` from the target's export table, gated on exports.

        Conservative: a function member propagates its return type; other kinds
        fall to UNKNOWN. Gating errors mirror a selective import."""
        _, declared, exports, spec = ns
        if name not in declared:
            self.err(f"'{name}' is not defined in '{spec}'", span)
            return UNKNOWN
        if name not in exports:
            self.err(f"'{name}' is not exported by '{spec}'", span)
            return UNKNOWN
        info = exports[name]
        if info[0] == "fn":
            return ("fn", info[1] if info[1] is not None else _FnInfo([], UNKNOWN))
        return UNKNOWN

    def _member(self, objty, name: str, span: Span, node=None):
        if isinstance(objty, tuple) and objty[0] == "named":
            # A cross-module type the checker has no member table for (e.g. a
            # value whose `("named", X)` type was never imported by name): stay
            # conservative rather than emit a false "has no member". Real typos on
            # a locally/imported type — which *is* in the table — still error below.
            if objty[1] not in self.type_members:
                # During the quiet pre-pass only, resolve members of a
                # namespace-imported type so an unannotated method calling into it
                # can infer its return. Emits no diagnostic, so the real pass stays
                # conservative on cross-module members.
                if self._quiet and objty[1] in self.ns_type_members:
                    members = self.ns_type_members[objty[1]]
                    if (name in members
                            and name in self.ns_type_exported.get(objty[1], set())):
                        return members[name]
                return UNKNOWN
            members = self.type_members[objty[1]]
            if name not in members:
                self.err(f"'{objty[1]}' has no member '{name}'", span)
                return UNKNOWN
            if objty[1] not in self.abi_structs and name not in self.type_exported.get(objty[1], set()):
                self.err(f"member '{name}' of '{objty[1]}' is private", span)
            # Member found on a provable concrete type: link the resolver's
            # pending member reference to the member binding so navigation works
            # (even to a private member within the same file).
            self._link_member(node, objty[1], name)
            return members[name]
        if isinstance(objty, tuple) and objty[0] == "array":
            if name == "count":
                return _INT_TAG
            return UNKNOWN
        if objty == "String" and name == "count":
            return _INT_TAG
        return UNKNOWN

    def _link_member(self, node, type_name, name) -> None:
        if node is None or self.resolution is None:
            return
        bid = self.resolution.member_index.get((type_name, name))
        if bid is not None:
            self.resolution.set_member_ref(id(node), bid)

    def _infer_Index(self, n: ast.Index, env):
        objty = self._infer(n.obj, env)
        self._infer(n.index, env)
        if isinstance(objty, tuple) and objty[0] == "array":
            return objty[1]
        return UNKNOWN

    def _infer_Try(self, n: ast.Try, env):
        return _trial_payload(self._infer(n.operand, env))  # unwrapped payload type not tracked yet

    def _infer_Cast(self, n: ast.Cast, env):
        self._infer(n.operand, env)
        return self._resolve(n.type_ref)

    def _infer_CanCheck(self, n: ast.CanCheck, env):
        self._infer(n.operand, env)
        self._resolve(n.type_ref)  # rejects Int/Float, validates the target type
        return "Bool"

    def _infer_IsCheck(self, n: ast.IsCheck, env):
        self._infer(n.operand, env)
        self._resolve(n.type_ref)  # rejects Int/Float, validates the target type
        return "Bool"

    def _infer_Assign(self, n: ast.Assign, env):
        self._infer(n.value, env)
        return "Void"

    def _infer_Match(self, n: ast.Match, env):
        sty = self._infer(n.scrutinee, env)
        has_catch_all = n.else_body is not None
        result = UNKNOWN
        first = True
        for arm in n.arms:
            scope = dict(env)
            self._bind_pattern(arm.pattern, sty, scope)
            if isinstance(arm.pattern, ast.BindingPattern):
                has_catch_all = True
            bt = self._infer(arm.body, scope)
            result = bt if first else (result if result == bt else UNKNOWN)
            first = False
        if n.else_body is not None:
            self._infer(n.else_body, env)
        # static exhaustiveness over a known union
        if isinstance(sty, tuple) and sty[0] == "union" and not has_catch_all:
            covered = {
                a.pattern.name for a in n.arms if isinstance(a.pattern, ast.ConstructorPattern)
            }
            missing = [v for v in self.union_variants.get(sty[1], []) if v not in covered]
            if missing:
                self.err(
                    f"non-exhaustive match on '{sty[1]}': missing {', '.join(missing)}",
                    n.span,
                    notes=["cover the remaining case(s) or add an 'else' arm"],
                )
        return result

    def _bind_pattern(self, pat, sty, scope: dict) -> None:
        if isinstance(pat, ast.BindingPattern):
            scope[pat.name] = sty
            self._record_type(pat.span, pat.name, sty)
        elif isinstance(pat, ast.ConstructorPattern):
            members = self.type_members.get(pat.name, {})
            ctor_field_types = list(members.values())
            for sub, fty in zip(pat.subpatterns, ctor_field_types):
                if isinstance(sub, ast.BindingPattern):
                    scope[sub.name] = fty
                    self._record_type(sub.span, sub.name, fty)
                else:
                    self._bind_pattern(sub, UNKNOWN, scope)


class _FnInfo:
    def __init__(self, params, ret) -> None:
        self.params = params
        self.ret = ret


def check(text: str, filename: str = "<input>") -> list[Diagnostic]:
    from .lexer import Lexer
    from .parser import Parser

    source = Source(text, filename)
    module = Parser(Lexer(source).tokenize(), source).parse_program()
    return Checker(source).check_module(module)
