# workloads/landing — thoxa.moonlight.pm

Public site for **thoxa.moonlight.pm**, written in Thoxa. Two pages:

| Path | What |
|------|------|
| `/` | Design rationale, features, samples, status, building (`app/pages/home.thx`) |
| `/manual` | The handbook (`docs/handbook/*.md`) rendered as one page (`app/pages/manual.thx`) |

Anything else falls through to the static files in `web/`, then a 404 page.

Not product std. Not an `examples/` demo.

**Live (aulos):** https://thoxa.moonlight.pm/ — workload `thoxa`
(`wl-cada9b4cd855`), image `registry.wicket.cloud/thoxa-landing:latest`.
thoxa.dev is parked: no A record, no route (D246).

## How it works

- `app/markdown.thx` renders the subset of Markdown the handbook keeps to
  (headings, paragraphs, lists, fenced code, pipe tables, blockquotes, inline
  code, emphasis, links). Sections are numbered; a link to another chapter
  becomes an anchor on the page, any other relative link a GitHub URL. The
  rules for writing that subset are in `docs/handbook/README.md`.
- `app/highlight.thx` colours Thoxa code blocks (comments, strings with their
  holes, numbers, keywords, type names, calls, tag literals) and shell
  comments.
- `app/samples.thx` reads `samples/*.thx`. A sample's leading `##` lines are
  its title and description; a sibling `.out` is its output. `cargo make test`
  runs every sample that has an `.out` and compares the output, and
  typechecks the others (D189). Adding a sample: write the file, add its name
  to `names` in `app/samples.thx`, and record its `.out` if it terminates
  deterministically.
- Pages are rendered per request from the files on disk (the manual takes
  about 30 ms), so an edit to the handbook shows on the next load locally.

## Local

From the workspace root (paths are cwd-relative):

```bash
cargo make run workloads/landing
# prints http://127.0.0.1:8088/ and opens it (xdg-open)
# THOXA_NO_BROWSER=1 skips the browser
```

| Env | Default |
|-----|---------|
| `THOXA_PORT` | `8088` locally (Crema is 9091). Image sets `8080`. |
| `THOXA_STATIC` | `workloads/landing/web` |
| `THOXA_MANUAL` | `docs/handbook` |
| `THOXA_SAMPLES` | `workloads/landing/samples` |

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
emitted earlier. The final stage also copies `docs/handbook` (as
`/app/manual`) and `samples/` (as `/app/samples`), so the deployed manual is
the handbook of the commit the image was built from. Bases are pinned by
digest (bump them by hand); the server runs as UID 65534 and, as PID 1, ends
on SIGTERM (D170). The first build pulls the Rust base image; the compiler
then builds in about 15 s.

```bash
podman build -f workloads/landing/Containerfile \
  --ignorefile workloads/landing/.containerignore \
  -t registry.wicket.cloud/thoxa-landing:latest .
wicketctl registry push aulos --image thoxa-landing:latest
wicketctl workload apply aulos --file workloads/landing/workload.toml --confirm
```

Wicket injects `PORT` / `WICKET_PORT_HTTP`.
