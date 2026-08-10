# Thoxa `std/`

Product standard library modules, imported with the **`std/…`** named root (D97).

User handbook: [`docs/handbook/std.md`](../docs/handbook/std.md).  
As-built rows: [`docs/capabilities.md`](../docs/capabilities.md) (`std-*`).

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
import logging, { info } from "std/Logging"  ## ns + short emit (D127)
import { readFile } from "std/Io"
import net from "std/Net"
import http from "std/Http"
import { Router } from "std/HttpStatic"
import { defaultPrompt } from "std/Prompt"   ## session shell.thx
import { startsWith, dropPrefix } from "std/Str"
import { cwdShort, shortPath } from "std/Path"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/Ansi"
import { localHms, userName, hostname } from "std/Env"
import { branch } from "std/Git"
```

| Module | Role |
|--------|------|
| `Shell` | Session `type Shell` + builtins (`echo`/`cd`/`run`/`runIo`/`pipeline`/…); auto-injected for session scripts |
| `Fallible` | Exported `May`/`Trial`/`Has`/`Not`/`Pass`/`Fail` — **import required** for programs (D123; not ambient) |
| `Logging` | Default logger: levels, sinks, formats; prefer `import logging, { info, … }` (**D125**/**D127**); no ambient `println` |
| `Io` | Files: `readFile`/`writeFile`/`appendFile`, `File`, OpenMode (**D126**) |
| `Net` | TCP sockets: `Socket`, `receiveRequest`, `sendAll` (**D126**) |
| `Http` | HTTP/1.1 parse/write + status helpers (**D126**) |
| `HttpStatic` | Static-file `Router` over `std/Io` + `std/Http` (**D126**) |
| `Prompt` | `defaultPrompt(signature: …)` — zsh-style prompt; optional `#RRGGBB` signature color |
| `Str` | search/slice/replace/`split`/`join`/`lines`/trim/pad/case/`removePrefix`/`truncate`/`reverse`/… |
| `Path` | dirname/basename/join/`joinAll`/`parts`/slash helpers/extension/`cwd`/`shortPath`/… |
| `Ansi` | truecolor `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` (`#RRGGBB` or `#RRGGBBAA`) |
| `Env` | `getenv`/`hasEnv`/`home`/`tmpdir`/`shell`/`pathVar`/`pwd`/`localHms`/`hostname`/`userName` |
| `Git` | `branchName`/`isRepo`/`repoRoot`/`isDetached`/`headLine`/… via `.git/HEAD` |

Full shipped surface: [`docs/handbook/std.md`](../docs/handbook/std.md).

**Resolution:** `THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to the `thoxa` binary.

This is a **thin** library grown from real samples — not a full prelude.
**`ffi` remains deferred.**

**Growth rule:** do not schedule “expand std/” as its own todo. When working
samples, dogfood scripts, or session rc and a helper clearly belongs in
product `std/`, add it there (with conformance) in the same change. Speculative
stdlib bulk is out.
