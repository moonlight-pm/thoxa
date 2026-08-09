# Vendored sources

Upstream trees we ship and build against. Prefer this name over
`third_party` (same role) or `contrib` (that usually means *our* optional
add-ons — closer to `extras/`).

## TinyCC (`tinycc/`)

Vendored [TinyCC](https://github.com/TinyCC/tinycc) for the **session step**
C backend (D105 / in-process libtcc).

| | |
|--|--|
| Pin | See `TINYCC_PIN` (git commit) |
| License | LGPL-2.1 (`tinycc/COPYING`) — packaging polish deferred (D105.d) |
| Built by | `crates/compiler/build.rs` → `target/c/tinycc/` |
| Used for | REPL / session `thoxa_session_step` only (not batch programs) |

Do not edit TinyCC sources for product features; upgrade by re-vendoring a new
pin. Build products stay under `target/` (not committed).
