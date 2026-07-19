# ThoxaNative — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**R9 — HTTP example (or subset)**

Forces: integration of modules + FFI + structs toward a real networked program. Oracle: Thoxa HTTP / Net examples.

Not yet: Buffer COW, full M4 conversions, `address()` explicit, struct value-copy, `sizeof`, `std/` / `ffi` named roots, `Main.thx` directory entry.

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

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `as Bytes` with M4 conversions later; no overloaded `from` |
| Buffer / COW | SPEC §9.1 + `../Thoxa` growable-buffer design | Later; not blocking R9 |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |
| FFI libraries beyond `"c"` | R6 subset | Only `"c"` (libc via headers) for now |
| Explicit `address()`, struct copy, `sizeof` | D23–D25 | Deferred (oracle LF-B §9) |
| Named roots `std/` / `ffi`, dir `Main.thx` | D30–D32 | Deferred; R8 is `./` + `~/` only |

## Last session

- **2026-07-19 (R8):** Multi-file modules — `import {…} from` / namespace import, `export` on fn/type/let, `./` + `~/` loader, module-level let/mut statics + init, name mangling; `modules`, `namespaces`, `modstate` conformance.
- **2026-07-19 (R7):** C-layout structs (`type Regs { mut a: UInt32 … }`), zero-init `Regs()`, field/index assign, `Array<Byte,N>`, `Address<T>` implicit on mut places, bitwise, hex lits, UInt32/Byte; `cstruct_ffi`.
- **2026-07-19 (later):** R6 `extern fn` libc.
- **2026-07-19:** R5 (`mut`, `Bytes`), D14–D18, agent handoff.

## Verify quickly

```bash
cargo make test
```
