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
