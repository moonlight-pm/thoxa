# Vendored sources

Upstream trees we ship and build against. Prefer this name over
`third_party` (same role) or `contrib` (that usually means *our* optional
add-ons — closer to `extras/`).

## SQLite (`sqlite/`)

Vendored [SQLite amalgamation](https://sqlite.org/amalgamation.html) for
product `std/sqlite` (**D143**). Compiled into `libthoxa_rt.a`.

| | |
|--|--|
| Version | See `sqlite/VERSION` |
| License | Public domain / blessing |
| Built by | `build_runtime` (`sqlite3.c` + `runtime/src/thoxa_sql.c`) |
| Used for | Programs that import `std/sqlite` |

See [`sqlite/README.md`](sqlite/README.md).
