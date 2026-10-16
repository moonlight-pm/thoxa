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

## Headers

`finish` in `app/routes.thx` adds them to every response (D172): a
Content-Security-Policy (styles and images from here, fonts from
`fonts.gstatic.com` with the Google Fonts stylesheet; no script, frames, or
forms), `nosniff`, `Referrer-Policy`, and HSTS. A 200 carries an ETag, so a
repeat request gets a 304. Pages and CSS are `no-cache` (checked on every
load); images and fonts are reused for a day. A page that loads something new
needs the CSP widened there.

## Image + Wicket

The image builds `thoxa` from the build context, emits the site's C, and
compiles it with `thoxa_rt` (Debian gcc). It is not tied to the host linker,
and an image built from a commit runs that commit's Thoxa, not C the host
emitted earlier. Bases
are pinned by digest (bump them by hand); the server runs as UID 65534 and, as
PID 1, ends on SIGTERM (D170). The first build pulls the Rust base image; the
compiler then builds in about 15 s, and the three site images share that stage.

```bash
podman build -f containers/landing/Containerfile \
  --ignorefile containers/landing/.containerignore \
  -t registry.wicket.cloud/thoxa-landing:latest .
wicketctl registry push aulos --image thoxa-landing:latest
wicketctl workload apply aulos --file containers/landing/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP`.
