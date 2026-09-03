# Standard library (`std/`)

Import product modules with the **`std/…`** named root:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"
import logging, { info, warn } from "std/logging"   ## ns + selective (D127)
import { readFile } from "std/io"
import net, { Socket } from "std/net"
import http from "std/http"
import server from "std/http/server"
import { Router } from "std/http/static"
import { defaultPrompt } from "std/prompt"
import { startsWith, dropPrefix } from "std/str"
import { cwdShort, shortPath } from "std/path"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/ansi"
import { localHms, userName, hostname } from "std/env"
import { branch } from "std/git"
import json, { Json, decode, encode } from "std/json"
import sqlite, { lit, ident } from "std/sqlite"
```

## Modules (current)

| Module | Role |
|--------|------|
| `std/shell` | Session `type Shell` + process builtins; auto-injected for session scripts (pulls fallible + logging) |
| `std/fallible` | Exported `May<T> = Has<T!> \| Not?`, `Trial<P,F> = Pass<P!> \| Fail<F?>` — **import required** for programs (D123; not ambient) |
| `std/logging` | Default process logger (**D125**): `trace`/`debug`/`info`/`warn`/`error`/`emit`; configure via namespace; short emit via selective import. No ambient `println` |
| `std/io` | Files (**D126**): `readFile` / `writeFile` / `appendFile`, `File`, OpenMode |
| `std/net` | TCP sockets (**D126**): `Socket`, `receiveRequest`, `sendAll` |
| `std/http` | HTTP/1.1 protocol (**D126**/**D136**/**D137**/**D142**/**D145**/**D146**): parse/write, query/form/cookie helpers, `htmlResponse` / `htmlResponseStatus` / `htmlFragment`, `cached` / ETag, `Cookie` / `withCookie`, `streamed()` / `peerFd` |
| `std/http/server` | HTTP/1.1 host loop (**D136**/**D139**/**D146**): `serve` / `serveOne` / `readBody`; SSE `beginEventStream` / `sendEvent` |
| `std/http/static` | Static-file `Router` (**D126**/**D139**); depends on io + http |
| `std/prompt` | `defaultPrompt(signature: … = "#3B9DC2")` — session-style prompt |
| `std/str` | match/search (incl. ignore-case), slice/`byteAt`/`truncate`/`reverse`, replace, `split`/`splitOnce`/`join`/`lines`, `parseUInt`, pad/trim, `removePrefix`/`removeSuffix`, `isAscii` |
| `std/path` | `dirname`/`parent`/`basename`/`fileName`/`join`/`joinAll`/`parts`/`stripTrailingSlash`/`ensureTrailingSlash`/`isAbsolute`/`isRelative`/`normalize`/`extension`/`hasExtension`/`stem`/`withExtension`/`withFileName`/`cwd`/`shortPath`/`cwdShort` |
| `std/ansi` | `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` |
| `std/env` | `getenv`, `hasEnv`, `getenvOr`, `home`, `tmpdir`, `shell`, `pathVar`, `pwd`, `localHms`, `hostname`, `userName` |
| `std/git` | `branchName` / `branchSuffix` / `branch` / `branchHere` / `isRepo` / `repoRoot` / `isDetached` / `headLine` via `.git/HEAD` |
| `std/json` | Dynamic `Json` tree (**D141**): `decode` / `encode`. **`Issue`** + `message` for **`Type.read`** (**D149**) |
| `std/sqlite` | Embedded SQLite (**D143**/**D144**): `open` / `exec` / `query`; `sql"…"` |

### Logging (default logger)

**Preferred import style** (apps): namespace for configure + selective level names
for short call sites (see `examples/http`):

```thx
import logging, { info, warn, error, debug } from "std/logging"

fn main() -> Int64 {
    info("listen")                   ## stderr, full format, min level Info
    logging.setLevel(logging.levelDebug)
    debug("detail")
    match logging.toFile("/tmp/app.log") {
        Pass(_) { }
        Fail(e) { error("log file: {e}") }
    }
    ## Golden / data on stdout:
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    info("payload")
    0
}
```

Namespace-only or selective-only still work. Combined
`import logging, { info, … } from "std/logging"` is the usual app face (**D127**).

| Knob | Default |
|------|---------|
| Min level | Info (`levelInfo`) |
| Sink | stderr (`toStderr`) |
| Format | full (`HH:MM:SS LEVEL message`) |

Env (applied once on first use): `THOXA_LOG_LEVEL`, `THOXA_LOG_SINK`. Multiple
named loggers (`initLog`) are **not** shipped yet.

### Io (`std/io`)

Linux file I/O. Fail payloads are **String** (`"op: strerror"` or a short
message).

| Surface | Notes |
|---------|--------|
| `readFile` / `writeFile` / `appendFile` | Whole-file helpers; close on both success and failure after open |
| `openFile(path, flags = oRdonly, mode = oMode644)` | Returns `File` |
| `File` | `readAll`, `write`, `seek`, `position`, `close`, `isOpen`. Last RC runs `drop()` which closes (**D155**) |
| `OpenMode` | `Rdonly \| Wronly \| Rdwr` + `openModeBits` |
| Flag lets | `oRdonly` / `oWronly` / `oRdwr` / `oCreat` / `oTrunc` / `oAppend` / `oMode644` |
| Seek | `seekSet` / `seekCur` / `seekEnd` |

### Net (`std/net`)

IPv4 TCP (`AF_INET` stream). Fail payloads are String.

| Surface | Notes |
|---------|--------|
| `createSocket()` | `Trial<Socket, String>` |
| `Socket` | `reuseAddr`, `bind`, `listen`, `accept`, `connect`, `localPort`, `close`, `isOpen`. `close` writes `fd = -1` (second close is a no-op, same as `File`). Last RC runs `drop()` which closes (**D155**) |
| `receive()` | One `recv` chunk (empty on close/error) |
| `receiveRequest()` | Until `\r\n\r\n` or 64 KiB cap / peer close |
| `send` / `sendAll` | One write vs drain partial writes |

### Http (`std/http`)

Pure protocol (no syscalls). `parseRequest` fills `query` (no leading `?`) and
leaves `body` empty; hosts complete the body via `std/http/server.readBody`.

| Surface | Notes |
|---------|--------|
| `Header` / `Request` / `Response` | Constructor-param products. `Request.query` is the `?…` tail |
| `parseRequest` | `Trial<Request, String>`; requires `\r\n\r\n`; path is query-stripped |
| `writeResponse` | Status line + headers + body → `Bytes` |
| Lookup | `headerGet`, `contentLength`, `queryMap`, `queryParam`, `formField`, `cookie` |
| Stream | `Request.peerFd` (host fd, else 0); `streamed()` (status 0 — handler owns the socket) |
| Maps | `parseQuery` / `parseForm` / `parseCookies` → `Map<String, String>`; `urlDecode` |
| Cookies | `Cookie` (`.header()`), `SameSite`, `withCookie`, `clearCookie` (**D142**) |
| Response | `ok`, `okHead`, `notFound`, `forbidden`, `badRequest`, `methodNotAllowed`, `seeOther`, `withHeader`, `htmlResponse`, `htmlResponseStatus`, `htmlFragment`, `notModified`, `cached` / `etagOf` / `withCache`, `response` / `responseHead` |
| Connection | `wantsKeepAlive`, `finalize` (HEAD drops body; rewrites `Connection`) |

`htmlResponse` takes a builtin `Node` (**D147**) and sends `<!DOCTYPE html>` plus
`root.html()` as `text/html; charset=utf-8` (status 200). `htmlResponseStatus`
is the same at a chosen status (HTML 404 / 400). `htmlFragment` is the snippet
without the doctype. `cached(req, res, cacheControl)` attaches a
strong content-hash `ETag` and `Cache-Control`; a matching `If-None-Match`
returns **304**. Cache *policy* (`private, no-cache` vs `max-age`) stays in
the caller. `cookie` looks up one name on the `Cookie` header (empty value
is `Not`).

`Cookie(name, value, …)` builds a `Set-Cookie` value. Defaults: `path="/"`,
`sameSite="Lax"`, session (`maxAge` is `Not`), not HttpOnly/Secure. Empty
`path` / `domain` omit those attributes. `withCookie` appends the header;
`clearCookie(name)` expires it (`Max-Age=0`). Values are not escaped. App
policy (which name, how long, HttpOnly) stays in the caller.

### HttpServer (`std/http/server`)

Host loop over `std/net`. Completes `Request.body` (64 KiB cap).

| Surface | Notes |
|---------|--------|
| `serve(port, handle)` | Listen forever; `--> serveOne` per accept (**D132** handle, **D148** nowait). Logs `http://127.0.0.1:{port}/`. App `handle` is often `Routes().route` (**D150**) |
| `serveOne(peer, handle)` | One keep-alive connection |
| `bindListen(port)` | `createSocket` + reuse + bind + listen |
| `readBody` / `readRequest` | `Content-Length` entity; empty method/path = quiet close |
| `writeResponse(peer, method, res, keepAlive)` | `finalize` + `sendAll` |
| `beginEventStream` / `sendEvent` | SSE prelude + `data:` lines (**D146**). `streamed()` skips the normal write |

HEAD is answered by GET handlers; `finalize` still sees the real method.

### HttpStatic (`std/http/static`)

| Surface | Notes |
|---------|--------|
| `Router(root: String)` | `handle(req) -> Response` — always a Response (never Trial) |
| Methods | **GET** and **HEAD** only; else **405** + `Allow` |
| Paths | Absolute (`/…`); `.` / `..` → 404; directories serve `index.html` |
| Types | `.html` `text/html`; `.css` `text/css`; `.js` `text/javascript`; `.txt` `text/plain`; `.jpg`/`.jpeg` `image/jpeg`; `.png` `.gif` `.svg` `.webp` `.ico`; `.woff`/`.woff2`; else octet-stream |
| Errors | Permission denied → **403** (logged); missing → **404**; other Io → log + 404 |

Runnable demos: `cargo make run examples/http` (Crema-lite **experiment** —
`server.serve(routes.port, routes.handle)` on product `http` / `http/server` / `http/static`;
app/domain is not product std; see `examples/http/README.md`).
`cargo make run containers/landing` — thoxa.dev (port **8088**; news sign-up).
`cargo make run containers/sola` — sola.computer teaser (port **8089**; ISO notify).

Hex colors: primary `#RRGGBB`, also `#RRGGBBAA` (alpha ignored for SGR). Invalid
hex → black SGR.

### Json (`std/json`)

Dynamic tree. Callers map to domain types by hand (**D141**). Fail payloads
are String.

```thx
import { decode, encode } from "std/json"
import { Pass, Fail } from "std/fallible"
import { info, error } from "std/logging"

match decode("[1,true,\"mocha\"]") {
    Fail(e) => error(e)
    Pass(v) => info(encode(v))
}
```

Object literals in Thoxa strings need `{{` / `}}` (`"{{\"qty\":2}}"` is
`{"qty":2}`). Match `JObj` / `JInt` / … to walk the tree; `qty: 2` stays
`JInt`.

| Surface | Notes |
|---------|--------|
| `Json` | `JNull \| JBool \| JInt \| JFloat \| JStr \| JArr \| JObj` |
| `decode(s)` | `Trial<Json, String>`. One value; trailing junk fails |
| `encode(v)` | Compact JSON. Object keys in insertion order |
| Numbers | No `.` / exponent → `JInt` (Int64 range or fail). Else `JFloat`. Non-finite fails |
| Strings | Escapes `\" \\ \/ \b \f \n \r \t \uXXXX` including surrogate pairs |
| `Issue` | `Missing \| Mismatch \| Rejected` — fail-fast shape/refinement for **`Type.read`** (**D149**). `message(e)` |
| Typed read | Compiler-attached `T.read(s)` / `T.from(x)` / `t.encode()` — language, not a derive. See [language.md](language.md#refinements-and-typeread) |
| Not shipped | Pretty-print, comments, trailing commas, `NaN` / `Infinity` tokens, `jsonResponse`, collect-all issues |

### Sqlite (`std/sqlite`)

Embedded SQLite (**D143**). The amalgamation is compiled into `libthoxa_rt`.
Write **`sql"…"`** (**D144**) — a tagged interpolator (no space after `sql`).
Holes are embedded as SQL: strings are quoted, numbers are digits, `Sql`
fragments (`ident`, `null`, `lit`) are spliced. `exec` / `query` take `Sql`.

```thx
import sqlite from "std/sqlite"
import { ident } from "std/sqlite"

let db = sqlite.open(":memory:")?
db.exec(sql"
    CREATE TABLE orders (
        slug TEXT,
        qty  INTEGER
    )
")?
db.exec(sql"
    INSERT INTO orders (slug, qty)
    VALUES ({slug}, {qty})
")?
let rows = db.query(sql"
    SELECT slug, qty
    FROM {ident(table)?}
")?
```

| Surface | Notes |
|---------|--------|
| `sql"…"` | Tagged interpolator → `Sql`. Import `std/sqlite` so `Sql` exists |
| `open(path)` | `Trial<Db, String>`. `":memory:"` is allowed |
| `Db` | `exec` / `query` / `close` / `isOpen`. Last RC runs `drop()` which closes (**D155**) |
| `Db.exec` / `Db.query` | Take `Sql`. `query` → `Array<Row>` |
| `Row` | `cells: Map<String, SqlValue>` |
| `SqlValue` | `SqlNull \| SqlInt \| SqlFloat \| SqlText` |
| `ident(name)` | Quoted `"name"` as `Sql` (`Trial`; empty fails) |
| `null` / `lit` | `NULL` token / explicit literal fragment |
| Numbers | `{qty}` → digits |

A plain `"… {slug} …"` string is still concatenation. `sql"…"` is the
safe form. `sql "…"` (space) is not a tag. Both `"…"` and `sql"…"` may
span lines; newlines are kept (no dedent).

Not shipped: blobs, prepared `?` binds, explicit transactions, extension
loading, general `tag"…"` interpolators.

## Resolution order

1. `THOXA_STD` if set  
2. `{THOXA_ROOT}/std`  
3. Workspace `std/`  
4. `std/` next to the `thoxa` binary  

## Limits

This is a **thin** library grown from real samples. Fallibility types live in
`std/fallible` and are imported like other std modules. Named **`ffi`** root is
deferred. I/O and HTTP are **Linux product track** (x86_64 sockaddr pack, Linux
open flags). Windows is out of scope. Fail payloads are String (not an
`Error` protocol).

New helpers land when real samples need them — not via a free-floating
“expand the stdlib” backlog.

Source tree: [`../../std/`](../../std/).
