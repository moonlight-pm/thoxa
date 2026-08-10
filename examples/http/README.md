# examples/http — static file server

Minimal HTTP/1.1 server in pure Thoxa: bind, accept, parse request, map path
to files under `web/`, write response.

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

| File | Role |
|------|------|
| `Main.thx` | Listen loop |
| `Net.thx` | TCP sockets (`extern` libc) |
| `Http.thx` | Request/Response parse + write |
| `Router.thx` | Path → file under `web/` |
| `Io.thx` | `readFile` for static bodies |
| `web/` | Demo assets |

Modules are packaged next to `Main.thx` (same surface as the `httplib` /
`iolib` conformance helpers; not yet product `std/`).

## Robustness notes

What this example models (and what it still simplifies):

| Concern | Behavior |
|---------|----------|
| Setup (`socket` / `bind` / `listen`) | Fail → process exit with log |
| `accept` errors | Log and continue (do not kill the server) |
| Request read | Loop until `\r\n\r\n` (or peer close / 64 KiB header cap) |
| Response write | `sendAll` drains partial `send`s |
| Methods | `GET` and `HEAD` only; others → **405** + `Allow` |
| Path traversal | `.` / `..` segments → **404** |
| Missing file | **404**; permission denied → **403** (logged) |
| Unexpected Io errors | Log + **404** (still a Response, not process death) |
| Parse errors | **400** + log |
| Request bodies | Ignored (static server; headers only) |

Fail payloads stay `String` (same as product Io/Net convention). Domain
structure is in control flow (Response vs Trial), not an `Error` hierarchy.
