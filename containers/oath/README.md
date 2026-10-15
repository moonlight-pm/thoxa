# containers/oath — oath.wicket.cloud

Public site for **https://oath.wicket.cloud/**. Written in Thoxa. Front page
is the OS pitch. `GET /map` is a short page (not the
Archify HTML — serving that file through Thoxa OOMs on aulos).

Not product std. Not an `examples/` demo.

**Live (aulos):** https://oath.wicket.cloud/ — workload `oath`
(`wl-b6703b0fa806`), image `registry.wicket.cloud/oath-landing:latest`.

## Local

From the workspace root (asset paths are cwd-relative):

```bash
cargo make run containers/oath
# prints http://127.0.0.1:8090/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_PORT` | `8090` locally (landing is 8088, sola 8089). Image sets `8080`. |
| `THOXA_STATIC` | `containers/oath/web` |

## Image + Wicket

```bash
cargo make emit-c containers/oath
podman build -f containers/oath/Containerfile \
  --ignorefile containers/oath/.containerignore \
  -t oath-landing:latest -t registry.wicket.cloud/oath-landing:latest .
wicketctl registry push aulos --image oath-landing:latest
wicketctl workload apply aulos --file containers/oath/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP`.
