# ThoxaNative — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**R7 — C-layout struct (packing)**

Forces: C-ABI structs (`type Termios { … }` style), field layout/alignment, likely `Address<T>` for extern params that write structs. Oracle: SPEC §C-struct / `../Thoxa/docs/specs/2026-07-16-thoxa-cstruct-ffi-design.md` + `cstruct_ffi` conformance.

Not yet: modules (R8), HTTP (R9), Buffer COW, full M4 conversions.

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

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `as Bytes` with M4 conversions later; no overloaded `from` |
| Buffer / COW | SPEC §9.1 + `../Thoxa` growable-buffer design | Later; not blocking R7 |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |
| FFI libraries beyond `"c"` | R6 subset | Only `"c"` (libc via headers) for now |

## Last session

- **2026-07-19 (later):** R6 `extern fn` libc — parse `from "c"` / optional symbol, Int32+UInt64, write/abs/getpid conformance; int width coerce at FFI and comparisons.
- **2026-07-19:** R5 (`mut`, `Bytes`), D14–D18, agent handoff (`AGENTS.md`, status, permissions).

## Verify quickly

```bash
cargo make test
```
