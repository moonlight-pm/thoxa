# Examples

Runnable Thoxa programs that exercise the product surface. **Not** the
conformance gate (`tests/conformance/`) and **not** DX tooling (`extras/`).

Run from the **workspace root** so relative asset paths resolve:

```bash
cargo make run examples/http
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt   # needs a TTY
```

| Path | What |
|------|------|
| [`http/`](http/) | **Crema-lite experiment** — `server.serve(routes.port, routes.handle)` (**D132**); layered `framework/` + `app/`; port **9091**; not product std |
| [`kilo/`](kilo/) | Terminal text editor — **Ctrl-Q** quit (3× if dirty); real TTY required |

Oracle reference copies (history only):
`apocrypha/old/ThoxaPython/examples/`.
