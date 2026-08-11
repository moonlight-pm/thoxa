# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std.**

| Layer | Path | Role |
|-------|------|------|
| **framework** | `framework/` | Pure host library — **only imports `std/` + itself** |
| **app** | `app/` | Little Crema + accept-loop wiring (`Listen`) |
| **entry** | `Main.thx` | Boot: logging + `app/Listen.run` |
| **assets** | `web/` | Static files |

Charter: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Port **9091**. Ctrl-C to stop.

## Layout

```text
examples/http/
  Main.thx                 # entry only
  framework/               # candidate std — closed world
    Server.thx             # bindListen, readIncoming, writeResponse
    Wire.thx               # body, query, form, headers, 303, HEAD
    Router.thx             # named routes, :param, serveStatic
  app/
    Listen.thx             # accept loop (wires framework → Routes)
    Routes.thx             # buildRouter + handle + dispatch
    Pages.thx Layout.thx Api.thx Models.thx Customer.thx
  web/
  README.md
```

### Dependency direction

```text
Main → app/Listen → framework/Server + app/Routes
app  → framework/{Server, Wire, Router}
framework → std only     (never app/, never package-root mounts)
```

Composition root is **app/Listen** (application owns “run the host with my handler”). Framework stays a library.

### Framework API (host)

| API | Role |
|-----|------|
| `Server.bindListen(port)` | Listening socket |
| `Server.readIncoming(peer)` | `Incoming` (or quiet-close sentinel) |
| `Server.requestOf(inc)` | Rebuild `std/Http.Request` |
| `Server.writeResponse(peer, method, res)` | HEAD finalize + send |
| `Router.newRouter` / `.get` / `.post` / `.resolve` / `.serveStatic` | App-facing routing |
| `Wire.*` | Body, query, form, cookies helpers |

### App router setup

```thx
router.newRouter("examples/http/web")
    .get("/", "home")
    .get("/menu", "menu")
    .get("/menu/:slug", "drink")
    .get("/api/health", "health")
    .post("/", "signIn")
```

## Rules

- **No `std/` growth** — grow `framework/` instead.
- **`framework/` imports only `std/` and `framework/*`.**
- Directory names **lowercase**.
