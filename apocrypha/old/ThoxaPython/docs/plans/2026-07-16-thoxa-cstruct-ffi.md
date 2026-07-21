# LF-B Typed C-Struct FFI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Thoxa typed C-ABI structs that cross the libc FFI by address, so `struct termios` (and any C struct) can be declared once and edited by field.

**Architecture:** A C struct is an ordinary markerless `type` aggregate whose fields are all ABI types; in the Python prototype it is backed by a synthesized `ctypes.Structure` (a `CStruct` runtime value) instead of the usual `dict`-backed `Instance`. It marshals to C as a `void*` to that backing block via a new `Address<T>` parameter type, reusing the exact pointer path `Bytes` already uses. Field/array access reuses the existing `.field` and `a[i]` machinery.

**Tech Stack:** Python 3.13 (stdlib only — `ctypes` is stdlib), the Thoxa lexer/parser/checker/interp/ffi modules, pytest.

## Global Constraints

- **Zero runtime dependencies** — stdlib only. `ctypes` is the sole FFI mechanism; do not add packages.
- **Checker and runtime must not drift** — the checker reads `ffi.MARSHALABLE_PARAMS`/`ffi.MARSHALABLE_RETURNS` for the boundary set; keep it the single source of truth.
- **Pointer type is `Address<T>`; fixed inline array field is `Array<T, N>`** — exact spellings, verbatim.
- **Markerless ABI structs:** a `type` is an *ABI struct* iff it has no constructor params, no methods, ≥1 field, and every field is a fixed-width scalar (`Int8/16/32/64`, `UInt8/16/32/64`, `Byte`, `Float32/64`) or `Array<scalar, N>`. Such a struct's fields are all **public** (a C struct has no private fields). Any other `type` stays a normal `dict`-backed `Instance`.
- **Native/host endianness; field writes truncate to the field's declared width on store** (delegated to ctypes).
- **Deferred — do NOT build (design §9):** the explicit `address()` operator, the `mut`-at-boundary enforcement, const/read-only pointers, struct value-copy, pointer returns, `sizeof`, pointer arithmetic, malloc/free, by-value struct args, nested/union/bitfield structs. Under implicit addressing a struct value passed to an `Address<T>` parameter is marshalled by address automatically — no explicit operator is shipped.
- **Test command:** `.venv/bin/python -m pytest -q`. Before Task 1, run it once and record the passing count as the baseline in the progress ledger. Every task must only *add* passing tests (no regressions).
- **Conformance auto-discovery:** dropping `spec/conformance/NAME.thx` (+ `NAME.out`) adds three passing cases at once (the execution / type-check / LSP-diagnostics glob gates). A conformance program is run through `run()` (which bypasses the checker) **and** must type-check clean and produce zero LSP diagnostics.

---

### Task 1: Const-generic integer type argument (`Array<Byte, 32>` parses)

**Files:**
- Modify: `thoxa/ast.py` (the `TypeRef` dataclass, lines 16-31)
- Modify: `thoxa/parser.py` (`parse_type`, lines 286-308)
- Test: `tests/test_cstruct_ffi.py` (new file)

**Interfaces:**
- Produces: `ast.TypeRef` gains `int_value: Optional[int] = None`. A numeric type argument parses to `TypeRef(name="", int_value=N)`. `Array<Byte, 32>` parses to `TypeRef("Array", [TypeRef("Byte"), TypeRef("", int_value=32)])`. Later tasks read the size via `type_ann.args[1].int_value`.

- [ ] **Step 1: Write the failing test**

Create `tests/test_cstruct_ffi.py`:

```python
"""Tests for LF-B typed C-struct FFI: fixed-array type args, ABI structs,
Address<T> marshalling, and the conformance round-trip."""

import pytest

from thoxa import ast
from thoxa.checker import check
from thoxa.errors import Source, ThoxaError
from thoxa.interp import run
from thoxa.lexer import Lexer
from thoxa.parser import Parser


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def parse(src):
    s = Source(src, "<test>")
    return Parser(Lexer(s).tokenize(), s).parse_program()


def _messages(src):
    return [d.message for d in check(src)]


def _field_type(src, type_index=0, field_index=0):
    td = parse(src).body[type_index]
    return td.members[field_index].type_ann


def test_fixed_array_type_arg_parses():
    ann = _field_type("type Buf {\n  mut cc: Array<Byte, 32>\n}")
    assert ann.name == "Array"
    assert len(ann.args) == 2
    assert ann.args[0].name == "Byte"
    assert ann.args[1].int_value == 32


def test_ordinary_generic_type_still_parses():
    ann = _field_type("type Box {\n  mut xs: Array<Int64>\n}")
    assert ann.name == "Array"
    assert len(ann.args) == 1
    assert ann.args[0].name == "Int64"
    assert ann.args[0].int_value is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: FAIL — `TypeRef.__init__() got an unexpected keyword argument 'int_value'` (and the parser rejects the `32`).

- [ ] **Step 3: Add `int_value` to `TypeRef`**

In `thoxa/ast.py`, add `int_value` as the **last** field of `TypeRef` (after `span`). It must go last so the existing positional call sites — `ast.TypeRef("Fn", args, ret, sp)` and `ast.TypeRef(name, args, None, sp)`, which pass `span` as the 4th positional argument — keep working unchanged. The whole dataclass becomes:

```python
@dataclass
class TypeRef:
    """A type expression, e.g. `Int64`, `Array<T>`, `May<String>`, `Fn(Int64)->Int64`."""

    name: str
    args: list["TypeRef"] = field(default_factory=list)
    ret: Optional["TypeRef"] = None  # for Fn(...) -> ret
    span: Optional[Span] = None
    int_value: Optional[int] = None  # a numeric type argument, e.g. the 32 in Array<Byte, 32>

    def __str__(self) -> str:
        if self.name == "Fn":
            a = ", ".join(str(x) for x in self.args)
            return f"Fn({a}) -> {self.ret}"
        if self.int_value is not None:
            return str(self.int_value)
        if self.args:
            return f"{self.name}<{', '.join(str(a) for a in self.args)}>"
        return self.name
```

- [ ] **Step 4: Accept an integer type argument in `parse_type`**

In `thoxa/parser.py`, replace the generic-argument loop inside `parse_type` (lines 308-315) so each argument may be a type OR an integer literal:

```python
        args = []
        if self.check(T.LT):
            self.advance()
            args.append(self._type_arg())
            while self.match(T.COMMA):
                args.append(self._type_arg())
            self._expect_gt("'>' to close type arguments")
        return ast.TypeRef(name, args, None, sp)
```

Add the helper method just below `parse_type`:

```python
    def _type_arg(self) -> ast.TypeRef:
        """One generic argument: a type, or an integer literal size (Array<Byte, 32>)."""
        if self.check(T.INT):
            tok = self.advance()
            return ast.TypeRef("", [], None, tok.span, int_value=tok.value[0])
        return self.parse_type()
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: PASS (both tests).

- [ ] **Step 6: Run the full suite (no regressions)**

Run: `.venv/bin/python -m pytest -q`
Expected: baseline count + 2, all passing.

- [ ] **Step 7: Commit**

```bash
git add thoxa/ast.py thoxa/parser.py tests/test_cstruct_ffi.py
git commit -m "parser+ast: const-generic integer type argument (Array<Byte, N>)"
```

---

### Task 2: ABI-struct classification + `ctypes.Structure` synthesis + `CStruct` value (ffi.py)

**Files:**
- Modify: `thoxa/ffi.py` (add `import ast`; new: `SCALAR_NAMES`, `abi_field_spec`, `abi_struct_specs`, `make_struct_type`, `is_c_array`, `CStruct`)
- Test: `tests/test_cstruct_ffi.py`

**Interfaces:**
- Consumes: `ast.TypeDef`, `ast.Field`, `ast.FunctionDef`, `TypeRef.int_value` (Task 1).
- Produces:
  - `ffi.abi_struct_specs(defn: ast.TypeDef) -> list[tuple[str, str, int | None]] | None` — field specs `(fname, elem_typename, count_or_None)` if `defn` is a pure ABI data struct, else `None`.
  - `ffi.make_struct_type(name: str, specs) -> type` — a cached `ctypes.Structure` subclass.
  - `ffi.CStruct(ctype, thoxa_type)` — a runtime C-struct value; `.type_name`, `.has_field(name)`, `.get(name)`, `.set(name, value)`, `._c` (the live `ctypes.Structure`), `._type` (the opaque Thoxa type, carrying `.name` and `.field_mut`).
  - `ffi.is_c_array(v) -> bool` — True for a `ctypes.Array` (a fixed-array field view).
  - `ffi.SCALAR_NAMES: frozenset[str]` — the ABI scalar type names.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_cstruct_ffi.py`:

```python
def test_make_struct_type_matches_termios_layout():
    import ctypes
    from thoxa import ffi
    specs = [
        ("cIflag", "UInt32", None), ("cOflag", "UInt32", None),
        ("cCflag", "UInt32", None), ("cLflag", "UInt32", None),
        ("cLine", "Byte", None), ("cCc", "Byte", 32),
        ("cIspeed", "UInt32", None), ("cOspeed", "UInt32", None),
    ]
    cls = ffi.make_struct_type("Termios", specs)
    assert ctypes.sizeof(cls) == 60
    assert cls.cIflag.offset == 0
    assert cls.cLflag.offset == 12
    assert cls.cLine.offset == 16
    assert cls.cCc.offset == 17
    assert cls.cIspeed.offset == 52
    assert cls.cOspeed.offset == 56
    # cached: same specs -> identical class object
    assert ffi.make_struct_type("Termios", specs) is cls


def test_abi_struct_specs_accepts_scalar_and_array_fields():
    from thoxa import ffi
    two = parse("type Two {\n  mut a: UInt32\n  mut b: UInt32\n}").body[0]
    assert ffi.abi_struct_specs(two) == [("a", "UInt32", None), ("b", "UInt32", None)]
    buf = parse("type Buf {\n  mut cc: Array<Byte, 4>\n}").body[0]
    assert ffi.abi_struct_specs(buf) == [("cc", "Byte", 4)]


def test_abi_struct_specs_rejects_non_abi():
    from thoxa import ffi
    with_str = parse("type Bad {\n  name: String\n}").body[0]
    assert ffi.abi_struct_specs(with_str) is None
    with_method = parse("type M {\n  mut a: UInt32\n  fn f() -> Int64 { 0 }\n}").body[0]
    assert ffi.abi_struct_specs(with_method) is None
    with_ctor = parse("type C(fd: Int32) {\n  mut a: UInt32\n}").body[0]
    assert ffi.abi_struct_specs(with_ctor) is None
    empty = parse("type E { }").body[0]
    assert ffi.abi_struct_specs(empty) is None


def test_cstruct_value_get_set_and_addressing():
    from thoxa import ffi

    class _FakeType:  # stands in for a ThoxaType (only .name/.field_mut are used)
        name = "Two"
        field_mut = {"a": True, "b": True}

    cls = ffi.make_struct_type("Two", [("a", "UInt32", None), ("b", "UInt32", None)])
    v = ffi.CStruct(cls, _FakeType())
    assert v.get("a") == 0 and v.get("b") == 0           # zero-initialized
    v.set("a", 0xDEADBEEF)
    assert v.get("a") == 0xDEADBEEF
    assert v.has_field("a") and not v.has_field("z")
    assert v.type_name == "Two"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: FAIL — `module 'thoxa.ffi' has no attribute 'abi_struct_specs'` etc.

- [ ] **Step 3: Add the classifier, layout synthesis, and `CStruct` to `ffi.py`**

At the top of `thoxa/ffi.py`, add the ast import next to the existing imports (after `import os`):

```python
from . import ast
```

Add `SCALAR_NAMES` right after the `_SCALAR` dict (near line 30):

```python
# The ABI scalar type names (the keys of _SCALAR), as a set for membership tests.
SCALAR_NAMES = frozenset(_SCALAR)
```

Add the following near the `Bytes` class (e.g. just after `Bytes`, around line 137):

```python
def abi_field_spec(ann):
    """(elem_typename, count | None) if `ann` is an ABI field type — a fixed-width
    scalar, or Array<scalar, N> — else None. count None means a scalar field."""
    if ann is None:
        return None
    if ann.name in _SCALAR:
        return (ann.name, None)
    if ann.name == "Array" and len(ann.args) == 2:
        elem, size = ann.args[0], ann.args[1]
        if elem.name in _SCALAR and size.int_value is not None and size.int_value >= 0:
            return (elem.name, size.int_value)
    return None


def abi_struct_specs(defn: ast.TypeDef):
    """Field specs [(fname, elem_typename, count | None)] if `defn` is a pure ABI
    data struct — no constructor params, no methods, at least one field, every field
    an ABI type — else None. (A valued type or unit type has no fields → None.)"""
    if defn.ctor_params:
        return None
    specs = []
    for m in defn.members:
        if isinstance(m, ast.Field):
            fs = abi_field_spec(m.type_ann)
            if fs is None:
                return None
            specs.append((m.name, fs[0], fs[1]))
        else:  # a method (FunctionDef) disqualifies the struct from ABI layout
            return None
    return specs if specs else None


_struct_cache: dict = {}


def make_struct_type(name: str, specs):
    """A cached ctypes.Structure subclass for an ABI struct. `specs` is a list of
    (fname, elem_typename, count | None); count None = scalar, else a fixed array.
    ctypes computes offsets, alignment, and tail padding to match the platform ABI."""
    key = (name, tuple(specs))
    cls = _struct_cache.get(key)
    if cls is not None:
        return cls
    cfields = []
    for fname, elem, count in specs:
        base = _SCALAR[elem]
        cfields.append((fname, base if count is None else base * count))
    cls = type(name, (ctypes.Structure,), {"_fields_": cfields})
    _struct_cache[key] = cls
    return cls


def is_c_array(v) -> bool:
    """True for a ctypes fixed-array (what a struct's Array<T, N> field reads as)."""
    return isinstance(v, ctypes.Array)


class CStruct:
    """A C-ABI struct value: a zero-initialized ctypes.Structure the FFI addresses.

    Field get/set and fixed-array element get/set route through ctypes, which
    enforces per-field offset, native endianness, and width truncation on store.
    `_type` is the opaque Thoxa type (only its `.name`/`.field_mut` are read)."""

    __slots__ = ("_c", "_type", "_names")

    def __init__(self, ctype, thoxa_type) -> None:
        self._c = ctype()  # ctypes zero-initializes
        self._type = thoxa_type
        self._names = frozenset(f[0] for f in ctype._fields_)

    @property
    def type_name(self) -> str:
        return self._type.name

    def has_field(self, name: str) -> bool:
        return name in self._names

    def get(self, name: str):
        return getattr(self._c, name)

    def set(self, name: str, value) -> None:
        setattr(self._c, name, value)

    def __repr__(self) -> str:
        return self._type.name
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: PASS (all Task 1 + Task 2 tests).

- [ ] **Step 5: Run the full suite**

Run: `.venv/bin/python -m pytest -q`
Expected: baseline + 6, all passing.

- [ ] **Step 6: Commit**

```bash
git add thoxa/ffi.py tests/test_cstruct_ffi.py
git commit -m "ffi: ABI-struct classifier, ctypes.Structure synthesis, CStruct value"
```

---

### Task 3: Language-level C structs (construction, field & array access) + checker acceptance

**Files:**
- Modify: `thoxa/interp.py` — `ThoxaType.__init__` (155-172), `construct` (1247-1271), `_member` (1387-1430), `eval_Index` (876-885), `eval_Assign` (694-730), `_type_name`
- Modify: `thoxa/checker.py` — `_collect` (179-214) pre-scan + `self.abi_structs`, `_member` (952-986) privacy exemption
- Test: `tests/test_cstruct_ffi.py`

**Interfaces:**
- Consumes: `ffi.abi_struct_specs`, `ffi.make_struct_type`, `ffi.CStruct`, `ffi.is_c_array` (Task 2).
- Produces: `ThoxaType.abi: bool` and `ThoxaType.abi_ctype` (the `ctypes.Structure` subclass or `None`); `Checker.abi_structs: set[str]`. A constructed ABI struct is a `ffi.CStruct`; `t.field` reads/writes it, `t.arr[i]` reads/writes a fixed-array element. Same-file access to an ABI struct's (public) fields is checker-clean.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_cstruct_ffi.py`:

```python
def test_cstruct_construct_zeroed_and_field_io():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        "fn main() -> Int64 {\n"
        '  mut t = Two()\n'
        '  println("{t.a} {t.b}")\n'
        "  t.a = 16909060\n"
        "  t.b = t.a & 0xFFFF\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "0 0\n16909060 772\n"


def test_cstruct_fixed_array_element_io():
    src = (
        "type Buf {\n  mut cc: Array<Byte, 4>\n}\n"
        "fn main() -> Int64 {\n"
        "  mut b = Buf()\n"
        '  println("{b.cc[0]} {b.cc[3]}")\n'
        "  b.cc[0] = 65\n"
        "  b.cc[3] = 200\n"
        '  println("{b.cc[0]} {b.cc[3]}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "0 0\n65 200\n"


def test_cstruct_immutable_field_rejected_at_runtime():
    src = (
        "type P {\n  let x: UInt32\n}\n"
        "fn main() -> Int64 {\n  mut p = P()\n  p.x = 1\n  0\n}\n"
    )
    with pytest.raises(ThoxaError, match="not mutable"):
        run_out(src)


def test_checker_allows_abi_struct_field_access_same_file():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        "fn main() -> Int64 {\n"
        "  mut t = Two()\n"
        "  t.a = 5\n"
        "  t.b = t.a\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert _messages(src) == []
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -k cstruct_construct -q`
Expected: FAIL — the constructed value is a `dict`-backed `Instance` whose fields are `VOID`, so `t.a` prints `Void` / errors.

- [ ] **Step 3: Classify ABI structs on `ThoxaType`**

In `thoxa/interp.py`, at the end of `ThoxaType.__init__` (after `self.fallible = _ctor_is_fallible(defn)`), add:

```python
        specs = ffi.abi_struct_specs(defn)
        self.abi = specs is not None
        self.abi_ctype = ffi.make_struct_type(defn.name, specs) if self.abi else None
```

- [ ] **Step 4: Construct a `CStruct` for an ABI struct**

In `thoxa/interp.py`, at the very top of `construct` (before `inst = Instance(type_)`), add:

```python
        if type_.abi:
            if args:
                self.error(f"{type_.name}() takes no arguments (a C struct is zero-initialized)", span)
            return ffi.CStruct(type_.abi_ctype, type_)
```

- [ ] **Step 5: Read fields/array elements of a `CStruct`**

In `thoxa/interp.py` `_member`, add a `CStruct` branch (place it right after the `Instance` branch, before the `list` branch):

```python
        if isinstance(recv, ffi.CStruct):
            if not recv.has_field(name):
                self.error(f"'{recv.type_name}' has no field '{name}'", span)
            return recv.get(name)  # scalar -> int; fixed-array field -> a ctypes array view
```

In `eval_Index`, add a ctypes-array branch before the final `self.error("only arrays and strings can be indexed", ...)`:

```python
        if ffi.is_c_array(obj):
            if not isinstance(idx, int) or isinstance(idx, bool):
                self.error("array index must be an integer", node.span)
            if idx < 0 or idx >= len(obj):
                self.error(f"index {idx} out of bounds (length {len(obj)})", node.span)
            return obj[idx]
```

Add a `CStruct` case to `_type_name` (find the method; add near its other `isinstance` checks) so error text is clean:

```python
        if isinstance(value, ffi.CStruct):
            return value.type_name
```

- [ ] **Step 6: Write fields/array elements of a `CStruct`**

In `thoxa/interp.py` `eval_Assign`, replace the `elif isinstance(target, ast.FieldAccess):` branch (lines 720-728) with one that handles `CStruct` first:

```python
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
```

And replace the `elif isinstance(target, ast.Index):` branch (lines 711-719) to handle a ctypes array:

```python
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
```

- [ ] **Step 7: Teach the checker that ABI-struct fields are public**

In `thoxa/checker.py`, ensure the `Checker` has an `abi_structs` set. Add `self.abi_structs: set[str] = set()` to `Checker.__init__` (next to `self.type_members`/`self.type_kind`). Then at the very start of `_collect` (before the existing `for stmt in body:` loop), add a pre-scan:

```python
        for stmt in body:
            if isinstance(stmt, ast.TypeDef) and ffi.abi_struct_specs(stmt) is not None:
                self.abi_structs.add(stmt.name)
```

In `_member` (the `("named", ...)` branch), change the privacy check so ABI-struct members are never flagged private. Replace:

```python
            if name not in self.type_exported.get(objty[1], set()):
                self.err(f"member '{name}' of '{objty[1]}' is private", span)
```

with:

```python
            if objty[1] not in self.abi_structs and name not in self.type_exported.get(objty[1], set()):
                self.err(f"member '{name}' of '{objty[1]}' is private", span)
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: PASS (all tests through Task 3).

- [ ] **Step 9: Run the full suite**

Run: `.venv/bin/python -m pytest -q`
Expected: baseline + 10, all passing.

- [ ] **Step 10: Commit**

```bash
git add thoxa/interp.py thoxa/checker.py tests/test_cstruct_ffi.py
git commit -m "interp+checker: C-struct construction, field/array access, public ABI fields"
```

---

### Task 4: Cross the FFI — `Address<T>` marshalling + checker boundary gate

**Files:**
- Modify: `thoxa/ffi.py` — `MARSHALABLE_PARAMS` (line 31), `_arg_ctype` (324-331), `_to_c` (342-369), `_typename` (372-385)
- Modify: `thoxa/checker.py` — `_check_marshalable` (235-247)
- Test: `tests/test_cstruct_ffi.py`

**Interfaces:**
- Consumes: `ffi.CStruct` (Task 2), `Checker.abi_structs` (Task 3).
- Produces: an `Address<T>` extern parameter marshals a `CStruct` value as a `void*` to its backing block (by implicit addressing — pass the struct directly). The checker accepts `Address<AbiStruct>` params and rejects `Address<non-ABI>` and `Address<T>` returns. No `interp.py` change is needed: `declare_extern` already derives the param type name `"Address"` from `p.type_ann.name`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_cstruct_ffi.py`:

```python
def test_cstruct_roundtrips_through_libc():
    # C reads/writes the struct in place through its address (implicit addressing).
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memcpy(dst!: Address<Two>, src!: Address<Two>, n!: UInt64) from "c"\n'
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 {\n"
        "  mut t = Two()\n"
        "  t.a = 16909060\n"
        "  t.b = 168496141\n"
        "  mut u = Two()\n"
        "  memcpy(u, t, 8)\n"
        '  println("{u.a} {u.b}")\n'
        "  memset(t, 0, 8)\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "16909060 168496141\n0 0\n"


def test_checker_accepts_address_of_abi_struct():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    assert _messages(src) == []


def test_checker_rejects_address_of_non_abi_struct():
    src = (
        "type Bad {\n  let name: String\n}\n"
        'extern fn f(p!: Address<Bad>) from "c" "memset"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    msgs = _messages(src)
    assert any("Bad" in m and "ABI" in m for m in msgs), msgs


def test_checker_rejects_address_return():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn f() -> Address<Two> from "c" "malloc"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    msgs = _messages(src)
    assert any("Address" in m and ("return" in m or "pointer" in m) for m in msgs), msgs


def test_address_param_rejects_non_struct_value_at_runtime():
    # A clean FFIError, not a raw pointer deref, when a non-struct reaches the boundary.
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 {\n  memset(5, 0, 8)\n  0\n}\n"
    )
    with pytest.raises(ThoxaError, match="C struct"):
        run_out(src)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -k address -q`
Expected: FAIL — `'Address' cannot cross the C FFI as a parameter type` from the checker, and `type 'Address' cannot be an FFI parameter` from ffi.

- [ ] **Step 3: Marshal `Address<T>` in `ffi.py`**

Add `"Address"` to `MARSHALABLE_PARAMS` (line 31):

```python
MARSHALABLE_PARAMS = frozenset(_SCALAR) | {"String", "Bytes", "Address"}
```

In `_arg_ctype`, add an `Address` case before the final `raise`:

```python
    if tyname == "Address":
        return ctypes.c_void_p
```

In `_to_c`, add an `Address` case (place it right after the `Buffer` rejection, before the `String` case):

```python
    if tyname == "Address":
        if not isinstance(val, CStruct):
            raise FFIError(f"expected a C struct for an Address parameter, got {_typename(val)}")
        return ctypes.addressof(val._c)
```

In `_typename`, add a `CStruct` case (before the final `return type(val).__name__`):

```python
    if isinstance(val, CStruct):
        return val._type.name
```

- [ ] **Step 4: Gate `Address<T>` in the checker**

In `thoxa/checker.py`, replace `_check_marshalable` (lines 235-247) with a version that special-cases `Address` before the generic membership check:

```python
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_cstruct_ffi.py -q`
Expected: PASS (all tests through Task 4).

- [ ] **Step 6: Run the full suite**

Run: `.venv/bin/python -m pytest -q`
Expected: baseline + 15, all passing.

- [ ] **Step 7: Commit**

```bash
git add thoxa/ffi.py thoxa/checker.py tests/test_cstruct_ffi.py
git commit -m "ffi+checker: Address<T> marshals a C struct by pointer across the FFI"
```

---

### Task 5: Conformance program + SPEC.md

**Files:**
- Create: `spec/conformance/cstruct_ffi.thx`
- Create: `spec/conformance/cstruct_ffi.out`
- Modify: `spec/SPEC.md` (add a C-struct-FFI section)
- Test: (auto-discovered) `tests/test_conformance.py`, `tests/test_checker.py`, `tests/test_lsp.py`

**Interfaces:**
- Consumes: everything from Tasks 1-4.
- Produces: a self-contained, deterministic conformance program exercising zero-init, typed `UInt32` field read/write, a bitwise (LF-A) field edit, fixed `Array<Byte, N>` element I/O, and a `memcpy`/`memset` round-trip by address — plus SPEC documentation.

- [ ] **Step 1: Write the conformance program**

Create `spec/conformance/cstruct_ffi.thx`:

```thoxa
## C-struct FFI: zero-init, typed UInt32 fields, a bitwise (LF-A) field edit, a
## fixed Array<Byte, N> field, and a round-trip through libc memcpy/memset by
## address. Self-contained and deterministic — no terminal needed.
type Regs {
    mut a: UInt32
    mut b: UInt32
    mut tag: Byte
    mut cc: Array<Byte, 4>
}

extern fn memcpy(dst!: Address<Regs>, src!: Address<Regs>, n!: UInt64) from "c"
extern fn memset(p!: Address<Regs>, c!: Int32, n!: UInt64) from "c"

fn main() -> Int64 {
    mut r = Regs()
    println("{r.a} {r.b} {r.tag} {r.cc[0]}")
    r.a = 0x01020304
    r.b = r.a & 0xFFFF
    r.tag = 200
    r.cc[0] = 65
    r.cc[3] = 90
    println("{r.a} {r.b} {r.tag} {r.cc[0]} {r.cc[3]}")

    mut s = Regs()
    memcpy(s, r, 16)
    println("{s.a} {s.b} {s.tag} {s.cc[0]} {s.cc[3]}")

    memset(r, 0, 16)
    println("{r.a} {r.b} {r.cc[0]}")

    memset(s, 67, 16)
    println("{s.cc[0]} {s.cc[3]} {s.tag}")
    0
}
```

- [ ] **Step 2: Write the expected output**

Create `spec/conformance/cstruct_ffi.out` with EXACTLY these bytes (note the trailing newline; `sizeof(Regs)` is 16 = 4+4+1+padding, and `0x01020304 & 0xFFFF = 0x0304 = 772`):

```
0 0 0 0
16909060 772 200 65 90
16909060 772 200 65 90
0 0 0
67 67 67
```

- [ ] **Step 3: Verify the conformance program runs and matches**

Run: `.venv/bin/python -m pytest "tests/test_conformance.py" -k cstruct_ffi -q`
Expected: PASS. If output differs, print it directly to confirm the bytes:
Run: `.venv/bin/python -c "from thoxa.interp import run; b=[]; run(open('spec/conformance/cstruct_ffi.thx').read(),'cstruct_ffi.thx',out=b.append); print(repr(''.join(b)))"`
Expected: `'0 0 0 0\n16909060 772 200 65 90\n16909060 772 200 65 90\n0 0 0\n67 67 67\n'`

- [ ] **Step 4: Verify the type-check and LSP gates pass for the new program**

Run: `.venv/bin/python -m pytest tests/test_checker.py tests/test_lsp.py -q`
Expected: PASS — the conformance-program type-check and no-diagnostics gates now include `cstruct_ffi.thx` and are clean.

- [ ] **Step 5: Document in SPEC.md**

In `spec/SPEC.md`, add a new section documenting the feature. Place it after the FFI section (search for the extern/`from "c"` documentation). Use this content:

```markdown
### Typed C structs and `Address<T>`

A `type` whose fields are all fixed-width scalars (`Int8`…`Int64`, `UInt8`…`UInt64`,
`Byte`, `Float32/64`) or fixed inline arrays `Array<T, N>`, with no constructor
params and no methods, is a **C-ABI struct**: it is laid out in declaration order
with natural C alignment, and its fields are all public.

```
type Termios {
    mut cIflag: UInt32
    mut cLflag: UInt32
    let cLine:   Byte
    mut cCc: Array<Byte, 32>   // fixed inline array of 32 bytes
    let cIspeed: UInt32
}
```

`Termios()` constructs a zero-initialized value. Read/write fields with `t.cLflag`
and `t.cLflag = v` (a write truncates `v` to the field's width, native byte order);
read/write a fixed-array element with `t.cCc[i]` / `t.cCc[i] = b`.

An `Address<T>` is a pointer to a C-ABI struct `T`, used in `extern fn`
signatures. Passing a struct value to an `Address<T>` parameter takes its address
implicitly, so C can read or write it in place:

```
extern fn tcgetattr(fd!: Int32, p!: Address<Termios>) -> Int32 from "c"
mut raw = Termios()
tcgetattr(0, raw)   // the kernel fills raw
```

Deferred: an explicit address-of operator, const/read-only pointers, struct
value-copy, pointer returns, and `sizeof`.
```

- [ ] **Step 6: Run the full suite (final)**

Run: `.venv/bin/python -m pytest -q`
Expected: baseline + 18 (15 unit tests + 3 conformance glob-gate cases), all passing.

- [ ] **Step 7: Commit**

```bash
git add spec/conformance/cstruct_ffi.thx spec/conformance/cstruct_ffi.out spec/SPEC.md
git commit -m "conformance+spec: typed C-struct FFI (Address<T>, fixed arrays, round-trip)"
```

---

## Notes for the executor

- **Deferred items are out of scope** (Global Constraints). If a task tempts you to add the `address()` operator, `mut`-at-boundary enforcement, const pointers, or `sizeof`, stop — they are the named later pass (design §9).
- **No `interp.py` change in Task 4** by design: implicit addressing routes a `CStruct` value straight through `declare_extern` → `_to_c("Address", ...)`. If you find yourself editing `declare_extern`, re-read Task 4's Interfaces block.
- **`ctypes` truncation:** assigning an out-of-range int to a `c_uintN` struct field wraps modulo 2^N (the intended width truncation); a non-int raises `TypeError`, caught and reported as a located error.
