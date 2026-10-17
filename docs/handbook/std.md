# Standard library

The standard library is a set of Thoxa modules under `std/`, imported by path:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"
import logging, { info, warn } from "std/logging"
import { readFile } from "std/io"
import net, { Socket } from "std/net"
import http from "std/http"
import server from "std/http/server"
import { Router } from "std/http/static"
import json, { Json, decode, encode } from "std/json"
import sqlite, { lit, ident } from "std/sqlite"
import { indexOf, padLeft } from "std/str"
import { cwdShort, shortPath } from "std/path"
import { localHms, userName, hostname } from "std/env"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/ansi"
import { branch } from "std/git"
import { defaultPrompt } from "std/prompt"
```

The library is small and grows when a real program needs something. Failures
are reported as `Trial<…, String>` with a short message
(`"open: No such file or directory"`), except where noted. File and socket code is Linux-specific.
The source is in [`std/`](../../std/).

## Modules

| Module | Contents |
|--------|----------|
| `std/fallible` | `May<T>` (`Has` / `Not`) and `Trial<P, F>` (`Pass` / `Fail`) |
| `std/logging` | The process logger: `trace`, `debug`, `info`, `warn`, `error`, `emit`; level, sink, format |
| `std/io` | Files: `readFile`, `writeFile`, `appendFile`, `File`, open modes |
| `std/net` | TCP sockets: `Socket`, `createSocket`, `receiveRequest`, `sendAll` |
| `std/http` | HTTP/1.1 messages: parse and write, query / form / cookie helpers, HTML responses, ETags, cookies |
| `std/http/server` | An HTTP/1.1 server loop: `serve`, `serveOne`, `readBody`, server-sent events |
| `std/http/static` | A static-file `Router` |
| `std/json` | The `Json` tree, `decode` / `encode`, and `Issue` for typed reads |
| `std/sqlite` | Embedded SQLite: `open`, `exec`, `query`, `sql"…"` |
| `std/str` | Byte-indexed string helpers: positions, padding, `splitOnce`, `parseUInt` (search, case, trimming, and splitting are [String methods](language.md#string)) |
| `std/path` | Path manipulation and the working directory |
| `std/env` | Environment variables, user, host, time of day |
| `std/ansi` | Terminal colors |
| `std/git` | Branch and repository facts read from `.git/HEAD` |
| `std/prompt` | `defaultPrompt` for the interactive shell |
| `std/shell` | The interactive shell's `Shell` type and process builtins, loaded automatically for session scripts (see [Session](session.md)) |

## std/fallible

```thx
type May<T> = Has<T!> | Not?
type Trial<P, F> = Pass<P!> | Fail<F?>
```

These are ordinary unions, not built-ins, and a program must import the names
it uses. See [Errors and panics](language.md#errors-and-panics).

## std/logging

The logger is process-wide. Import the namespace to configure it and the level
functions by name to call them:

```thx
import logging, { info, error, debug } from "std/logging"
import { Pass, Fail } from "std/fallible"

fn main() -> Int64 {
    info("listen")                   ## stderr, full format, level Info
    logging.setLevel(logging.levelDebug)
    debug("detail")
    match logging.toFile("/tmp/app.log") {
        Pass(_) { }
        Fail(problem) { error("log file: {problem}") }
    }
    logging.toStdout()               ## plain lines on stdout
    logging.setFormat(logging.formatPlain)
    info("payload")
    0
}
```

| Setting | Default | Change with |
|---------|---------|-------------|
| Minimum level | Info | `setLevel(levelTrace \| levelDebug \| levelInfo \| levelWarn \| levelError)` |
| Sink | standard error | `toStderr()`, `toStdout()`, `toFile(path)`, `setSink(target)` |
| Format | full (`HH:MM:SS LEVEL message`) | `setFormat(formatFull \| formatPlain)` |

`THOXA_LOG_LEVEL` and `THOXA_LOG_SINK` set the level and sink; they are read
once, on first use. There is one logger per process; named loggers do not
exist.

## std/io

| Name | Description |
|------|-------------|
| `readFile(path)` / `writeFile(path, data)` / `appendFile(path, data)` | Whole-file operations; the file is closed on success and on failure |
| `openFile(path, flags = oRdonly, mode = oMode644)` | Opens a `File` |
| `File` | `readAll`, `write`, `seek`, `position`, `close`, `isOpen`. The last reference closes it (`drop`) |
| `OpenMode` | `Rdonly \| Wronly \| Rdwr`, with `openModeBits` |
| Flags | `oRdonly`, `oWronly`, `oRdwr`, `oCreat`, `oTrunc`, `oAppend`, `oMode644` (combine with `+`) |
| Seek origins | `seekSet`, `seekCur`, `seekEnd` |

## std/net

IPv4 TCP sockets.

| Name | Description |
|------|-------------|
| `createSocket()` | `Trial<Socket, String>` |
| `Socket` | `reuseAddr`, `bind`, `listen`, `accept`, `connect`, `localPort`, `close`, `isOpen`. `close` sets `fd` to -1; a second `close` does nothing. The last reference closes it. Only `std/net` constructs a `Socket`: get one from `createSocket`, `accept`, or `server.peerOf` |
| `receive(deadline: 0)` | One `recv` chunk; empty on close, error, or deadline |
| `receiveRequest(deadline: 0, buffered: Bytes(0))` | Reads until `\r\n\r\n`, the 64 KiB cap, or the peer closing, starting from `buffered` (bytes already read; it does not read at all if they already hold a header block). The result can run past the headers. A deadline with nothing read is an empty `Pass` (a quiet close); a deadline in the middle of the headers is `Fail("request timed out")` |
| `send(data)` / `sendAll(data, deadline: 0)` | One write / write until everything is sent |
| `nowMs()` | Monotonic milliseconds. A deadline is `nowMs() + budget`; 0 means none |
| `ownByTask(socket)` | The running task owns the socket: if the task panics, the runtime closes it and sets `fd` to -1 in every copy. Ownership ends at `close` / `drop` or when the task returns. Does nothing outside a task |
| `taskOwnedSockets()` | The number of open sockets owned by tasks |

Socket operations park the calling task instead of blocking the process; see
[Concurrency](language.md#concurrency).

## std/http

HTTP/1.1 messages, with no system calls of its own. `parseRequest` fills
`query` (without the leading `?`) and leaves `body` empty; the server fills the
body with `readBody`.

| Name | Description |
|------|-------------|
| `Header`, `Request`, `Response` | Plain products. `Request.query` is the text after `?` |
| `parseRequest(bytes)` | `Trial<Request, String>`; needs a complete header block. The path excludes the query. The method, target, and header names must be visible ASCII |
| `writeResponse(response)` | Status line, headers, and body as `Bytes`. CR and LF are removed from header names and values, so a value cannot start another header |
| Lookup | `headerGet`, `contentLength`, `queryMap`, `queryParam`, `formField`, `cookie` |
| Maps | `parseQuery`, `parseForm`, `parseCookies` (each to `Map<String, String>`), `urlDecode` |
| Typed route bodies | `bodyKind(request)` (`"query"`, `"json"`, `"form"`, or `"other"`), `unsupportedMediaType()` (415), `issuesResponse(request, issues)` (400, JSON or text); the code generated for route body parameters calls these |
| Forms | `formPairs(text)` to `Array<FormPair>` (repeated names kept); `formValue` (the last value) / `formValues` (all); `formInt` / `formFloat` (JSON number syntax, `Array<Issue>` on failure). `T.readForm` is built on these |
| Cookies | `Cookie` (`.header()`), `SameSite`, `withCookie`, `clearCookie` |
| Responses | `ok`, `okHead`, `notFound`, `forbidden`, `badRequest`, `methodNotAllowed`, `seeOther`, `withHeader`, `htmlResponse`, `htmlResponseStatus`, `htmlFragment`, `notModified`, `cached` / `etagOf` / `withCache`, `response` / `responseHead` |
| Streaming | `Request.peerFd` (the connection's descriptor, the key `server.peerOf` uses; 0 outside a server); `streamed()` (status 0: the handler wrote the response itself) |
| Connection | `wantsKeepAlive`, `finalize` (drops the body for `HEAD`; sets `Connection`) |

`htmlResponse(root)` takes a `Node` and sends `<!DOCTYPE html>` plus
`root.html()` as `text/html; charset=utf-8` with status 200.
`htmlResponseStatus` does the same with another status (an HTML 404 or 400).
`htmlFragment` sends the markup without a doctype.

`cached(request, response, cacheControl)` adds a strong `ETag` (a hash of the
body) and `Cache-Control`. When the request's `If-None-Match` matches, it
returns 304 with no body. The cache policy (`private, no-cache`,
`max-age=…`) is the caller's choice.

`cookie(request, name)` looks up one cookie; an empty value is `Not`.
`Cookie(name, value, …)` builds a `Set-Cookie` value. Defaults: `path` `"/"`,
`sameSite` `"Lax"`, a session cookie (`maxAge` is `Not`), neither `httpOnly`
nor `secure`. An empty `path` or `domain` omits that attribute. Values are not
escaped. `withCookie` appends the header; `clearCookie(name)` expires a cookie
(`Max-Age=0`).

## std/http/server

A server loop on top of `std/net`.

```thx
import server from "std/http/server"
import { Request, Response, ok, notFound } from "std/http"
import { Pass, Fail } from "std/fallible"
import { warn } from "std/logging"

type Routes {
    GET "/" {
        "hello"
    }
}

fn handle(request: Request) -> Response {
    Routes().route(request)
}

fn main() -> Int64 {
    match server.serve(8080, handle) {
        Pass => 0
        Fail(problem) {
            warn("server: {problem}")
            1
        }
    }
}
```

| Name | Description |
|------|-------------|
| `serve(port, handle)` | Listens forever and starts `serveOne` as a task for each connection. Logs `http://127.0.0.1:{port}/` |
| `serveOne(peer, handle)` | Serves one keep-alive connection. It owns `peer` (`ownByTask`), so a panic in `handle` closes that connection only. Bytes read past one request start the next |
| `peerOf(request)` | `May<Socket>`: the connection the request arrived on, while it is open. A server-sent-events handler writes its stream there and may keep the socket; it reads closed once the client leaves |
| `bindListen(port)` | `createSocket`, `reuseAddr`, `bind`, `listen` |
| `readBody(peer, raw, request, deadline: 0)` / `readRequest(peer, deadline: 0)` | Reads a `Content-Length` body; no `Content-Length` means an empty body. An empty method or path is a quiet close. `Transfer-Encoding`, two `Content-Length` headers, or a non-numeric one get a 400. `readRequest` drops bytes past its request; `serveOne` keeps them |
| `writeResponse(peer, method, response, keepAlive, deadline: 0)` | `finalize`, then `sendAll` |
| `beginEventStream(peer)` / `sendEvent(peer, data)` | Server-sent events: the stream header, then `data:` lines, on the socket from `peerOf`. Return `streamed()` from the handler |

Limits and timeouts: a request body may be at most 64 KiB. A client has 10 s
from accept to send its first request's headers, 30 s of idle time between
kept-alive requests, and 30 s each to send a body and to receive a response.
While 1024 connections are open, `serve` closes new ones at accept. Requests a
client sends without waiting for replies (pipelining) are answered in order.
`HEAD` is answered by the `GET` handler; `finalize` drops the body.

## std/http/static

| Name | Description |
|------|-------------|
| `Router(root: String)` | `.handle(request)` serves a file under `root` and always returns a `Response` |
| Methods | `GET` and `HEAD`; anything else is 405 with `Allow` |
| Paths | Absolute (`/…`). Paths containing `.` or `..` segments are 404. A directory serves its `index.html` |
| Types | `.html` `text/html`, `.css` `text/css`, `.js` `text/javascript`, `.txt` `text/plain`, `.jpg` / `.jpeg` `image/jpeg`, `.png`, `.gif`, `.svg`, `.webp`, `.ico`, `.woff` / `.woff2`; anything else `application/octet-stream` |
| Errors | permission denied: 403 (logged); missing: 404; any other error: logged, 404 |

Example servers in the repository: `cargo make run examples/http` (an
order-taking demo using `http`, `http/server`, `http/static`, `json`, and
`sqlite`; see `examples/http/README.md`), and the deployed sites
`workloads/landing` (thoxa.dev, port 8088), `workloads/sola` (sola.computer,
port 8089), and `workloads/oath` (oath.wicket.cloud, port 8090).

## std/json

A dynamic JSON tree. For typed data, use `T.read` / `t.encode()` (see
[Typed data](language.md#typed-data)); this module is the layer underneath.

```thx
import { decode, encode } from "std/json"
import { Pass, Fail } from "std/fallible"
import logging, { info, error } from "std/logging"

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match decode("[1,true,\"mocha\"]") {
        Fail(problem) => error(problem)
        Pass(value) => info(encode(value))
    }
    0
}
```

| Name | Description |
|------|-------------|
| `Json` | `JNull \| JBool \| JInt \| JFloat \| JStr \| JArr \| JObj` |
| `decode(text)` | `Trial<Json, String>`. One value; trailing text fails. Arrays and objects nest at most 128 deep |
| `encode(value)` | Compact JSON; object keys in insertion order |
| Numbers | Without `.` or an exponent: `JInt` (must fit `Int64`). Otherwise `JFloat`. Non-finite numbers fail |
| Strings | Escapes `\" \\ \/ \b \f \n \r \t \uXXXX`, including surrogate pairs |
| `kind(value)`, `isNull(value)` | The member name / whether it is `JNull` |
| `Issue` | `Missing(path) \| Mismatch(path, expected, got) \| Rejected(path, reason)`: what typed reads report |
| Issue helpers | `message(issue)`, `messages(issues)` (joined with `; `), `issuePath`, `issueMissing` / `issueMismatch` / `issueRejected` (constructors returning `Issue`), `underPath(issues, parent)` (each path moved under `parent`) |
| Error replies | `issueObject(issue)`, `errorObject(issue)` (`{"ok": false, …}`), `errorsObject(issues)` (`{"ok": false, "errors": […]}`) |

A JSON object written in a Thoxa string needs `{{` / `}}` (`"{{\"qty\":2}}"`
is `{"qty":2}`); a `Json` literal avoids that (see
[Json literals](language.md#json-literals)). Not supported: pretty printing,
comments, trailing commas, `NaN` / `Infinity`.

## std/sqlite

SQLite, compiled into the runtime. Statements are `Sql` values written with
the `sql"…"` tagged string: each `{hole}` is embedded as SQL, so strings are
quoted, numbers are digits, and `Sql` fragments are spliced. A plain `"…"`
string with holes is ordinary concatenation and is not accepted by `exec` or
`query`.

```thx
import sqlite, { ident } from "std/sqlite"
import { Pass, Fail, Trial } from "std/fallible"
import logging, { info } from "std/logging"

fn run() -> Trial<Int64, String> {
    let db = sqlite.open(":memory:")?
    db.exec(sql"
        CREATE TABLE orders (
            slug TEXT,
            qty  INTEGER
        )
    ")?
    let slug = "mocha"
    let qty = 2
    db.exec(sql"
        INSERT INTO orders (slug, qty)
        VALUES ({slug}, {qty})
    ")?
    let table = "orders"
    let rows = db.query(sql"
        SELECT slug, qty
        FROM {ident(table)?}
    ")?
    Pass(rows.count)
}

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    match run() {
        Pass(count) => info("{count} row")
        Fail(problem) => info(problem)
    }
    0
}
```

| Name | Description |
|------|-------------|
| `sql"…"` | A tagged string producing `Sql`. No space after `sql`. Import `std/sqlite` so `Sql` exists. Like any string it may span lines; newlines are kept |
| `open(path)` | `Trial<Db, String>`; `":memory:"` is allowed |
| `Db` | `exec(sql)` (`Trial<Void, String>`), `query(sql)` (`Trial<Array<Row>, String>`), `lastInsertRowid()`, `changes()`, `close()`, `isOpen()`. The last reference closes it |
| `Row` | `cells: Map<String, SqlValue>`, keyed by column name |
| `SqlValue` | `SqlNull \| SqlInt \| SqlFloat \| SqlText` |
| `ident(name)` | A quoted identifier (`"name"`) as `Sql`; `Trial`, empty names fail |
| `null`, `lit(value)` | The `NULL` token; an explicit literal fragment |

Not supported: blobs, prepared statements with `?` parameters, explicit
transactions, extension loading, and tags other than `sql`.

## Utility modules

| Module | Functions |
|--------|-----------|
| `std/str` | Byte-indexed helpers (searching, case, trimming, splitting, and joining are [String methods](language.md#string)): `length`, `count` (occurrences), `indexOf`, `lastIndexOf` (`-1` when missing), `substring`, `byteAt`, `truncate`, `reverse`, `padLeft`, `padRight`, `splitOnce` (`May<Split>`), `parseUInt` (`May<Int64>`; above the `Int64` maximum is `Not`). Positions and widths count bytes, so they can cut a character in two |
| `std/path` | `dirname`, `parent`, `basename`, `fileName`, `stem`, `extension`, `hasExtension`, `withExtension`, `withFileName`, `join`, `joinAll`, `parts`, `normalize`, `isAbsolute`, `isRelative`, `stripTrailingSlash`, `ensureTrailingSlash`, `cwd`, `shortPath`, `cwdShort` |
| `std/env` | `getenv`, `hasEnv`, `getenvOr`, `home`, `tmpdir`, `shell`, `pathVar`, `pwd`, `localHms`, `hostname`, `userName` |
| `std/ansi` | `fg`, `bg`, `fgHex`, `bgHex`, `sgr0`, the 16 named colors, `parseRgba`. Hex colors are `#RRGGBB` or `#RRGGBBAA` (alpha ignored); an invalid one gives black |
| `std/git` | `branchName`, `branchSuffix`, `branch`, `branchHere`, `isRepo`, `repoRoot`, `isDetached`, `headLine` |
| `std/prompt` | `defaultPrompt(signature: … = "#3B9DC2")`, the shell's default prompt |

Positions and lengths in `std/str` count bytes, not characters.

## Resolution order

The compiler looks for `std/` in this order:

1. `THOXA_STD`, if set
2. `{THOXA_ROOT}/std`
3. `std/` in the workspace
4. `std/` next to the `thoxa` binary
