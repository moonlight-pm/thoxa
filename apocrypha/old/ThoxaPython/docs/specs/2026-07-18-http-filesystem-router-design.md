# HTTP filesystem Router — design

**Status:** implemented 2026-07-18 (`examples/http/`).
**Scope:** `examples/http/` — evolve the static one-file demo into a request-aware
filesystem router over `web/`.
**Files (planned):** `Http.thx`, `Net.thx`, `Main.thx`, new `Router.thx`, expanded `web/`.

## Why this exists

The HTTP example today is a real TCP stack (`Net`) and a couple of hard-coded response
builders (`Http`), but it never looks at the request: `Socket.receive` drains bytes and
throws them away, and `serve` always opens `examples/http/web/index.html`. That proves
syscalls; it does not prove protocol or static serving.

This design turns the example into a small but honest static file server:

1. Parse a real HTTP request into a `Request` value.
2. Map the URL path onto the `web/` tree (directory → `index.html`).
3. Build a real `Response` (status, headers, body) and serialize it to bytes.

It stays an **example**, not a stdlib HTTP framework: no keep-alive, no POST handlers, no
range requests. The types and module boundaries should still be ones we could later lift.

## Goals / non-goals

**Goals**

- Browse the site: `/` and nested paths resolve under `web/` and respond appropriately.
- First-class `Request` and `Response` types in `Http`.
- Full access to the request **except body** (body field stubbed empty for now).
- Filesystem `Router` module: root directory, path safety, extension → Content-Type.
- Directory paths serve `index.html` only (no directory listings).
- Small multi-page sample site under `web/` so the mapping is visible in a browser.
- Failures (missing file, bad path) become 404 responses, not process crashes.

**Non-goals**

- Directory listings, CGI, templates, MIME sniffing of content.
- Request body parsing / POST / forms.
- Keep-alive, pipelining, HTTP/2, TLS.
- URL-decoding percent escapes beyond what we need for a simple demo (paths are treated as
  raw path segments; `%2e%2e` style traversal is not decoded — rejected only as literal `..`
  segments unless we add decoding later).
- Absolute-URI request targets (`GET http://host/path`) — only origin-form paths.

## Architecture

Five modules, each with one job:

| Module | Responsibility |
|--------|----------------|
| `Net` | Sockets. `receive` returns the raw request `Bytes` (empty on close / error). |
| `Http` | Protocol: `Request` / `Response`, parse request, serialize response. Pure — no syscalls. |
| `Io` | File open/read/close (unchanged). |
| `Router` | Map `Request` → `Response` via a filesystem root. Uses `Io` + `Http`. |
| `Main` | Accept loop: receive → parse → route → send. |

Request flow:

```
accept
  → client.receive()        # Bytes (empty = peer closed)
  → http.parseRequest(raw)  # Request (or Fail)
  → router.handle(req)      # Response
  → http.writeResponse(res) # Bytes
  → client.send(bytes)
  → client.close()
```

## Protocol types (`Http.thx`)

### `Request`

```
type Request(
    method!: String,
    path!: String,
    version!: String,          # e.g. "HTTP/1.1"
    headers!: Array<Header>,
    body!: Bytes               # stub: always empty Bytes(0) for now
)
```

```
type Header(name!: String, value!: String)
```

**Parsing (`parseRequest(raw!: Bytes) -> Trial<Request, String>`):**

1. Find the first `\r\n\r\n` (header/body separator). If missing, fail `"incomplete request"`.
2. Work on the header block only (body bytes after the separator are **ignored**;
   `body` is set to `Bytes(0)` — the stub).
3. Split the header block into lines on `\r\n`.
4. First line: request-line. Split on spaces into method, target, version (exactly three
   tokens). Fail on malformed lines.
5. Target: if it contains `?`, strip the query string and keep only the path for routing
   (query is not stored in v1 — YAGNI; path is what the filesystem cares about).
6. Remaining lines: each `Name: value` → `Header`. Split on the first `:`; trim a single
   leading space from the value (HTTP's usual optional space after colon). Empty names fail.
7. Return `Pass(Request(...))`.

No method restriction at parse time. The router may still only serve GET-like static files
and answer others with 405 or treat them like GET for this demo — **decision: static router
answers every method the same for a successful file hit** (typical static-file behavior) so
we do not invent a method policy yet. Documented as intentional simplicity.

### `Response`

```
type Response(
    status!: Int64,            # e.g. 200, 404
    reason!: String,           # e.g. "OK", "Not Found"
    headers!: Array<Header>,
    body!: Bytes
)
```

**Helpers**

- `ok(body!, contentType!) -> Response` — 200, Content-Type + Content-Length + Connection: close.
- `notFound() -> Response` — 404, `text/plain` body `"Not Found\n"`.
- `badRequest(msg!) -> Response` — 400 for parse failures the loop chooses to answer.
- `writeResponse(res!) -> Bytes` — status line + headers + `\r\n` + body via `Buffer`/`freeze`.

Legacy `ok200` / `okFile` / `notFound` (bytes-returning) are **replaced** by the typed API so
there is one way to build responses. Call sites move to `writeResponse`.

Content-Length is always `body.count` (byte length), correct for binary bodies.

## Filesystem router (`Router.thx`)

```
type Router(root!: String) {   # e.g. "examples/http/web"
    export fn handle(req!: Request) -> Response { ... }
}
```

### Path resolution

Input: `req.path` (must start with `/`; if not, router returns 404 without I/O).

1. **Reject traversal and odd segments** before any I/O:
   - Split path on `/` into segments (empty segments from leading/trailing `/` are fine).
   - Any segment equal to `..` or `.` → 404 (do not resolve).
   - Empty path after strip → treat as `/`.
2. **Join** under `root` with `/` separators → candidate filesystem path.
3. **Directory rule:** try open candidate as a file first.
   - Open/read success → serve that file.
   - Open fails → try `candidate + "/index.html"` (for `/` this is `root/index.html`; for
     `/about` or `/about/` this is `root/about/index.html`).
   - That fails → 404.
4. No `stat`/`readdir` in v1: “is it a directory?” is encoded as “file open failed, try
   index.html”. That can false-positive a missing file into a second open of
   `missing/index.html`, which is fine (still 404).

### Content-Type

From the **final** path’s extension (case-sensitive ASCII, lowercased only if we already
have easy char walks; otherwise exact match on common lowercase extensions):

| Extension | Content-Type |
|-----------|--------------|
| `.html` / `.htm` | `text/html` |
| `.css` | `text/css` |
| `.js` | `application/javascript` |
| `.json` | `application/json` |
| `.txt` | `text/plain` |
| `.png` | `image/png` |
| `.jpg` / `.jpeg` | `image/jpeg` |
| `.gif` | `image/gif` |
| `.svg` | `image/svg+xml` |
| `.ico` | `image/x-icon` |
| other / none | `application/octet-stream` |

### I/O

Reuse `io.File`: open ReadOnly, read whole file, close. Open/read errors → 404 (do not leak
errno strings in the HTTP body; log optionally via `println` in Main if desired). Router’s
`handle` always returns a `Response` (never a Trial) so the accept loop stays simple.

## Plumbing changes

### `Net.Socket.receive`

Change from “return byte count, discard buffer” to:

```
export fn receive() -> Bytes {
    let buf = Bytes(recvBufSize)
    let n = sysRecv(fd, buf, recvBufSize, 0)
    if n <= 0 { Bytes(0) } else { buf.slice(0, n) }
}
```

One 4 KiB read remains enough for request-line + headers of this demo. Incomplete large
headers are out of scope.

### `Client` in `Main.thx`

- `receive() -> Bytes` (empty means closed / nothing to serve).
- `serve` becomes: parse → if Fail, send `badRequest`; else `router.handle` →
  `writeResponse` → `send`.

### Sample site (`web/`)

```
web/
  index.html          # home; links to /about/ and style.css
  style.css           # shared styles
  about/
    index.html        # nested directory → index.html
```

Keep pages tiny and self-explanatory so `curl` and a browser both make the routing obvious.

## Error handling summary

| Condition | HTTP outcome |
|-----------|--------------|
| Peer closed / empty recv | skip send, close connection |
| Malformed request | 400 Bad Request |
| `..` / `.` segment | 404 |
| File missing, no index | 404 |
| File open/read error | 404 |
| Success | 200 + typed Content-Type |

## Testing / manual verification

No new conformance suite for the live server (it binds a port). Manual checks after
implementation:

```bash
uv run thoxa run examples/http
# elsewhere:
curl -sD- http://127.0.0.1:9091/
curl -sD- http://127.0.0.1:9091/about/
curl -sD- http://127.0.0.1:9091/style.css
curl -sD- http://127.0.0.1:9091/nope
curl -sD- http://127.0.0.1:9091/../Http.thx   # must 404
```

If request parsing is unit-testable without sockets later, prefer pure functions on
`Bytes` fixtures — out of scope for this change set unless easy.

## Implementation order (for the plan)

1. Extend `Net.receive` to return `Bytes`.
2. Add `Header` / `Request` / `Response` + parse + write in `Http`.
3. Add `Router` with path safety, join, index fallback, Content-Type, file read.
4. Rewire `Main` accept loop.
5. Expand `web/` sample site.
6. Manual curl verification.

## Open points intentionally closed

- **Body:** present on `Request`, always empty; no Content-Length body read yet.
- **Query string:** stripped from path; not stored.
- **Method policy:** static serve ignores method differences.
- **Percent-decoding:** not implemented; literal `..` only.
- **Root path:** remains cwd-relative `"examples/http/web"` as today (run from repo root).
