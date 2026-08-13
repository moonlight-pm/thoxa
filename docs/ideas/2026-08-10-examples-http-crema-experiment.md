# Experiment: `examples/http` Crema-lite (Topcoat-shaped app dogfood)

**Status:** **active experiment** — not product surface; not a freeze; not `std/` work.  
**Date:** 2026-08-10  
**Home:** [`examples/http/`](../../examples/http/)  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) (focus) · this file (charter + checklist) ·
[`examples/http/README.md`](../../examples/http/README.md) (run + layout)  
**Related ideas:** [webapp SSR / components](2026-07-21-webapp-ssr-components.md)
(language-level components — **out of scope** here; pages now use experiment
JSX / `Node` tags, not a component system)

---

## What this is

Grow the HTTP example from a **static-file demo** into a small **multi-route
server app** inspired by [Topcoat](https://github.com/tokio-rs/topcoat)
patterns (coffee-shop / manual-router shape), without building a framework.

Inspiration sources (read-only):

- Topcoat demo: coffee-shop (layout, pages, models, forms, path params)
- Topcoat examples: `manual-router`, `request-response`, `path-query-params`

Thoxa already has product **`std/Net` + `std/Http` + `std/HttpStatic`** (**D126**).
This experiment **dogfoods** that stack under realistic app structure.

## Hard rules

1. **No `std/` growth for this experiment.** All new app logic lives under
   `examples/http/`. Product std stays as-shipped.
2. **Not official product.** Handbook must not present Crema-lite routing /
   layouts as language features. Capability row stays an **example** experiment.
3. **Language breaks allowed.** If the sample is blocked by a real language
   gap, stop, fix the language (conformance + CURRENT), then resume. Do **not**
   work around by inventing std helpers “just for the sample” — keep helpers
   in the example package.
4. **One slice at a time.** Update the checklist in this file + `CURRENT.md`
   when a phase/step lands.
5. **No second handoff files.** This idea + CURRENT + example README are the
   pickup path (progress model).

## Non-goals (do not port)

- `view!` / component macros / client signals / shards / procedures  
- Async runtime, ORM, Tailwind asset pipeline  
- Module-tree auto-routing  
- Middleware / layer stacks  
- Growing product `std/Http*` for this experiment  

---

## Target shape (app vs framework)

```text
examples/http/
  Main.thx                 # server.serve(routes.app())
  framework/
    Server.thx             # serve(app) — full host loop
    Wire.thx               # body / query / form / cookies
  app/
    Routes.thx             # type Routes { GET/POST … }, App.handle, app()
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
  README.md
```

**Layers:** App is routes/views/domain (`App` value with `handle`). Framework owns
all plumbing via `serve(app)`. Framework imports **only** the `App` type from
`app/Routes` for the serve signature (stand-in for future fn values / traits);
it does not import pages or models. Main is boot only.

**Router:** type-body `GET "/…"` / `POST "/…"` on a nullary `Routes` type
(compiler → `route(req)`). Path params are `:slug` clause formals. `App.handle`
calls `route` and falls through to `std/HttpStatic` on 404.

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
| A3 Models + `/menu/:slug` path param | **done** | in-memory catalog; roast is `String`; `:param` not `{braces}` (interp) |
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

**B1 resolution (no product break):** Body is completed in
`framework/Wire.thx` / `Server.readIncoming` using the raw header buffer +
optional further `receive()` for `Content-Length`. Product
`std/Http.parseRequest` still leaves `Request.body` empty — `Server.requestOf`
puts the completed body on the `Request` the app sees. Promote to std only if
a product slice is scheduled later.

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

**C1 resolution (no product break):** Query recovered from the raw request-line
in `framework/Wire.rawQuery`; `Server.requestOf` attaches it as example-only
header `X-Thoxa-Query` (`Wire.requestQuery`) because std `Request` has no query
field.

**Phase C smoke**

```bash
curl -s  'http://127.0.0.1:9091/menu?q=mocha'     # Mocha
curl -s  'http://127.0.0.1:9091/menu?q=chocolate' # Cold Brew + Mocha
curl -s  'http://127.0.0.1:9091/menu?q=zzz'       # empty + hint
```

### Phase D+ — Parked

htmx-ish partials, persistence, JSON POST APIs, sessions — only if still useful
after A–C.

---

## Topcoat → Thoxa mapping

| Topcoat | This experiment |
|---------|-----------------|
| `#[layout]` | `Layout.thx` function wrapping body HTML |
| `#[page]` | functions in `Pages.thx` |
| `#[route]` | functions in `Api.thx` |
| Manual `Router::builder` | type-body `GET`/`POST` clauses on `Routes` → `route(req)` |
| `path_param` | `:slug` capture → `Drink(slug)` (and friends) on the route value |
| Models / seed | `Models.thx` constants |
| `current_customer(cx)` | Phase B `Customer.thx` |
| `view!` | JSX `Node` tags (`type Page` + `.html()`) |

---

## Session handoff (for agents)

1. Read this file’s **Phases** table (truth for experiment progress).  
2. Read `examples/http/README.md` (as-built layout).  
3. **Do not** add to `std/` unless the user reopens that rule.  
4. If blocked by language: document under Phase step as `blocked`, fix language
   as its own CURRENT slice, return here.  
5. After any step: flip checklist rows + update `CURRENT.md` Now line.

**As of last update:** **Phase A–C complete** + layered **`framework/` / `app/`**
with **`server.serve(routes.app())`**. Routing is type-body **`GET`/`POST`
clauses** on `Routes` (compiler `route(req)` dogfood) + **`App.handle`** static
fallback. **Pages are JSX `Node`** (`app/Layout.thx` `Page`, `app/Pages.thx`).
Next is Phase D+ only if wanted — still no `std/` growth (grow `framework/`
instead). Language discussion next: traits / free fn pointers for the host
boundary (FCF parked).

### Language / design notes (experiment)

| Gap | Workaround | Product break? |
|-----|------------|----------------|
| Constructor-param fields must be Int64 / Bool / String / Bytes / Array — **not** user sum types | `Drink.roast: String` instead of `Roast` union | Optional later language slice |
| String interp treats `{…}` as interpolation | Path params use `:slug` not `{slug}` | Document; escape story later if needed |
| Session desugar only tracked `{}` depth (mangled multi-line type fields) | **Fixed** — nesting tracks `()` / `[]` too | Shipped compiler fix |
| Multi-file package diags painted on open buffer | **Fixed** — `Error.path` + per-URI publish | Shipped compiler/LSP fix |
| `server.serve(app)` needs typed `App` without fn values | Framework imports **`App` type only** from `app/Routes` | Ideal: **trait** or free **fn pointer** (no capture); full FCF parked |
| String path table → dispatch | **Type-body `GET`/`POST` clauses** → generated `route(req)` | Miss is `notFound()` (no fallthrough); `handle` treats 404 as static try. Query/body still example-stitched onto `Request`. Host boundary still `App` |
