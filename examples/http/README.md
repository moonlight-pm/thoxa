# examples/http — Little Crema (experiment)

Multi-route HTTP/1.1 example in pure Thoxa. **Experiment, not product std** —
all app logic lives in this folder; product **`std/Net` + `std/Http` +
`std/HttpStatic` + `std/Logging`** are used as-shipped.

Charter + living checklist:
[`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](../../docs/ideas/2026-08-10-examples-http-crema-experiment.md).

## Run

From the **workspace root**:

```bash
cargo make run examples/http
```

Then:

```bash
curl -s  http://127.0.0.1:9091/
curl -s  http://127.0.0.1:9091/menu
curl -s  http://127.0.0.1:9091/menu/espresso
curl -s  http://127.0.0.1:9091/api/health
curl -sD- http://127.0.0.1:9091/style.css
curl -s  http://127.0.0.1:9091/menu/nope   # 404
curl -sI http://127.0.0.1:9091/menu        # HEAD
curl -s -X POST http://127.0.0.1:9091/     # 405
```

Stop with Ctrl-C. Port **9091** (`SO_REUSEADDR` on).

## Layout (as-built)

| Path | Role |
|------|------|
| `Main.thx` | Listen / accept / parse / send |
| `App.thx` | Method+path dispatch (pages, API, static fallback) |
| `Layout.thx` | Shared HTML shell |
| `Pages.thx` | `/`, `/menu`, `/menu/{slug}` |
| `Api.thx` | `/api/health` (no layout) |
| `Models.thx` | In-memory drink catalog |
| `web/` | Static assets (`style.css`, leftover files) |

## Routes (Phase A)

| Method | Path | Handler |
|--------|------|---------|
| GET/HEAD | `/` | Dynamic home (`Pages.home`) |
| GET/HEAD | `/menu` | Drink list |
| GET/HEAD | `/menu/{slug}` | Drink detail or 404 |
| GET/HEAD | `/api/health` | Plain `ok` |
| GET/HEAD | other | `std/HttpStatic` under `web/` |
| other | * | 405 |

## Rules

- **No `std/` additions** for this experiment.
- Language gaps → break, fix language, resume (see experiment charter).
- Phase B+ (forms, cookies, body) is checklist-only until CURRENT says so.

## Inspiration

Topcoat coffee-shop + manual-router patterns (layout, pages vs routes, models,
path params) — **not** a port of signals, `view!`, or middleware.
