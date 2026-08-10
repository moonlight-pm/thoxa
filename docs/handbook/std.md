# Standard library (`std/`)

Import product modules with the **`std/…`** named root:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
import logging from "std/Logging"
import { info } from "std/Logging"
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
import logging from "std/Logging"
import { info, warn, error, debug } from "std/Logging"   ## pick what you emit

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

Either import alone is valid: only `import logging from …` (qualify every call)
or only `import { info, toStdout, … }` (no `logging.` prefix). Combining both
is the usual app face.

| Knob | Default |
|------|---------|
| Min level | Info (`levelInfo`) |
| Sink | stderr (`toStderr`) |
| Format | full (`HH:MM:SS LEVEL message`) |

Env (applied once on first use): `THOXA_LOG_LEVEL`, `THOXA_LOG_SINK`. Multiple
named loggers (`initLog`) are **not** shipped yet.

Hex colors: primary `#RRGGBB`, also `#RRGGBBAA` (alpha ignored for SGR). Invalid
hex → black SGR.

## Resolution order

1. `THOXA_STD` if set  
2. `{THOXA_ROOT}/std`  
3. Workspace `std/`  
4. `std/` next to the `thoxa` binary  

## Limits

This is a **thin** library for session prompts and shared helpers. Fallibility
types live in `std/Fallible` and are imported like other std modules. Named
**`ffi`** root is deferred. File I/O for programs often lives in conformance
helpers (`iolib`) / free `readFile` rather than a large `std/Io` yet.

New helpers land when real samples need them — not via a free-floating
“expand the stdlib” backlog.

Source tree: [`../../std/`](../../std/).
