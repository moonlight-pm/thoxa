# examples/http — static file server

Minimal HTTP/1.1 server in pure Thoxa. Application code is just `Main.thx` +
`web/`; sockets, protocol, files, and static routing come from product **`std/`**
(**D126**).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Then:

```bash
curl -s  http://127.0.0.1:9091/
curl -s  http://127.0.0.1:9091/about/
curl -sD- http://127.0.0.1:9091/style.css
curl -s  http://127.0.0.1:9091/missing.html   # 404
curl -sI http://127.0.0.1:9091/               # HEAD
curl -s -X POST http://127.0.0.1:9091/        # 405
```

Stop with Ctrl-C. Port is **9091** (`SO_REUSEADDR` on).

## Layout

| Path | Role |
|------|------|
| `Main.thx` | Listen loop + per-connection serve |
| `web/` | Demo assets |
| `std/Net` | TCP sockets |
| `std/Http` | Request/Response parse + write |
| `std/HttpStatic` | Path → file under a root (`Router`) |
| `std/Io` | `readFile` (used by the router) |
| `std/Logging` | Request / error logs |

## Behavior (product std)

| Concern | Behavior |
|---------|----------|
| Setup (`socket` / `bind` / `listen`) | Fail → process exit with log |
| Accept loop | Infinite `loop` is last expr of unannotated `run()` — diverges (**D128**); return inferred as fallible (**D129**), no `Pass(Void)` tail |
| `accept` errors | Log and continue |
| Request read | `Socket.receiveRequest` until `\r\n\r\n` (64 KiB cap) |
| Response write | `Socket.sendAll` drains partial sends |
| Methods | `GET` and `HEAD`; others → **405** |
| Path traversal | `.` / `..` → **404** |
| Missing file | **404**; permission denied → **403** |
| Parse errors | **400** |
| Imports | Combined e.g. `import logging, { info, warn } from "std/Logging"` (**D127**) |

Handbook: [`docs/handbook/std.md`](../../docs/handbook/std.md) · language:
[`docs/handbook/language.md`](../../docs/handbook/language.md).
