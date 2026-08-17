# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std**
(the coffee-shop app). Host and wire helpers are product
`std/http` + `std/http/server` (**D136**/**D139**).

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
| **host** | `std/http/server` | Accept loop, body read, HEAD, keep-alive |
| **wire** | `std/http` | Parse/write, query/form/cookie Maps, redirects |
| **app** | `app/` | Type-body `GET`/`POST` clauses, free `handle`, pages/domain |
| **entry** | `main.thx` | Logging + `server.serve(routes.port, routes.handle)` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(port, handle)
         std/http/server owns accept / parse / body / write
         handle(req) per request

app → std/http/static          miss → files under web/
app → std/http                query / form / cookies / 303
```

**Routing shape (what you have now):**

1. App `type Routes` — type-body `GET "/…"` / `POST "/…"` (compiler → `route(req)`)
2. App `handle` — `Routes().route(req)`; **404** falls through to `std/http/static`
3. `parseRequest` keeps `Request.query`; `std/http/server.readBody` fills `Request.body`.
   Query / form / cookies parse to `Map<String, String>`. `Request.headers`
   stays `Array<Header>`.
4. Host takes `port` + `(req: Request) -> Response` (**D132**)

Domain modules (Pages, Models, …) are never imported by the host.

## Run

```bash
cargo make run examples/http
```

Port **9091** (`routes.port`).

JSON order API (`std/json` **D141**):

```bash
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{"slug":"mocha","qty":2}' http://127.0.0.1:9091/api/orders
# {"ok":true,"slug":"mocha","name":"Mocha","qty":2,"unitCents":600,"totalCents":1200}
```

## Layout

```text
examples/http/
  main.thx
  app/
    routes.thx    # type-body GET/POST, handle, static fallback
    pages.thx layout.thx api.thx models.thx customer.thx
  web/
```

### App writes the routes

```thx
type Routes {
    GET "/" (req: Request) -> Response { … }
    GET "/menu" (req: Request) -> Response { … }
    GET "/menu/:slug" (slug: String, req: Request) -> Response { … }
    GET "/api/health" -> Response { … }
    POST "/api/orders" (req: Request) -> Response { … }
    POST "/" (req: Request) -> Response { … }
}
```

Path captures (`:slug`) are clause formals. HTML handlers return `Response`
(so they are not wrapped as `text/plain`). Pages are **JSX `Node` trees**
(`type Page` in `layout.thx`, views in `pages.thx`, `std/http.htmlResponse`).
Miss is `notFound()`; `handle` then tries static files.

## Rules

- App code = routes/views/domain only.
- Host **`--> serveOne` per accept** (in `std/http/server`). An idle browser
  socket parks in `recv` and does not stall other tabs. HTTP/1.1 keep-alive:
  one started peer can serve several requests. `handle` is an ordinary
  function passed as a **D132** pointer.
