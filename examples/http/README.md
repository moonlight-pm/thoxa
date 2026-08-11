# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

| Layer | Path | Role |
|-------|------|------|
| **framework** | `framework/` | Host pipeline — candidate `std/` (**never imports `app/`**) |
| **app** | `app/` | Little Crema — routes, pages, domain |
| **mount** | `Handler.thx` | App-owned shim; only thing framework imports for handlers |
| **entry** | `Main.thx` | Boot: logging + `server.run(port)` |
| **assets** | `web/` | Static files |

Product **`std/Net` + `std/Http` + `std/HttpStatic` + `std/Logging`** as-shipped.

Charter: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Port **9091**. Ctrl-C to stop.

## Layout

```text
examples/http/
  Main.thx                 # entry: logging + framework.Server.run
  Handler.thx              # mount: re-exports app/Routes.handle
  framework/
    Server.thx             # accept loop, parse, body/query, write → Handler
    Wire.thx               # body, query, form, headers, 303, HEAD
    Router.thx             # named routes, :param, serveStatic
  app/
    Routes.thx             # buildRouter() + handle() + dispatch
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
  README.md
```

### Dependency direction

```text
Main → framework/Server → Handler.thx → app/Routes → app/*
                       ↘ framework/Wire
app  → framework/{Wire, Router}
framework ↛ app/*          (only ~/Handler at package root)
```

`Handler.thx` is the app mount until Thoxa has first-class function values
(`server.run(port, app.handle)`).

### Framework router (app-facing)

```thx
router.newRouter("examples/http/web")
    .get("/", "home")
    .get("/menu", "menu")
    .get("/menu/:slug", "drink")
    .get("/api/health", "health")
    .post("/", "signIn")
```

`resolve` → `May<Hit(name, param)>`. Unmatched → `serveStatic`.
GET also matches HEAD; `Wire.finalize` strips the body.

## Rules

- **No `std/` growth** — grow `framework/` instead.
- **`framework/` never imports `app/*`** — only `~/Handler`.
- Directory names are **lowercase**.
