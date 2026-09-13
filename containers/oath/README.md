# containers/oath — oath.wicket.cloud

Public site for **https://oath.wicket.cloud/**. Written in Thoxa. Front page
is the OS pitch plus a news sign-up. `GET /map` is a short page (not the
Archify HTML — serving that file through Thoxa OOMs on aulos).

Not product std. Not an `examples/` demo.

**Live (aulos):** https://oath.wicket.cloud/ — workload `oath`
(`wl-b6703b0fa806`), image `registry.wicket.cloud/oath-landing:latest`,
database `news` (`db-6d8fbcbdc05a`).

## Local

From the workspace root (asset paths are cwd-relative):

```bash
cargo make run containers/oath
# prints http://127.0.0.1:8090/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_DATABASE` | `/tmp/oath-news.sqlite` (also accepts `sqlite:///path`) |
| `THOXA_PORT` | `8090` locally (landing is 8088, sola 8089). Image sets `8080`. |
| `THOXA_STATIC` | `containers/oath/web` |

`POST /signup` stores a unique email in SQLite.

## Image + Wicket

```bash
cargo make emit-c containers/oath
podman build -f containers/oath/Containerfile \
  --ignorefile containers/oath/.containerignore \
  -t oath-landing:latest -t registry.wicket.cloud/oath-landing:latest .
# Host `wicket` podman cannot pull registry.wicket.cloud (401). Load the
# image into that user's store, then apply:
podman save -o /tmp/oath-landing.tar oath-landing:latest
scp -i ~/.config/wicket/deploy/wicket-deploy -o IdentitiesOnly=yes \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  /tmp/oath-landing.tar root@10.0.0.146:/tmp/oath-landing.tar
# on aulos, cwd /var/lib/wicket:
runuser -u wicket -- env HOME=/var/lib/wicket XDG_RUNTIME_DIR=/run/user/$(id -u wicket) \
  podman load -i /tmp/oath-landing.tar
runuser -u wicket -- env HOME=/var/lib/wicket XDG_RUNTIME_DIR=/run/user/$(id -u wicket) \
  podman tag localhost/oath-landing:latest registry.wicket.cloud/oath-landing:latest
wicketctl workload apply aulos --file containers/oath/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP` and `WICKET_DATABASE_NEWS`.
The image default database path is `/data/news.sqlite`. `sqlite:///path`
URLs are accepted.
