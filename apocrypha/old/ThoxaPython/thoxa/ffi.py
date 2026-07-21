"""The libc FFI bridge for Thoxa.

Zero external dependencies: the bridge is Python's stdlib ``ctypes``. An
``extern fn`` declaration becomes a callable that marshals Thoxa values to and
from C; ``Bytes`` is a mutable byte buffer that C functions write into. This
module owns the single source of truth for what may cross the boundary
(:data:`MARSHALABLE_PARAMS` / :data:`MARSHALABLE_RETURNS`), which the static
checker also consults so its errors can never drift from the runtime.

See ``docs/specs/2026-07-12-thoxa-libc-ffi-design.md``.
"""

from __future__ import annotations

import ctypes
import os

from . import ast

# Thoxa sized numeric types → the ctypes scalar they marshal as.
_SCALAR = {
    "Int8": ctypes.c_int8, "Int16": ctypes.c_int16,
    "Int32": ctypes.c_int32, "Int64": ctypes.c_int64,
    "UInt8": ctypes.c_uint8, "UInt16": ctypes.c_uint16,
    "UInt32": ctypes.c_uint32, "UInt64": ctypes.c_uint64,
    "Byte": ctypes.c_uint8,
    "Float32": ctypes.c_float, "Float64": ctypes.c_double,
}

# The ABI scalar type names (the keys of _SCALAR), as a set for membership tests.
SCALAR_NAMES = frozenset(_SCALAR)

# Type names permitted at the boundary. `String` is a read-only `const char*`
# argument; `Bytes` is a mutable buffer. Neither may be *returned* (that is a
# pointer return — deferred with Ptr[T]); returns are scalars or Void.
MARSHALABLE_PARAMS = frozenset(_SCALAR) | {"String", "Bytes", "Address"}
MARSHALABLE_RETURNS = frozenset(_SCALAR) | {"Void"}

# Inclusive value ranges per integer type — ctypes silently masks out-of-range
# values for the narrow widths (and raises an unlocated OverflowError for the
# 64-bit ones), so we range-check before marshaling and report a clean error.
_INT_RANGES = {
    "Int8": (-128, 127), "Int16": (-32768, 32767),
    "Int32": (-(2 ** 31), 2 ** 31 - 1), "Int64": (-(2 ** 63), 2 ** 63 - 1),
    "UInt8": (0, 255), "UInt16": (0, 65535),
    "UInt32": (0, 2 ** 32 - 1), "UInt64": (0, 2 ** 64 - 1),
    "Byte": (0, 255),
}
_FLOAT_NAMES = frozenset({"Float32", "Float64"})

# The FFI helpers exposed by the built-in `"ffi"` pseudo-module.
FFI_EXPORTS = ("errno", "strerror")

# Libraries an `extern fn` may bind to. v1: just the process's own libc.
SUPPORTED_LIBRARIES = frozenset({"c"})


class FFIError(Exception):
    """A boundary problem (unknown symbol, unmarshalable type, bad library).

    The interpreter catches this and re-raises it as a located Thoxa error.
    """


_libc = None


def _library(name: str):
    """Return the ctypes handle for a library name, loaded lazily."""
    if name != "c":
        raise FFIError(f'unsupported FFI library "{name}"; only "c" is available in v1')
    global _libc
    if _libc is None:
        _libc = ctypes.CDLL(None, use_errno=True)
    return _libc


# ---------------------------------------------------------------- Bytes buffer
class Bytes:
    """A mutable, fixed-size byte buffer — what externs pass pointers to.

    ``count`` is the logical length in bytes; :meth:`from_string` buffers are
    additionally NUL-terminated (one byte past ``count``) so they work as C
    strings, but that terminator is not part of ``count`` and cannot be indexed.
    """

    __slots__ = ("_buf", "_len")

    def __init__(self, size: int) -> None:
        size = int(size)
        if size < 0:
            raise ValueError("Bytes size must be non-negative")
        self._buf = ctypes.create_string_buffer(size)  # zero-filled, `size` bytes
        self._len = size

    @classmethod
    def from_string(cls, s: str) -> "Bytes":
        data = s.encode("utf-8")
        obj = cls.__new__(cls)
        obj._buf = ctypes.create_string_buffer(data)  # appends a trailing NUL
        obj._len = len(data)
        return obj

    @classmethod
    def from_bytes(cls, data: bytes) -> "Bytes":
        """A fixed Bytes holding exactly ``data`` — no implicit NUL, no reinterpretation.

        This is what :meth:`Buffer.freeze` hands back: a growable buffer's contents
        settled into a stable, FFI-safe allocation of exactly ``len(data)`` bytes.
        """
        data = bytes(data)
        n = len(data)
        obj = cls.__new__(cls)
        # size given explicitly, so embedded NULs are preserved (no C-string cutoff)
        obj._buf = ctypes.create_string_buffer(data or b"\x00", max(1, n))
        obj._len = n
        return obj

    @property
    def count(self) -> int:
        return self._len

    def get(self, i: int) -> int:
        i = int(i)
        if not 0 <= i < self._len:
            raise IndexError(i)
        return self._buf.raw[i]

    def set(self, i: int, b: int) -> None:
        i = int(i)
        if not 0 <= i < self._len:
            raise IndexError(i)
        self._buf[i] = bytes([int(b) & 0xFF])

    def to_string(self) -> str:
        raw = self._buf.raw[: self._len]
        nul = raw.find(b"\x00")
        if nul != -1:
            raw = raw[:nul]
        return raw.decode("utf-8", errors="replace")

    def slice(self, start: int, end: int) -> "Bytes":
        """A fresh, independently owned Bytes holding the half-open range [start, end).

        Copying, not a view — the result never aliases this buffer, so it can never
        dangle (consistent with the growable-Buffer COW design, which defers borrowing
        views). Strict bounds: 0 <= start <= end <= count, else IndexError.
        """
        start, end = int(start), int(end)
        if not (0 <= start <= end <= self._len):
            raise IndexError((start, end))
        return Bytes.from_bytes(self._buf.raw[start:end])

    def insert_at(self, at: int, b: int) -> "Bytes":
        """A fresh Bytes with byte `b` inserted at index `at` (0 <= at <= count).

        Copying, never a view — like `slice`, the result is independently owned, so
        it can never dangle. `b` is masked to a byte. Strict bounds, else IndexError.
        """
        at = int(at)
        if not (0 <= at <= self._len):
            raise IndexError(at)
        data = self._buf.raw[: self._len]
        return Bytes.from_bytes(data[:at] + bytes([int(b) & 0xFF]) + data[at:])

    def remove_at(self, at: int) -> "Bytes":
        """A fresh Bytes with the byte at index `at` removed (0 <= at < count)."""
        at = int(at)
        if not (0 <= at < self._len):
            raise IndexError(at)
        data = self._buf.raw[: self._len]
        return Bytes.from_bytes(data[:at] + data[at + 1 :])

    def find(self, needle: "Bytes") -> int:
        """First index where `needle` occurs, or -1 if absent (bytes.find semantics).

        Length-counted, so an embedded NUL never cuts the search short; an empty
        needle matches at 0. The Thoxa `Bytes.find` builtin lifts the -1 into `Not`.
        """
        hay = self._buf.raw[: self._len]
        ndl = needle._buf.raw[: needle._len]
        return hay.find(ndl)

    def equals(self, other: "Bytes") -> bool:
        """True iff `other` has the same length and bytes (length-counted)."""
        return self._buf.raw[: self._len] == other._buf.raw[: other._len]

    def __repr__(self) -> str:
        return f"Bytes({self._len})"


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


# ---------------------------------------------------------------- growable Buffer
class _Alloc:
    """One heap allocation, shared by one or more :class:`Buffer` handles.

    This is the object that carries the *reference count* and that a real backend
    would ``malloc`` / ``realloc`` / ``free``. ``store`` is a ``bytearray`` standing
    in for the raw block; ``cap`` is its size in bytes (what ``realloc`` would ask
    the allocator for); ``rc`` is how many live handles share it; ``reallocs`` counts
    how many times we have had to grow (observable, so a test can *see* the amortised
    geometric growth rather than trust it).
    """

    __slots__ = ("store", "cap", "rc", "reallocs")

    def __init__(self, cap: int) -> None:
        self.cap = cap
        self.store = bytearray(cap)  # `cap` zero bytes — our stand-in for one malloc'd block
        self.rc = 1
        self.reallocs = 0


class Buffer:
    """A growable, value-semantic byte buffer — the prototype's model of ``Vec<u8>``.

    WHY THIS SHAPE (the thought line, so we remember when we leave Python):

    A growable buffer is where Thoxa's central promise — "Rust-like safety without a
    borrow checker" — is first tested. The moment a buffer can *reallocate*, every raw
    pointer or alias taken before the growth can dangle. Wrapping Python's ``bytearray``
    would hide exactly the three things a systems language must get right, so we model
    them explicitly instead:

    1. ALLOCATION + GEOMETRIC GROWTH.  A :class:`Buffer` is a small *handle*
       ``{alloc, len}``; the bytes live in a separately reference-counted
       :class:`_Alloc` ``{store, cap, rc}``.  ``append`` doubles ``cap`` and copies on
       overflow (amortised O(1)), and every such move bumps ``_Alloc.reallocs``.  This
       mirrors ``struct { u8* ptr; usize len; usize cap; }`` + ``realloc``.

    2. IN-PLACE-IFF-UNIQUE, COPY-ON-WRITE-IF-SHARED (the Perceus/FBIP essence).  The
       handle is the unit of aliasing; the allocation is refcounted.  Binding a buffer
       from an existing owner ("``let a = b``") is a *dup*: a new handle over the SAME
       allocation, ``rc += 1`` (see :meth:`share`).  A mutation first calls
       :meth:`_ensure_unique`: if ``rc == 1`` it writes in place (garbage-free reuse);
       if ``rc > 1`` it copies its allocation out first, so aliases never observe the
       write.  This is precisely Swift's ``isKnownUniquelyReferenced`` + copy-on-write
       and what Koka/Lean get from Perceus statically — safety by construction, no
       borrow checker, because you never mutate shared memory.

    3. OWNERSHIP TRANSFER AT THE FFI BOUNDARY.  A growable buffer must NEVER be handed
       to C: a later ``append`` could ``realloc`` the block out from under a pointer C
       still holds.  So :meth:`freeze` *consumes* the buffer and returns a fixed
       :class:`Bytes` (stable pointer, FFI-safe) — the ``BytesMut -> Bytes`` handoff.
       Using a Buffer after ``freeze`` is a use-after-move error.  ``Buffer`` is
       deliberately absent from :data:`MARSHALABLE_PARAMS`, so the checker rejects it
       at the boundary; :func:`_to_c` is the runtime backstop.

    WHAT THE PROTOTYPE APPROXIMATES (and the real backend will sharpen):
      * ``dup`` is hooked at the language's binding sites (let / assign / call args);
        there is no precise ``drop`` at last-use yet, so ``rc`` only ever rises except
        where a copy-on-write releases a share.  The cost is purely *conservative*: a
        buffer whose alias has died may copy once when Perceus would have reused it in
        place.  It is never unsafe — we only ever copy too much, never too little.
      * ``freeze`` copies the live prefix; with a unique buffer the real backend
        transfers the block with zero copy.

    See ``docs/specs/2026-07-15-thoxa-growable-buffer-design.md``.
    """

    __slots__ = ("_alloc", "_len", "_consumed")

    _INITIAL_CAP = 8   # first non-empty allocation; grows geometrically from here
    _GROWTH = 2        # doubling → amortised O(1) append

    def __init__(self, capacity: int = 0) -> None:
        capacity = int(capacity)
        if capacity < 0:
            raise ValueError("Buffer capacity must be non-negative")
        self._alloc = _Alloc(capacity)
        self._len = 0
        self._consumed = False

    # -- refcount / copy-on-write: the Perceus stand-in --------------------------
    def share(self) -> "Buffer":
        """``dup``: a second handle over the same allocation (``rc += 1``).

        Called by the interpreter when a buffer flows from an existing owner into a
        new binding — the aliasing point. The two handles read as equal until one is
        mutated, at which point copy-on-write splits them.
        """
        h = Buffer.__new__(Buffer)
        h._alloc = self._alloc
        h._len = self._len
        h._consumed = False
        self._alloc.rc += 1
        return h

    def _ensure_unique(self) -> None:
        """Copy-on-write: before any mutation, if the allocation is shared, give this
        handle its own private copy so no alias observes the coming write."""
        a = self._alloc
        if a.rc > 1:
            fresh = _Alloc(max(a.cap, self._len))
            fresh.store[: self._len] = a.store[: self._len]
            a.rc -= 1
            self._alloc = fresh

    def _reserve_cap(self, needed: int) -> None:
        """Ensure the (already-unique) allocation holds at least ``needed`` bytes,
        growing geometrically. This is the ``realloc`` — a new, larger block plus a
        copy — which is exactly why a raw pointer into the old block would dangle."""
        a = self._alloc
        if needed <= a.cap:
            return
        new_cap = max(self._INITIAL_CAP, a.cap)
        while new_cap < needed:
            new_cap *= self._GROWTH
        grown = bytearray(new_cap)
        grown[: self._len] = a.store[: self._len]
        a.store = grown
        a.cap = new_cap
        a.reallocs += 1

    # -- language-facing operations ---------------------------------------------
    def append(self, b: int) -> None:
        self._ensure_unique()
        self._reserve_cap(self._len + 1)
        self._alloc.store[self._len] = int(b) & 0xFF
        self._len += 1

    def append_bytes(self, other: "Bytes") -> None:
        """Append every byte of a fixed :class:`Bytes` (this is our header+body concat)."""
        data = other._buf.raw[: other._len]
        self._ensure_unique()
        self._reserve_cap(self._len + len(data))
        self._alloc.store[self._len: self._len + len(data)] = data
        self._len += len(data)

    def reserve(self, extra: int) -> None:
        """Pre-grow so ``extra`` more bytes can be appended without a realloc."""
        extra = int(extra)
        if extra < 0:
            raise ValueError("Buffer.reserve expects a non-negative count")
        self._ensure_unique()
        self._reserve_cap(self._len + extra)

    def get(self, i: int) -> int:
        i = int(i)
        if not 0 <= i < self._len:
            raise IndexError(i)
        return self._alloc.store[i]

    def set(self, i: int, b: int) -> None:
        i = int(i)
        if not 0 <= i < self._len:
            raise IndexError(i)
        self._ensure_unique()
        self._alloc.store[i] = int(b) & 0xFF

    def freeze(self) -> "Bytes":
        """Consume the buffer and return its contents as a fixed, FFI-safe Bytes.

        Ownership transfer: after this the handle is spent (use-after-move traps). In
        the real backend a unique buffer hands over its block with no copy; here we
        copy the live prefix.
        """
        out = Bytes.from_bytes(bytes(self._alloc.store[: self._len]))
        self._alloc.rc -= 1
        self._consumed = True
        return out

    @property
    def count(self) -> int:
        return self._len

    @property
    def capacity(self) -> int:
        return self._alloc.cap

    def __repr__(self) -> str:
        state = "consumed" if self._consumed else f"count={self._len} cap={self._alloc.cap}"
        return f"Buffer({state})"


# ---------------------------------------------------------------- marshaling
def _arg_ctype(tyname: str):
    if tyname in _SCALAR:
        return _SCALAR[tyname]
    if tyname == "String":
        return ctypes.c_char_p
    if tyname == "Bytes":
        return ctypes.c_void_p
    if tyname == "Address":
        return ctypes.c_void_p
    raise FFIError(f"type {tyname!r} cannot be an FFI parameter")


def _ret_ctype(tyname):
    if tyname in (None, "Void"):
        return None
    if tyname in _SCALAR:
        return _SCALAR[tyname]
    raise FFIError(f"type {tyname!r} cannot be an FFI return type")


def _to_c(tyname: str, val):
    # The checker does not compare argument types to parameter types, so this is
    # the last line of defence: a mistyped value must become a clean FFIError,
    # never a raw pointer dereference or a silent width truncation.
    if isinstance(val, Buffer):
        raise FFIError(
            "a Buffer cannot cross the FFI; freeze() it to a Bytes first "
            "(a growable buffer may realloc and invalidate the pointer)"
        )
    if tyname == "Address":
        if not isinstance(val, CStruct):
            raise FFIError(f"expected a C struct for an Address parameter, got {_typename(val)}")
        return ctypes.addressof(val._c)
    if tyname == "String":
        if not isinstance(val, str):
            raise FFIError(f"expected a String for a String parameter, got {_typename(val)}")
        return val.encode("utf-8")
    if tyname == "Bytes":
        if not isinstance(val, Bytes):
            raise FFIError(f"expected a Bytes buffer, got {_typename(val)}")
        return ctypes.cast(val._buf, ctypes.c_void_p)
    if tyname in _FLOAT_NAMES:
        if isinstance(val, bool) or not isinstance(val, (int, float)):
            raise FFIError(f"expected a number for {tyname}, got {_typename(val)}")
        return float(val)
    # Integer scalar.
    if isinstance(val, bool) or not isinstance(val, int):
        raise FFIError(f"expected an integer for {tyname}, got {_typename(val)}")
    lo, hi = _INT_RANGES[tyname]
    if not lo <= val <= hi:
        raise FFIError(f"{val} is out of range for {tyname} [{lo}, {hi}]")
    return val


def _typename(val) -> str:
    if isinstance(val, bool):
        return "Bool"
    if isinstance(val, int):
        return "Int64"
    if isinstance(val, float):
        return "Float64"
    if isinstance(val, str):
        return "String"
    if isinstance(val, Bytes):
        return "Bytes"
    if isinstance(val, Buffer):
        return "Buffer"
    if isinstance(val, CStruct):
        return val._type.name
    return type(val).__name__  # scalar — ctypes coerces through the argtype


def make_extern(name: str, library: str, symbol, param_types, return_type):
    """Resolve a C symbol and return ``call(values) -> python value | None``.

    ``param_types`` is a list of Thoxa type-name strings; ``return_type`` is a
    type-name string or ``None`` (Void). Raises :class:`FFIError` on an unknown
    library/symbol or an unmarshalable signature.

    Each extern gets its *own* function pointer (via ``CFUNCTYPE``) rather than
    the library's cached attribute, so two externs binding the same C symbol
    with different signatures do not clobber each other's argtypes/restype.
    """
    lib = _library(library)
    cname = symbol or name
    argtypes = [_arg_ctype(t) for t in param_types]
    restype = _ret_ctype(return_type)
    prototype = ctypes.CFUNCTYPE(restype, *argtypes, use_errno=True)
    try:
        cfunc = prototype((cname, lib))
    except (AttributeError, OSError, ValueError):
        raise FFIError(f'no such symbol "{cname}" in library "{library}"')

    def call(values):
        if len(values) != len(param_types):
            raise FFIError(
                f"{cname} takes {len(param_types)} argument(s), got {len(values)}"
            )
        c_args = [_to_c(t, v) for t, v in zip(param_types, values)]
        return cfunc(*c_args)

    return call


# ---------------------------------------------------------------- error channel
def errno_value() -> int:
    """The captured errno from the most recent extern call."""
    return ctypes.get_errno()


def strerror_text(code: int) -> str:
    return os.strerror(int(code))
