# Thoxa Typed C-Struct FFI — Design (LF-B)

- **Date:** 2026-07-16
- **Status:** Approved for implementation (design calls locked with Joshua)
- **Context:** Language feature LF-B of the Kilo port (see `2026-07-16-kilo-port-roadmap.md`),
  fork #1. Chosen over a `Bytes`-hand-packing wrapper: *build the real primitive.* Uses LF-A's
  bitwise operators to edit flag fields.

## 1. Motivation

A terminal editor enters raw mode by round-tripping a `struct termios` through the kernel:

```c
struct termios raw;
tcgetattr(STDIN_FILENO, &raw);              // kernel writes current settings into raw
raw.c_iflag &= ~(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
raw.c_oflag &= ~(OPOST);
raw.c_cflag |=  (CS8);
raw.c_lflag &= ~(ECHO | ICANON | IEXTEN | ISIG);
raw.c_cc[VMIN]  = 0;
raw.c_cc[VTIME] = 1;
tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw);   // kernel reads raw back
```

Today Thoxa can only fake this with a hand-packed `Bytes(60)`: manual byte offsets, hand-coded
endianness, single-byte `get`/`set`, silent `& 0xFF` truncation, and no field names — exactly the
sockaddr_in hack in `examples/http/Net.thx`. The roadmap (fork #1) rejects a `cfmakeraw`-style
wrapper in favor of a real primitive: **a typed C struct you declare once and address by field.**

The whole raw-mode path is *reachable* today via that `Bytes` hack, so LF-B's value is not new
capability — it is faithfulness, typing, and ergonomics.

## 2. Concepts & surface syntax

Four ideas, each decided below:

### 2.1 A C struct is an ordinary Thoxa struct (no marker)

There is **one** struct concept. A C struct is a normal `type` aggregate; Thoxa gives every struct
**declaration-order, naturally-aligned (C-ABI) layout by default**, so a struct whose fields are all
ABI types is directly exposable to C — no `extern type`, no `repr(C)`, no annotation.

```thoxa
type Termios {
    mut cIflag:  UInt32       // offset 0
    mut cOflag:  UInt32       // 4
    mut cCflag:  UInt32       // 8
    mut cLflag:  UInt32       // 12
    let cLine:   Byte         // 16
    mut cCc:     Array<Byte, 32>   // 17  (NCCS = 32, fixed inline)
    let cIspeed: UInt32       // 52  (3 pad bytes at 49-51 inserted automatically)
    let cOspeed: UInt32       // 56
}   // total size 60, computed — never written by the author
```

Fields are declared `[mut] name: Type` (the existing `parse_field` grammar). `mut` marks the fields
kilo writes (`cIflag/cOflag/cCflag/cLflag/cCc`); the rest are plain. No constructor params, no
defaults.

*Cost of markerless, and its mitigation:* choosing C-ABI layout as the language-wide default
forecloses field-reordering layout optimization. That freedom is reclaimable later by a native
compiler pass that proves a struct is **never** used across the ABI and is therefore free to reorder
(see §9). Choosing markerless now costs nothing then.

*Prototype note (invisible to the language):* the Python tree-walker backs a normal struct with a
`dict`, but an **ABI struct** — one all of whose fields are ABI types (scalars or fixed ABI arrays) —
is backed by a synthesized `ctypes.Structure` so its offsets, padding, and alignment match the native
ABI exactly (ctypes computes the 3 termios pad bytes for free). This dict-vs-ctypes split is a
prototype artifact; in native Thoxa every struct is already a laid-out block.

### 2.2 Fixed inline array fields — `Array<T, N>`

A struct embeds a fixed array with a **sized generic**: `Array<Byte, 32>` — the existing `Array<>`
vocabulary plus a compile-time size argument (a *const generic argument*). Dynamic vs. fixed is
distinguished by the presence of the size arg: `Array<Byte>` is the heap-backed growable array;
`Array<Byte, 32>` is 32 inline elements. It lowers internally to a fixed array (`c_uint8 * 32` in the
prototype).

Fixed-array fields are **field-only** and accessed **element-wise**: `t.cCc[i]` reads a `Byte`,
`t.cCc[i] = x` writes one (reusing the existing `a[i]` / `a[i] = x` index machinery). Binding the
whole array to a value (`let a = t.cCc`) or slicing it is out of scope for LF-B.

### 2.3 Construction — `Termios()` zero-initializes

`Termios()` constructs a **zero-initialized** instance (a C struct's clean-slate default), matching
the existing `Socket(...)`/`File(...)` call-style construction. Fields are read/written afterward:
`t.field` read, `t.field = x` write (mut-gated), all reusing existing machinery unchanged.

### 2.4 Pointers — `Address<T>` type + `address()` operator, implicit at the boundary

The raw-pointer type is **`Address<T>`**; the explicit address-of operator is **`address(x)`**. Under
**implicit addressing**, you pass a struct directly to a pointer-typed extern parameter and the
compiler coerces it to `Address<T>` — the `&` is invisible at the call site. `address(x)` remains
available as the explicit escape hatch (rarely needed; kilo never writes it).

```thoxa
extern fn tcgetattr(fd!: Int32, p!: Address<Termios>) -> Int32 from "c"
extern fn tcsetattr(fd!: Int32, act!: Int32, p!: Address<Termios>) -> Int32 from "c"

mut raw = Termios()
tcgetattr(fd, raw)                 // implicit: raw coerces to Address<Termios>; kernel writes it
raw.cLflag = raw.cLflag & ~(echo | icanon | iexten | isig)   // LF-A bitwise, full rebind
tcsetattr(fd, tcsaflush, raw)
```

### 2.5 Mutation direction across the pointer (`mut` discipline)

There is a **single, always-mutable `Address<T>`**. Because implicit addressing hides the `&`, and C
may write through the pointer, **coercing a struct into an `Address<T>` parameter requires the
argument denote a `mut` place** — restoring the "mutation requires `mut`" signal that would otherwise
be lost. Passing a `mut` struct where C wants `const T *` is a safe widening. A `const`/read-only
pointer companion is **deferred** (§9): rigorous read/write-through safety is a later pass.

## 3. Layout & ABI

- **Layout rule:** fields are laid out in **declaration order** with **natural alignment** and C tail
  padding — identical to the platform C ABI. In the prototype this is delegated wholesale to
  `ctypes.Structure`, which reproduces the exact native layout (verified: `struct termios` = 60 bytes,
  fields at 0/4/8/12/16/17, pad at 49–51, speeds at 52/56 on x86-64 glibc).
- **Field types:** the ABI scalar set already marshalable at the boundary — `Int8/16/32/64`,
  `UInt8/16/32/64`, `Byte`, `Float32/64` — plus fixed `Array<T, N>` of those. termios uses `UInt32`
  (tcflag_t, speed_t) and `Byte` (cc_t, c_line). No `Bool`, `String`, dynamic `Array`, union, or
  nested-struct fields in an ABI struct (those keep a struct dict-backed and non-ABI).
- **Endianness:** native/host order (correct for termios and ioctl; ctypes uses host order).
- **`sizeof` is not needed:** the author never writes a size — construction sizes the block from the
  type, and `tcgetattr`/`tcsetattr` take only a pointer. `sizeof` is deferred (§9).

## 4. Runtime semantics

- **Zero-init:** `Termios()` yields an all-zero block.
- **Field read:** `t.cLflag` loads the field's typed value (a `UInt32` field reads as an integer).
- **Field write:** `t.cLflag = v` **truncates `v` to the field's declared width** on store — this is
  precisely the "width truncation happens at typed boundaries" rule LF-A deferred (bitwise design §4);
  a struct-field write **is** such a boundary. `raw.cLflag = raw.cLflag & ~mask` therefore stores the
  low 32 bits. (Reads of a live field returned by `tcgetattr` preserve the kernel's high bits, so the
  read-modify-write is faithful.)
- **Array element read/write:** `t.cCc[i]` / `t.cCc[i] = b`, single `Byte`, bounds-checked against `N`.
- **In-place C mutation:** passing a struct to a C function hands C a pointer to the *same* backing
  block; C's writes are visible to Thoxa afterward (the mechanism `test_memcpy_writes_into_buffer`
  already proves for `Bytes`).
- **No copy semantics:** LF-B adds **no** struct value-copy primitive. Where kilo's C does
  `raw = orig_termios`, Thoxa issues a second `tcgetattr` to reproduce the working state. Struct
  value/copy semantics are deferred until a real use forces them (§9).
- **`Address<T>` lifetime:** an `Address<T>` is FFI-scoped — produced at a call boundary, valid
  because the struct outlives the call. LF-B does **not** allow an `Address<T>` to be stored in a
  field or returned, and there is no borrow checker (Thoxa's stated ethos). Each ABI struct owns a
  stable, unique backing block, so addressing never dangles — unlike `Buffer`, whose realloc is
  exactly why it cannot cross the FFI.

## 5. Marshalling (how it plugs into `ffi.py`)

- **ABI-struct classification:** a `TypeDef` is an *ABI struct* iff every field resolves to an ABI type
  (scalar or fixed ABI array). This is a static property of the type — no marker — consistent with the
  markerless decision.
- **`ctypes.Structure` synthesis:** for each ABI struct, synthesize and cache a `ctypes.Structure`
  subclass whose `_fields_` are the declared fields in order (`UInt32 → c_uint32`, `Byte → c_uint8`,
  `Array<Byte, 32> → c_uint8 * 32`, …). ctypes computes offsets/padding/alignment.
- **Runtime value:** an ABI-struct instance owns a live `ctypes.Structure` instance. Field get/set and
  array-element get/set route through the ctypes fields (which enforce width/endianness/offset).
- **`Address<T>` marshalling:** a `Address<T>` parameter marshals as a pointer to the struct's backing
  (`ctypes.byref(value._c)`); `_arg_ctype` maps `Address<T>` to `ctypes.POINTER(StructT)`. The extern
  param representation must carry the **pointee type**, not just the head name (today `make_extern`
  keeps only `type_ann.name`; it must keep `Address`'s argument so it can build `POINTER(StructT)`).
- **Static gate:** `MARSHALABLE_PARAMS` gains `Address`; `_check_marshalable` recurses into the
  `Address` argument and requires the pointee be a declared ABI struct (later: also scalars). The
  checker keeps reading `ffi`'s frozensets so checker and runtime never drift.
- **Returns unchanged:** externs still return scalars or `Void`. Pointer returns stay deferred (§9);
  kilo needs none (`tcgetattr`/`tcsetattr` return `int`).

## 6. Implementation surface (hooks)

Concrete spots, from the current tree:

- **`thoxa/ast.py`** — `TypeRef` gains an optional integer type-argument (e.g. `int_value:
  Optional[int]`) so `Array<Byte, 32>` can carry `32`. No new node for the struct (reuse `TypeDef`);
  no new node for `address` (a builtin, §below).
- **`thoxa/parser.py`** — in the type-argument list inside `parse_type`, accept an `INT` literal as an
  argument (sets `TypeRef.int_value`). The struct declaration itself already parses via `type_decl` +
  `type_body` + `parse_field`; `Address<T>` already parses. No other parser change.
- **`thoxa/checker.py`** — classify ABI structs (all fields ABI); `_resolve` keeps `Array<T, N>` as
  `("array", elem)` (N ignored for typing — the existing array-index inference then types `t.cCc[i]`
  as its element for free) and gives `Address<T>` a retained rep; extend `_check_marshalable` to
  accept `Address` and recurse into its pointee; type field read/write of an ABI struct via the
  existing `type_members` member table.
- **`thoxa/interp.py`** — a runtime ABI-struct value (a `CStruct` wrapping the `ctypes.Structure`
  instance); construct it in the type-construction path when the type is an ABI struct (zero-init);
  handle it in `_member` (field get) and `eval_Assign`'s `FieldAccess` branch (field set, width-mask);
  handle indexing a fixed-array field in `eval_Index` and `eval_Assign`'s `Index` branch (the ctypes
  array); an `address()` builtin returning an `Address` value; implicit coercion + the `mut`-place
  requirement at the extern call site (the site has the AST argument and env mutability).
- **`thoxa/ffi.py`** — `ctypes.Structure` synthesis + cache; `_arg_ctype`/`_to_c` cases for
  `Address<T>` (→ `POINTER`/`byref`); carry the `Address` pointee through `make_extern`;
  `MARSHALABLE_PARAMS |= {"Address"}`.
- **`editor/` (tree-sitter/LSP)** — lexical grammar already tokenizes `<`, `>`, identifiers, and `[`
  is unused here; note a later grammar `rev` if it enumerates types. Semantic tokens/diagnostics reuse
  the frontend. Low priority.

## 7. Worked example — the kilo `Terminal` module (Stage 0, built on LF-B)

Illustrative (built in Stage 0, not in LF-B itself), showing the primitive carries the real use:

```thoxa
// private flag constants (Net.thx / Io.thx precedent): named for documentation
let echo = 0x8         let icanon = 0x2      let isig = 0x1        let iexten = 0x8000
let ixon = 0x400       let icrnl = 0x100     let brkint = 0x2      let inpck = 0x10
let istrip = 0x20      let opost = 0x1       let cs8 = 0x30
let vmin = 6           let vtime = 5         let tcsaflush = 2     let stdinFileno = 0

type Termios {
    mut cIflag: UInt32
    mut cOflag: UInt32
    mut cCflag: UInt32
    mut cLflag: UInt32
    let cLine: Byte
    mut cCc: Array<Byte, 32>
    let cIspeed: UInt32
    let cOspeed: UInt32
}

extern fn sysTcgetattr(fd!: Int32, p!: Address<Termios>) -> Int32 from "c" "tcgetattr"
extern fn sysTcsetattr(fd!: Int32, act!: Int32, p!: Address<Termios>) -> Int32 from "c" "tcsetattr"

fn enableRawMode() -> Trial<Termios, String> {
    mut orig = Termios()
    if sysTcgetattr(stdinFileno, orig) < 0 { return Fail("tcgetattr: {strerror(errno())}") }
    mut raw = Termios()
    sysTcgetattr(stdinFileno, raw)                 // second read == working copy (no struct-copy needed)
    raw.cIflag = raw.cIflag & ~(brkint | icrnl | inpck | istrip | ixon)
    raw.cOflag = raw.cOflag & ~opost
    raw.cCflag = raw.cCflag | cs8
    raw.cLflag = raw.cLflag & ~(echo | icanon | iexten | isig)
    raw.cCc[vmin]  = 0
    raw.cCc[vtime] = 1
    if sysTcsetattr(stdinFileno, tcsaflush, raw) < 0 { return Fail("tcsetattr: {strerror(errno())}") }
    Pass(orig)                                     // caller keeps `orig` to restore on exit
}
```

## 8. Conformance

`spec/conformance/cstruct_ffi.thx` (+ `.out`) and `tests/test_cstruct_ffi.py` must be **self-contained
— no terminal, deterministic output** — so they exercise the primitive via padding-free ABI structs
round-tripped through libc `memcpy`/`memset`:

- **Zero-init + u32 field write/read:** `type Two { mut a: UInt32; mut b: UInt32 }` (exactly 8 bytes,
  no padding); `Two()` reads `0`; set `a`/`b` from hex literals; read back exact ints.
- **Addressing + in-place C mutation:** `memcpy(u, t, 8)` copies one struct into another through their
  addresses; `memset(t, 0, 8)` has C zero a struct through the pointer; both visible to Thoxa after.
- **Fixed-array field element I/O:** `type Buf { mut data: Array<Byte, 4> }`; write/read `b.data[i]`;
  `memset(b, 67, 4)` fills it with `'C'` through the pointer; read elements back.
- **Bitwise field edit (LF-A integration):** `t.b = t.a & 0xFFFF` — a masked read-modify-write into a
  field, proving width-truncation-on-store and the bitwise operators reach struct fields.
- **`mut` requirement:** a negative check — passing a non-`mut` struct to an `Address<T>` extern
  parameter is a checker/interp error (unit test in `tests/test_cstruct_ffi.py`).

Exact `.out` bytes are pinned in the plan. As with LF-A, one new `.thx` adds three passing cases
(execution / type-check / LSP-diagnostics glob gates).

`spec/SPEC.md`: add a C-struct-FFI section documenting markerless ABI structs, `Array<T, N>` fixed
fields, `Address<T>`/`address()` with implicit addressing, the `mut` rule, zero-init construction, and
native-endian width-truncating field writes.

## 9. Out of scope / deferred

- **Const/read-only pointers** (`const T *`) — a `ReadOnly<T>`/`View<T>` companion so read-only externs
  (`tcsetattr`) need no `mut` binding, with rigorous read/write-through safety. **This is the named
  "later pass."**
- **Struct value-copy semantics** (`let b = a` deep-copies) — kilo uses a second `tcgetattr`; add when a
  real use forces it. Interacts with the general value-semantics/COW direction.
- **Layout-optimization analysis** — a native pass that reclaims field-reordering freedom for structs
  proven never to cross the ABI (the mitigation for choosing markerless).
- **`sizeof`, pointer returns, pointer arithmetic, `malloc`/heap ownership + `free`, by-value struct
  args, nested structs / unions / bitfields, C→Thoxa callbacks, whole-fixed-array binding/slicing,
  scalar-pointee `Address<Int32>`, the combinable valued-flag enum bitset** (now unblocked by LF-A, its
  own small design). None are exercised by raw mode.
- **`struct winsize`** (4×`UInt16`) for window size is expressible for free with this primitive, but
  termios is the driver and window size already works via the `Bytes` path.
