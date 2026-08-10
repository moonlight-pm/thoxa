# Examples

Runnable Thoxa programs that exercise the product surface. **Not** the
conformance gate (`tests/conformance/`) and **not** DX tooling (`extras/`).

Run from the **workspace root** so relative asset paths resolve:

```bash
cargo make run examples/http
# or: cargo run -p thoxa -- run examples/http
```

| Path | What |
|------|------|
| [`http/`](http/) | Static HTTP/1.1 file server (Net + Http + Router + Io) |
| [`kilo/`](kilo/) | Terminal text editor (raw mode, edit, save, find, highlight) |

Oracle reference copies (history only):
`apocrypha/old/ThoxaPython/examples/`.
