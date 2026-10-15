# containers/sola — sola.computer

Public teaser for **sola.computer**. Written in Thoxa. Serves the Paper
**Teaser** (desktop + mobile).

Not product std. Not an `examples/` demo.

Paper: [sola.computer](https://app.paper.design/file/01KZF8TSPFDJZ4APR05E2ADXBJ) —
artboards Teaser · Desktop / Teaser · Mobile.

## Local

From the workspace root (asset paths are cwd-relative):

```bash
cargo make run containers/sola
# prints http://127.0.0.1:8089/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_PORT` | `8089` locally (landing is 8088). Image sets `8080`. |
| `THOXA_STATIC` | `containers/sola/web` |

## Image + Wicket

```bash
cargo make emit-c containers/sola
podman build -f containers/sola/Containerfile \
  --ignorefile containers/sola/.containerignore \
  -t registry.wicket.cloud/sola-landing:latest .
wicketctl registry push aulos --image sola-landing:latest
wicketctl workload apply aulos --file containers/sola/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP`.
