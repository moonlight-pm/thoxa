# Thoxa libc FFI — Design

**Date:** 2026-07-12
**Status:** Implemented as M7 (see CHANGELOG). This is the as-built design.

## 1. Motivation

The driver is a real web server written *in Thoxa* — one that opens a socket,
binds, listens, accepts, and speaks HTTP by making real system calls. But the
server is not the point. **The FFI is the point.** It is the mechanism that
forces the language to grow real teeth: a way to reach the operating system
without smuggling logic into the interpreter.

Today `thoxa/web.py` provides a `serve` builtin: an all-Python
`ThreadingHTTPServer` that hands a Thoxa handler only `(method, path)`. All the
interesting work — sockets, parsing, the HTTP dance — happens in Python. That is
exactly backwards. This design **retires `serve` and `web.py`** and replaces the
whole edifice with a general C FFI, so the syscalls are driven from Thoxa
itself.

### Non-negotiables carried in from the language

- **Zero runtime dependencies.** The bridge is Python's stdlib `ctypes`
  (`CDLL(None, use_errno=True)`), so `pyproject.toml` stays `dependencies = []`.
- **Casing rules hold.** `extern fn` names are values → camelCase / lowercase.
  Types in signatures are PascalCase primitives.
- **Explicit int sizes.** The programmer picks the C-accurate width per
  prototype (`fd: Int32` because C `int`, `count: UInt64` because `size_t`).
  This is a feature, not friction — it is why the FFI can be correct.
- **No ambient names.** A file declares every name it uses. There are no magic
  globals: even the FFI's own error helpers arrive through an explicit import.
- **No ambiguous primitives.** This work removes the width-ambiguous `Int` and
  `Float` types from the language entirely: integer literals now default to
  `Int64`, float literals to `Float64`, and every type annotation names a
  concrete width. A prerequisite language change (§4, §7), motivated by the FFI's
  need for exactness at the boundary — but applied language-wide, not just there.

## 2. Scope

**In scope (v1):**

- `extern fn` declarations — the language surface for a C function binding.
- A `Bytes` runtime primitive — the mutable byte buffer externs pass pointers to.
- A marshaling layer mapping Thoxa primitives ↔ ctypes ↔ C.
- An error channel: `errno()` + `strerror(code)`, reached through an
  `import { errno, strerror } from "ffi"` (§6).
- Enough to build, in a **single runnable `.thx` file**: stdout writing →
  read/write over fds → a TCP echo server → a minimal HTTP responder → and then
  delete `serve`/`web.py`.

**Explicitly deferred (roadmap, §11):** typed `Ptr[T]`, C structs, libraries
other than `"c"`, C→Thoxa callbacks, varargs, and **the module system** (see §8 —
v1 is single-file and touches nothing about modules).

## 3. `extern fn` — the language feature

An `extern fn` declares the Thoxa-side signature of a C function. It has **no
body**: the body lives in the named library.

```thoxa
extern fn getpid() -> Int32 from "c"
extern fn close(fd!: Int32) -> Int32 from "c"
extern fn write(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c"
extern fn socket(domain!: Int32, kind!: Int32, protocol!: Int32) -> Int32 from "c"
```

### 3.1 Positional markers

C functions have no named arguments, so **every `extern fn` parameter carries
the `!` positional marker** — consistent with how positional parameters are
spelled everywhere else in Thoxa. Calls are ordinary positional calls:

```thoxa
let fd = socket(2, 1, 0)   # AF_INET, SOCK_STREAM, 0
close(fd)
```

The checker rejects an `extern fn` parameter written without `!`.

### 3.2 The `from` clause: library, and an optional C symbol

Every `extern fn` ends with a **mandatory `from "<library>"` clause** naming the
library it binds to. There is no default — the C world is always named
explicitly. In v1 the only supported library is **`"c"`** (the process's own
libc, `ctypes.CDLL(None)`); any other name is a checker error.

A Thoxa function name is camelCase and cannot contain underscores, but C symbols
routinely do (`inet_pton`, `bind_textdomain_codeset`). So the clause takes an
**optional second string — the C symbol name** — used only when it differs from
the Thoxa name:

```thoxa
# C symbol == Thoxa name → library only
extern fn socket(domain!: Int32, kind!: Int32, proto!: Int32) -> Int32 from "c"

# C symbol differs → name it explicitly
extern fn inetPton(af!: Int32, src!: Bytes, dst!: Bytes) -> Int32
    from "c" "inet_pton"
```

When the symbol string is omitted, the Thoxa name is used verbatim as the C
symbol. This same syntax extends forward-compatibly to other libraries
(`from "ssl" "SSL_new"`), which v1 does not yet permit.

### 3.3 Where externs live (v1)

Externs are **file-local declarations**: you declare the ones you need in the one
file that uses them. There is no module system yet (§8), so there is no shared
`sys` module to import from — and there does not need to be for a single-file
program.

### 3.4 Runtime semantics

When the interpreter evaluates an `extern fn` declaration it binds, in the value
namespace, a callable that:

1. Resolves the library handle from the `from` clause (v1: `"c"` → the cached
   `CDLL(None, use_errno=True)`), then looks up the symbol — the optional symbol
   string, else the Thoxa name — on it (`getattr(lib, symbol)`).
2. Sets `.argtypes` / `.restype` **once**, derived from the declared Thoxa types
   via the mapping in §4.
3. On each call: marshals Thoxa arguments → ctypes values, invokes the C
   function, marshals the return value back to a Thoxa value.

Symbol resolution is **eager**: the lookup and `argtypes`/`restype` binding happen when the
`extern fn` is hoisted, so an unknown symbol is a located Thoxa error at that point
(`no such symbol 'frobnicate' in library "c"`), not a crash.

Each **call** validates before marshaling: argument count, each argument's type (a non-`String`
where C wants `char*`, a non-`Bytes` where it wants a buffer), and integer range against the
declared width. Any mismatch is a located Thoxa error — never a raw `ctypes` exception, a silent
truncation, or a pointer dereference.

## 4. Type mapping (marshaling)

Only these Thoxa types are **FFI-marshalable**. The checker rejects any other
type in an `extern fn` signature.

| Thoxa type | ctypes            | C type        | Notes |
|------------|-------------------|---------------|-------|
| `Int8`     | `c_int8`          | `int8_t`      | |
| `Int16`    | `c_int16`         | `int16_t`     | |
| `Int32`    | `c_int32`         | `int` (LP64)  | file descriptors, socket flags |
| `Int64`    | `c_int64`         | `long`, `ssize_t` | read/write returns |
| `UInt8`    | `c_uint8`         | `uint8_t`     | |
| `UInt16`   | `c_uint16`        | `uint16_t`    | `htons` ports, etc. |
| `UInt32`   | `c_uint32`        | `unsigned int`, `socklen_t` | |
| `UInt64`   | `c_uint64`        | `size_t`      | buffer counts |
| `Byte`     | `c_uint8`         | `unsigned char` | |
| `Float32`  | `c_float`         | `float`       | |
| `Float64`  | `c_double`        | `double`      | |
| `String`   | `c_char_p`        | `const char*` | UTF-8, NUL-terminated, **read-only to C** |
| `Bytes`    | `c_void_p`        | `void*`/`char*` | mutable buffer C can write into (§5) |
| `Void`     | `restype=None`    | `void`        | return position only |

**Deliberately not marshalable in v1:**

- `Char` — a Thoxa `Char` is a Unicode scalar, not a C `char`. Keeping it out of
  the FFI preserves the "chars aren't bytes/numbers" invariant. Use `Byte` /
  `Bytes` for C character data.
- `Bool` — disallowed at the boundary. C's `_Bool` ABI is finicky and rarely what
  a syscall wants; pass an explicit `Int32`/`UInt8` and compare. `Bool` stays a
  normal Thoxa type — it just cannot cross the FFI.
- `Int` / `Float` — no longer exist as types (§1); name a concrete width.
- `Array`, `Any`, `Type`, and user types — deferred with structs/`Ptr` (§11).
- **Pointer *returns*** (e.g. `malloc`, `strerror`'s `char*`) — v1 externs return
  a marshalable scalar or `Void`. No C path we need (`socket`/`accept` → fd,
  `read`/`recv` → `ssize_t`) returns a pointer. Pointer returns arrive with
  `Ptr[T]` in §11.

`String → const char*` is one-directional: C may read the bytes but must not
write through them. Anything C writes to goes through `Bytes`.

## 5. `Bytes` — the buffer primitive

Externs that read or write memory (`read`, `recv`, `sockaddr` for `bind`) need a
mutable, sized byte buffer whose address can be handed to C. That is `Bytes`. It
is a **language primitive** (ambient, like `String`/`Int32`) — never imported.

### 5.1 Surface (minimal)

```thoxa
let buf = Bytes(1024)          # 1024 zeroed bytes
buf.count                      # -> UInt64: length in bytes
buf.get(i!: UInt64)            # -> Byte:  byte at index i
buf.set(i!: UInt64, b!: Byte)  # in-place write
buf.toString()                 # -> String: decode UTF-8 up to first NUL / end
Bytes.fromString(s!: String)   # -> Bytes:  UTF-8 bytes of s (NUL-terminated)
```

`.get` / `.set` bounds-check and raise a Thoxa error on out-of-range, so a stray
index is a language error, not a memory-safety bug.

### 5.2 Runtime

Backed by `ctypes.create_string_buffer(size)`. When passed to an `extern fn`
whose parameter is `Bytes`, ctypes hands C the buffer's address; C reads and
writes it in place, and the mutation is visible to Thoxa afterward (this is how
`read`/`recv` deliver data). `Bytes` is a reference type: passing it to a
function shares the buffer.

## 6. Errors, errno, and the `ffi` import

libc reports failure through a sentinel return (usually `-1`) plus the global
`errno`. The bridge preserves this honestly rather than inventing exceptions:

- Raw `extern fn` calls **return the raw C value**, including `-1`. They do not
  throw on failure — that is the C contract.
- **`errno() -> Int32`** returns `ctypes.get_errno()` for the most recent extern
  call (the libc handle is opened `use_errno=True`, so ctypes captures per-call
  errno).
- **`strerror(code!: Int32) -> String`** returns `os.strerror(code)` (stdlib,
  zero-dep) — readable messages without needing pointer-return support for C's
  own `strerror`.

### 6.1 No ambient globals — errno/strerror are imported

Per Thoxa's rule that a file declares every name it uses, `errno` and `strerror`
are **not** globally available. They live in a built-in **`"ffi"` pseudo-module**
and must be imported before use:

```thoxa
import { errno, strerror } from "ffi"
```

Without the import, `errno` / `strerror` are undeclared names and the checker
reports them as such.

This requires the interpreter to recognise the `import { … } from "…"` form, but
**it is not the module system.** The only recognised source is the built-in
`"ffi"` pseudo-module, whose exports are a fixed allowlist (`errno`, `strerror`).
Importing an unknown name from `"ffi"`, or importing from any other source, is a
clear error (*"modules are not implemented yet"* for a non-`"ffi"` source). No
filesystem resolution, no `.thx` loading, no `export` — see §8.

### 6.2 The intended error pattern

Raw externs stay `-1`-style. Ergonomic, `Trial`-returning wrappers that check the
sentinel and lift into `Trial[T, Error]` are a **later, module-era concern**
(they belong in a real stdlib, which needs the module system) — so v1 code
handles the raw return and calls `errno()` / `strerror()` directly.

**Host-seam honesty.** `errno()`/`strerror()` and the ctypes marshaling are thin
host glue a tree-walking interpreter cannot avoid — a different thing from what
we are deleting, where `serve` put *program logic* (the HTTP protocol) in Python.
The FFI puts only *marshaling* there. Flagged for visibility.

## 7. Checker rules

For an `extern fn` declaration the static checker enforces:

- **No body** (parse-level: signature then terminator, no block).
- **Name is a value** → camelCase / lowercase, like any `fn`.
- **Every parameter has `!`** (positional); reject a named-style extern param.
- **A `from "<library>"` clause is present.** Missing it is an error. In v1 the
  only permitted library is `"c"`; any other name is a clear diagnostic
  (*"only the \"c\" library is supported in v1"*).
- **The C symbol string is optional.** Present → it is the C symbol; absent → the
  Thoxa name is the C symbol. Whether that symbol actually exists is a link fact,
  surfaced as a runtime error on first call, not a static one.
- **Every parameter type and the return type is FFI-marshalable** (§4) — a clear
  diagnostic naming the offending type otherwise (e.g. *"Char cannot cross the C
  FFI; use Byte or Bytes"*).
- The bound name enters the value namespace and is **callable like any function**.

For imports, the checker enforces:

- `import { … } from "ffi"` binds the listed FFI helpers; an unknown name in the
  list is an error.
- Any import whose source is **not** the built-in `"ffi"` pseudo-module errors
  with *"modules are not implemented yet"* (§6.1, §8).
- `errno` / `strerror` must be imported before use — no ambient fallback.

## 8. Modules — out of scope (deferred to their own spec)

The module system is **not** part of this spec. v1 is a single runnable `.thx`
file. Concretely, for v1 that means: externs are file-local (§3.3); the only
import is the built-in `"ffi"` pseudo-import for the FFI helpers (§6.1); there is
no `.thx`→`.thx` resolution, no `export`, no cross-file anything.

This is called out rather than ignored because both prior Thoxa attempts
(`old/Thoxa`, `old/ThoxaFail`) invested heavily in a module system, and a real
Thoxa standard library will need one. We are deliberately **not** prejudging its
design here — nothing in this FFI spec should be read as a module-system
decision.

**Open questions for the module spec (to decide there — none are settled or
dropped):**

- Is every `.thx` file implicitly a module? How are module files named/cased?
- Visibility: `export`-gating, private-by-default?
- Resolution: string paths vs. dotted names; where the standard library lives.
- **Binding model: a module as a plain namespace of its exports vs. the prior
  "a module *is* an instantiable type" model.** Explicitly *open* — Joshua wants
  to work through the reasoning for the instantiation model when we write that
  spec. This FFI spec takes no position.
- Where the raw-`extern` layer lives (a `sys`-like module?), and how safe
  wrappers over it lift `-1`/`errno` into `Trial` (§6.2).
- How today's file-local externs and the `"ffi"` pseudo-import migrate into real
  modules without changing the `extern fn` surface.

## 9. What gets retired

- **`serve` builtin** and **`thoxa/web.py`** in their entirety.
- Retirement is **sequenced after** the net path works (milestone M-FFI-4), so we
  never lose a working HTTP demo in between: the Thoxa-native server must respond
  to a real request *before* the Python one is deleted.

## 10. Milestones

Each milestone = interpreter changes + at least one example `.thx` + tests. Every
example is a single file; externs are declared with `from "c"`.

| ID | Milestone | Proves |
|----|-----------|--------|
| **M-FFI-1** | `extern fn` end-to-end: parse → check → marshal. Call `getpid()`; `write(1, Bytes.fromString("hi\n"), 3)` to stdout. | The language surface, the `from "c"` clause + libc resolution, and scalar/`Bytes`/`String` marshaling. |
| **M-FFI-2** | `Bytes` read path + errno. `read`/`write` over an fd; on failure, `import { errno, strerror } from "ffi"` and report. | Mutable buffers C writes into; the `ffi` import + error channel. |
| **M-FFI-3** | **TCP echo server in Thoxa.** `socket`/`bind`/`listen`/`accept`/`recv`/`send` driven from `.thx`; echoes a line back. | The whole syscall dance runs from Thoxa. |
| **M-FFI-4** | **Minimal HTTP in Thoxa.** Parse the request line, write a hard-coded `200 OK` response. Handler logic is Thoxa, not Python. | The original driver goal, honestly. |
| **M-FFI-5** | **Delete `serve` + `web.py`.** Update any example/test that referenced them to the Thoxa-native server. | The point of the whole exercise. |

## 11. Roadmap (out of scope for v1)

- **Typed `Ptr[T]`** — real pointers, pointer returns (`malloc`, `strerror`'s
  `char*`), pointer arithmetic behind a safe surface.
- **C structs** — `sockaddr_in` as a Thoxa type with a C-compatible layout,
  instead of hand-packed `Bytes`.
- **Libraries other than `"c"`** — the `from "<library>"` syntax already exists;
  v1 restricts the library to `"c"`. Supporting real other libraries
  (`from "ssl" "SSL_new"`, dlopen of named shared objects) is the extension.
- **C→Thoxa callbacks** — passing a Thoxa function as a C function pointer
  (signal handlers, `qsort`).
- **Varargs** — `printf`-style.
- **The module system** — its own spec (§8).

## 12. Resolved

- **`Int` / `Float` removed from the language** (§1) — no ambiguous-width
  primitives. Integer literals default to `Int64`, float literals to `Float64`;
  every annotation names a concrete width. Applied language-wide, not just at the
  boundary.
- **`Bool` disallowed at the FFI boundary** (§4) — use `Int32` / `UInt8`.

*(Module-design questions — including the namespace-vs-instantiation model — are
collected in §8 for their own spec.)*
