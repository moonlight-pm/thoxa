# Experiment: `examples/http` Crema-lite (Topcoat-shaped app dogfood)

**Status:** **active experiment** — not product surface; not a freeze; not `std/` work.  
**Date:** 2026-08-10  
**Home:** [`examples/http/`](../../examples/http/)  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) (focus) · this file (charter + checklist) ·
[`examples/http/README.md`](../../examples/http/README.md) (run + layout)  
**Related ideas:** [webapp SSR / components](2026-07-21-webapp-ssr-components.md)
(language-level components — **out of scope** here; string HTML only)

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

## Target shape (app layout)

```text
examples/http/
  Main.thx       # listen / accept / parse / body / send
  App.thx        # method+path dispatch → page | api | static fallback
  Layout.thx     # shared HTML shell (nav, title, greeting)
  Pages.thx      # GET/HEAD pages + POST / sign-in
  Api.thx        # non-page routes (e.g. GET /api/health)
  Models.thx     # in-memory drink catalog
  Wire.thx       # body, form, redirect, header helpers (example-only)
  Customer.thx   # Cookie remember / forget / current
  web/           # static assets only (CSS, optional leftover files)
  README.md      # run instructions + architecture (as-built example)
```

**Dispatch order:** exact dynamic routes → path-param pages → static
`HttpStatic.Router` → 404 from static path.

---

## Phases (living checklist)

Update status in place: `pending` | `in progress` | `done` | `blocked` | `parked`.

### Phase A — Dynamic multi-route app (no protocol growth)

**Goal:** Real multi-module GET app on today’s std. No body / cookies / POST.

| Step | Status | Notes |
|------|--------|--------|
| A0 Charter + CURRENT pointer + hard rules | **done** | this file |
| A1 Multi-module skeleton; listen loop thin | **done** | `Main` + `App` + modules |
| A2 Layout + dynamic `/` and `/menu` | **done** | string HTML |
| A3 Models + `/menu/{slug}` path param | **done** | in-memory catalog; roast is `String` (see language notes) |
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

**B1 resolution (no product break):** Body is completed in `examples/http/Wire.thx`
using the raw header buffer + optional further `receive()` for
`Content-Length`. Product `std/Http.parseRequest` still leaves `Request.body`
empty — the app passes `body: Bytes` separately into `App.handle`. Promote to
std only if a product slice is scheduled later.

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
in `Wire.rawQuery`; passed into `App.handle` as a third `query: String` arg.

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
| Manual `Router::builder` | `App.handle` if-chain |
| `path_param` | path prefix + slug segment in `App` / `Pages` |
| Models / seed | `Models.thx` constants |
| `current_customer(cx)` | Phase B `Customer.thx` |
| `view!` | string templates with `{interp}` |

---

## Session handoff (for agents)

1. Read this file’s **Phases** table (truth for experiment progress).  
2. Read `examples/http/README.md` (as-built layout).  
3. **Do not** add to `std/` unless the user reopens that rule.  
4. If blocked by language: document under Phase step as `blocked`, fix language
   as its own CURRENT slice, return here.  
5. After any step: flip checklist rows + update `CURRENT.md` Now line.

**As of last update:** **Phase A + B + C complete** (query search smoke green).
Next is Phase D+ only if wanted — still no `std/` growth.

### Language notes hit during Phase A

| Gap | Workaround | Product break? |
|-----|------------|----------------|
| Constructor-param fields must be Int64 / Bool / String / Bytes / Array — **not** user sum types | `Drink.roast: String` instead of `Roast` union | Optional later language slice; not blocking experiment |
| Module-level `Router(root: …)` typed oddly (`Void` for root) | Construct `Router` inside `routeRead` | Investigate only if it bites again |
