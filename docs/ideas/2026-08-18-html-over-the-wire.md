# Idea: HTML-over-the-wire (Thoxa web apps without a SPA)

**Status:** **active experiment** — 2026-08-18. Not a freeze, not product std,
not handbook.  
**Sparked by:** Crema initial charter (A–D2) is complete; next use of
`examples/http` is to dogfood Thoxa as a **whole-app web stack**, not to
grow the coffee-shop domain into `std/`.  
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · Crema checklist
[`2026-08-10-examples-http-crema-experiment.md`](2026-08-10-examples-http-crema-experiment.md)
(Phase E) · this file (protocol + runtime).  
**Related:** [JSX / `Node`](2026-08-13-jsx-node-syntax-experiment.md) (SSR
tree, on master); [webapp SSR / components](2026-07-21-webapp-ssr-components.md)
(sleeping longer fork); [route clauses](2026-08-12-router-clause-syntax-experiment.md);
[file-based routing](2026-08-15-file-based-routing.md) (**parked**);
[built-in validation](2026-08-17-builtin-validation.md).

---

## What this is

A person writing an entire web app in Thoxa should not need React, Solid,
Svelte, or a JSON API as the default interaction model.

The 90% case is **server-rendered HTML** plus **small HTML snippets** the
browser injects. GET snippets are real URLs, so **ordinary HTTP caching**
applies. POST is a form (or the same form via the tiny runtime) that returns
another snippet or a redirect.

Crema stays the sample. Domain stays in `examples/http/`. Product `std/`
grows only when a helper is clearly not cafe-specific (D97).

## Locked preferences (do not re-litigate without CURRENT)

1. **HTML is the payload.** Pages and fragments are `Node` → escaped HTML.
   JSON `/api/orders` stays as the **codec / `Type.read` demo**, not the
   cafe’s interaction style.
2. **No vendored HTMX / Unpoly / Turbo / Datastar / LiveView.** Same
   *paradigm* as HTMX (declarative fetch + swap). Own tiny runtime, so the
   stack is Thoxa’s, not a third-party SPA-in-disguise.
3. **Progressive enhancement.** `<a href>` and `<form method>` work with JS
   off (full document). The runtime is an accelerator, not a requirement.
4. **Cacheable fragments have their own GET URLs.** Do **not** make one URL
   return a full page or a snippet based on `HX-Request` / `Accept` — that
   fights caches. `GET /menu` is the page; `GET /menu/items?q=` is the
   `<ul>` (or the runtime swaps `<main>` from a dedicated fragment path).
5. **SSE before WebSockets.** Live updates are `text/event-stream` pushing
   HTML or tiny events the runtime swaps. WebSockets wait until a sample
   needs client→server chatter SSE cannot do.
6. **No client VDOM, hydration, or islands.** The server owns the tree.
   The browser owns swap / history / EventSource.
7. **Example JS first.** Land the runtime under `examples/http/web/`.
   Promote to `std/` only when the surface is stable and cafe-free.

## As-built (today)

| Layer | Have |
|-------|------|
| SSR pages | JSX `Node` + `htmlResponse` + `type Page` |
| Routes | Type-body `GET`/`POST` on `Routes` |
| Forms / cookies | **E4** session cookie `crema`; PRG sign-in; **E1** HTML `POST /orders` + ticket page |
| Fragments | **E2** `GET /menu/items?q=` + `GET /orders/:id/ticket`; **D145** `htmlFragment` + content-hash ETag / 304 |
| Swap runtime | **E3** `web/crema.js` — `thx-get` / `thx-post` / `thx-target` / `thx-sse` (example-only) |
| Sessions | **E4** opaque `crema` cookie; flash; CSRF on form POSTs (in-memory) |
| SSE | **E5** `/live` board + `/events` stream (**D146**) |
| Persist | `/tmp/crema.db` |
| JSON demo | `POST /api/orders` + `CreateOrder.read` |
| HTTP host | One `Response` blob, `Content-Length`, keep-alive, `--> serveOne` |

Missing for this idea: WebSocket upgrade (parked). Session bag is
process-local, not sqlite.

## 90% fundamentals (target, not a ship list)

**In the cafe / stack (this idea):**

| Need | Why it’s 90% | First home |
|------|----------------|------------|
| Full-document SSR | First paint, no-JS, SEO, simple | **have** |
| Layouts + components | `type Page` / `DrinkCard` | **have** (JSX experiment) |
| HTML forms + PRG | Mutations without an API | **have** sign-in; order form is next |
| Fragment GET/POST | Search, order ticket, lists | **have** (E2 + **D145**) |
| Tiny swap runtime | Click/submit → fetch HTML → swap | **have** (`web/crema.js`; example-only) |
| History | `pushState` + popstate re-fetch | same runtime |
| Sessions | “your tickets”, not just a name | **have** (example Map; not sqlite) |
| CSRF | Cookie POSTs | **have** (hidden `csrf` on forms) |
| Flash | “Order placed” after redirect | **have** |
| SSE | Live order board / “now brewing” | **have** (**D146**) |
| Static assets | CSS, the runtime itself | **have** `std/http/static` |
| Validation | `Type.read` / `where` | **on master**; dogfood forms too |
| Persist | sqlite | **have** |

**Out until a sample forces them:**

- WebSockets (upgrade + framing + per-connection `-->`)
- Multipart / file uploads
- File-based routing (parked)
- Asset fingerprint / Tailwind pipeline
- Email, jobs, ORM, middleware stacks
- Client components, signals, hydration
- Pretty JSON APIs as the default UI

WebSockets are real, just **not the default live path**. SSE covers “server
tells the page something changed.”

## Runtime sketch (example-only until promoted)

A few kilobytes. No build step. Served as a static file. Layout includes
`<script src="/crema.js" defer></script>`.

Declarative attrs (hyphenated; JSX already allows `data-*` / custom names):

```html
<form thx-post="/orders" thx-target="#ticket" thx-swap="inner">
<input name="slug" value="mocha" type="hidden" />
…
</form>

<div thx-sse="/events" thx-target="#board" thx-swap="outer"></div>
```

v0 behavior:

- Intercept `thx-get` / `thx-post` on links and forms.
- `fetch` the href/action with the form body (`application/x-www-form-urlencoded`).
- Swap into `thx-target` (`inner` / `outer` / `append`).
- Optional `thx-push` updates history.
- `EventSource` on `thx-sse`; each event’s data is HTML to swap.

No special compiler syntax. No `hx-*` names (that’s HTMX’s protocol). No
requirement that the server speak `HX-*` headers.

## Language / std pressure (expected)

Crema still stops when the sample hits a real gap.

| Gap | Why it shows up | Likely product? |
|-----|-----------------|-----------------|
| Fragment helper | `htmlResponse` always wraps a full document | maybe `htmlFragment(node)` in `std/http` |
| Streaming / chunked write | `writeResponse` emits one `Content-Length` body | **yes**, before SSE can be honest |
| Long-lived GET | `serveOne` wants a finished `Response` | host change; `-->` already parks peers |
| Session helper | signed cookie or server table | only if not cafe-specific |
| Injected runtime | every Thoxa web app wants the same JS | `std/` **after** example JS settles |
| WebSocket upgrade | only if SSE is not enough | later |

Do **not** invent a component-to-JS compiler or a second `Node` for the
browser.

## Non-goals

- Shipping handbook “Thoxa is a web framework” before a freeze + D-id  
- Coffee-shop types in `std/`  
- Replacing the JSON order API (keep it; stop using it as the UI)  
- Vendoring htmx.org “for now” (that teaches HTMX, not Thoxa)

## Open (not locked)

- Exact attr prefix (`thx-` vs `data-thx-`).  
- Whether a full-page “boost” (`<body thx-boost>`) is v0 or later.  
- Session: signed cookie vs sqlite row.  
- CSRF: double-submit cookie vs hidden field.  
- Promote runtime to `std/` on which slice (not first).

## Crema slices live on the charter

Checklist is Phase **E** in
[`2026-08-10-examples-http-crema-experiment.md`](2026-08-10-examples-http-crema-experiment.md).
This file does not duplicate it.
