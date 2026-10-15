# containers/landing — thoxa.dev

Public site for **thoxa.dev**. Written in Thoxa. Serves the Paper landing
(Byzantine pass). No download yet, and no sign-up.

Not product std. Not an `examples/` demo.

**Live (aulos):** https://thoxa.dev/ — workload `thoxa`
(`wl-cada9b4cd855`), image `registry.wicket.cloud/thoxa-landing:latest`.

## Local

From the workspace root (asset paths are cwd-relative):

```bash
cargo make run containers/landing
# prints http://127.0.0.1:8088/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_PORT` | `8088` locally (Crema is 9091). Image sets `8080`. |
| `THOXA_STATIC` | `containers/landing/web` |

## Image + Wicket

`cargo make build` on this machine produces a host-linked binary (on NixOS,
Nix glibc). The Containerfile **recompiles** the emitted C plus `thoxa_rt` with
Debian gcc so the image is portable.

```bash
cargo make emit-c containers/landing
podman build -f containers/landing/Containerfile \
  --ignorefile containers/landing/.containerignore \
  -t registry.wicket.cloud/thoxa-landing:latest .
wicketctl registry push aulos --image thoxa-landing:latest
wicketctl workload apply aulos --file containers/landing/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP`.
