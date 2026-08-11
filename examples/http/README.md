# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

## Layers

| Layer | Path | Responsibility |
|-------|------|----------------|
| **framework** | `framework/` | All host plumbing (library). **Imports only `std/` + `framework/*`.** |
| **app** | `app/` | Routes, views, domain only. **No listen/accept/parse/send.** |
| **entry** | `Main.thx` | Process composition root: run the host, call `app` once per request. |
| **assets** | `web/` | Static files |

Until Thoxa has first-class function values (`server.run(port, app.handle)`),
**only Main can see both sides**. That is not “application logic”; it is the
binary’s wiring. Putting that wiring under `app/` was the wrong place.

```text
Main (composition)
  ├── framework/Server   bindListen · readIncoming · writeResponse
  ├── framework/Wire     body · query · form · HEAD · redirects
  ├── framework/Router   .get/.post · :param · serveStatic
  └── app/Routes.handle  routes + views + domain
```

```text
framework  ↛  app     (never)
app        →  framework
Main       →  framework + app
```

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Port **9091**. Ctrl-C to stop.

## Layout

```text
examples/http/
  Main.thx                 # composition root (plumbing glue only)
  framework/
    Server.thx             # bindListen, readIncoming, writeResponse, …
    Wire.thx               # body, query, form, headers, 303, HEAD
    Router.thx             # named routes, :param, static fallback
  app/
    Routes.thx             # buildRouter() + handle() + dispatch
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
  README.md
```

### App configures the framework router

```thx
router.newRouter("examples/http/web")
    .get("/", "home")
    .get("/menu", "menu")
    .get("/menu/:slug", "drink")
    .get("/api/health", "health")
    .post("/", "signIn")
```

`handle` maps Hits to page/api functions. No sockets, no HEAD special cases
(HEAD is applied in `framework` `writeResponse`).

### Ideal shape (needs language support)

```thx
server.run(9091, routes.handle)   ## fn values / handlers as values
```

Then Main shrinks to logging + that call, and the accept loop lives entirely
inside `framework/Server` with zero knowledge of `app/`’s module path.

## Rules

- **No `std/` growth** — grow `framework/`.
- **`framework/` imports only `std/` and `framework/*`.**
- **`app/` is routes/views/domain only.**
- Directory names **lowercase**.
