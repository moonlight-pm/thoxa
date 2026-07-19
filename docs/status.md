# ThoxaNative — status (keep current)

> Agents: read this at session start. After finishing work, update **Next**, **Done**, and **Last session** before stopping.

## Next

**R6 — `extern fn` libc (FFI)**

Forces: declare and call C functions; marshal scalars / `String` / `Bytes` as the SPEC requires; link against libc (and later user libs as needed). Oracle: `../Thoxa` FFI docs + conformance (e.g. libc-shaped tests under `spec/conformance/`, design under `docs/specs/*ffi*`).

Not yet: full Buffer COW, modules, C-layout structs (R7), HTTP (R9).

## Done (ladder)

| Rung | Summary | Conformance |
|------|---------|-------------|
| R0 | hello / driver / C emit | `hello` |
| R1 | let, arith, if | `arithmetic`, `lets_if` |
| R2 | Trial, match, `?` (`Trial<Int64,Int64>`) | `trials` |
| R3 | user `type` + methods (Int64/Bool fields) | `types` |
| R4 | String, interpolation, simple RC | `strings` |
| R5 | `mut` + assign; `Bytes` (fromString/count/get/set/toString) | `bytes` |

## Open design notes (pointers only)

| Topic | Where | Stance |
|-------|--------|--------|
| Bytes construction sugar | `docs/decisions.md` D14–D18 | Keep `fromString`; `as Bytes` with M4 conversions later; no overloaded `from` |
| Buffer / COW | SPEC §9.1 + `../Thoxa` growable-buffer design | Later; not blocking R6 |
| Algebraic effects | D2 | Deferred until async forces it |
| Perceus precision RC | D8 | Later MIR pass |

## Last session

- **2026-07-19:** Implemented R5 (`mut`, `Bytes`, runtime + conformance). Logged D14–D18 (Bytes construction analysis). Added agent handoff (`AGENTS.md`, this file, project permissions).

## Verify quickly

```bash
cargo make test
```
