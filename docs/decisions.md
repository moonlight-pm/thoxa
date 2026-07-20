# Thoxa — Decision Log

Living log of load-bearing choices. Newest first within each section when updated.

**Doc layout (keep it small until it hurts):**

| Path | Role |
|------|------|
| `AGENTS.md` | **Session bootstrap** — auto-loaded by Grok every session; keep short |
| `docs/status.md` | Living “where we are / next rung” — agents update after work |
| `docs/decisions.md` | Locked choices + deferred analyses (this file) |
| `docs/specs/` | Full design docs (architecture, rungs, systems) |
| `.grok/config.toml` | Project permission allows (fewer approval prompts) |

No knowledge graph (`docs/graph/`, node databases, etc.) until cross-links and “where did we say X?” pain are real. Prefer prose + IDs here over premature graph tooling. **Serena-like code-intelligence MCPs are optional**; this repo’s handoff is file-based (`AGENTS.md` + `status.md`), not a separate memory server.

---

## 2026-07-20 — Request / Response / Header (PR5)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D69 | HTTP protocol types | `Header(name,value)`, `Request(method,path,version,headers,body)`, `Response(status,reason,headers,body)`; `parseRequest(raw) -> Trial<Request,String>`; `writeResponse(res) -> Bytes`; `ok`/`notFound`/`badRequest` → `Response` | Constructor-param fields may be `Array<T>` (heap RC via existing User field walk); `user.arrField[i]` loads dynamic Array then indexes (FixedArray peel unchanged). Headers: simple `Name: value` until blank; body always empty. `Bytes.find` → `-1` (not May). **`summarizeRequest` is intentionally an adapter over `parseRequest`** (requires `\r\n\r\n`; path is query-stripped) — not the pre-PR5 request-line-only scan. Empty method/path/version tokens not rejected (oracle parity; tighten later if server forces). Phase-A RC may over-dup on `Trial<Request>` / `Array<Header>` (LSan noise; no UAF; not a conformance gate). Conformance: `http_parse`. |

## 2026-07-19 — Post-R9 HTTP plan (PR1)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D63 | Early `return` | `return` / `return expr` as expression (like `break`); type-checked against enclosing `fn`/`method` return type | Bare `return` only for `Void`; MIR `Stmt::Return { value: Option<Place> }`; drop live `needs_drop` **Assign** locals before early return / `?` Fail (same set as end-of-fn `collect_drop_places` — **not params**; caller still owns args); exits function from inside loops. **No ASI:** optional value is parsed whenever the next token can start an expression (newline does not end a bare `return`); write `return` then `}` / non-expr, or put the following statement after a non-expr-start token — intentional `return expr` may span lines. |

## 2026-07-20 — Array&lt;User&gt; elements (PR3)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D65 | `Array<User>` storage | `ThoxaArray { len, cap, flags, elem_size, data[] }`; User elems **inline** (`elem_size = sizeof(C struct)`) | Int64/String keep `elem_size=8` + HEAP_ELEMS; User uses cgen `ThoxaArrayElemFn` field RC (dup/drop helpers). Constructor-param User only (Int64/Bool/String/Bytes fields). Conformance: `array_user` |

## 2026-07-20 — User+heap field RC (PR2)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D64 | User+heap RC | `needs_drop` (not `is_heap`) for Trial Pass/Fail/`?`/match extract; method field prolog Dups heap fields; match binds use fresh places | cgen `emit_rc` already field-walks User; MIR must insert balanced Dup/Drop. Unblocks `Trial<User+String>` / File-shaped APIs. Conformance: `user_rc` |

## 2026-07-20 — Match discard binding

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D66 | Match discard `_` | `Pass(_)` / `Fail(_)` — pattern `_` does not introduce a binding | Standalone `_` lexes as ordinary `Ident("_")` (usable as a normal name in `let`/params/etc.); **only match patterns special-case it** (`bind: None`). A `_` in the arm body is ordinary name lookup (error if unbound; resolves an outer `let _` if present — discard does not shadow). Payload still extracted + RC-owned for end-of-fn drop; no bare `Fail`/`Pass` without parens (grammar churn deferred); `_foo` still rejected (not an ident start) |

## 2026-07-19 — R9+ file I/O + Router

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D56 | File surface | Free `readFile(path) -> Trial<Bytes, String>` | Subset of oracle `Io.thx`; whole-file via lseek size; OpenMode/`File` type deferred (User+String in Trial RC soft) |
| D58 | File errors | Simple `Fail("open"\|"read"\|"seek")` strings | errno/`strerror` need named `ffi` root (D30) — deferred like Net |
| D59 | User zero-init | Constructor-param User with String/Bytes fields zero as NULL heap | Match out-places; **field-RC hardened in D64** |
| D60 | String equality | `==` / `!=` via `thoxa_string_eq` (length + bytes) | Ordering compares not supported |
| D61 | Static Router | `handle(root, path) -> Bytes` HTTP response | Segments + `..` reject + index.html + Content-Type; no Request type / early return |
| D62 | for-in RC | Drop previous loop-var each iter; last value dropped at fn end | Fixes double-drop of last heap element (was drop-per-iter + fn-end drop) |

## 2026-07-19 — R9+ dynamic Array

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D53 | Dynamic `Array<T>` | Heap RC `ThoxaArray*` (slots as `int64_t`; heap elems bitcast) | Distinct from fixed field `Array<T,N>` (D27); elems: Int64, Bool, String, Bytes; **User via D65** |
| D54 | Array mutation model | Functional `append`/`dropLast` return new arrays; index set mutates shared storage | Matches oracle list semantics; not Buffer-style COW write-back |
| D55 | Array surface | Literals, empty `[]` + type ann, `.count`, `[i]`, append, dropLast, `for x in arr`, display | `insertAt`/`removeAt` deferred; typed let `let xs: Array<T> = []` |

## 2026-07-19 — R9+ Buffer

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D50 | Growable Buffer | Heap RC `ThoxaBuffer*` with COW mutate | Geometric cap 0→8→double; `append`/`appendBytes` return unique ptr (MIR write-back on name recv); not FFI-marshalable |
| D51 | Buffer surface | `Buffer()` / `Buffer(cap)`, `.count`/`.capacity`, append, appendBytes, freeze→Bytes | `freeze` copies live prefix (consume-on-freeze deferred); `reserve` deferred |
| D52 | Buffer vs Bytes | Buffer builds; freeze → fixed Bytes for FFI/send | Matches D17; no Buffer at extern boundary |

## 2026-07-19 — R9+ control flow + String fields

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D47 | Loops (subset) | `while`, `loop`, `for x in lo..hi` / `lo..=hi`, `break`, `continue` | Loops yield `Void`; **`for x in arr` shipped with D55**; `continue` in range-for hits C for-step |
| D48 | Constructor-param field types | Int64, Bool, **String**, **Bytes** | Nested User still deferred; C-layout body fields remain ABI-only |
| D49 | RC on user values | `needs_drop(User)` always; cgen walks heap fields | Dup after `StructLit` and heap field loads; method `_self` is a param (not auto-dropped) |

## 2026-07-19 — R9+ Trial monomorphs + wire I/O

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D40 | Trial representation | `Ty::Trial { ok, err }` monomorphized C structs | Fields: `tag` + optional `ok`/`err` (omitted when Void) |
| D41 | Trial payloads | Void, ints, Bool, String, Bytes, User | Nested Trial / Array / Address not yet |
| D42 | Pass/Fail typing | Prefer enclosing `-> Trial<…>` annotation | Legacy unannotated Pass/Fail(Int) → `Trial<Int64,Int64>` |
| D43 | `?` across monomorphs | Same `E`; rewrap `Fail(e)` into outer `Trial<Ok',E>` | Allows `createSocket()?` inside `-> Trial<Void,String>` |
| D44 | Trial RC | Dup live heap side on copy; drop live side on drop | Zero-init trial locals so branch-skipped drops are safe |
| D45 | Wire conformance | One-shot listen/connect/accept/recv/send | Ephemeral port via `getsockname`; no infinite accept loop |
| D46 | `Bytes.find` | `find(needle)` / `find(needle, from)` → Int64 | `-1` if missing; May/Has deferred until May lands |

## 2026-07-19 — R9 HTTP subset

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D36 | R9 scope | Subset, not full oracle server | Multi-module Net + Http; offline parse/response; socket lifecycle (create→reuse→bind→listen→close). No accept loop, file router, Buffer, dynamic Array |
| D18′ | `Bytes(n)` | Ship zero-filled fixed buffer | Forced by sockaddr / setsockopt packing; locks D18 “when forced” |
| D37 | `Bytes.slice(start, end)` | Half-open copy → new Bytes | Enough for request-line parse without `find` / Array; OOB panics |
| D38 | Socket errors (R9) | Was `Trial<Int64,Int64>` codes | **Superseded by D40–D45** — String / User Trial + wire I/O |
| D39 | Net surface | Private `extern` + `export type Socket` / `createSocket` | Linux sockaddr_in pack by hand; port `0` = ephemeral for CI |

### Notes

- Oracle `examples/http` remains the north star; R9 proves modules + FFI + Bytes integration without inventing stdlib roots (`ffi` / `std/`).
- errno/`strerror` not wrapped (would need named `ffi` root or local externs + TLS errno story).
- Constructor-param types were Int64/Bool only at R9 → `Socket(fd: Int64)`; **String/Bytes fields shipped later (D48–D49)**.

## 2026-07-19 — R8 modules / multi-file

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D30 | Import origins (R8) | `./…` and `~/…` only | Named roots `std/` / `ffi` deferred |
| D31 | Resolution | No `..` segments; `~/` = entry file’s directory | Matches oracle “never climb” |
| D32 | Export surface | `export` on top-level `fn` / `type` / `let` / `extern`; `mut` never exported | Selective + namespace imports |
| D33 | Namespace import | `import ns from "…"` camelCase binding; members via `ns.x` | Not a first-class value |
| D34 | Module-level values | Static C globals + `thoxa_module_init()` before main | Dep-order init; private mut state |
| D35 | Name mangling | Non-entry: `{Module}_{name}` types/fns; globals `g_{Module}_{name}` | Entry keeps unprefixed names; `main` special |

## 2026-07-19 — R7 C-layout struct / packing

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D23 | Struct layout default | Declaration-order, natural C alignment (markerless) | Matches oracle LF-B; no `repr(C)` |
| D24 | Body-field types | `type T { [mut\|let] f: Ty }` with ABI scalars / `Array<T,N>` | Separate from R3 constructor-param types |
| D25 | Construction | `T()` zero-inits C-layout types | No field args; R3 still uses named constructor args |
| D26 | `Address<T>` | Extern params only; implicit `&` on mut User places | Explicit `address()`, storing Address deferred |
| D27 | Fixed arrays | Field-only `Array<T, N>`; element get/set via `[i]` | No whole-array bind/slice |
| D28 | New scalar widths | UInt32, Byte (=UInt8); bitwise on integers in Int64 then truncate on field store | Hex `0x…` literals |
| D29 | Struct value-copy / `sizeof` | Deferred | Oracle LF-B §9; kilo re-reads via tcgetattr |

### Notes

- Field write is a typed width boundary (truncate). Bitwise ops promote to Int64 for the op.
- Passing a non-`mut` binding where `Address<T>` is required is a compile error.
- C codegen emits real C structs (`uint32_t`, `uint8_t f_cc[4]`, …) so padding matches the platform ABI (Regs = 16 bytes on x86-64).

## 2026-07-19 — R6 extern libc (subset)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D19 | Extern libraries (R6) | `"c"` only | Headers (`unistd.h`, `stdlib.h`, `string.h`) supply prototypes |
| D20 | Extern types (R6) | Int32, Int64, UInt64, String, Bytes, Void | R7 adds UInt32, Byte, `Address<T>` |
| D21 | Int width at boundary | Coerce Int64↔Int32/UInt64 at args; widen ints for `==`/`<` etc. | Literals stay Int64; R7 extends to UInt32/Byte |
| D22 | C symbol rename | `from "c" "symbol"` | When Thoxa name ≠ C name |

---

## 2026-07-19 — Bytes construction sugar (R5 follow-up)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D14 | String → Bytes surface (now) | Keep `Bytes.fromString(s)` | Matches `apocrypha/old/ThoxaPython` SPEC §9.1; R5 ship shape |
| D15 | `"…" as Bytes` | Deferred to conversions | Belongs with `as` / `can` / `is` ([M4] in SPEC §11), not a one-off |
| D16 | Overloaded `Bytes.from(…)` | Reject (for now) | No ad-hoc overloading story; prefer explicit names if more factories appear |
| D17 | Multi-byte / dynamic build | Prefer `Buffer` path later | `append` / `freeze` → `Bytes`; not `from([1,2,3])` as the main story |
| D18 | Extra factories | Only when forced | e.g. `Bytes(n)` zero buffer for FFI (R6 pressure); maybe `fromArray` when arrays exist |

### Analysis (why)

**`mut b = "abc" as Bytes`**

- Fits Thoxa’s conversion triad once it exists: explicit, no implicit coercion, same family as `n as String`.
- String and Bytes are different kinds: immutable UTF-8 text vs fixed **mutable FFI buffer**. `as` should mean “encode/copy payload,” not “rebrand this string.”
- Implementing a single `as` without the rest of M4 is a design leak (special case, no `can`/`is`, no conversion table).
- Reverse path (`Bytes` → `String`) needs a policy for non-UTF-8; that is conversion work, not R5.

**`Bytes.from("abc")` with overloads (`from([1,2,3])`, …)**

- Friendly, but SPEC already names `fromString`. Renaming reopens the oracle.
- Overload sets need either ad-hoc resolution or a `From`-like trait; Thoxa has neither.
- Bare `from` hides source/encoding (UTF-8 text? raw ints? size?).
- Array form needs array literals + element rules — later rungs.
- If multiple sources appear, prefer **distinct names** (`fromString`, `fromArray`, `Bytes(n)`), same spirit as Buffer’s `append` vs `appendBytes`.

**Rule of thumb**

- Domain construction from a specific source → **named static method** (`fromString`).
- Cross-type encoding/reinterpretation between established types → **`as`**, when the conversion system lands.
- Multiple input shapes → **multiple names**, not silent overload, until overloading is a deliberate language feature.

Revisit D15 when native work takes on [M4] conversions; revisit D17 with Buffer / growable-buffer design; revisit D18 when R6 FFI needs a sized empty buffer.

---

## 2026-07-18 — Kernel kickoff

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D1 | Growth model | Example ladder toward HTTP | Small kernel first |
| D2 | Algebraic effects | Deferred | Revisit with async |
| D3 | Error model | `May`/`Trial` + force-to-deal | Not effect-`exn` |
| D4 | Value model | Hybrid (A) | Unboxed scalars; inline small sums; RC heap; C-layout FFI structs |
| D5 | Phase A backend | Compile-to-C | No QBE yet |
| D6 | Compiler host | Rust under `crates/compiler` | |
| D7 | Runtime | C `thoxa_rt` | Headered RC objects |
| D8 | RC ambition day one | Simple correct RC | Perceus later |
| D9 | Docs path | `docs/specs/`, `docs/decisions.md` | No `docs/superpowers`; no graph until needed |
| D10 | Build automation | `crates/make` xtask, `cargo make …` | No project `.sh` scripts |
| D11 | Build products | Single `target/` tree: `rust/`, `c/`, `thoxa/` | No separate `targets/`; Cargo uses `target/rust` via `.cargo/config.toml` |
| D12 | Language oracle | `apocrypha/old/ThoxaPython` SPEC + conformance | Reopen syntax only deliberately |
| D13 | Old QBE tree | Reference only | Do not revive megapass codegen |
```