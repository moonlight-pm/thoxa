# Thoxa `std/`

Product standard library modules, imported with the **`std/…`** named root (D97).

```thx
import { defaultPrompt } from "std/Prompt"   ## session shell.thx
import { startsWith, dropPrefix } from "std/Str"
import { cwdShort, shortPath } from "std/Path"
import { fg, fgHex, sgr0, cyan, brightWhite } from "std/Ansi"
import { localHms, userName, hostname } from "std/Env"
import { branch } from "std/Git"
```

| Module | Role |
|--------|------|
| `Prompt` | `defaultPrompt(signature: …)` — zsh-style prompt; optional `#RRGGBB` signature color |
| `Str` | `startsWith`, `dropPrefix`, `trimNewline` |
| `Path` | `dirname`, `basename`, `cwd`, `shortPath`, `cwdShort` |
| `Ansi` | truecolor `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` (`#RRGGBB` or `#RRGGBBAA`) |
| `Env` | `getenv`, `localHms`, `hostname`, `userName` |
| `Git` | `branch` / `branchSuffix` via `.git/HEAD` |

**Resolution:** `THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to the `thoxa` binary.

This is a **thin** first cut for session prompts and shared helpers — not a full prelude. `ffi` remains deferred.
