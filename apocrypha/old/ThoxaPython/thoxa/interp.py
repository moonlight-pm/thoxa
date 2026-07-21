"""Tree-walking interpreter for Thoxa.

Values are plain Python values for primitives (int/float/bool/str), ``VOID`` for
unit, Python lists for arrays, plus runtime objects for user types (``ThoxaType``,
``Instance``), unions (``Union``), and callables (``Function``, ``BoundMethod``,
``Builtin``). Memory is the host's — explicitly *scaffolding* per the spec; the
language commits to Perceus reference counting before 1.0.
"""

from __future__ import annotations

import contextlib
import math
import os
import sys
import tempfile
from dataclasses import fields as dc_fields, is_dataclass, replace
from typing import Any

from . import ast
from . import ffi
from .errors import Diagnostic, Source, Span, ThoxaError
from .ffi import Buffer, Bytes
from .loader import (
    Loader,
    LoaderError,
    is_pascal_case,
    module_name,
    pascal_module_error,
    value_cycle_message,
)


def _default_out(text: str) -> None:
    sys.stdout.write(text)

PRIMITIVE_TYPES = {
    "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32", "UInt64",
    "Byte", "Float32", "Float64", "Bool", "Char", "String",
    "Void", "Type", "Array", "Any", "Bytes", "Buffer",
}
ARRAY_METHODS = {"append", "dropLast", "first", "last", "reversed", "contains", "insertAt", "removeAt"}
_INT_TYPES = {
    "Int8", "Int16", "Int32", "Int64",
    "UInt8", "UInt16", "UInt32", "UInt64", "Byte",
}
_FLOAT_TYPES = {"Float32", "Float64"}
_NO_CONVERSION = object()  # sentinel: conversion is not defined


# ---------------------------------------------------------------- runtime values
class VoidType:
    _instance = None

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

    def __repr__(self) -> str:
        return "Void"


VOID = VoidType()


class PrimType:
    """A primitive type used as a first-class value (`let T = Int64`)."""

    def __init__(self, name: str) -> None:
        self.name = name

    def __repr__(self) -> str:
        return self.name


def _subtree_has_try(node) -> bool:
    """Whether `node`'s AST subtree contains a `?` (`ast.Try`), not descending
    into nested function bodies — a `?` inside a method is that method's, not
    the enclosing constructor's."""
    stack = [node]
    while stack:
        cur = stack.pop()
        if isinstance(cur, ast.Try):
            return True
        if isinstance(cur, ast.FunctionDef):
            continue
        if isinstance(cur, list):
            stack.extend(cur)
        elif is_dataclass(cur) and not isinstance(cur, type):
            stack.extend(getattr(cur, f.name) for f in dc_fields(cur))
    return False


def _ctor_is_fallible(defn: ast.TypeDef) -> bool:
    """A constructor is fallible iff its body uses `?`: any ctor-param default or
    field initializer that can short-circuit with a Fail/Not. Such a constructor
    is Trial-returning (the result and error types are inferred from the `?`),
    yielding `Pass(instance)` on success and the short-circuited error otherwise."""
    roots = [p.default for p in defn.ctor_params if p.default is not None]
    roots += [m.default for m in defn.members
              if isinstance(m, ast.Field) and m.default is not None]
    return any(_subtree_has_try(r) for r in roots)


TRIAL_TYPE_NAMES = frozenset({"Trial", "May"})
_TRIAL_VARIANTS = frozenset({"Pass", "Fail", "Has", "Not"})
_TRIAL_CTORS = frozenset({"Pass", "Fail"})
_MAY_CTORS = frozenset({"Has", "Not"})


def _is_trial_value(value) -> bool:
    """Whether `value` is already a Trial/May variant (Pass/Fail/Has/Not)."""
    return isinstance(value, Instance) and value.type.name in _TRIAL_VARIANTS


def _subtree_constructs(node, names) -> bool:
    """Whether `node`'s subtree constructs one of the variants in `names`. A
    construction is a `TypeValue` in expression position — `Fail("x")`, `Not()` —
    so this fires on a real construction but never on a `match` arm, whose
    `Pass(v)`/`Fail(e)` are `ConstructorPattern`s (a bare name string), not
    `TypeValue`s. Nested function bodies are not descended into."""
    stack = [node]
    while stack:
        cur = stack.pop()
        if isinstance(cur, ast.FunctionDef):
            continue
        if isinstance(cur, ast.TypeValue) and cur.name in names:
            return True
        if isinstance(cur, list):
            stack.extend(cur)
        elif is_dataclass(cur) and not isinstance(cur, type):
            stack.extend(getattr(cur, f.name) for f in dc_fields(cur))
    return False


def _fn_trial_family(defn: ast.FunctionDef):
    """Which family a function/method auto-threads — "trial", "may", or None. A body
    is fallible (and so Trial/May-returning) when it uses `?` or directly constructs a
    variant: the success tail is then wrapped (`Pass`/`Has`) and an already-wrapped tail
    passes through. `?` and any `Pass`/`Fail` select "trial"; a lone `Has`/`Not` selects
    "may". An explicit non-Trial/May return type (e.g. `main -> Int64`) opts out, keeping
    the raw value so a stray `?` there stays the author's concern, not a silent wrap."""
    ret = defn.return_type
    if ret is not None and ret.name not in TRIAL_TYPE_NAMES:
        return None
    body = defn.body
    if _subtree_has_try(body) or _subtree_constructs(body, _TRIAL_CTORS):
        return "trial"
    if _subtree_constructs(body, _MAY_CTORS):
        return "may"
    return None


class ThoxaType:
    def __init__(self, defn: ast.TypeDef, env: "Environment") -> None:
        self.name = defn.name
        self.type_params = defn.type_params
        self.ctor_params = defn.ctor_params
        self.fields = [m for m in defn.members if isinstance(m, ast.Field)]
        self.methods = {m.name: m for m in defn.members if isinstance(m, ast.FunctionDef)}
        self.env = env
        self.value = defn.value  # intrinsic scalar of a valued type, else None
        self.exported = {f.name for f in self.fields if f.exported}
        self.exported |= {n for n, fn in self.methods.items() if fn.exported}
        # Ctor params marked `export` are public fields (same name as the param).
        self.exported |= {p.name for p in self.ctor_params if p.exported}
        self.field_mut = {p.name: False for p in self.ctor_params}
        for f in self.fields:
            self.field_mut[f.name] = f.mutable
        self.fallible = _ctor_is_fallible(defn)
        specs = ffi.abi_struct_specs(defn)
        self.abi = specs is not None
        self.abi_ctype = ffi.make_struct_type(defn.name, specs) if self.abi else None

    def __repr__(self) -> str:
        return f"<type {self.name}>"


class Union:
    def __init__(self, defn: ast.TypeAlias) -> None:
        self.name = defn.name
        self.type_params = defn.type_params
        self.variants = [v.name for v in defn.variants]

    def __repr__(self) -> str:
        return f"<type {self.name} = {' | '.join(self.variants)}>"


class Instance:
    def __init__(self, type_: ThoxaType) -> None:
        self.type = type_
        self.fields: dict[str, Any] = {}

    def __repr__(self) -> str:
        return f"{self.type.name}"


class Function:
    def __init__(self, defn: ast.FunctionDef, env: "Environment") -> None:
        self.defn = defn
        self.env = env

    def __repr__(self) -> str:
        return f"<fn {self.defn.name}>"


class BoundMethod:
    def __init__(self, instance: Instance, defn: ast.FunctionDef) -> None:
        self.instance = instance
        self.defn = defn


class Builtin:
    def __init__(self, name: str, fn) -> None:
        self.name = name
        self.fn = fn

    def __repr__(self) -> str:
        return f"<builtin {self.name}>"


class BoundBuiltin:
    def __init__(self, receiver, name: str) -> None:
        self.receiver = receiver
        self.name = name


# ---------------------------------------------------------------- control flow
class BreakSignal(Exception):
    pass


class ContinueSignal(Exception):
    pass


class ReturnSignal(Exception):
    def __init__(self, value) -> None:
        self.value = value


# ---------------------------------------------------------------- environment
class Undefined(Exception):
    pass


class Immutable(Exception):
    pass


class Environment:
    __slots__ = ("vars", "parent", "instance")

    def __init__(self, parent: "Environment | None" = None, instance: Instance | None = None) -> None:
        self.vars: dict[str, list] = {}
        self.parent = parent
        self.instance = instance

    def define(self, name: str, value, mutable: bool) -> None:
        self.vars[name] = [value, mutable]

    def get(self, name: str):
        env = self
        while env is not None:
            if name in env.vars:
                return env.vars[name][0]
            if env.instance is not None and name in env.instance.fields:
                return env.instance.fields[name]
            env = env.parent
        raise Undefined(name)

    def set(self, name: str, value) -> None:
        env = self
        while env is not None:
            if name in env.vars:
                if not env.vars[name][1]:
                    raise Immutable(name)
                env.vars[name][0] = value
                return
            if env.instance is not None and name in env.instance.fields:
                if not env.instance.type.field_mut.get(name, False):
                    raise Immutable(name)
                env.instance.fields[name] = value
                return
            env = env.parent
        raise Undefined(name)

    def has(self, name: str) -> bool:
        env = self
        while env is not None:
            if name in env.vars or (env.instance is not None and name in env.instance.fields):
                return True
            env = env.parent
        return False


class ModuleRecord:
    """A loaded module: its parsed body, own environment, and export surface.

    `declared` is every top-level name; `exports` is the subset published with
    `export`; `values` is the top-level names bound by a non-`mut` `let` (the
    ones whose initializers create a cross-module init-order dependency, §7).
    Selective imports read a name's value out of `env`, gated on `exports`.
    `init_state` drives on-demand phase-2 initialization: "pending" → "active"
    (re-entry through a value dependency is a value-init cycle) → "done".
    """

    __slots__ = ("key", "module", "source", "env", "importer_dir", "exports",
                 "declared", "values", "init_state")

    def __init__(self, key, module, source, env, importer_dir) -> None:
        self.key = key
        self.module = module
        self.source = source
        self.env = env
        self.importer_dir = importer_dir
        self.exports: set = set()
        self.declared: set = set()
        self.values: set = set()
        self.init_state = "pending"


class Namespace:
    """A module-namespace binding, e.g. `strings` in `import strings from …`.

    A namespace is *not a value*: it is valid only as the base of a member
    access (`strings.split`). Member lookup goes through the target's
    `ModuleRecord`, gated on its `exports` exactly like a selective import. The
    checker is the primary enforcement point; `eval_Name` is the runtime backstop
    that stops a namespace from leaking into a value position, and `display`
    refuses to render one.
    """

    __slots__ = ("record", "spec")

    def __init__(self, record: "ModuleRecord", spec: str) -> None:
        self.record = record
        self.spec = spec

    def __repr__(self) -> str:  # pragma: no cover - defensive
        return f"<module namespace {self.spec!r}>"


# ---------------------------------------------------------------- interpreter
class Interpreter:
    def __init__(self, source: Source | None = None, out=None) -> None:
        self.src = source or Source("", "<input>")
        self._out = out or _default_out
        # `base` holds builtins + the prelude and is shared by every module; each
        # module gets its own Environment(parent=base), so the prelude is ambient
        # everywhere while a module's top-level names stay its own.
        self.base = Environment()
        self._install_builtins()
        self._load_prelude()
        self.globals = Environment(parent=self.base)  # the entry module's env
        self._loader: Loader | None = None
        self._modules: dict[str, ModuleRecord] = {}
        # The entry module's registry key, stashed in `run_module` so `exec_import`
        # can exempt imports-of-the-entry from the PascalCase rule even while
        # `self.src` is swapped to a dependency during phase-2 initialization.
        self._entry_module_key: str | None = None
        # The chain of modules currently being initialized in phase 2, so that a
        # namespace value member accessed inside an initializer can pull its
        # target in (and locate a value-init cycle). Empty outside phase 2.
        self._init_stack: list = []

    def _load_prelude(self) -> None:
        from .loader import load_prelude_source
        from .parser import parse as _parse

        src = load_prelude_source()
        module = _parse(src.text, src.filename)
        for stmt in module.body:
            if isinstance(stmt, ast.ExternFn):   # e.g. `sysWrite` behind print/println
                self.declare_extern(stmt, self.base)
            else:
                self.declare(stmt, self.base)

    # -- errors -----------------------------------------------------------
    def error(self, msg: str, span: Span | None, notes=None, source: Source | None = None):
        raise ThoxaError(Diagnostic(msg, span or Span(1, 1), source or self.src, notes or []))

    # -- entry ------------------------------------------------------------
    def run_module(self, module: ast.Module):
        # Two-phase load over the import graph (design §6). The module we were
        # handed is the entry: it keeps its script body, everything it imports is
        # a declarations-only library. A fresh registry per call keeps the REPL —
        # which reuses one interpreter across lines — from wedging.
        project_root, entry_dir = self._entry_location()
        self._loader = Loader(project_root)
        self._modules = {}
        entry_key = self._entry_key()
        self._entry_module_key = entry_key

        # Register the entry up front so a dependency that imports it back (via
        # `~/…` or `./…`) finds the same instance instead of re-registering it
        # with fresh state — otherwise the entry loads and initializes twice.
        entry = ModuleRecord(entry_key, module, self.src, self.globals, entry_dir)
        entry.exports = self._export_names(module)
        entry.declared = self._declared_names(module)
        entry.values = self._value_names(module)
        self._modules[entry_key] = entry

        # Phase 1: declare the entry's own top-level fn/type/extern, then
        # transitively register the modules it reaches.
        for stmt in module.body:
            if isinstance(stmt, (ast.FunctionDef, ast.TypeDef, ast.TypeAlias)):
                self.declare(stmt, self.globals)
            elif isinstance(stmt, ast.ExternFn):
                self.declare_extern(stmt, self.globals)
        self._register_imports(module, self.src, entry_dir)

        # Phase 2: initialize every dependency, then the entry last (it owns the
        # script body and the program's return value). A dependency that value-
        # imports the entry may already have initialized it — guard on init_state.
        for key in list(self._modules):
            if key != entry_key:
                self._ensure_init(self._modules[key], [])
        if entry.init_state == "done":
            return VOID
        entry.init_state = "active"
        result = self._init_module(entry, [entry_key])
        entry.init_state = "done"
        return result

    def _entry_location(self):
        """(project_root, importer_dir) for the entry, or (None, None) in-memory."""
        fn = self.src.filename
        if fn and not fn.startswith("<"):
            d = os.path.dirname(os.path.abspath(fn))
            return d, d
        return None, None

    def _entry_key(self) -> str:
        fn = self.src.filename
        if fn and not fn.startswith("<"):
            return os.path.abspath(fn)
        return fn or "<entry>"

    def _register_imports(self, module, source, importer_dir) -> None:
        for stmt in module.body:
            if not isinstance(stmt, ast.Import):
                continue
            resolved = self._resolve_import(stmt, importer_dir, source)
            if resolved.kind != "file":
                continue  # builtin (ffi) — no module to register
            self._register(resolved, stmt, source)

    def _register(self, resolved, origin_node, origin_source) -> None:
        key = resolved.key
        if key in self._modules:
            return  # already registered (diamond dependency, or an import cycle)
        try:
            module, source = self._loader.module_ast(resolved, origin_node.module, origin_node.span)
        except LoaderError as exc:
            self.error(exc.message, exc.span, notes=exc.notes, source=origin_source)

        self._enforce_decls_only(module, source)
        env = Environment(parent=self.base)
        for stmt in module.body:
            if isinstance(stmt, (ast.FunctionDef, ast.TypeDef, ast.TypeAlias)):
                self.declare(stmt, env)
            elif isinstance(stmt, ast.ExternFn):
                prev = self.src
                self.src = source
                try:
                    self.declare_extern(stmt, env)
                finally:
                    self.src = prev
        importer_dir = os.path.dirname(key)
        record = ModuleRecord(key, module, source, env, importer_dir)
        record.exports = self._export_names(module)
        record.declared = self._declared_names(module)
        record.values = self._value_names(module)
        # Register this module *before* recursing into its imports: a cyclic
        # import will then find it already in `self._modules` (handled by the
        # early return above) instead of erroring. fn/type mutual recursion
        # across files is legitimate and resolves purely from these phase-1
        # name declarations (§7); only value-init cycles are caught in phase 2.
        self._modules[key] = record
        self._register_imports(module, source, importer_dir)

    def _enforce_decls_only(self, module, source) -> None:
        for stmt in module.body:
            if not isinstance(stmt, (ast.LetBinding, ast.FunctionDef, ast.ExternFn,
                                     ast.TypeDef, ast.TypeAlias, ast.Import)):
                self.error(
                    "a module's top level is declarations only",
                    getattr(stmt, "span", None),
                    source=source,
                )

    def _declared_names(self, module) -> set:
        names = set()
        for stmt in module.body:
            if isinstance(stmt, (ast.FunctionDef, ast.TypeDef, ast.TypeAlias,
                                 ast.ExternFn, ast.LetBinding)):
                names.add(stmt.name)
        return names

    def _export_names(self, module) -> set:
        names = set()
        for stmt in module.body:
            if isinstance(stmt, (ast.FunctionDef, ast.TypeDef, ast.TypeAlias, ast.ExternFn)):
                if stmt.exported:
                    names.add(stmt.name)
            elif isinstance(stmt, ast.LetBinding):
                # A top-level `mut` is always private (§9); only immutable `let` exports.
                if stmt.exported and not stmt.mutable:
                    names.add(stmt.name)
        return names

    def _value_names(self, module) -> set:
        # Top-level names bound by a non-`mut` `let`. A cross-file import of one
        # of these creates an initialization-order dependency (§7); fn/type/extern
        # do not, since they are usable straight from phase-1 registration.
        names = set()
        for stmt in module.body:
            if isinstance(stmt, ast.LetBinding) and not stmt.mutable:
                names.add(stmt.name)
        return names

    def _init_module(self, record, init_stack):
        """Phase 2 for one module: bind all its imports, then run its
        initializers/body in source order.

        Every `import` is processed *before* any initializer, so a `let` may use
        an imported name regardless of where the `import` sits in the file (and a
        selective *value* import pulls its target in first — that is where a
        value-init cycle surfaces). fn/type/extern were declared in phase 1.
        `self._init_stack` tracks the active chain so a namespace value member
        reached from an initializer can init its target on demand.
        """
        prev = self.src
        prev_stack = self._init_stack
        self.src = record.source
        self._init_stack = init_stack
        result = VOID
        try:
            for stmt in record.module.body:
                if isinstance(stmt, ast.Import):
                    self.exec_import(stmt, record.env, record.importer_dir,
                                     record.source, init_stack)
            for stmt in record.module.body:
                if isinstance(stmt, (ast.Import, ast.FunctionDef, ast.TypeDef,
                                     ast.TypeAlias, ast.ExternFn)):
                    continue  # imports bound above; names declared in phase 1
                # A control-flow signal escaping a top-level statement (a `?`,
                # `return`, `break`, or `continue` in a module initializer or the
                # entry script body) has no enclosing function/loop to catch it —
                # turn it into a located diagnostic instead of a Python traceback.
                try:
                    result = self.eval(stmt, record.env)
                except ReturnSignal:
                    self.error("'return' / '?' is only valid inside a function",
                               getattr(stmt, "span", None), source=record.source)
                except (BreakSignal, ContinueSignal):
                    self.error("'break' / 'continue' is only valid inside a loop",
                               getattr(stmt, "span", None), source=record.source)
        finally:
            self.src = prev
            self._init_stack = prev_stack
        return result

    def _ensure_init(self, record, init_stack, span=None, source=None):
        """Initialize `record` if it hasn't been, detecting value-init cycles.

        Reached from the phase-2 driver (every reachable module runs its own
        initializers), from `exec_import` when a selective *value* import needs
        its target's `let` evaluated first, and from a namespace value member
        accessed inside an initializer. Re-entering an "active" module means a
        value initializer depends, transitively, on itself — no valid order
        exists (§7). `span`/`source` locate the trigger.
        """
        if record.init_state == "done":
            return
        if record.init_state == "active":
            path = init_stack[init_stack.index(record.key):] + [record.key]
            self.error(value_cycle_message(path), span, source=source)
        record.init_state = "active"
        self._init_module(record, init_stack + [record.key])
        record.init_state = "done"

    def _resolve_import(self, node, importer_dir, source):
        try:
            return self._loader.resolve(node.module, importer_dir, node.span)
        except LoaderError as exc:
            self.error(exc.message, exc.span, notes=exc.notes, source=source)

    def declare(self, stmt, env: Environment) -> None:
        if isinstance(stmt, ast.FunctionDef):
            env.define(stmt.name, Function(stmt, env), False)
        elif isinstance(stmt, ast.TypeDef):
            env.define(stmt.name, ThoxaType(stmt, env), False)
        elif isinstance(stmt, ast.TypeAlias):
            env.define(stmt.name, Union(stmt), False)

    def declare_extern(self, node: ast.ExternFn, env: Environment) -> None:
        param_types = [p.type_ann.name if p.type_ann else None for p in node.params]
        ret = node.return_type.name if node.return_type else "Void"
        try:
            call = ffi.make_extern(node.name, node.library, node.symbol, param_types, ret)
        except ffi.FFIError as exc:
            self.error(str(exc), node.span)

        def invoke(interp, values, span, _call=call):
            try:
                result = _call(values)
            except ffi.FFIError as exc:
                interp.error(str(exc), span)
            return VOID if result is None else result

        env.define(node.name, Builtin(node.name, invoke), False)

    def exec_import(self, node: ast.Import, env: Environment, importer_dir, source,
                    init_stack=()) -> None:
        resolved = self._resolve_import(node, importer_dir, source)
        if resolved.kind == "builtin":  # the built-in `ffi` root
            self._bind_ffi(node, env, source)
            return
        spec = node.module
        # A file module describes a type, so its name is PascalCase (design §2.1).
        # The entry is exempt — it is a program, not an importable module, and may
        # be lowercase (a single-file script) or `Main`; a self-import or a library
        # that imports the entry resolves to the entry key and is exempt too.
        if resolved.key != self._entry_module_key and not is_pascal_case(module_name(spec)):
            self.error(pascal_module_error(spec), node.span, source=source)
        record = self._modules[resolved.key]
        if node.names is None:
            # Namespace import binds a singleton instance under the freely-chosen
            # camelCase name (the parser enforces the casing); there is no
            # canonical-name constraint. A namespace does not initialize its
            # target — member access happens later, once every module is loaded.
            env.define(node.namespace, Namespace(record, spec), False)
            return
        for imported, local in node.names:
            if imported not in record.declared:
                self.error(f"'{imported}' is not defined in '{spec}'", node.span, source=source)
            if imported not in record.exports:
                self.error(f"'{imported}' is not exported by '{spec}'", node.span, source=source)
            # A value import (a top-level `let`) forces the target to initialize
            # first — this is where a value-init cycle is caught. fn/type/extern
            # are ready from phase-1 registration and bind directly.
            if imported in record.values:
                self._ensure_init(record, init_stack, node.span, source)
            # Bind an immutable reference to the target's export under its local name.
            env.define(local, record.env.get(imported), False)

    def _bind_ffi(self, node: ast.Import, env: Environment, source) -> None:
        if node.names is None:
            self.error(
                'import from "ffi" must name its members, '
                'e.g. import { errno, strerror } from "ffi"',
                node.span,
                source=source,
            )
        for imported, local in node.names:
            helper = self._ffi_helpers.get(imported)
            if helper is None:
                self.error(f'"ffi" has no export \'{imported}\'', node.span, source=source)
            env.define(local, helper, False)

    def eval_ExternFn(self, node: ast.ExternFn, env: Environment):
        self.declare_extern(node, env)
        return VOID

    def call_main(self, arguments=()):
        if self.globals.has("main"):
            main = self.globals.get("main")
            if isinstance(main, Function):
                # main may declare one `arguments: Array<String>` parameter, or none.
                # Route through apply()/_bind_values (which binds already-evaluated
                # runtime values by position) — NOT call_function, whose bind_args
                # expects AST argument nodes.
                if main.defn.params:
                    return self.apply(main, [list(arguments)], Span(1, 1))
                return self.apply(main, [], Span(1, 1))
        return VOID

    def entry_exports_main(self) -> bool:
        """Whether the entry module exports a top-level `main` (design §4.1).

        A `Main.thx` project entry must publish `main`, since the runtime calls it
        from outside the module. Single-file scripts are not projects and are not
        held to this contract.
        """
        rec = self._modules.get(self._entry_module_key)
        return bool(rec) and "main" in rec.exports

    # -- dispatch ---------------------------------------------------------
    def eval(self, node, env: Environment):
        method = getattr(self, "eval_" + type(node).__name__, None)
        if method is None:
            self.error(f"interpreter: cannot evaluate {type(node).__name__}", getattr(node, "span", None))
        return method(node, env)

    # -- statements -------------------------------------------------------
    def eval_LetBinding(self, node: ast.LetBinding, env: Environment):
        value = self._maybe_alias(self.eval(node.value, env), node.value)
        if node.name in env.vars and env is not self.globals:
            self.error(f"'{node.name}' is already defined in this scope", node.span)
        env.define(node.name, value, node.mutable)
        return VOID

    def eval_Assign(self, node: ast.Assign, env: Environment):
        rhs = self.eval(node.value, env)
        if node.op != "=":
            cur = self.eval(node.target, env)
            rhs = self._binop(node.op[:-1], cur, rhs, node.span)
        else:
            rhs = self._maybe_alias(rhs, node.value)
        target = node.target
        if isinstance(target, ast.Name):
            try:
                env.set(target.ident, rhs)
            except Immutable:
                self.error(f"cannot reassign '{target.ident}': it is an immutable 'let' binding",
                           target.span, notes=["declare it with 'mut' to allow reassignment"])
            except Undefined:
                self.error(f"'{target.ident}' is not defined", target.span)
        elif isinstance(target, ast.Index):
            self._assert_mutable_root(target, env)
            obj = self.eval(target.obj, env)
            idx = self.eval(target.index, env)
            if ffi.is_c_array(obj):
                if not isinstance(idx, int) or isinstance(idx, bool) or idx < 0 or idx >= len(obj):
                    self.error(f"index {idx} out of bounds (length {len(obj)})", target.span)
                if isinstance(rhs, bool) or not isinstance(rhs, int):
                    self.error("a C array element takes an integer", target.span)
                obj[idx] = rhs
                return VOID
            if not isinstance(obj, list):
                self.error("only arrays can be index-assigned", target.span)
            if not isinstance(idx, int) or idx < 0 or idx >= len(obj):
                self.error(f"index {idx} out of bounds (length {len(obj)})", target.span)
            obj[idx] = rhs
        elif isinstance(target, ast.FieldAccess):
            obj = self.eval(target.obj, env)
            if isinstance(obj, ffi.CStruct):
                if not obj.has_field(target.name):
                    self.error(f"'{obj.type_name}' has no field '{target.name}'", target.span)
                if not obj._type.field_mut.get(target.name, False):
                    self.error(f"field '{target.name}' is not mutable (declare it with 'mut')", target.span)
                if isinstance(rhs, bool):
                    self.error("cannot store a Bool in a C struct field; use an integer", target.span)
                try:
                    obj.set(target.name, rhs)
                except (TypeError, ValueError, OverflowError):
                    self.error(f"cannot store {self._type_name(rhs)} in C struct field '{target.name}'", target.span)
                return VOID
            if not isinstance(obj, Instance):
                self.error("only type instances have assignable fields", target.span)
            if target.name not in obj.fields:
                self.error(f"'{obj.type.name}' has no field '{target.name}'", target.span)
            if not obj.type.field_mut.get(target.name, False):
                self.error(f"field '{target.name}' is not mutable (declare it with 'mut')", target.span)
            obj.fields[target.name] = rhs
        else:
            self.error("cannot assign to this expression", node.span)
        return VOID

    def _assert_mutable_root(self, target, env: Environment) -> None:
        node = target
        while isinstance(node, (ast.Index, ast.FieldAccess)):
            node = node.obj
        if isinstance(node, ast.Name):
            # find the binding's mutability
            e = env
            while e is not None:
                if node.ident in e.vars:
                    if not e.vars[node.ident][1]:
                        self.error(f"cannot mutate '{node.ident}': it is an immutable 'let' binding",
                                   target.span, notes=["declare it with 'mut' to allow mutation"])
                    return
                e = e.parent

    def eval_FunctionDef(self, node: ast.FunctionDef, env: Environment):
        env.define(node.name, Function(node, env), False)
        return VOID

    def eval_TypeDef(self, node: ast.TypeDef, env: Environment):
        env.define(node.name, ThoxaType(node, env), False)
        return VOID

    def eval_TypeAlias(self, node: ast.TypeAlias, env: Environment):
        env.define(node.name, Union(node), False)
        return VOID

    def eval_Return(self, node: ast.Return, env: Environment):
        raise ReturnSignal(self.eval(node.value, env) if node.value is not None else VOID)

    def eval_Break(self, node, env):
        raise BreakSignal()

    def eval_Continue(self, node, env):
        raise ContinueSignal()

    def eval_Import(self, node: ast.Import, env: Environment):
        # Imports are resolved during the load phase (see `run_module`), never by
        # evaluating the statement in place.
        return VOID

    # -- expressions ------------------------------------------------------
    def eval_Block(self, node: ast.Block, env: Environment):
        scope = Environment(parent=env)
        result = VOID
        for stmt in node.stmts:
            result = self.eval(stmt, scope)
        return result

    def eval_If(self, node: ast.If, env: Environment):
        if self._truth(self.eval(node.cond, env), node.cond):
            return self.eval(node.then, env)
        if node.otherwise is not None:
            return self.eval(node.otherwise, env)
        return VOID

    def eval_Loop(self, node: ast.Loop, env: Environment):
        while True:
            try:
                self.eval(node.body, env)
            except BreakSignal:
                break
            except ContinueSignal:
                continue
        return VOID

    def eval_While(self, node: ast.While, env: Environment):
        while self._truth(self.eval(node.cond, env), node.cond):
            try:
                self.eval(node.body, env)
            except BreakSignal:
                break
            except ContinueSignal:
                continue
        return VOID

    def eval_For(self, node: ast.For, env: Environment):
        seq = self._iterable(self.eval(node.iterable, env), node.iterable)
        for item in seq:
            scope = Environment(parent=env)
            scope.define(node.var, item, False)
            try:
                self.eval(node.body, scope)
            except BreakSignal:
                break
            except ContinueSignal:
                continue
        return VOID

    def eval_BinaryOp(self, node: ast.BinaryOp, env: Environment):
        left = self.eval(node.left, env)
        right = self.eval(node.right, env)
        return self._binop(node.op, left, right, node.span)

    def eval_Logical(self, node: ast.Logical, env: Environment):
        left = self._truth(self.eval(node.left, env), node.left)
        if node.op == "and":
            return self._truth(self.eval(node.right, env), node.right) if left else False
        return True if left else self._truth(self.eval(node.right, env), node.right)

    def eval_UnaryOp(self, node: ast.UnaryOp, env: Environment):
        val = self.eval(node.operand, env)
        if node.op == "not":
            return not self._truth(val, node.operand)
        if node.op == "-":
            if isinstance(val, bool) or not isinstance(val, (int, float)):
                self.error("unary '-' expects a number", node.span)
            return -val
        if node.op == "~":
            if isinstance(val, bool) or not isinstance(val, int):
                self.error("unary '~' expects an integer", node.span)
            return ~val
        self.error(f"unknown unary operator '{node.op}'", node.span)

    def eval_RangeExpr(self, node: ast.RangeExpr, env: Environment):
        start = self.eval(node.start, env)
        end = self.eval(node.end, env)
        if not isinstance(start, int) or not isinstance(end, int):
            self.error("ranges require integer bounds", node.span)
        return range(start, end + 1 if node.inclusive else end)

    def eval_Call(self, node: ast.Call, env: Environment):
        if isinstance(node.callee, ast.FieldAccess):
            recv = self._access_base(node.callee.obj, env)
            member = self._member(recv, node.callee.name, node.callee.span)
            return self.call_value(member, node.args, env, node.span)
        callee = self.eval(node.callee, env)
        return self.call_value(callee, node.args, env, node.span)

    def _access_base(self, obj, env: Environment):
        """Evaluate a member-access base, allowing a namespace (not a value).

        A namespace is fetched straight from the environment so it does not trip
        `eval_Name`'s not-a-value guard; everything else evaluates normally."""
        if isinstance(obj, ast.Name) and env.has(obj.ident):
            base = env.get(obj.ident)
            if isinstance(base, Namespace):
                return base
        return self.eval(obj, env)

    def eval_FieldAccess(self, node: ast.FieldAccess, env: Environment):
        recv = self._access_base(node.obj, env)
        return self._member(recv, node.name, node.span)

    def eval_Index(self, node: ast.Index, env: Environment):
        obj = self.eval(node.obj, env)
        idx = self.eval(node.index, env)
        if isinstance(obj, (list, str)):
            if not isinstance(idx, int) or isinstance(idx, bool):
                self.error("array index must be an integer", node.span)
            if idx < 0 or idx >= len(obj):
                self.error(f"index {idx} out of bounds (length {len(obj)})", node.span)
            return obj[idx]
        if ffi.is_c_array(obj):
            if not isinstance(idx, int) or isinstance(idx, bool):
                self.error("array index must be an integer", node.span)
            if idx < 0 or idx >= len(obj):
                self.error(f"index {idx} out of bounds (length {len(obj)})", node.span)
            return obj[idx]
        self.error("only arrays and strings can be indexed", node.span)

    def eval_Try(self, node: ast.Try, env: Environment):
        val = self.eval(node.operand, env)
        if isinstance(val, Instance):
            tname = val.type.name
            if tname in ("Pass", "Has"):
                return val.fields.get("value")
            if tname == "Fail":
                raise ReturnSignal(val)
            if tname == "Not":
                raise ReturnSignal(val)
        self.error("'?' can only be applied to a Trial (Pass/Fail) or May (Has/Not) value", node.span)

    def eval_Cast(self, node: ast.Cast, env: Environment):
        value = self.eval(node.operand, env)
        result = self._convert(value, node.type_ref, env)
        if result is _NO_CONVERSION:
            self.error(
                f"no conversion from {self._type_name(value)} to {node.type_ref.name}",
                node.span,
            )
        return result

    def eval_CanCheck(self, node: ast.CanCheck, env: Environment):
        value = self.eval(node.operand, env)
        return self._convert(value, node.type_ref, env) is not _NO_CONVERSION

    def eval_IsCheck(self, node: ast.IsCheck, env: Environment):
        value = self.eval(node.operand, env)
        return self._runtime_is(value, node.type_ref, env)

    def _runtime_is(self, value, type_ref: ast.TypeRef, env: Environment) -> bool:
        name = type_ref.name
        if name == "Any":
            return True
        if name in _INT_TYPES:
            return isinstance(value, int) and not isinstance(value, bool)
        if name in _FLOAT_TYPES:
            return isinstance(value, float)
        if name == "Bytes":
            return isinstance(value, Bytes)
        if name == "Bool":
            return isinstance(value, bool)
        if name in ("String", "Char"):
            return isinstance(value, str)
        if name == "Array":
            return isinstance(value, list)
        if name == "Void":
            return isinstance(value, VoidType)
        if isinstance(value, Instance):
            if value.type.name == name:
                return True
            if env.has(name):
                target = env.get(name)
                if isinstance(target, Union):
                    return value.type.name in target.variants
        return False

    def _convert(self, value, type_ref: ast.TypeRef, env: Environment):
        """Return the converted value, or the sentinel _NO_CONVERSION if undefined."""
        name = type_ref.name
        if self._runtime_is(value, type_ref, env):
            return value  # already of that type (incl. widening: all ints share a repr)
        # A valued nominal type (`type ReadOnly 0`, `type Tab '\t'`) unwraps to its
        # intrinsic scalar; the operand may be an instance or the bare type. Convert
        # that scalar to the target like any other value (identity, widening, ->String).
        vt = value.type if isinstance(value, Instance) else value
        if isinstance(vt, ThoxaType) and vt.value is not None:
            value = self.eval(vt.value, vt.env)
            if self._runtime_is(value, type_ref, env):
                return value
        if name == "String":
            return self._to_string(value, env)
        if name in _FLOAT_TYPES and self._numeric(value):
            return float(value)
        if name in _INT_TYPES:
            if isinstance(value, float):
                return int(value)  # truncate toward zero
            if isinstance(value, int) and not isinstance(value, bool):
                return value
        return _NO_CONVERSION

    def _to_string(self, value, env: Environment) -> str:
        if isinstance(value, Instance):
            m = value.type.methods.get("toString")
            if m is not None and "toString" in value.type.exported:
                return self.call_method(BoundMethod(value, m), [], env, Span(1, 1))
        return self.display(value)

    def eval_ArrayLit(self, node: ast.ArrayLit, env: Environment):
        return [self.eval(e, env) for e in node.elements]

    def eval_IntLit(self, node: ast.IntLit, env):
        return node.value

    def eval_FloatLit(self, node: ast.FloatLit, env):
        return node.value

    def eval_BoolLit(self, node: ast.BoolLit, env):
        return node.value

    def eval_CharLit(self, node: ast.CharLit, env):
        return node.value

    def eval_ByteLit(self, node: ast.ByteLit, env):
        return node.value

    def eval_VoidLit(self, node, env):
        return VOID

    def eval_StringLit(self, node: ast.StringLit, env: Environment):
        out = []
        for part in node.parts:
            if part[0] == "str":
                out.append(part[1])
            else:
                out.append(self.display(self.eval(part[1], env)))
        return "".join(out)

    def eval_Name(self, node: ast.Name, env: Environment):
        try:
            value = env.get(node.ident)
        except Undefined:
            self.error(f"'{node.ident}' is not defined", node.span)
        if isinstance(value, Namespace):
            # A namespace is valid only left of a `.`; reaching one here means it
            # was used as a value. (`eval_FieldAccess` fetches the base directly.)
            self.error("a module namespace cannot be used as a value", node.span)
        return value

    def eval_TypeValue(self, node: ast.TypeValue, env: Environment):
        try:
            return env.get(node.name)
        except Undefined:
            if node.name in PRIMITIVE_TYPES:
                return PrimType(node.name)
            self.error(f"type '{node.name}' is not defined", node.span)

    # -- match ------------------------------------------------------------
    def eval_Match(self, node: ast.Match, env: Environment):
        value = self.eval(node.scrutinee, env)
        for arm in node.arms:
            bindings: dict = {}
            if self._match_pattern(arm.pattern, value, bindings, env):
                scope = Environment(parent=env)
                for name, val in bindings.items():
                    scope.define(name, val, False)
                return self.eval(arm.body, scope)
        if node.else_body is not None:
            return self.eval(node.else_body, env)
        self.error("no match arm applied and there is no 'else' arm", node.span)

    def _match_pattern(self, pat, value, bindings: dict, env: Environment) -> bool:
        if isinstance(pat, ast.BindingPattern):
            bindings[pat.name] = value
            return True
        if isinstance(pat, ast.LiteralPattern):
            return value == self.eval(pat.value, env)
        if isinstance(pat, ast.ConstructorPattern):
            if not isinstance(value, Instance) or value.type.name != pat.name:
                return False
            field_names = [p.name for p in value.type.ctor_params]
            if pat.subpatterns and len(pat.subpatterns) != len(field_names):
                self.error(
                    f"pattern '{pat.name}' expects {len(field_names)} field(s), got {len(pat.subpatterns)}",
                    pat.span,
                )
            for sub, fname in zip(pat.subpatterns, field_names):
                if not self._match_pattern(sub, value.fields.get(fname), bindings, env):
                    return False
            return True
        self.error("unsupported pattern", getattr(pat, "span", None))

    # -- calls ------------------------------------------------------------
    def call_value(self, callee, args: list[ast.Arg], env: Environment, span: Span):
        if isinstance(callee, Function):
            return self.call_function(callee, args, env, span)
        if isinstance(callee, BoundMethod):
            return self.call_method(callee, args, env, span)
        if isinstance(callee, ThoxaType):
            return self.construct(callee, args, env, span)
        if isinstance(callee, Builtin):
            values = [self.eval(a.value, env) for a in args]
            return callee.fn(self, values, span)
        if isinstance(callee, BoundBuiltin):
            values = [self.eval(a.value, env) for a in args]
            if isinstance(callee.receiver, Bytes):
                return self._bytes_method(callee.receiver, callee.name, values, span)
            if isinstance(callee.receiver, Buffer):
                return self._buffer_method(callee.receiver, callee.name, values, span)
            return self._array_method(callee.receiver, callee.name, values, span)
        if isinstance(callee, PrimType) and callee.name == "Bytes":
            values = [self.eval(a.value, env) for a in args]
            return self._make_bytes(values, span)
        if isinstance(callee, PrimType) and callee.name == "Buffer":
            values = [self.eval(a.value, env) for a in args]
            return self._make_buffer(values, span)
        self.error(f"this value is not callable ({self.display(callee)})", span)

    def _make_bytes(self, values, span: Span):
        if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
            self.error("Bytes(size) takes a single integer size", span)
        if values[0] < 0:
            self.error("Bytes size must be non-negative", span)
        return Bytes(values[0])

    def _bytes_method(self, buf: Bytes, name: str, values, span: Span):
        if name == "get":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Bytes.get(i) expects an integer index", span)
            try:
                return buf.get(values[0])
            except IndexError:
                self.error(f"Bytes index {values[0]} out of range (count {buf.count})", span)
        if name == "set":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Bytes.set(i, b) expects two integers", span)
            try:
                buf.set(values[0], values[1])
            except IndexError:
                self.error(f"Bytes index {values[0]} out of range (count {buf.count})", span)
            return VOID
        if name == "toString":
            return buf.to_string()
        if name == "slice":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Bytes.slice(start, end) expects two integers", span)
            try:
                return buf.slice(values[0], values[1])
            except IndexError:
                self.error(
                    f"slice [{values[0]}..{values[1]}] out of bounds "
                    f"(length {buf.count})",
                    span,
                )
        if name == "insertAt":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Bytes.insertAt(at, byte) expects two integers", span)
            try:
                return buf.insert_at(values[0], values[1])
            except IndexError:
                self.error(
                    f"Bytes.insertAt index {values[0]} out of range (count {buf.count})",
                    span,
                )
        if name == "removeAt":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Bytes.removeAt(at) expects an integer index", span)
            try:
                return buf.remove_at(values[0])
            except IndexError:
                self.error(
                    f"Bytes.removeAt index {values[0]} out of range (count {buf.count})",
                    span,
                )
        if name == "find":
            if len(values) != 1 or not isinstance(values[0], Bytes):
                self.error("Bytes.find(needle) expects a Bytes buffer", span)
            idx = buf.find(values[0])
            return self._wrap_has(idx) if idx >= 0 else Instance(self.base.get("Not"))
        if name == "equals":
            if len(values) != 1 or not isinstance(values[0], Bytes):
                self.error("Bytes.equals(other) expects a Bytes buffer", span)
            return buf.equals(values[0])
        self.error(f"Bytes has no method '{name}'", span)

    def _make_buffer(self, values, span: Span):
        if len(values) == 0:
            return Buffer()
        if len(values) == 1 and isinstance(values[0], int) and not isinstance(values[0], bool):
            if values[0] < 0:
                self.error("Buffer(capacity) must be non-negative", span)
            return Buffer(values[0])
        self.error("Buffer() takes an optional integer capacity", span)

    def _buffer_method(self, buf: Buffer, name: str, values, span: Span):
        if buf._consumed:
            self.error("this Buffer was consumed by freeze(); it cannot be used again", span)
        if name == "append":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Buffer.append(b) expects a single integer byte", span)
            buf.append(values[0])
            return VOID
        if name == "appendBytes":
            if len(values) != 1 or not isinstance(values[0], Bytes):
                self.error("Buffer.appendBytes(bytes) expects a Bytes buffer", span)
            buf.append_bytes(values[0])
            return VOID
        if name == "reserve":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Buffer.reserve(n) expects an integer count", span)
            if values[0] < 0:
                self.error("Buffer.reserve(n) expects a non-negative count", span)
            buf.reserve(values[0])
            return VOID
        if name == "get":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Buffer.get(i) expects an integer index", span)
            try:
                return buf.get(values[0])
            except IndexError:
                self.error(f"Buffer index {values[0]} out of range (count {buf.count})", span)
        if name == "set":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Buffer.set(i, b) expects two integers", span)
            try:
                buf.set(values[0], values[1])
            except IndexError:
                self.error(f"Buffer index {values[0]} out of range (count {buf.count})", span)
            return VOID
        if name == "freeze":
            if values:
                self.error("Buffer.freeze() takes no arguments", span)
            return buf.freeze()
        self.error(f"Buffer has no method '{name}'", span)

    def _maybe_alias(self, value, src):
        """A Buffer flowing from an existing owner (an lvalue) into a NEW binding is a
        *share*, not a move: hand out a fresh handle over the same allocation (a `dup`,
        refcount++). A value born from a temporary (constructor / call / literal) has no
        other owner, so it moves in unchanged. Non-Buffer values pass straight through.

        This is the interpreter's stand-in for the `dup` a Perceus backend would insert
        at each binding site; copy-on-write at the next mutation splits the aliases. See
        `ffi.Buffer` for the full reasoning (and what the prototype leaves conservative)."""
        if isinstance(value, Buffer) and isinstance(src, (ast.Name, ast.FieldAccess, ast.Index)):
            if value._consumed:
                self.error("this Buffer was consumed by freeze(); it cannot be used again", src.span)
            return value.share()
        return value

    def _thread_result(self, value, family):
        """Coerce a fallible callable's result into its family: an already-wrapped Trial/
        May value passes through (no double-wrap); any other value is the success payload,
        wrapped in `Pass` for "trial" or `Has` for "may"."""
        if _is_trial_value(value):
            return value
        if family == "may":
            return self._wrap_has(value)
        return self._wrap_pass(value)

    def _run_body(self, defn: ast.FunctionDef, scope: Environment):
        """Evaluate a function/method body, applying implicit Trial/May-threading when the
        callable infers a fallible return (a `?` or a variant construction in its body).
        `?` short-circuits and explicit returns both flow through the same coercion."""
        try:
            result = self.eval(defn.body, scope)
        except ReturnSignal as r:
            result = r.value
        family = _fn_trial_family(defn)
        return self._thread_result(result, family) if family else result

    def call_function(self, fn: Function, args, env: Environment, span: Span):
        scope = Environment(parent=fn.env)
        bound = self.bind_args(fn.defn.params, args, env, fn.env, span, fn.defn.name)
        for name, val in bound.items():
            scope.define(name, val, True)
        return self._run_body(fn.defn, scope)

    def call_method(self, bm: BoundMethod, args, env: Environment, span: Span):
        scope = Environment(parent=bm.instance.type.env, instance=bm.instance)
        bound = self.bind_args(bm.defn.params, args, env, bm.instance.type.env, span, bm.defn.name)
        for name, val in bound.items():
            scope.define(name, val, True)
        return self._run_body(bm.defn, scope)

    def apply(self, callee, values, span: Span):
        """Call a Thoxa function/method from host (Python) code with already-evaluated
        positional VALUES: the host->Thoxa bridge for native host functions registered
        through the ``install`` seam (see ``run``)."""
        if isinstance(callee, Function):
            scope = Environment(parent=callee.env)
            self._bind_values(callee.defn.params, values, scope, callee.defn.name, span)
            return self._run_body(callee.defn, scope)
        if isinstance(callee, BoundMethod):
            scope = Environment(parent=callee.instance.type.env, instance=callee.instance)
            self._bind_values(callee.defn.params, values, scope, callee.defn.name, span)
            return self._run_body(callee.defn, scope)
        self.error("host tried to call a value that is not a function", span)

    def _bind_values(self, params, values, scope: Environment, name: str, span: Span) -> None:
        fixed = [p for p in params if not p.variadic]
        var = next((p for p in params if p.variadic), None)
        collected = []
        for i, v in enumerate(values):
            if i < len(fixed):
                scope.define(fixed[i].name, v, True)
            elif var is not None:
                collected.append(v)
            else:
                self.error(f"host passed too many arguments to '{name}'", span)
        if var is not None:
            scope.define(var.name, collected, True)
        for p in fixed:
            if p.name not in scope.vars:
                if p.default is not None:
                    scope.define(p.name, self.eval(p.default, scope), True)
                else:
                    self.error(f"host did not supply argument '{p.name}' to '{name}'", span)

    def construct(self, type_: ThoxaType, args, env: Environment, span: Span):
        if type_.abi:
            if args:
                self.error(f"{type_.name}() takes no arguments (a C struct is zero-initialized)", span)
            return ffi.CStruct(type_.abi_ctype, type_)
        inst = Instance(type_)
        fenv = Environment(parent=type_.env, instance=inst)

        def _init():
            bound = self.bind_args(type_.ctor_params, args, env, type_.env, span, type_.name)
            for p in type_.ctor_params:
                inst.fields[p.name] = bound[p.name]
            for f in type_.fields:
                if f.default is not None:
                    inst.fields[f.name] = self.eval(f.default, fenv)
                else:
                    inst.fields[f.name] = VOID

        if not type_.fallible:
            _init()
            return inst
        # Fallible constructor: a `?` in the body may short-circuit. Catch the
        # escaping Fail/Not and surface it as the whole construction's result;
        # otherwise the finished instance is wrapped in `Pass`.
        try:
            _init()
        except ReturnSignal as r:
            return r.value
        return self._wrap_pass(inst)

    def _wrap_pass(self, value):
        """`Pass(value)`, built directly from the prelude's `Pass` type."""
        inst = Instance(self.base.get("Pass"))
        inst.fields["value"] = value
        return inst

    def _wrap_has(self, value):
        """`Has(value)`, built directly from the prelude's `Has` type."""
        inst = Instance(self.base.get("Has"))
        inst.fields["value"] = value
        return inst

    def _check_signature(self, params, callee_name: str, span: Span) -> None:
        seen_named = False
        seen_default_pos = False
        for i, p in enumerate(params):
            if p.variadic:
                if i != len(params) - 1:
                    self.error(f"variadic parameter '...{p.name}' must be last", p.span)
                continue
            if p.positional:
                if seen_named:
                    self.error(
                        f"positional parameter '{p.name}!' must come before named parameters", p.span
                    )
                if p.default is not None:
                    seen_default_pos = True
                elif seen_default_pos:
                    self.error(
                        f"required positional parameter '{p.name}!' cannot follow "
                        f"a defaulted positional parameter", p.span
                    )
            else:
                seen_named = True

    def bind_args(self, params, args, call_env, default_env, span, callee_name):
        """Named-by-default dispatch: labels required and in declaration order; defaults
        omittable; `!` params are positional (no label); a trailing `...` collects the rest.
        An unlabeled bare identifier that names a keyword parameter is punned to `name: name`."""
        self._check_signature(params, callee_name, span)
        pos_params = [p for p in params if p.positional]
        var_param = next((p for p in params if p.variadic), None)
        named_params = [p for p in params if not p.positional and not p.variadic]
        named_names = [p.name for p in named_params]

        unlabeled, labeled, seen_label = [], [], False
        for a in args:
            # Bare-name punning: an unlabeled identifier that matches a keyword
            # parameter is the named argument `name: name`. Positional (`!`) and
            # variadic parameters are not keyword parameters, so a bare name that
            # happens to match one of them stays positional. Only bare names pun —
            # literals and compound expressions never do, and spreads are exempt.
            if (a.label is None and not a.spread
                    and isinstance(a.value, ast.Name) and a.value.ident in named_names):
                a = replace(a, label=a.value.ident)
            if a.label is None:
                if seen_label:
                    self.error(f"positional argument after a named argument in call to '{callee_name}'", a.span)
                unlabeled.append(a)
            else:
                seen_label = True
                labeled.append(a)

        values: dict = {}
        collected = []
        for i, a in enumerate(unlabeled):
            if i < len(pos_params):
                values[pos_params[i].name] = self._maybe_alias(self.eval(a.value, call_env), a.value)
            elif var_param is not None:
                collected.append(self._maybe_alias(self.eval(a.value, call_env), a.value))
            else:
                self.error(
                    f"too many positional arguments to '{callee_name}' "
                    f"(expected {len(pos_params)})",
                    a.span,
                )
        for p in pos_params:
            if p.name not in values:
                if p.default is not None:
                    values[p.name] = self.eval(p.default, default_env)
                else:
                    self.error(f"missing positional argument '{p.name}' for '{callee_name}'", span)
        if var_param is not None:
            values[var_param.name] = collected

        idx = 0
        for a in labeled:
            while idx < len(named_names) and named_names[idx] != a.label:
                idx += 1
            if idx >= len(named_names):
                if a.label in named_names:
                    self.error(
                        f"named argument '{a.label}' is out of order "
                        f"(arguments must be in declaration order)", a.span
                    )
                elif any(p.name == a.label for p in pos_params):
                    self.error(f"'{a.label}' is a positional parameter — pass it without a label", a.span)
                else:
                    self.error(f"'{callee_name}' has no parameter '{a.label}'", a.span)
            values[named_params[idx].name] = self._maybe_alias(self.eval(a.value, call_env), a.value)
            idx += 1

        for p in named_params:
            if p.name not in values:
                if p.default is not None:
                    values[p.name] = self.eval(p.default, default_env)
                else:
                    self.error(
                        f"missing argument '{p.name}' for '{callee_name}' "
                        f"(named arguments require a label)", span
                    )
        return values

    # -- member access ----------------------------------------------------
    def _member(self, recv, name: str, span: Span):
        if isinstance(recv, Namespace):
            return self._namespace_member(recv, name, span)
        if isinstance(recv, Instance):
            if name in recv.type.methods:
                if name not in recv.type.exported:
                    self.error(f"method '{name}' of '{recv.type.name}' is private", span)
                return BoundMethod(recv, recv.type.methods[name])
            if name in recv.fields:
                if name not in recv.type.exported:
                    self.error(f"field '{name}' of '{recv.type.name}' is private", span)
                return recv.fields[name]
            self.error(f"'{recv.type.name}' has no member '{name}'", span)
        if isinstance(recv, ffi.CStruct):
            if not recv.has_field(name):
                self.error(f"'{recv.type_name}' has no field '{name}'", span)
            return recv.get(name)  # scalar -> int; fixed-array field -> a ctypes array view
        if isinstance(recv, list):
            if name == "count":
                return len(recv)
            if name in ARRAY_METHODS:
                return BoundBuiltin(recv, name)
            self.error(f"arrays have no member '{name}'", span)
        if isinstance(recv, str):
            if name == "count":
                return len(recv)
            self.error(f"strings have no member '{name}'", span)
        if isinstance(recv, Bytes):
            if name == "count":
                return recv.count
            if name in ("get", "set", "toString", "slice", "insertAt", "removeAt", "find", "equals"):
                return BoundBuiltin(recv, name)
            self.error(f"Bytes has no member '{name}'", span)
        if isinstance(recv, Buffer):
            if recv._consumed:
                self.error("this Buffer was consumed by freeze(); it cannot be used again", span)
            if name == "count":
                return recv.count
            if name == "capacity":
                return recv.capacity
            if name in ("append", "appendBytes", "reserve", "get", "set", "freeze"):
                return BoundBuiltin(recv, name)
            self.error(f"Buffer has no member '{name}'", span)
        if isinstance(recv, PrimType) and recv.name == "Bytes":
            if name == "fromString":
                return self._bytes_from_string_builtin
            self.error(f"'Bytes' has no static member '{name}'", span)
        self.error(f"value of type {self._type_name(recv)} has no member '{name}'", span)

    def _namespace_member(self, ns: Namespace, name: str, span: Span):
        """Resolve `ns.name` through the target module, gated on its exports.

        A non-exported or undefined member is the same located error a selective
        import would raise. A *value* member reached while its module is still
        being initialized (a top-level `let` reaching another module's value
        through a namespace) initializes the target on demand — and surfaces a
        value-init cycle if that target is already active. After phase 2 every
        module is "done", so this is a no-op.
        """
        record = ns.record
        if name not in record.declared:
            self.error(f"'{name}' is not defined in '{ns.spec}'", span)
        if name not in record.exports:
            self.error(f"'{name}' is not exported by '{ns.spec}'", span)
        if name in record.values and record.init_state != "done":
            self._ensure_init(record, self._init_stack, span, self.src)
        return record.env.get(name)

    def _array_method(self, recv: list, name: str, values, span: Span):
        if name == "append":
            return recv + [values[0]]
        if name == "dropLast":
            return recv[:-1]
        if name == "first":
            return recv[0] if recv else VOID
        if name == "last":
            return recv[-1] if recv else VOID
        if name == "reversed":
            return list(reversed(recv))
        if name == "contains":
            return values[0] in recv
        if name == "insertAt":
            if len(values) != 2 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Array.insertAt(at, item) expects an integer index", span)
            at = values[0]
            if not (0 <= at <= len(recv)):
                self.error(f"Array.insertAt index {at} out of range (count {len(recv)})", span)
            return recv[:at] + [values[1]] + recv[at:]
        if name == "removeAt":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Array.removeAt(at) expects an integer index", span)
            at = values[0]
            if not (0 <= at < len(recv)):
                self.error(f"Array.removeAt index {at} out of range (count {len(recv)})", span)
            return recv[:at] + recv[at + 1 :]
        self.error(f"arrays have no method '{name}'", span)

    # -- operators & coercion --------------------------------------------
    def _binop(self, op: str, left, right, span: Span):
        if op in ("==", "!="):
            eq = self._equal(left, right)
            return eq if op == "==" else not eq
        if op in ("<", "<=", ">", ">="):
            if not self._numeric(left) or not self._numeric(right):
                self.error(f"'{op}' expects two numbers", span)
            return {"<": left < right, "<=": left <= right, ">": left > right, ">=": left >= right}[op]
        if op in ("&", "|", "^", "<<", ">>"):
            if (not isinstance(left, int) or isinstance(left, bool)
                    or not isinstance(right, int) or isinstance(right, bool)):
                self.error(f"'{op}' expects two integers", span)
            if op in ("<<", ">>") and right < 0:
                self.error(f"negative shift amount in '{op}'", span)
            if op == "&":
                return left & right
            if op == "|":
                return left | right
            if op == "^":
                return left ^ right
            if op == "<<":
                return left << right
            return left >> right
        if not self._numeric(left) or not self._numeric(right):
            self.error(f"'{op}' expects two numbers (use string interpolation to join text)", span)
        if op == "+":
            return left + right
        if op == "-":
            return left - right
        if op == "*":
            return left * right
        if op == "/":
            if right == 0:
                self.error("division by zero", span)
            if isinstance(left, int) and isinstance(right, int):
                return int(left / right)  # truncate toward zero
            return left / right
        if op == "%":
            if right == 0:
                self.error("division by zero (in '%')", span)
            return math.fmod(left, right) if isinstance(left, float) or isinstance(right, float) else int(math.fmod(left, right))
        self.error(f"unknown operator '{op}'", span)

    @staticmethod
    def _numeric(v) -> bool:
        return isinstance(v, (int, float)) and not isinstance(v, bool)

    def _equal(self, a, b) -> bool:
        if isinstance(a, Instance) and isinstance(b, Instance):
            return a is b
        return a == b

    def _truth(self, v, node) -> bool:
        if isinstance(v, bool):
            return v
        self.error("condition must be a Bool", getattr(node, "span", None))

    def _iterable(self, v, node):
        if isinstance(v, (list, range)):
            return v
        if isinstance(v, str):
            return list(v)
        self.error("value is not iterable (expected an array, range, or string)", getattr(node, "span", None))

    # -- display ----------------------------------------------------------
    def display(self, v) -> str:
        if isinstance(v, bool):
            return "true" if v else "false"
        if isinstance(v, VoidType):
            return "Void"
        if isinstance(v, str):
            return v
        if isinstance(v, float):
            return repr(v)
        if isinstance(v, int):
            return str(v)
        if isinstance(v, list):
            return "[" + ", ".join(self.display(x) for x in v) + "]"
        if isinstance(v, Instance):
            fields = ", ".join(f"{k}: {self.display(val)}" for k, val in v.fields.items())
            return f"{v.type.name}({fields})" if fields else f"{v.type.name}()"
        if isinstance(v, Namespace):  # a namespace must never leak into output
            self.error("a module namespace cannot be used as a value", None)
        return repr(v)

    @staticmethod
    def _type_name(v) -> str:
        # The runtime does not track integer/float width (all ints are Python
        # ints), so a value's reflected type is the language's default width.
        if isinstance(v, bool):
            return "Bool"
        if isinstance(v, int):
            return "Int64"
        if isinstance(v, float):
            return "Float64"
        if isinstance(v, str):
            return "String"
        if isinstance(v, list):
            return "Array"
        if isinstance(v, Bytes):
            return "Bytes"
        if isinstance(v, Buffer):
            return "Buffer"
        if isinstance(v, VoidType):
            return "Void"
        if isinstance(v, ffi.CStruct):
            return v.type_name
        return type(v).__name__

    # -- builtins ---------------------------------------------------------
    def _install_builtins(self) -> None:
        # `print`/`println` used to be host builtins here; they are now ordinary
        # Thoxa functions in the prelude, written over a libc `write` extern (see
        # std/Prelude.thx). Only the primitive types, the FFI error helpers, and
        # `Bytes.fromString` remain host-provided.
        for prim in PRIMITIVE_TYPES:
            self.base.define(prim, PrimType(prim), False)

        # FFI error helpers, reached only via `import { errno, strerror } from "ffi"`.
        def _errno(interp, args, span):
            return ffi.errno_value()

        def _strerror(interp, args, span):
            if not args or not isinstance(args[0], int) or isinstance(args[0], bool):
                interp.error("strerror(code) expects an integer error code", span)
            return ffi.strerror_text(args[0])

        self._ffi_helpers = {
            "errno": Builtin("errno", _errno),
            "strerror": Builtin("strerror", _strerror),
        }

        # `Bytes.fromString` — the static constructor on the Bytes primitive.
        def _bytes_from_string(interp, args, span):
            if not args or not isinstance(args[0], str):
                interp.error("Bytes.fromString(s) expects a String", span)
            return Bytes.from_string(args[0])

        self._bytes_from_string_builtin = Builtin("Bytes.fromString", _bytes_from_string)

    def out(self, text: str) -> None:
        self._out(text)

    @contextlib.contextmanager
    def _capture_stdout(self):
        """FD1-CAPTURE (temporary bridge): `print`/`println` now write to real
        fd 1 via libc, so a non-default output sink (tests via `run(out=...)`, the
        notebook) can no longer observe them the old way. For the duration of a
        run, redirect fd 1 into a temp file, then replay its contents into the
        sink. The default (terminal) sink needs nothing — output goes straight to
        the real fd 1. Remove this once callers capture fd 1 themselves; see the
        `thoxa-println-libc` note. Grep marker: FD1-CAPTURE."""
        if self._out is _default_out:
            yield
            return
        sys.stdout.flush()
        try:
            saved = os.dup(1)
        except OSError:            # no real fd 1 (unusual host) — best-effort noop
            yield
            return
        tmp = tempfile.TemporaryFile()
        try:
            os.dup2(tmp.fileno(), 1)
            yield
        finally:
            sys.stdout.flush()
            os.dup2(saved, 1)
            os.close(saved)
            tmp.seek(0)
            data = tmp.read()
            tmp.close()
            if data:
                self._out(data.decode("utf-8", errors="replace"))


def run(text: str, filename: str = "<input>", *, run_main: bool = True, out=None, install=(), arguments=()):
    from .parser import Parser
    from .lexer import Lexer

    source = Source(text, filename)
    module = Parser(Lexer(source).tokenize(), source).parse_program()
    interp = Interpreter(source, out=out)
    for installer in install:
        installer(interp)
    # FD1-CAPTURE: `print`/`println` write to real fd 1, so bridge it to a custom
    # `out` sink for the whole run (module init + `main`); a no-op for the default.
    with interp._capture_stdout():
        interp.run_module(module)
        if run_main:
            # A `Main.thx` entry is a *project*: it must export `main`, since the
            # runtime calls it from outside the module (design §4.1). Other entries
            # (single-file scripts) are run as-is, calling `main` if present.
            if os.path.basename(filename) == "Main.thx" and not interp.entry_exports_main():
                interp.error("Main.thx must export a `main` function", Span(1, 1))
            return interp.call_main(arguments)
        return VOID
