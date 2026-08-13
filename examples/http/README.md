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
| **framework** | `framework/` | Host: accept loop, wire I/O, HEAD, body/query stitch |
| **app** | `app/` | Type-body `GET`/`POST` clauses, `App.handle`, pages/domain |
| **entry** | `Main.thx` | Logging + `server.serve(routes.app())` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(app)
         framework owns accept / parse / body / query / write
         app.handle(req) per request

app → std/HttpStatic          miss → files under web/
framework → std + framework/*
framework → app/Routes only for `App` type (serve signature)
```

**Routing shape (what you have now):**

1. App `type Routes` — type-body `GET "/…"` / `POST "/…"` (compiler → `route(req)`)  
2. App `handle` — `Routes().route(req)`; **404** falls through to `std/HttpStatic`  
3. Framework completes `Request.body` and attaches the raw query (std still
   strips `?` and leaves body empty)  
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
    Wire.thx      # body, query, form, cookies, redirects, HEAD finalize
  app/
    Routes.thx    # type-body GET/POST, App.handle, static fallback
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
```

### App writes the routes

```thx
type Routes {
    GET "/" (req: Request) -> Response { … }
    GET "/menu" (req: Request) -> Response { … }
    GET "/menu/:slug" (slug: String, req: Request) -> Response { … }
    GET "/api/health" -> Response { … }
    POST "/" (req: Request) -> Response { … }
}
```

Path captures (`:slug`) are clause formals. HTML handlers return `Response`
(so they are not wrapped as `text/plain`). Miss is `notFound()`; `handle`
then tries static files.

## Rules

- Grow `framework/`, not `std/`, for this experiment.
- App code = routes/views/domain only.
- Ideal later: fn values → zero framework→app imports.
