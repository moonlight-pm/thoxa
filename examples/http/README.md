# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

## Main (what you write to boot)

```thx
fn main() {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match server.serve(routes.app()) {
        Pass => 0
        Fail(e) {
            warn("server error: {e}")
            1
        }
    }
}
```

## Layers

| Layer | Path | Role |
|-------|------|------|
| **framework** | `framework/` | Host: accept loop, wire I/O, router, HEAD |
| **app** | `app/` | Routes, views, domain — builds `App` with `handle` |
| **entry** | `Main.thx` | Logging + `server.serve(routes.app())` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(app)
         framework owns accept / parse / body / query / write
         app.handle(req, body, query) per request

app → framework/{Wire, Router}     configure router, use helpers
framework → std + framework/*
framework → app/Routes only for `App` type (serve signature)
```

The `App` type import is the monorepo stand-in for a future
`server.serve(port, handle)` with first-class functions (or a trait). Domain
modules (Pages, Models, …) are never imported by framework.

## Run

```bash
cargo make run examples/http
```

Port **9091** (from `App.port` / `routes.app()`).

## Layout

```text
examples/http/
  Main.thx
  framework/
    Server.thx    # serve(app), bindListen, readIncoming, writeResponse
    Wire.thx
    Router.thx    # .get/.post, :param, serveStatic
  app/
    Routes.thx    # type App { handle }, app(), buildRouter, dispatch
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
```

### App configures the router

```thx
router.newRouter("examples/http/web")
    .get("/", "home")
    .get("/menu", "menu")
    .get("/menu/:slug", "drink")
    .get("/api/health", "health")
    .post("/", "signIn")
```

## Rules

- Grow `framework/`, not `std/`, for this experiment.
- App code = routes/views/domain only.
- Ideal later: fn values → zero framework→app imports.
