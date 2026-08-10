# Standard library (`std/`)

Import product modules with the **`std/…`** named root:

```thx
import { Has, Not, May, Pass, Fail, Trial } from "std/Fallible"
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
| `Shell` | Session `type Shell` + process builtins; auto-injected for session scripts |
| `Fallible` | Exported `May` / `Trial` / `Has` / `Not` / `Pass` / `Fail` — **import required** for programs |
| `Prompt` | `defaultPrompt(signature: … = "#3B9DC2")` — session-style prompt |
| `Str` | match/search (incl. ignore-case), slice/`byteAt`/`truncate`/`reverse`, replace, `split`/`join`/`lines`, pad/trim, `removePrefix`/`removeSuffix`, `isAscii` |
| `Path` | `dirname`/`parent`/`basename`/`fileName`/`join`/`joinAll`/`parts`/`stripTrailingSlash`/`ensureTrailingSlash`/`isAbsolute`/`isRelative`/`normalize`/`extension`/`hasExtension`/`stem`/`withExtension`/`withFileName`/`cwd`/`shortPath`/`cwdShort` |
| `Ansi` | `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` |
| `Env` | `getenv`, `hasEnv`, `getenvOr`, `home`, `tmpdir`, `shell`, `pathVar`, `pwd`, `localHms`, `hostname`, `userName` |
| `Git` | `branchName` / `branchSuffix` / `branch` / `branchHere` / `isRepo` / `repoRoot` / `isDetached` / `headLine` via `.git/HEAD` |

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
