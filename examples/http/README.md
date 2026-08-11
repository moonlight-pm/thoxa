# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

| Layer | Path | Role |
|-------|------|------|
| **framework** | `framework/` | Host helpers — candidate future `std/` (no app imports) |
| **app** | `app/` | Little Crema — routes, pages, domain |
| **entry** | `Main.thx` | Glue: accept loop + call `app/Routes` |
| **assets** | `web/` | Static files |

Product **`std/Net` + `std/Http` + `std/HttpStatic` + `std/Logging`** as-shipped.

Charter: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

```bash
curl -s  http://127.0.0.1:9091/
curl -s  'http://127.0.0.1:9091/menu?q=mocha'
curl -s  http://127.0.0.1:9091/menu/espresso
curl -s  http://127.0.0.1:9091/api/health
curl -sD- -c /tmp/crema.ck -X POST -d 'name=Ada' http://127.0.0.1:9091/
```

Port **9091**. Ctrl-C to stop.

## Layout

```text
examples/http/
  Main.thx                 # entry glue (accept loop; no framework→app)
  framework/
    Server.thx             # bindListen only
    Wire.thx               # body, query, form, headers, 303, HEAD finalize
    Router.thx             # named routes + :param + static fallback
  app/
    Routes.thx             # buildRouter() + handle() + dispatch
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
  README.md
```

### Dependency direction

```text
Main → framework/{Server,Wire}  (host)
Main → app/Routes               (application)
app  → framework/{Wire,Router}  (app uses framework APIs)
framework ↛ app                 (never)
```

### Framework router (app-facing)

```thx
router.newRouter("examples/http/web")
    .get("/", "home")
    .get("/menu", "menu")
    .get("/menu/:slug", "drink")   ## :param segment (not {braces} — string interp)
    .get("/api/health", "health")
    .post("/", "signIn")
```

`resolve(method, path)` → `May<Hit(name, param)>`. Unmatched → `serveStatic`.
GET routes also match HEAD; `Wire.finalize` strips the body.

App maps Hit names to handlers in `dispatch` (string names until fn values exist).

## Routes

| Method | Path | Handler name |
|--------|------|----------------|
| GET/HEAD | `/` | `home` |
| POST | `/` | `signIn` |
| GET/HEAD | `/menu` | `menu` (`?q=` in app) |
| GET/HEAD | `/menu/:slug` | `drink` |
| GET/HEAD | `/api/health` | `health` |
| GET/HEAD | other | static `web/` |

## Rules

- **No `std/` growth** — grow `framework/` instead.
- **`framework/` never imports `app/`.**
- Directory names are **lowercase**.
- Language gaps → fix language, then resume.
