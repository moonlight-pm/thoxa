# Standard library (`std/`)

Import product modules with the **`std/…`** named root:

```thx
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
| `Prompt` | `defaultPrompt(signature: … = "#3B9DC2")` — session-style prompt |
| `Str` | `startsWith`, `dropPrefix`, `trimNewline` |
| `Path` | `dirname`, `basename`, `cwd`, `shortPath`, `cwdShort` |
| `Ansi` | `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` |
| `Env` | `getenv`, `localHms`, `hostname`, `userName` |
| `Git` | `branch` / `branchSuffix` via `.git/HEAD` |

Hex colors: primary `#RRGGBB`, also `#RRGGBBAA` (alpha ignored for SGR). Invalid
hex → black SGR.

## Resolution order

1. `THOXA_STD` if set  
2. `{THOXA_ROOT}/std`  
3. Workspace `std/`  
4. `std/` next to the `thoxa` binary  

## Limits

This is a **thin** library for session prompts and shared helpers — not a full
prelude. Named **`ffi`** root is deferred. File I/O for programs often lives in
conformance helpers (`iolib`) / free `readFile` rather than a large `std/Io`
yet.

Source tree: [`../../std/`](../../std/).
