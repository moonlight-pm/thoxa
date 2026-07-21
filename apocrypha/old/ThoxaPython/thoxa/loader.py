"""Import resolution and a shared parse cache (design §4.1).

Turns a ``from "…"`` specifier into a concrete target — a real file on disk or a
built-in pseudo-module — and parses each resolved file at most once. Both the
interpreter and the checker load through here so they agree on what a specifier
means. Resolution is pure string/path math with no side effects; only
``module_ast`` touches the filesystem.

A specifier's leading form names one of three origins:

- ``./…`` (or ``../…``) — local, relative to the *importing file's* directory. A
  ``..`` segment anywhere is a compile error — you never climb.
- ``~/…`` — from the project root, wherever the importing file sits.
- a bare first segment — a *named root*: ``std/…`` (the bundled standard library)
  or ``ffi`` (the built-in FFI pseudo-module). Anything else is unknown.

The ``.thx`` extension is implicit; specifiers never carry it.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field

from .errors import Source, Span

# The bare first segments that name a root you cannot walk to on disk.
NAMED_ROOTS = ("std", "ffi")

# Built-in pseudo-modules — named roots that resolve to interpreter logic, not a file.
BUILTIN_ROOTS = ("ffi",)

# The bundled standard library lives next to this module: `std/<x>` → `std/<x>.thx`.
STD_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "std")

# The prelude is a real `std/Prelude` module auto-imported into every file (§4.2).
PRELUDE_PATH = os.path.join(STD_DIR, "Prelude.thx")


def load_prelude_source() -> Source:
    """The auto-imported prelude as a `Source` (design §4.2).

    The prelude is an ordinary `std/Prelude` module read from disk; both the
    interpreter and the checker load it through here so they agree on its content.
    """
    with open(PRELUDE_PATH, encoding="utf-8") as f:
        return Source(f.read(), "std/Prelude")


_PRELUDE_PARSED = None  # cached (Module, Source); see parse_prelude()


def parse_prelude():
    """Parse the prelude once per process and cache the ``(Module, Source)``.

    The prelude file is fixed, and every consumer — the checker's ``_collect`` /
    ``_check_decl_bodies`` and the resolver's module seed — treats the AST as
    read-only (none of them mutate nodes), so one shared parse is safe. It saves
    re-lexing/re-parsing the prelude on every check, resolve, and cross-module
    hover, where it was previously reparsed two-plus times per call.
    """
    global _PRELUDE_PARSED
    if _PRELUDE_PARSED is None:
        from .parser import parse
        src = load_prelude_source()
        _PRELUDE_PARSED = (parse(src.text, src.filename), src)
    return _PRELUDE_PARSED


@dataclass
class Resolved:
    """A resolved import target.

    ``kind`` is ``"file"`` for a real ``.thx`` on disk (``path`` set) or
    ``"builtin"`` for a pseudo-module such as ``ffi`` (``path`` is None). ``key``
    is the canonical registry key: the absolute path for files, the root name for
    builtins.
    """

    kind: str
    key: str
    path: str | None = None


class LoaderError(Exception):
    """A located resolution/parse failure.

    Carries the message + span so the caller can re-raise it as its own diagnostic
    against the importing file's source (the loader is source-agnostic).
    """

    def __init__(self, message: str, span: Span, notes=None) -> None:
        self.message = message
        self.span = span
        self.notes = notes or []
        super().__init__(message)


def display_name(key: str) -> str:
    """A short, human-friendly name for a module key, for cycle diagnostics."""
    base = os.path.basename(key)
    if base.endswith(".thx"):
        base = base[:-4]
    return base or key


def cycle_message(keys) -> str:
    """The shared wording for an import-cycle error (interpreter and checker)."""
    return "import cycle detected: " + " -> ".join(display_name(k) for k in keys)


def value_cycle_message(keys) -> str:
    """The shared wording for a value-initializer cycle error (design §7)."""
    return "value-initializer cycle: " + " -> ".join(display_name(k) for k in keys)


def module_name(spec: str) -> str:
    """A module's own name: the last path segment of its specifier."""
    return spec.rsplit("/", 1)[-1]


def is_pascal_case(name: str) -> bool:
    """First character is uppercase — the lexer's PascalCase (type) rule."""
    return name[:1].isupper()


def is_camel_case(name: str) -> bool:
    """First character is lowercase — the lexer's camelCase (value) rule."""
    return name[:1].islower()


def pascal_module_error(spec: str) -> str:
    """Shared wording when a file module's name isn't PascalCase (design §2.1).

    A module file describes a *type*, so its name is `PascalCase`; the import that
    binds it is a *singleton instance*, and that binding is the `camelCase` name.
    """
    name = module_name(spec)
    suggest = name[:1].upper() + name[1:]
    return (
        f"a module name must be PascalCase; '{name}' is a value name — a module file "
        f"describes a type, so rename the module to '{suggest}'"
    )


class Loader:
    """Resolves specifiers against a project root and caches parsed modules."""

    def __init__(self, project_root: str | None = None) -> None:
        self.project_root = project_root
        self._modules: dict[str, object] = {}   # abs path -> ast.Module
        self._sources: dict[str, Source] = {}   # abs path -> Source

    def resolve(self, spec: str, importer_dir: str | None, span: Span) -> Resolved:
        """Map a specifier to a target, or raise ``LoaderError``."""
        if spec.startswith("."):
            return self._resolve_local(spec, importer_dir, span)
        if spec.startswith("~/") or spec == "~":
            return self._resolve_project(spec, span)
        return self._resolve_root(spec, span)

    def _resolve_local(self, spec: str, importer_dir: str | None, span: Span) -> Resolved:
        if importer_dir is None:
            raise LoaderError(
                f'cannot resolve local import "{spec}" without a source file', span
            )
        rel = spec[2:] if spec.startswith("./") else spec
        # An absolute `rel` (a `//` in the specifier makes it start with `/`) would
        # make `os.path.join` discard the base and escape; reject it alongside `..`.
        if spec.startswith("..") or os.path.isabs(rel) or ".." in rel.split("/"):
            raise LoaderError(
                'upward imports aren\'t allowed — '
                'address it from the project root with "~/…"',
                span,
            )
        base_abs = os.path.abspath(importer_dir)
        path = os.path.abspath(os.path.normpath(os.path.join(base_abs, rel)) + ".thx")
        # Belt-and-suspenders: the resolved file must stay inside the importer's dir.
        if os.path.commonpath([base_abs, path]) != base_abs:
            raise LoaderError("import escapes its root", span)
        return Resolved("file", path, path)

    def _resolve_project(self, spec: str, span: Span) -> Resolved:
        if self.project_root is None:
            raise LoaderError(
                f'cannot resolve project import "{spec}" without a project root', span
            )
        rel = spec[2:]
        # Reject an absolute `rel` (from a `//`) as well as `..` — either escapes.
        if os.path.isabs(rel) or ".." in rel.split("/"):
            raise LoaderError(
                'upward imports aren\'t allowed — '
                'address it from the project root with "~/…"',
                span,
            )
        base_abs = os.path.abspath(self.project_root)
        path = os.path.abspath(
            os.path.normpath(os.path.join(base_abs, rel)) + ".thx"
        )
        if os.path.commonpath([base_abs, path]) != base_abs:
            raise LoaderError("import escapes its root", span)
        return Resolved("file", path, path)

    def _resolve_root(self, spec: str, span: Span) -> Resolved:
        first = spec.split("/", 1)[0]
        if first == "ffi":
            # `ffi` is a single pseudo-module, not a package: a subpath is a mistake.
            if "/" in spec:
                raise LoaderError(
                    '"ffi" is a module, not a package — import its members directly',
                    span,
                )
            return Resolved("builtin", "ffi")
        if first == "std":
            rel = spec[len("std/"):] if "/" in spec else ""
            if not rel:
                raise LoaderError(f'"{spec}" names no module inside "std"', span)
            # An absolute `rel` (from a `//`) or a `..` segment would escape STD_DIR.
            if os.path.isabs(rel) or ".." in rel.split("/"):
                raise LoaderError(
                    'upward imports aren\'t allowed — '
                    'address it from the project root with "~/…"',
                    span,
                )
            base_abs = os.path.abspath(STD_DIR)
            path = os.path.abspath(os.path.normpath(os.path.join(base_abs, rel)) + ".thx")
            if os.path.commonpath([base_abs, path]) != base_abs:
                raise LoaderError("import escapes its root", span)
            return Resolved("file", path, path)
        raise LoaderError(
            f'unknown module root "{first}"; the named roots are "std" and "ffi"',
            span,
        )

    def module_ast(self, resolved: Resolved, spec: str, span: Span):
        """Parse (once) the file behind ``resolved``; returns ``(Module, Source)``."""
        from .lexer import Lexer
        from .parser import Parser

        path = resolved.path
        if path in self._modules:
            return self._modules[path], self._sources[path]
        try:
            with open(path, encoding="utf-8") as f:
                text = f.read()
        except (OSError, ValueError):
            # ValueError covers an embedded NUL byte in the path — `open` raises
            # it instead of an OSError, so it would otherwise escape as a crash.
            raise LoaderError(
                f'cannot find module "{spec}" (looked for {path})', span
            )
        source = Source(text, path)
        module = Parser(Lexer(source).tokenize(), source).parse_program()
        self._modules[path] = module
        self._sources[path] = source
        return module, source
