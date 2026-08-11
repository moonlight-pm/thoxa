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
| **app** | `app/` | Typed `Route` sum, multi-clause dispatch, `App.handle` |
| **entry** | `Main.thx` | Logging + `server.serve(routes.app())` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(app)
         framework owns accept / parse / body / query / write
         app.handle(req, body, query) per request

app → framework/{Wire, Router}     path table + helpers
framework → std + framework/*
framework → app/Routes only for `App` type (serve signature)
```

**Routing shape (what you have now):**

1. Framework `Router` — method + path **patterns** → `Hit` (pattern string + capture)  
2. App `routeOf` — `Hit` → typed `Route` sum (`Home | Menu | Drink | …`)  
3. App multi-clause `handleRoute(route: Home | Menu | …)` — **D131** type-driven dispatch  
4. Host still takes concrete `App` (stand-in for a future trait / fn pointer)

Domain modules (Pages, Models, …) are never imported by framework.

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
    Routes.thx    # Route sum, multi-clause handleRoute, App, buildRouter
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
```

### App configures the path table

```thx
router.newRouter("examples/http/web")
    .get("/")
    .get("/menu")
    .get("/menu/:slug")
    .get("/api/health")
    .post("/")
```

Hits become typed routes (`Home`, `Menu(q)`, `Drink(slug)`, …); dispatch is
multi-clause `handleRoute` rather than `match hit.name { "home" => … }`.

## Rules

- Grow `framework/`, not `std/`, for this experiment.
- App code = routes/views/domain only.
- Ideal later: fn values → zero framework→app imports.
