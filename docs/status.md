# ThoxaNative — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**Post-R9 polish / next pressure from oracle HTTP**

Likely: `Trial` monomorphs beyond `Int64` (String / user / Void), `accept`/`recv`/`send` one-shot client, `Bytes.find`, growable `Buffer`, dynamic `Array`, `loop`/`break`, file I/O for static router — pick by force, not all at once.

Not yet: Buffer COW, full M4 conversions, `address()` explicit, struct value-copy, `sizeof`, `std/` / `ffi` named roots, `Main.thx` directory entry, full accept-loop server.

## Done (ladder)

| Rung | Summary | Conformance |
|------|---------|-------------|
| R0 | hello / driver / C emit | `hello` |
| R1 | let, arith, if | `arithmetic`, `lets_if` |
| R2 | Trial, match, `?` (`Trial<Int64,Int64>`) | `trials` |
| R3 | user `type` + methods (Int64/Bool fields) | `types` |
| R4 | String, interpolation, simple RC | `strings` |
| R5 | `mut` + assign; `Bytes` (fromString/count/get/set/toString) | `bytes` |
| R6 | `extern fn` from `"c"` (+ optional C symbol); Int32/UInt64; marshal String/Bytes | `extern_libc` |
| R7 | C-layout struct packing; `Address<T>`; `Array<T,N>`; UInt32/Byte; bitwise | `cstruct_ffi` |
| R8 | modules / multi-file; `import`/`export`; module-level `let`/`mut` | `modules`, `namespaces`, `modstate` |
| R9 | HTTP subset: Net sockets + Http protocol; `Bytes(n)` / `.slice` | `http` (+ `bytes` slice) |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `Bytes(n)` zero buffer shipped (D18/D36); `as Bytes` with M4 later |
| Buffer / COW | SPEC §9.1 + `../Thoxa` growable-buffer design | Later; not blocking R9 subset |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |
| FFI libraries beyond `"c"` | R6 subset | Only `"c"` (libc via headers) for now |
| Explicit `address()`, struct copy, `sizeof` | D23–D25 | Deferred (oracle LF-B §9) |
| Named roots `std/` / `ffi`, dir `Main.thx` | D30–D32 | Deferred; R8 is `./` + `~/` only |
| Full oracle HTTP | D36–D38 | Accept loop, files, Array/Buffer, String Trial — post-R9 |

## Last session

- **2026-07-19 (R9):** HTTP subset — `httplib/Net` (socket/bind/listen/close), `httplib/Http` (request-line parse + response build), entry `http.thx`; `Bytes(n)` zero-fill, `Bytes.slice`; socket headers in cgen; Pass/Fail int widen.
- **2026-07-19 (R8):** Multi-file modules — `import {…} from` / namespace import, `export` on fn/type/let, `./` + `~/` loader, module-level let/mut statics + init, name mangling; `modules`, `namespaces`, `modstate` conformance.
- **2026-07-19 (R7):** C-layout structs (`type Regs { mut a: UInt32 … }`), zero-init `Regs()`, field/index assign, `Array<Byte,N>`, `Address<T>` implicit on mut places, bitwise, hex lits, UInt32/Byte; `cstruct_ffi`.
- **2026-07-19 (later):** R6 `extern fn` libc.
- **2026-07-19:** R5 (`mut`, `Bytes`), D14–D18, agent handoff.

## Verify quickly

```bash
cargo make test
```
