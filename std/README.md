# Thoxa `std/`

Product standard library modules, imported with the **`std/…`** named root (D97).

User handbook: [`docs/handbook/std.md`](../docs/handbook/std.md).  
As-built rows: [`docs/capabilities.md`](../docs/capabilities.md) (`std-*`).

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/fallible"
import logging, { info } from "std/logging"  ## ns + short emit (D127)
import { readFile } from "std/io"
import net from "std/net"
import http from "std/http"
import server from "std/http/server"
import { Router } from "std/http/static"
import { defaultPrompt } from "std/prompt"   ## session shell.thx
import { startsWith, dropPrefix } from "std/str"
import { cwdShort, shortPath } from "std/path"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/ansi"
import { localHms, userName, hostname } from "std/env"
import { branch } from "std/git"
import json, { Json, decode, encode } from "std/json"
import sqlite, { lit } from "std/sqlite"
```

| Module | Role |
|--------|------|
| `Shell` | Session `type Shell` + builtins (`echo`/`cd`/`run`/`runIo`/`pipeline`/…); auto-injected for session scripts |
| `Fallible` | Exported `May`/`Trial`/`Has`/`Not`/`Pass`/`Fail` — **import required** for programs (D123; not ambient) |
| `Logging` | Default logger: levels, sinks, formats; prefer `import logging, { info, … }` (**D125**/**D127**); no ambient `println` |
| `Io` | Files: `readFile`/`writeFile`/`appendFile`, `File`, OpenMode (**D126**) |
| `Net` | TCP sockets: `Socket`, `receiveRequest`, `sendAll`; `nowMs()` deadlines on receive / send (**D126**/**D165**) |
| `Http` | HTTP/1.1 parse/write + query/form/cookie helpers + `htmlResponse` + `Cookie` / `withCookie` (**D126**/**D136**/**D137**/**D142**) |
| `http/server` | Host loop: `serve` / `serveOne` / `readBody`; header / idle / body / response deadlines, 1024-connection cap, strict framing, pipelined requests in order (**D136**/**D139**/**D165**/**D171**) |
| `http/static` | Static-file `Router` over `std/io` + `std/http` (**D126**/**D139**) |
| `Prompt` | `defaultPrompt(signature: …)` — zsh-style prompt; optional `#RRGGBB` signature color |
| `Str` | search/slice/replace/`split`/`splitOnce`/`parseUInt`/`join`/`lines`/trim/pad/case/`removePrefix`/`truncate`/`reverse`/… |
| `Path` | dirname/basename/join/`joinAll`/`parts`/slash helpers/extension/`cwd`/`shortPath`/… |
| `Ansi` | truecolor `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` (`#RRGGBB` or `#RRGGBBAA`) |
| `Env` | `getenv`/`hasEnv`/`home`/`tmpdir`/`shell`/`pathVar`/`pwd`/`localHms`/`hostname`/`userName` |
| `Git` | `branchName`/`isRepo`/`repoRoot`/`isDetached`/`headLine`/… via `.git/HEAD` |
| `Json` | Dynamic `Json` tree: `decode` / `encode` (**D141**) |
| `Sqlite` | Embedded SQLite: `open` / `exec` / `query`; `sql"…"` (**D143**/**D144**) |

Full shipped surface: [`docs/handbook/std.md`](../docs/handbook/std.md).

**Resolution:** `THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to the `thoxa` binary.

This is a **thin** library grown from real samples — not a full prelude.
**`ffi` remains deferred.**

**Growth rule:** do not schedule “expand std/” as its own todo. When working
samples, dogfood scripts, or session rc and a helper clearly belongs in
product `std/`, add it there (with conformance) in the same change. Speculative
stdlib bulk is out.
