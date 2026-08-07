# Thoxa `std/`

Product standard library modules, imported with the **`std/…`** named root (D97).

User handbook: [`docs/handbook/std.md`](../docs/handbook/std.md).  
As-built rows: [`docs/capabilities.md`](../docs/capabilities.md) (`std-*`).

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
| `Shell` | Session `type Shell` + builtins (`echo`/`cd`/`run`/`runIo`/`pipeline`/…); auto-injected for session scripts |
| `Prompt` | `defaultPrompt(signature: …)` — zsh-style prompt; optional `#RRGGBB` signature color |
| `Str` | `startsWith`, `dropPrefix`, `trimNewline` |
| `Path` | `dirname`, `basename`, `cwd`, `shortPath`, `cwdShort` |
| `Ansi` | truecolor `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` (`#RRGGBB` or `#RRGGBBAA`) |
| `Env` | `getenv`, `localHms`, `hostname`, `userName` |
| `Git` | `branch` / `branchSuffix` via `.git/HEAD` |

**Resolution:** `THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to the `thoxa` binary.

This is a **thin** library for session prompts and shared helpers — not a full
prelude. **`ffi` remains deferred.**

**Growth rule:** do not schedule “expand std/” as its own todo. When working
samples, dogfood scripts, or session rc and a helper clearly belongs in
product `std/`, add it there (with conformance) in the same change. Speculative
stdlib bulk is out.
