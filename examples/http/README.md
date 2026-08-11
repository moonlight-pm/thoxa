# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std** —
all app logic lives in this folder; product **`std/Net` + `std/Http` +
`std/HttpStatic` + `std/Logging`** are used as-shipped.

Charter + living checklist:
[`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Then:

```bash
curl -s  http://127.0.0.1:9091/
curl -s  http://127.0.0.1:9091/menu
curl -s  'http://127.0.0.1:9091/menu?q=mocha'
curl -s  http://127.0.0.1:9091/menu/espresso
curl -s  http://127.0.0.1:9091/api/health
curl -sD- http://127.0.0.1:9091/style.css

# Remember me (PRG + cookie)
curl -sD- -c /tmp/crema.ck -X POST -d 'name=Ada' http://127.0.0.1:9091/
curl -s -b /tmp/crema.ck http://127.0.0.1:9091/          # Hi, Ada
curl -sD- -b /tmp/crema.ck -c /tmp/crema.ck -X POST -d 'name=' http://127.0.0.1:9091/
```

Stop with Ctrl-C. Port **9091** (`SO_REUSEADDR` on).

## Layout (as-built)

| Path | Role |
|------|------|
| `Main.thx` | Listen / accept / parse / **body** / send |
| `App.thx` | Method+path dispatch (pages, API, static fallback) |
| `Layout.thx` | Shared HTML shell + optional greeting |
| `Pages.thx` | `/`, `/menu`, `/menu/{slug}`, **POST `/` sign-in** |
| `Api.thx` | `/api/health` (no layout) |
| `Models.thx` | In-memory drink catalog |
| `Wire.thx` | Example-only: body, form, 303, header helpers |
| `Customer.thx` | Cookie remember / forget / current |
| `web/` | Static assets (`style.css`) |

## Routes

| Method | Path | Handler |
|--------|------|---------|
| GET/HEAD | `/` | Home + sign-in card |
| POST | `/` | Form `name` → Set-Cookie + **303** `/` |
| GET/HEAD | `/menu` | Drink list |
| GET/HEAD | `/menu/{slug}` | Drink detail or 404 |
| GET/HEAD | `/api/health` | Plain `ok` |
| GET/HEAD | other | `std/HttpStatic` under `web/` |
| other | * | 405 |

## Body handling (example-only)

Product `std/Http.parseRequest` still leaves `Request.body` empty.
`Main` calls `Wire.readBody` after headers: uses bytes already after
`\r\n\r\n` in the buffer, then `Socket.receive` until `Content-Length` is
satisfied. The completed body is passed as a separate `Bytes` into
`App.handle` — **not** a `std/` change.

## HEAD (framework-level)

Handlers always build a full response (as for GET). `App.handle` ends with
`Wire.finalize`: on `HEAD`, status and headers (including `Content-Length`)
are kept and the body is dropped. Handlers do not take a `headOnly` flag or
call `okHead`. Same idea as common servers: run the GET path, strip content
on the way out (RFC 9110 §9.3.2).

## Rules

- **No `std/` additions** for this experiment.
- Language gaps → break, fix language, resume (see experiment charter).
- Phase C+ (query search) is checklist-only until CURRENT says so.

## Inspiration

Topcoat coffee-shop + manual-router patterns (layout, pages vs routes, models,
path params, form POST + cookie) — **not** a port of signals, `view!`, or
middleware.
