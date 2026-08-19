# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std**
(the coffee-shop app). Host and wire helpers are product
`std/http` + `std/http/server` (**D136**/**D139**).

**Initial charter (A–D2) is done:** SSR pages, forms/cookies, search, JSON
`POST /api/orders`, SQLite persist. **Next** is Phase **E**
(HTML-over-the-wire cafe — own tiny runtime, not HTMX). Charter:
[`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Main (what you write to boot)

```thx
fn main() {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match store.open() {
        Fail(error) {
            warn("store error: {error}")
            1
        }
        Pass {
            match server.serve(routes.port, routes.handle) {
                Pass => 0
                Fail(error) {
                    warn("server error: {error}")
                    1
                }
            }
        }
    }
}
```

## Layers

| Layer | Path | Role |
|-------|------|------|
| **host** | `std/http/server` | Accept loop, body read, HEAD, keep-alive |
| **wire** | `std/http` | Parse/write, query/form/cookie Maps, `Cookie` / `withCookie`, redirects |
| **app** | `app/` | Type-body `GET`/`POST` clauses, free `handle`, pages/domain, SQLite store |
| **entry** | `main.thx` | Logging + `store.open` + `server.serve(routes.port, routes.handle)` |
| **assets** | `web/` | Static files |

```text
Main → server.serve(port, handle)
         std/http/server owns accept / parse / body / write
         handle(request) per request

app → std/http/static          miss → files under web/
app → std/http                query / form / cookies / Set-Cookie / 303
app → std/json                POST /api/orders decode/encode
app → std/sqlite              /tmp/crema.db (catalog + orders)
```

**Routing shape (what you have now):**

1. App `type Routes` — type-body `GET "/…"` / `POST "/…"` (compiler → `route(req)`)
2. App `handle` — `Routes().route(request)`; **404** falls through to `std/http/static`
3. `parseRequest` keeps `Request.query`; `std/http/server.readBody` fills `Request.body`.
   Query / form / cookies parse to `Map<String, String>`. `Request.headers`
   stays `Array<Header>`.
4. Host takes `port` + `(request: Request) -> Response` (**D132**)

Domain modules (Pages, Models, …) are never imported by the host.

## Run

```bash
cargo make run examples/http
```

Port **9091** (`routes.port`).

HTML order (JS off; **E1**):

```bash
curl -sD- -X POST -d 'slug=mocha&qty=2' http://127.0.0.1:9091/orders
# 303 Location: /orders/N
curl -s http://127.0.0.1:9091/orders/N
# Ticket N · 2 × Mocha
```

Swap runtime (**E3**, example-only `web/crema.js`): menu search uses
`thx-get="/menu/items"`; drink order uses `thx-post="/orders"` then loads
`/orders/N/ticket`. Forms still work with JavaScript off.

Cacheable fragments (**E2**, no layout; content-hash `ETag`):

```bash
curl -sD- 'http://127.0.0.1:9091/menu/items?q=mocha'
# 200 + ETag + Cache-Control: private, no-cache
curl -sD- -H 'If-None-Match: "<etag>"' 'http://127.0.0.1:9091/menu/items?q=mocha'
# 304
curl -s http://127.0.0.1:9091/orders/N/ticket
# <article id="ticket">…</article>
```

JSON order API (`std/json` **D141**; rows in `/tmp/crema.db`):

```bash
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{"slug":"mocha","qty":2}' http://127.0.0.1:9091/api/orders
# {"ok":true,"id":1,"slug":"mocha","name":"Mocha","qty":2,"unitCents":600,"totalCents":1200}
curl -s http://127.0.0.1:9091/api/orders
# [{"id":1,"slug":"mocha",…}]
```

The catalog is seeded from `app/models.thx` on boot. Schema lives in
`app/store.thx` (`CREATE TABLE IF NOT EXISTS` + upsert). Delete
`/tmp/crema.db` to start clean. There is no product `thoxa migrate`.

## Layout

```text
examples/http/
  main.thx
  app/
    routes.thx    # type-body GET/POST, handle, static fallback
    pages.thx layout.thx api.thx orders.thx models.thx customer.thx store.thx
  web/
```

### App writes the routes

```thx
type Routes {
    GET "/" (request: Request) -> Response { … }
    GET "/menu" (request: Request) -> Response { … }
    GET "/menu/items" (request: Request) -> Response { … }
    GET "/menu/:slug" (slug: String, request: Request) -> Response { … }
    GET "/api/health" -> Response { … }
    GET "/api/orders" -> Response { … }
    POST "/api/orders" (request: Request) -> Response { … }
    POST "/orders" (request: Request) -> Response { … }
    GET "/orders/:id/ticket" (id: Int64, request: Request) -> Response { … }
    GET "/orders/:id" (id: Int64, request: Request) -> Response { … }
    POST "/" (request: Request) -> Response { … }
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
