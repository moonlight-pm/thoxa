# ThoxaNative — Decision Log

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
| D14 | String → Bytes surface (now) | Keep `Bytes.fromString(s)` | Matches `../Thoxa` SPEC §9.1; R5 ship shape |
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
| D12 | Language oracle | `../Thoxa` SPEC + conformance | Reopen syntax only deliberately |
| D13 | Old QBE tree | Reference only | Do not revive megapass codegen |
```