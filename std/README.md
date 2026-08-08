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
| `Fallible` | Ambient May/Trial surface (compiler inject; no import) |
| `Prompt` | `defaultPrompt(signature: …)` — zsh-style prompt; optional `#RRGGBB` signature color |
| `Str` | search/slice/replace/`split`/`join`/`lines`/trim/pad/case/`removePrefix`/`truncate`/`reverse`/… |
| `Path` | dirname/basename/join/`joinAll`/`parts`/slash helpers/extension/`cwd`/`shortPath`/… |
| `Ansi` | truecolor `fg` / `bg` / `fgHex` / `bgHex`, `sgr0`, 16 colors, `parseRgba` (`#RRGGBB` or `#RRGGBBAA`) |
| `Env` | `getenv`/`hasEnv`/`home`/`tmpdir`/`shell`/`pathVar`/`pwd`/`localHms`/`hostname`/`userName` |
| `Git` | `branchName`/`isRepo`/`repoRoot`/`isDetached`/`headLine`/… via `.git/HEAD` |

Full shipped surface: [`docs/handbook/std.md`](../docs/handbook/std.md).

**Resolution:** `THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to the `thoxa` binary.

This is a **thin** library for session prompts and shared helpers — not a full
prelude. **`ffi` remains deferred.**

**Growth rule:** do not schedule “expand std/” as its own todo. When working
samples, dogfood scripts, or session rc and a helper clearly belongs in
product `std/`, add it there (with conformance) in the same change. Speculative
stdlib bulk is out.
