# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

Two layers under this package:

| Layer | Path | Role |
|-------|------|------|
| **Framework** | `Framework/` | Host pipeline that may grow into `std/` later |
| **App** | `App/` | Little Crema — what an application author writes |
| **Entry** | `Main.thx` | Boot only (logging + `Framework.Server.run`) |
| **Assets** | `web/` | Static files |

Product **`std/Net` + `std/Http` + `std/HttpStatic` + `std/Logging`** are used as-shipped.

Charter: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

```bash
curl -s  http://127.0.0.1:9091/
curl -s  http://127.0.0.1:9091/menu
curl -s  'http://127.0.0.1:9091/menu?q=mocha'
curl -s  http://127.0.0.1:9091/menu/espresso
curl -s  http://127.0.0.1:9091/api/health
curl -sD- -c /tmp/crema.ck -X POST -d 'name=Ada' http://127.0.0.1:9091/
curl -s -b /tmp/crema.ck http://127.0.0.1:9091/
```

Port **9091**. Ctrl-C to stop.

## Layout

```text
examples/http/
  Main.thx                 # entry glue
  Framework/
    Server.thx             # accept loop, parse, body/query, send
    Wire.thx               # body, query, form, cookies helpers, HEAD, 303
  App/
    Routes.thx             # method+path table (app mount)
    Pages.thx              # HTML pages + POST sign-in
    Layout.thx             # HTML shell
    Api.thx                # /api/health
    Models.thx             # drink catalog
    Customer.thx           # cookie remember/forget
  web/                     # style.css, …
  README.md
```

### Dependency direction

```text
Main → Framework.Server → App.Routes → App.{Pages,Api,…}
                       ↘ Framework.Wire
App.* may import Framework.Wire
Framework must not import App except Server → Routes (mount hook)
```

`Server` mounts the app at `~/App/Routes.handle` — the only Framework→App edge
(Thoxa has no first-class function values to pass a handler in).

### Framework (`Framework/`) — candidate std

| Module | Responsibility |
|--------|----------------|
| `Server` | Bind/listen/accept; parse request; complete body; recover query; call app; HEAD finalize; write response |
| `Wire` | `readBody`, `rawQuery`, `queryParam`, `formField`, `headerGet`, `seeOther`, `withHeader`, `finalize` (HEAD) |

Handlers always return full GET-shaped responses. `Wire.finalize` strips the body on HEAD (RFC 9110).

### App (`App/`) — user code

| Module | Responsibility |
|--------|----------------|
| `Routes` | Dispatch: `/`, `/menu`, `/menu/{slug}`, `POST /`, `/api/health`, static fallback |
| `Pages` / `Layout` | HTML |
| `Api` | Health |
| `Models` | Domain data + search |
| `Customer` | Cookie session-ish name (uses `Wire` for headers) |

## Routes

| Method | Path | Handler |
|--------|------|---------|
| GET/HEAD | `/` | Home + sign-in card |
| POST | `/` | Form `name` → cookie + 303 `/` |
| GET/HEAD | `/menu` | List (`?q=` filters) |
| GET/HEAD | `/menu/{slug}` | Detail or 404 |
| GET/HEAD | `/api/health` | `ok` |
| GET/HEAD | other | static under `web/` |
| other | * | 405 |

## Rules

- **No `std/` additions** for this experiment — grow `Framework/` instead.
- Keep **App** free of listen/accept/parse/send details.
- Language gaps → fix language, then resume.

## Inspiration

Topcoat coffee-shop shape (layout, pages, models, forms) — not signals/`view!`.
