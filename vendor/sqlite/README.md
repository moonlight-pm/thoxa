# SQLite amalgamation

Vendored [SQLite](https://sqlite.org) C amalgamation for product `std/sqlite`
(D143). Compiled into `libthoxa_rt.a` (batch programs and session steps).

| | |
|--|--|
| Version | See `VERSION` (3.53.4) |
| Files | `sqlite3.c`, `sqlite3.h` only (no shell / ext header) |
| License | Public domain / blessing (`https://sqlite.org/copyright.html`) |
| Built by | `crates/compiler` `build_runtime` → `target/c/` |
| Used for | `std/sqlite` via `thoxa_sql_*` in `runtime/src/thoxa_sql.c` |

Do not edit amalgamation sources. Upgrade by replacing both files and
`VERSION` from the official `sqlite-amalgamation-*.zip`.
