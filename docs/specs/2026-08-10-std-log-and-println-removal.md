# `std/Logging` and removal of ambient `println`

**Date:** 2026-08-10  
**Status:** target (freeze)  
**Implementation:** shipped  
**Gaps:** multi named loggers / `initLog` (trajectory only); structured/JSON lines; colors/TTY; source location in lines

**As-built:** [`../capabilities.md`](../capabilities.md) ·
[`../architecture.md`](../architecture.md) · focus
[`../../CURRENT.md`](../../CURRENT.md)  

**Decision:** **D125** (assign on ship; locked here as product intent).

**Related:** D97 (`std/…` root); D123 (import-required std surface, no ambient
prelude for Fallible); file I/O is product `std/Io` (**D126**; was iolib);
session Shell in `std/Shell.thx` (D104; removed by D227).

---

## 0. Product thesis

Program text emission is a **library** concern, not a compiler special case.

**Today:** ambient `println` / `print` are resolver builtins with MIR type
switches (`String` / `Int64` / `Bool`) into `thoxa_println_*`. That is a
one-off I/O channel: stdout only, no levels, no files, no filter.

**Target:** a thin product module **`std/Logging`** with the five standard
levels, process-wide **default logger** defaults, file sinks via **`Trial` on
configure**, and **fire-and-forget emit** (no `?` on every `debug`/`info`
call). Ambient `println` / `print` are **removed**.

**Naming:** the module is **`Logging`**, not `Log`. Reserve **`log`** as a
*logger instance* name for multi-stream work later.

**Preferred app import face** (shipped style; see `examples/http`):

```thx
import logging, { info, warn, error } from "std/logging"   ## ns + short emits (D127)

logging.toStdout()                 ## configure via namespace
logging.setFormat(logging.formatPlain)
info("up")                         ## emit via selective import

## later (not this freeze): named streams
## let access = logging.initLog("access")
## access.info("…")
```

Either import alone remains valid; combining both is the documented default for
apps. No import-mode dual export names (language unchanged).

**One sentence:**  
`import logging, { info, … } from "std/logging"` (D127) is how programs
log; `println` is gone.

---

## 1. Locked answers (open questions from design review)

| # | Question | Lock |
|---|----------|------|
| 1 | Default sink | **stderr** |
| 2 | Default min level | **Info** |
| 3 | File open failure | **`toFile` / path `setSink` → `Trial<(), String>`**; emit path never forces `?` |
| 4 | Dogfood | **All conformance tests + examples + product std (Shell, etc.)** migrate in the same program of work |
| 5 | Module name | **`std/Logging`** (not `std/Log`; keep `log` free for instances) |

**Import style (no language change):** prefer **both** —
`import logging, { info, warn, … } from "std/logging"` (combined **D127**).
Two separate import lines remain valid. Either form alone is
valid. No import-mode dual export names.

---

## 2. Scope

### In scope

| # | Deliverable |
|---|-------------|
| 1 | **`std/Logging.thx`** — level API on the **default logger**, format modes |
| 2 | **RT write primitive** — write a line (or bytes) to fd 1 / 2 / open file |
| 3 | **Remove ambient `println` / `print`** — resolver, MIR lower, cgen maps, unit fixtures |
| 4 | **Migrate dogfood** — every product `.thx` that used `println` |
| 5 | **Conformance gates** for Logging (level filter, sinks, file Trial, plain format) |
| 6 | **Docs** — capabilities, handbook, architecture if RT surface changes |

### Explicitly out of scope (this freeze)

| Item | Notes |
|------|--------|
| **Multiple named loggers / streams** | later — see §3.0 trajectory; do not implement `initLog` yet |
| Structured / JSON log lines | later |
| Colors / TTY detect | later (`std/Ansi` optional) |
| Source location in lines | needs compiler support; not free |
| Log rotation | OS / ops concern |
| Full product `std/Io` | still sample-driven idea; Logging may use thin extern/RT |
| Language **panic** story | unchanged; Logging is not panic |
| Removing other builtins (`Array`, `true`, …) | only print surface |
| Changing harness to golden **stderr** by default | no — see §5 |
| Import/export language changes | none required for Logging itself; combined `import ns, {…}` later as **D127** |

---

## 3. Public surface (`std/Logging`)

### 3.0 Multi-logger trajectory (structure only — not v0)

**Intent:** v0 is a single **default logger**. Later work adds multiple
independent loggers without renaming the module or thrashing call sites more
than necessary.

| Horizon | Shape |
|---------|--------|
| **v0 (this freeze)** | Free functions + module state = the **default** logger. Prefer `import logging` + selective `{ info, … }`. |
| **Later** | A `Logger` (name TBD) value: own level, sink, format. Factory e.g. `initLog(name!: String) -> Logger` (exact API when scheduled). Free functions remain **sugar for the default logger**. |
| **Naming** | Module path **`std/Logging`**. Namespace binding **`logging`**. Instance bindings **`log`**, `access`, `audit`, … — never use `Log` as the module leaf. |

**Implementation note for v0:** keep default state in module `mut` (or an
unexported singleton) so a later slice can reify the same fields onto a
`Logger` type and point free functions at the default instance. Do **not**
paint the API into “only ever one global with no name.”

### 3.1 Levels (five, ordered)

```text
Trace < Debug < Info < Warn < Error
```

Export integer constants for compare/filter:

| Export | Value |
|--------|------:|
| `levelTrace` | 0 |
| `levelDebug` | 1 |
| `levelInfo` | 2 |
| `levelWarn` | 3 |
| `levelError` | 4 |

### 3.2 Emit API (primary — default logger)

```thx
export fn trace(msg!: String)
export fn debug(msg!: String)
export fn info(msg!: String)
export fn warn(msg!: String)
export fn error(msg!: String)

## Explicit level on the default logger (not an instance factory).
export fn emit(level!: Int64, msg!: String)
```

**Note:** the generic emit is named **`emit`**, not `log`, so the word `log`
stays free for instance bindings later (`let log = …`) and for selective import
without clashing.

**Rules:**

- Message is **`String` only.** Callers format with interpolation:
  `info("n={n}")`, not a polymorphic builtin.
- If `level < getLevel()`, emit is a no-op.
- Emit is **`void`** — never `Trial` / never requires `?`.
- After a sink is configured, **write failures on emit are best-effort**
  (drop or ignore for v0). Do **not** turn every log site into fallible control
  flow. There is no panic story yet; logging must not invent one.

### 3.3 Configuration (default logger — process-wide for v0)

```thx
export fn setLevel(level!: Int64)
export fn getLevel() -> Int64

export fn levelName(level!: Int64) -> String
export fn parseLevel(s!: String) -> Int64   ## invalid → levelInfo (document)

## Sink: "stderr" | "stdout" | filesystem path (append).
## Path open is fallible; stream names are not.
export fn setSink(target!: String) -> Trial<(), String>
export fn getSink() -> String

export fn toStderr()            ## always succeeds (void)
export fn toStdout()            ## always succeeds (void)
export fn toFile(path!: String) -> Trial<(), String>
```

**File / path configure (elaboration of Q3):**

- User **asked to open a file** → failure is a normal I/O result:
  `Fail("…")` with path + reason (strerror-style, same spirit as Io).
- `toFile(path)` and `setSink("/some/path")` share that open path.
- `setSink("stderr")` / `setSink("stdout")` / `toStderr` / `toStdout` →
  never Fail.
- On **Pass**, switch the default logger’s sink; close previous file fd if any.
- On **Fail**, **leave the previous sink unchanged**.
- Call sites that care:

```thx
import logging, { error } from "std/logging"
import { Pass, Fail, Trial } from "std/fallible"

match logging.toFile("/var/log/app.log") {
    Pass(_) { }
    Fail(e) { error("log file: {e}") }
}
```

Programs that never call `toFile` never see this Trial.

### 3.4 Line format (default logger)

Two modes:

| Mode | Export | Line shape |
|------|--------|------------|
| **Full** (default) | `formatFull` | `HH:MM:SS LEVEL message` + newline |
| **Plain** | `formatPlain` | `message` + newline only |

```thx
export let formatFull = 0
export let formatPlain = 1
export fn setFormat(mode!: Int64)
export fn getFormat() -> Int64
```

- Timestamp: existing wall-clock helper (`localHms` / RT), not monotonic.
- Level token fixed width for grepping (`TRACE`/`DEBUG`/`INFO `/`WARN `/`ERROR`).
- **Full format timestamps are non-deterministic** → not for golden `.out`
  unless the test freezes time (out of scope). Conformance that goldens stdout
  uses **plain** (+ usually `toStdout`) — §5.

### 3.5 Environment bootstrap (optional, first use — default logger)

| Variable | Effect |
|----------|--------|
| `THOXA_LOG_LEVEL` | `trace` / `debug` / `info` / `warn` / `error` (ASCII, case-insensitive) |
| `THOXA_LOG_SINK` | `stderr` / `stdout` / path |

- Applied once on first emit or first explicit get/set that needs defaults.
- Explicit `setLevel` / `setSink` / `to*` after start always wins.
- If env names a **path** and open fails: **keep stderr**, do not abort the
  process (env is best-effort bootstrap; programmatic `toFile` remains the
  strict Trial path).
- Later multi-logger work may add per-name env rules; v0 only configures the
  **default** logger.

### 3.6 Defaults (default logger)

| Knob | Default |
|------|---------|
| Min level | **Info** |
| Sink | **stderr** |
| Format | **full** |

---

## 4. Remove ambient `println` / `print`

### 4.1 Compiler / RT

| Layer | Action |
|-------|--------|
| Resolver ambient names | Drop `println` (and `print` if listed) |
| MIR `lower_call` | Delete `println` / `print` special case and type switches |
| cgen | Drop maps to `thoxa_println_*` / `thoxa_print_*` as **user** builtins |
| RT | Prefer a small **fd write** used by `std/Logging`. Old `thoxa_println*` may become internal helpers or be deleted once unused |

### 4.2 Language rule

- There is **no** ambient print. Unknown `println` is an ordinary undefined name.
- Session command desugar must **not** treat `println(...)` as special beyond
  “already valid Thoxa” (today it is kept as Thoxa; after removal it is simply
  invalid unless imported — and it will not be exported under that name).

### 4.3 Replacement patterns

| Old | New |
|-----|-----|
| `println("hi")` | `info("hi")` (selective) or `logging.info("hi")` with sink/format (§5) |
| `println(x)` for Int64/Bool | `info("{x}")` |
| Diagnostics in apps | `info` / `warn` / `error` (default stderr, full format) |
| Golden / data lines on stdout | `logging.toStdout()` + `setFormat(formatPlain)` + `info("…")` |

Do **not** reintroduce a second ambient or compiler-known `print` under a new
name. If a thinner “write this string to stdout” helper is needed later, it
belongs in **std** (Logging or future Io), not the resolver builtin table.

---

## 5. Conformance, examples, and harness

### 5.1 Harness unchanged

`cargo make test` continues to compare **stdout** to sibling `.out` files.
Default logger sink is **stderr**, so unconfigured Logging does **not** affect
goldens.

### 5.2 Migrating tests that used `println` for expected output

Preferred (namespace + selective, as in apps):

```thx
import logging, { info } from "std/logging"

fn main() -> Int64 {
    logging.toStdout()
    logging.setFormat(logging.formatPlain)
    info("Hello, world!")
    0
}
```

Namespace-only or selective-only remain fine for tests:

```thx
import { info, toStdout, setFormat, formatPlain } from "std/logging"

fn main() -> Int64 {
    toStdout()
    setFormat(formatPlain)
    info("Hello, world!")
    0
}
```

- Level stays default **Info**, so `info` emits; `debug`/`trace` stay off unless
  the test calls `setLevel`.
- Plain format keeps `.out` deterministic (no clock).
- Tests **about Logging itself** may assert filter/sink/Trial behavior; prefer
  not golden-matching wall-clock full format.

### 5.3 Dogfood inventory (implementation checklist)

Rewrite all product uses of `println` / `print` as Thoxa source, including:

- `tests/conformance/**/*.thx` (and packages)
- `examples/**` (`http`, `kilo`, …)
- `std/Shell.thx` and any session-generated step bodies (`println(prompt())`,
  path listing, …)
- Handbook code samples under `docs/handbook/`
- Compiler unit-test Thoxa snippets that compile `println`

Shell **user-visible** command output (`echo`, `pwd`, `status`, `help`) is
still stdout data: use plain + stdout (or equivalent std write) so interactive
behavior stays sane. Shell **diagnostics** (`command not found`, …) should use
`warn` / `error` (stderr, full format by default).

### 5.4 New gates (minimum)

| Gate | Intent |
|------|--------|
| `logging_levels` (name flexible) | filter: debug silent at Info; info/warn/error emit |
| `logging_sink` / file case | `toFile` Pass writes; bad path Fail; prior sink unchanged |
| Existing suites | green after println migration |

---

## 6. Implementation slices (suggested order)

1. **RT + Logging skeleton** — fd write; `std/Logging` default logger with
   level filter, stderr/stdout sinks, full/plain format; conformance for
   Logging only.
2. **File sink** — `toFile` / path `setSink` → `Trial`; append writes.
3. **Env bootstrap** — `THOXA_LOG_LEVEL` / `THOXA_LOG_SINK` on the default logger.
4. **Remove builtins** + **mass migrate** conformance/examples/std/session —
   one coordinated change so tree never half-depends on ambient print.
5. **Handbook + capabilities shipped**; freeze Implementation → shipped.

Slices 1–3 may land before 4 if Logging is additive; **slice 4 is atomic**
(println removal + rewrites together).

**Later (not this freeze):** `Logger` type + `initLog` (or equivalent) + free
functions as default-logger wrappers.

---

## 7. Non-goals / anti-patterns

- **`debug(...)?` or `info(...)?`** — rejected for v0 emit API.
- **Panicking on log write failure** — rejected.
- **Default sink stdout** — rejected (pipelines / data tools).
- **Keeping `println` as deprecated alias** — rejected; delete the one-off.
- **Ambient inject of `std/Logging`** — rejected (same spirit as D123). Import
  required. Session may import Logging from Shell / step sources explicitly.
- **Module named `std/Log` or binding the module as `log` by convention in
  docs** — rejected; module is **Logging** / **`logging`**, so `log` stays free
  for instances.
- **Implementing multi-logger in v0** — rejected; only leave room (§3.0).

---

## 8. Acceptance

This freeze is **done** when:

1. `std/Logging` matches §3 with defaults §3.6.  
2. Ambient `println` / `print` are gone; no MIR special case.  
3. `cargo make test` green with migrated goldens.  
4. Examples and `std/Shell` compile without ambient print.  
5. Capabilities `std-logging` → **shipped**; handbook documents Logging;
   language handbook no longer teaches ambient `println`.  
6. D125 recorded as shipped in `docs/decisions.md`.

---

## 9. Capability / docs map (on ship)

| Doc | Update |
|-----|--------|
| `docs/capabilities.md` | ship `std-logging`; note println removal |
| `docs/handbook/std.md` | `std/Logging` module |
| `docs/handbook/language.md` | drop ambient println; show Logging / plain stdout pattern |
| `docs/architecture.md` | RT write / drop print builtins if map mentions them |
| `CURRENT.md` | clear this freeze from **Now** when shipped |
