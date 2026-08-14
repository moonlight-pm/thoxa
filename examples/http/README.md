# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

## Main (what you write to boot)

```thx
fn main() {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match server.serve(routes.port, routes.handle) {
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
| **app** | `app/` | Type-body `GET`/`POST` clauses, free `handle`, pages/domain |
| **entry** | `Main.thx` | Logging + `server.serve(routes.port, routes.handle)` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(app)
         framework owns accept / parse / body / query / write
         app.handle(req) per request

app → std/HttpStatic          miss → files under web/
framework → std + framework/*
framework → std + framework/* only (no app imports)
```

**Routing shape (what you have now):**

1. App `type Routes` — type-body `GET "/…"` / `POST "/…"` (compiler → `route(req)`)  
2. App `handle` — `Routes().route(req)`; **404** falls through to `std/HttpStatic`  
3. Framework completes `Request.body` and attaches the raw query (std still
   strips `?` and leaves body empty)  
4. Host takes `port` + `(req: Request) -> Response` (**D132**)

Domain modules (Pages, Models, …) are never imported by framework.

## Run

```bash
cargo make run examples/http
```

Port **9091** (`routes.port`).

## Layout

```text
examples/http/
  Main.thx
  framework/
    Server.thx    # serve(app), bindListen, readIncoming, writeResponse
    Wire.thx      # body, query, form, cookies, redirects, HEAD finalize
  app/
    Routes.thx    # type-body GET/POST, handle, static fallback
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
(so they are not wrapped as `text/plain`). Pages are **JSX `Node` trees**
(`type Page` in `Layout.thx`, views in `Pages.thx`, `.html()` + doctype in
`htmlResponse`). Miss is `notFound()`; `handle` then tries static files.

## Rules

- Grow `framework/`, not `std/`, for this experiment.
- App code = routes/views/domain only.
- Host **`--> serveOne` per accept**. An idle browser socket parks in
  `recv` and does not stall other tabs (experiment/implicit-async). HTTP/1.1
  keep-alive: one started peer can serve several requests. `handle` is an
  ordinary function passed as a **D132** pointer.
