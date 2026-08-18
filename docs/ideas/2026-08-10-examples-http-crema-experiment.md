# Experiment: `examples/http` Crema-lite (Topcoat-shaped app dogfood)

**Status:** **active experiment** — **initial charter (A–D2) complete**.
App/domain is not product surface; not a freeze. Host/wire helpers were
promoted to **`std/http` + `std/http/server` (**D136**/**D139**)**.
**Expansion:** Phase **E** — HTML-over-the-wire cafe (see
[`2026-08-18-html-over-the-wire.md`](2026-08-18-html-over-the-wire.md)).
**Date:** 2026-08-10 (expansion 2026-08-18)  
**Home:** [`examples/http/`](../../examples/http/)  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) (focus) · this file (charter + checklist) ·
[`examples/http/README.md`](../../examples/http/README.md) (run + layout)  
**Related ideas:** [HTML-over-the-wire](2026-08-18-html-over-the-wire.md)
(web-app dogfood); [JSX / `Node`](2026-08-13-jsx-node-syntax-experiment.md);
[webapp SSR / components](2026-07-21-webapp-ssr-components.md)
(sleeping longer fork — not a component system)

---

## What this is

Grow the HTTP example from a **static-file demo** into a small **multi-route
server app** inspired by [Topcoat](https://github.com/tokio-rs/topcoat)
patterns (coffee-shop / manual-router shape). **A–D2** proved the host/wire
and language gaps. **Phase E** uses the same cafe to dogfood Thoxa as a
**whole-app web stack** (SSR + fragments + tiny runtime) without putting
cafe domain in `std/` and without vendoring a JS framework.

Inspiration sources (read-only):

- Topcoat demo: coffee-shop (layout, pages, models, forms, path params)
- Topcoat examples: `manual-router`, `request-response`, `path-query-params`

Thoxa already has product **`std/net` + `std/http` + `std/http/static`** (**D126**/**D139**).
This experiment **dogfoods** that stack under realistic app structure.

## Hard rules

1. **App/domain stays in `examples/http/`.** Pages, models, cookie policy, and
   route clauses are not product std. Host/wire already live in `std/` (**D136**).
2. **Not official product.** Handbook must not present Crema-lite routing /
   layouts as language features. Capability row stays an **example** experiment.
3. **Language breaks allowed.** If the sample is blocked by a real language
   gap, stop, fix the language (conformance + CURRENT), then resume. Further
   std helpers only when they are clearly product (sample-driven, D97).
4. **One slice at a time.** Update the checklist in this file + `CURRENT.md`
   when a phase/step lands.
5. **No second handoff files.** This idea + CURRENT + example README are the
   pickup path (progress model).

## Non-goals (do not port)

- `view!` / component macros / client signals / shards / procedures  
- Async runtime, ORM, Tailwind asset pipeline  
- Module-tree auto-routing (file-based routing stays **parked**)  
- Middleware / layer stacks  
- Growing product `std/` with coffee-shop domain types  
- Vendoring HTMX / Unpoly / Turbo / LiveView (same *paradigm*, own runtime)

---

## Target shape (app vs framework)

```text
examples/http/
  main.thx                 # std/http/server.serve(routes.port, routes.handle)
  app/
    routes.thx             # type Routes { GET/POST … }, handle, port
    pages.thx layout.thx api.thx models.thx customer.thx store.thx
  web/
  README.md
```

**Layers:** App is routes/views/domain (free `handle` + `port`). Product
`std/http/server` owns plumbing via `serve(port, handle)`. The host imports no
app types (**D132**). Main is boot only.

**Router:** type-body `GET "/…"` / `POST "/…"` on a nullary `Routes` type
(compiler → `route(req)`). Path params are `:slug` clause formals. `handle`
calls `route` and falls through to `std/http/static` on 404.

---

## Phases (living checklist)

Update status in place: `pending` | `in progress` | `done` | `blocked` | `parked`.

### Phase A — Dynamic multi-route app (no protocol growth)

**Goal:** Real multi-module GET app on today’s std. No body / cookies / POST.

| Step | Status | Notes |
|------|--------|--------|
| A0 Charter + CURRENT pointer + hard rules | **done** | this file |
| A1 Multi-module skeleton; listen loop thin | **done** | `Main` + `App` + modules |
| A2 Layout + dynamic `/` and `/menu` | **done** | now JSX `Node` (`Page`) |
| A3 Models + `/menu/:slug` path param | **done** | in-memory catalog; `roast: Roast` (**D135**); `:param` not `{braces}` (interp) |
| A4 `GET /api/health` + README polish | **done** | no layout shell |
| A5 Smoke matrix (curl) green | **done** | `/` `/menu` `/menu/{slug}` `/api/health` CSS HEAD 404 405 |

**Phase A exit criteria**

```bash
cargo make run examples/http
curl -s  http://127.0.0.1:9091/
curl -s  http://127.0.0.1:9091/menu
curl -s  http://127.0.0.1:9091/menu/espresso
curl -s  http://127.0.0.1:9091/api/health
curl -sD- http://127.0.0.1:9091/style.css
curl -s  http://127.0.0.1:9091/menu/nope   # 404
```

### Phase B — Forms and cookies (protocol pressure; still no `std/` growth)

**Goal:** Topcoat-style “remember me” name form with PRG.

| Step | Status | Notes |
|------|--------|--------|
| B1 Request body available to the app | **done** | Example-only: `Wire.readBody` after `receiveRequest` + `Content-Length` + `Socket.receive` (std `Request.body` still empty) |
| B2 Form urldecode in example package | **done** | `Wire.formField` / `urlDecode` |
| B3 303 + Location helper (example) | **done** | `Wire.seeOther` |
| B4 Cookie set/read (example) | **done** | `Customer.thx` |
| B5 POST `/` sign-in + forget + nav greeting | **done** | PRG; empty name forgets |

**B1 resolution (promoted D136):** Body is completed in `std/http/server.readBody`
using the raw header buffer + optional further `receive()` for `Content-Length`.
`std/Http.parseRequest` still leaves `Request.body` empty; the host fills it
before `handle`.

**Phase B smoke**

```bash
curl -sD- -c /tmp/c.ck -X POST -d 'name=Ada' http://127.0.0.1:9091/
curl -s -b /tmp/c.ck http://127.0.0.1:9091/   # Hi, Ada
curl -sD- -b /tmp/c.ck -c /tmp/c.ck -X POST -d 'name=' http://127.0.0.1:9091/
```

### Phase C — Query string search (optional)

| Step | Status | Notes |
|------|--------|--------|
| C1 Keep raw query on request (product or parse raw in example) | **done** | Example-only: `Wire.rawQuery` / `queryParam` (std still strips `?` from path) |
| C2 `GET /menu?q=` filter | **done** | Server-side filter + GET form; `Models.searchDrinks` |

**C1 resolution (promoted D136):** `parseRequest` keeps `Request.query` (no
leading `?`) and still strips path. The `X-Thoxa-Query` header hack is gone.

**Phase C smoke**

```bash
curl -s  'http://127.0.0.1:9091/menu?q=mocha'     # Mocha
curl -s  'http://127.0.0.1:9091/menu?q=chocolate' # Cold Brew + Mocha
curl -s  'http://127.0.0.1:9091/menu?q=zzz'       # empty + hint
```

### Phase D — JSON POST + persist (initial charter; **complete**)

| Step | Status | Notes |
|------|--------|-------|
| D1 `POST /api/orders` | **done** | `std/json` `decode`/`encode`; `CreateOrder.read` (`qty: Qty` where `>= 1`, `slug: DrinkSlug`); 200 priced ticket / 400 `{ok:false}` |
| D2 persistence | **done** | App `store.thx`; `/tmp/crema.db`; drinks seeded + orders persisted; `GET /api/orders`. No product `thoxa migrate` |
| D3 htmx-ish partials, sessions | **superseded** | Was parked “only if useful.” Replaced by Phase **E** (not vendored HTMX) |

**Initial charter (A–D2) is finished.** The JSON order API stays as the codec /
`Type.read` demo. Further cafe work is **web-stack dogfood**, not more
coffee-shop domain in `std/`.

### Phase E — HTML-over-the-wire cafe (web-framework dogfood)

**Goal:** The cafe UI is SSR HTML + cacheable fragments + a tiny own runtime.
Protocol and non-goals:
[`2026-08-18-html-over-the-wire.md`](2026-08-18-html-over-the-wire.md).

| Step | Status | Notes |
|------|--------|-------|
| E0 Charter + CURRENT pointer | **done** | this file + html-over-the-wire idea |
| E1 HTML order on the drink page | **pending** | `POST` form, PRG or ticket HTML; no JS; JSON `/api/orders` stays |
| E2 Cacheable fragment routes | **pending** | Own GET URLs (e.g. menu items, ticket); not `HX-Request` content negotiation |
| E3 Tiny swap runtime in `web/` | **pending** | Example JS; `thx-get` / `thx-post` / `thx-target`; progressive enhancement |
| E4 Sessions + flash (example) | **pending** | Beyond the name cookie; CSRF if POSTs stay cookie-authed |
| E5 Live board via SSE | **pending** | Forces streaming / chunked HTTP; not WebSockets |
| E6 WebSockets | **parked** | Only if a sample needs client→server chatter SSE cannot do |

**Phase E smoke (as slices land — not a gate yet)**

```bash
# E1 — form works with JS off
curl -sD- -X POST -d 'slug=mocha&qty=2' http://127.0.0.1:9091/orders
```

---

## Topcoat → Thoxa mapping

| Topcoat | This experiment |
|---------|-----------------|
| `#[layout]` | `Layout.thx` function wrapping body HTML |
| `#[page]` | functions in `Pages.thx` |
| `#[route]` | functions in `Api.thx` |
| Manual `Router::builder` | type-body `GET`/`POST` clauses on `Routes` → `route(req)` |
| `path_param` | `:slug` capture → `Drink(slug)` (and friends) on the route value |
| Models / seed | `models.thx` types + seed; `store.thx` SQLite |
| `current_customer(cx)` | Phase B `Customer.thx` |
| `view!` | JSX `Node` tags (`type Page` + `.html()`) |

---

## Session handoff (for agents)

1. Read this file’s **Phases** table (truth for experiment progress).  
2. Read `examples/http/README.md` (as-built layout).  
3. **Do not** add coffee-shop domain to `std/`. Host/wire already shipped
   (**D136** / **D137** / **D142**).
4. If blocked by language: document under Phase step as `blocked`, fix language
   as its own CURRENT slice, return here.  
5. After any step: flip checklist rows + update `CURRENT.md` Now line.

**As of last update:** **Phase A–D2 complete** (initial charter). **Phase E**
is the active expansion (E0 done; E1 next). Host/wire promoted to
**`std/http` + `std/http/server` (**D136**/**D139**)** — example `framework/` removed.
**D137:** `queryParam` / `formField` / `cookie` / `htmlResponse` + Str
`parseUInt` / `splitOnce`. **D142:** `Cookie` / `withCookie` / `clearCookie`
(Crema remember/forget no longer format `Set-Cookie` by hand). **`server.serve(routes.port, routes.handle)`**
(**D132**). Routing is type-body **`GET`/`POST` clauses** on `Routes` + free
**`handle`** static fallback. **Pages are JSX `Node`**. Host **`--> serveOne`**
+ HTTP/1.1 keep-alive. **Map dogfood (D134):** query/form/cookies are
`Map<String, String>`. Headers stay `Array<Header>` (no std flip).
**D135:** `Drink.roast: Roast` + `DrinkCard(d: Drink)`.
**D141:** `POST /api/orders` via `std/json` `decode`/`encode`.
**D2:** `std/sqlite` file at `/tmp/crema.db` (app-owned schema; no product
migrate). Domain stays example-only. Do not vendor HTMX.

### Language / design notes (experiment)

| Gap | Workaround | Product break? |
|-----|------------|----------------|
| Constructor-param fields must be Int64 / Bool / String / Bytes / Array / Map — **not** user sum types | **Done (D135):** `Drink.roast: Roast` + `DrinkCard(d: Drink)`. **D140** also allows `Float64` fields. | Shipped |
| String interp treats `{…}` as interpolation | Path params use `:slug` not `{slug}` | Document; escape story later if needed |
| Session desugar only tracked `{}` depth (mangled multi-line type fields) | **Fixed** — nesting tracks `()` / `[]` too | Shipped compiler fix |
| Multi-file package diags painted on open buffer | **Fixed** — `Error.path` + per-URI publish | Shipped compiler/LSP fix |
| `server.serve(app)` needs typed `App` without fn values | **Done (D132):** `serve(port, handle)`; framework imports no app types | Traits still separate. Full FCF parked |
| Single-threaded `accept` + blocking `recv` | **`--> serveOne`** + HTTP/1.1 keep-alive loop (implicit-async); 200ms `SO_RCVTIMEO` removed | — |
| String path table → dispatch | **Type-body `GET`/`POST` clauses** → generated `route(req)` | Miss is `notFound()` (no fallthrough); `handle` treats 404 as static try. Query/body are product `Request` fields (**D136**) |
| Linear-scan query / form / cookies | **`parseQuery` / `parseForm` / `parseCookies` → `Map<String, String>`** | `Request.headers` stays `Array<Header>` (D134). `m[k]` needs `May` in scope |
