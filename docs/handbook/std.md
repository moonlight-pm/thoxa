# Standard library (`std/`)

Import product modules with the **`std/…`** named root:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
import logging, { info, warn } from "std/Logging"   ## ns + selective (D127)
import { readFile } from "std/Io"
import net, { Socket } from "std/Net"
import http from "std/Http"
import server from "std/HttpServer"
import { Router } from "std/HttpStatic"
import { defaultPrompt } from "std/Prompt"
import { startsWith, dropPrefix } from "std/Str"
import { cwdShort, shortPath } from "std/Path"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/Ansi"
import { localHms, userName, hostname } from "std/Env"
import { branch } from "std/Git"
```

## Modules (current)

| Module | Role |
|--------|------|
| `Shell` | Session `type Shell` + process builtins; auto-injected for session scripts (pulls Fallible + Logging) |
| `Fallible` | Exported `May<T> = Has<T!> \| Not?`, `Trial<P,F> = Pass<P!> \| Fail<F?>` — **import required** for programs (D123; not ambient) |
| `Logging` | Default process logger (**D125**): `trace`/`debug`/`info`/`warn`/`error`/`emit`; configure via namespace; short emit via selective import. No ambient `println` |
| `Io` | Files (**D126**): `readFile` / `writeFile` / `appendFile`, `File`, OpenMode |
| `Net` | TCP sockets (**D126**): `Socket`, `receiveRequest`, `sendAll` |
| `Http` | HTTP/1.1 protocol (**D126**/**D136**): parse/write, `Request.query`, helpers |
| `HttpServer` | HTTP/1.1 host loop (**D136**): `serve` / `serveOne` / `readBody` |
| `HttpStatic` | Static-file `Router` (**D126**); depends on Io + Http |
| `Prompt` | `defaultPrompt(signature: … = "#3B9DC2")` — session-style prompt |
| `Str` | match/search (incl. ignore-case), slice/`byteAt`/`truncate`/`reverse`, replace, `split`/`join`/`lines`, pad/trim, `removePrefix`/`removeSuffix`, `isAscii` |
| `Path` | `dirname`/`parent`/`basename`/`fileName`/`join`/`joinAll`/`parts`/`stripTrailingSlash`/`ensureTrailingSlash`/`isAbsolute`/`isRelative`/`normalize`/`extension`/`hasExtension`/`stem`/`withExtension`/`withFileName`/`cwd`/`shortPath`/`cwdShort` |
| `Ansi` | `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` |
| `Env` | `getenv`, `hasEnv`, `getenvOr`, `home`, `tmpdir`, `shell`, `pathVar`, `pwd`, `localHms`, `hostname`, `userName` |
| `Git` | `branchName` / `branchSuffix` / `branch` / `branchHere` / `isRepo` / `repoRoot` / `isDetached` / `headLine` via `.git/HEAD` |

### Logging (default logger)

**Preferred import style** (apps): namespace for configure + selective level names
for short call sites (see `examples/http`):

```thx
import logging, { info, warn, error, debug } from "std/Logging"

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
`import logging, { info, … } from "std/Logging"` is the usual app face (**D127**).

| Knob | Default |
|------|---------|
| Min level | Info (`levelInfo`) |
| Sink | stderr (`toStderr`) |
| Format | full (`HH:MM:SS LEVEL message`) |

Env (applied once on first use): `THOXA_LOG_LEVEL`, `THOXA_LOG_SINK`. Multiple
named loggers (`initLog`) are **not** shipped yet.

### Io (`std/Io`)

Linux file I/O. Fail payloads are **String** (`"op: strerror"` or a short
message).

| Surface | Notes |
|---------|--------|
| `readFile` / `writeFile` / `appendFile` | Whole-file helpers; close on both success and failure after open |
| `openFile(path, flags = oRdonly, mode = oMode644)` | Returns `File` |
| `File` | `readAll`, `write`, `seek`, `position`, `close`, `isOpen` |
| `OpenMode` | `Rdonly \| Wronly \| Rdwr` + `openModeBits` |
| Flag lets | `oRdonly` / `oWronly` / `oRdwr` / `oCreat` / `oTrunc` / `oAppend` / `oMode644` |
| Seek | `seekSet` / `seekCur` / `seekEnd` |

### Net (`std/Net`)

IPv4 TCP (`AF_INET` stream). Fail payloads are String.

| Surface | Notes |
|---------|--------|
| `createSocket()` | `Trial<Socket, String>` |
| `Socket` | `reuseAddr`, `bind`, `listen`, `accept`, `connect`, `localPort`, `close` |
| `receive()` | One `recv` chunk (empty on close/error) |
| `receiveRequest()` | Until `\r\n\r\n` or 64 KiB cap / peer close |
| `send` / `sendAll` | One write vs drain partial writes |

### Http (`std/Http`)

Pure protocol (no syscalls). `parseRequest` fills `query` (no leading `?`) and
leaves `body` empty; hosts complete the body via `std/HttpServer.readBody`.

| Surface | Notes |
|---------|--------|
| `Header` / `Request` / `Response` | Constructor-param products. `Request.query` is the `?…` tail |
| `parseRequest` | `Trial<Request, String>`; requires `\r\n\r\n`; path is query-stripped |
| `writeResponse` | Status line + headers + body → `Bytes` |
| Lookup | `headerGet`, `contentLength`, `queryMap` |
| Maps | `parseQuery` / `parseForm` / `parseCookies` → `Map<String, String>`; `urlDecode` |
| Response | `ok`, `okHead`, `notFound`, `forbidden`, `badRequest`, `methodNotAllowed`, `seeOther`, `withHeader`, `response` / `responseHead` |
| Connection | `wantsKeepAlive`, `finalize` (HEAD drops body; rewrites `Connection`) |

### HttpServer (`std/HttpServer`)

Host loop over `std/Net`. Completes `Request.body` (64 KiB cap).

| Surface | Notes |
|---------|--------|
| `serve(port, handle)` | Listen forever; `--> serveOne` per accept (**D132**) |
| `serveOne(peer, handle)` | One keep-alive connection |
| `bindListen(port)` | `createSocket` + reuse + bind + listen |
| `readBody` / `readRequest` | `Content-Length` entity; empty method/path = quiet close |
| `writeResponse(peer, method, res, keepAlive)` | `finalize` + `sendAll` |

HEAD is answered by GET handlers; `finalize` still sees the real method.

### HttpStatic (`std/HttpStatic`)

| Surface | Notes |
|---------|--------|
| `Router(root: String)` | `handle(req) -> Response` — always a Response (never Trial) |
| Methods | **GET** and **HEAD** only; else **405** + `Allow` |
| Paths | Absolute (`/…`); `.` / `..` → 404; directories serve `index.html` |
| Errors | Permission denied → **403** (logged); missing → **404**; other Io → log + 404 |

Runnable demo: `cargo make run examples/http` (Crema-lite **experiment** —
`server.serve(routes.port, routes.handle)` on product Http/HttpServer/HttpStatic;
app/domain is not product std; see `examples/http/README.md`).

Hex colors: primary `#RRGGBB`, also `#RRGGBBAA` (alpha ignored for SGR). Invalid
hex → black SGR.

## Resolution order

1. `THOXA_STD` if set  
2. `{THOXA_ROOT}/std`  
3. Workspace `std/`  
4. `std/` next to the `thoxa` binary  

## Limits

This is a **thin** library grown from real samples. Fallibility types live in
`std/Fallible` and are imported like other std modules. Named **`ffi`** root is
deferred. I/O and HTTP are **Linux product track** (x86_64 sockaddr pack, Linux
open flags). Windows is out of scope. Fail payloads are String (not an
`Error` protocol).

New helpers land when real samples need them — not via a free-floating
“expand the stdlib” backlog.

Source tree: [`../../std/`](../../std/).
