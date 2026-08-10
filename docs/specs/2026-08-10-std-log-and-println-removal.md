# `std/Log` and removal of ambient `println`

**Date:** 2026-08-10  
**Status:** target (freeze)  
**Implementation:** not started  
**Gaps:** full surface (this freeze)  

**As-built:** [`../capabilities.md`](../capabilities.md) ·
[`../architecture.md`](../architecture.md) · focus
[`../../CURRENT.md`](../../CURRENT.md)  

**Decision:** **D125** (assign on ship; locked here as product intent).

**Related:** D97 (`std/…` root); D123 (import-required std surface, no ambient
prelude for Fallible); file I/O still primarily `iolib` / free Io (D56+);
session Shell in `std/Shell.thx` (D104).

---

## 0. Product thesis

Program text emission is a **library** concern, not a compiler special case.

**Today:** ambient `println` / `print` are resolver builtins with MIR type
switches (`String` / `Int64` / `Bool`) into `thoxa_println_*`. That is a
one-off I/O channel: stdout only, no levels, no files, no filter.

**Target:** a thin product module **`std/Log`** with the five standard levels,
process-wide defaults, file sinks via **`Trial` on configure**, and
**fire-and-forget emit** (no `?` on every `debug`/`info` call). Ambient
`println` / `print` are **removed**.

**One sentence:**  
`import { info } from "std/Log"` is how programs log; `println` is gone.

---

## 1. Locked answers (open questions from design review)

| # | Question | Lock |
|---|----------|------|
| 1 | Default sink | **stderr** |
| 2 | Default min level | **Info** |
| 3 | File open failure | **`toFile` / path `setSink` → `Trial<(), String>`**; emit path never forces `?` |
| 4 | Dogfood | **All conformance tests + examples + product std (Shell, etc.)** migrate in the same program of work |
| 5 | Module name | **`std/Log`** |

---

## 2. Scope

### In scope

| # | Deliverable |
|---|-------------|
| 1 | **`std/Log.thx`** — level API, process-wide config, format modes |
| 2 | **RT write primitive** — write a line (or bytes) to fd 1 / 2 / open file; enough for Log without ambient print builtins |
| 3 | **Remove ambient `println` / `print`** — resolver, MIR lower, cgen maps, unit fixtures that assume the builtin |
| 4 | **Migrate dogfood** — every product `.thx` that used `println` (conformance, examples, `std/Shell`, session step sources, handbook samples) |
| 5 | **Conformance gates** for Log (level filter, sinks, file Trial, plain format for golden stdout) |
| 6 | **Docs** — capabilities, handbook `std.md` / language, architecture if RT surface changes |

### Explicitly out of scope (this freeze)

| Item | Notes |
|------|--------|
| Structured / JSON log lines | later |
| Multiple handlers / per-module loggers | later |
| Colors / TTY detect | later (`std/Ansi` optional) |
| Source location in lines | needs compiler support; not free |
| Log rotation | OS / ops concern |
| Full product `std/Io` | still sample-driven idea; Log may use thin extern/RT, not block on Io promotion |
| Language **panic** story | unchanged; Log is not panic |
| Removing other builtins (`Array`, `true`, …) | only print surface |
| Changing harness to golden **stderr** by default | no — see §5 |

---

## 3. Public surface (`std/Log`)

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

### 3.2 Emit API (primary)

```thx
export fn trace(msg!: String)
export fn debug(msg!: String)
export fn info(msg!: String)
export fn warn(msg!: String)
export fn error(msg!: String)

export fn log(level!: Int64, msg!: String)
```

**Rules:**

- Message is **`String` only.** Callers format with interpolation:
  `info("n={n}")`, not a polymorphic builtin.
- If `level < getLevel()`, emit is a no-op.
- Emit is **`void`** — never `Trial` / never requires `?`.
- After a sink is configured, **write failures on emit are best-effort**
  (drop or ignore for v0). Do **not** turn every log site into fallible control
  flow. There is no panic story yet; logging must not invent one.

### 3.3 Configuration (process-wide module mut)

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
- On **Pass**, switch the process-wide sink; close previous file fd if any.
- On **Fail**, **leave the previous sink unchanged**.
- Call sites that care:

```thx
match toFile("/var/log/app.log") {
    Pass(_) { }
    Fail(e) { error("log file: {e}") }   ## or return Fail(e) from main setup
}
```

Programs that never call `toFile` never see this Trial.

### 3.4 Line format

Two modes (process-wide):

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

### 3.5 Environment bootstrap (optional, first use)

| Variable | Effect |
|----------|--------|
| `THOXA_LOG_LEVEL` | `trace` / `debug` / `info` / `warn` / `error` (ASCII, case-insensitive) |
| `THOXA_LOG_SINK` | `stderr` / `stdout` / path |

- Applied once on first emit or first explicit get/set that needs defaults.
- Explicit `setLevel` / `setSink` / `to*` after start always wins.
- If env names a **path** and open fails: **keep stderr**, do not abort the
  process (env is best-effort bootstrap; programmatic `toFile` remains the
  strict Trial path).

### 3.6 Defaults

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
| RT | Prefer a small **fd write** used by `std/Log` (and anything else that needs bytes on a fd). Old `thoxa_println*` may become internal helpers or be deleted once unused |

### 4.2 Language rule

- There is **no** ambient print. Unknown `println` is an ordinary undefined name.
- Session command desugar must **not** treat `println(...)` as special beyond
  “already valid Thoxa” (today it is kept as Thoxa; after removal it is simply
  invalid unless imported — and it will not be exported under that name).

### 4.3 Replacement patterns

| Old | New |
|-----|-----|
| `println("hi")` | `info("hi")` with appropriate sink/format (see §5) |
| `println(x)` for Int64/Bool | `info("{x}")` |
| Diagnostics in apps | `info` / `warn` / `error` (default stderr, full format) |
| Golden / data lines on stdout | `toStdout()` + `setFormat(formatPlain)` + `info("…")` (or `debug` if level lowered) |

Do **not** reintroduce a second ambient or compiler-known `print` under a new
name. If a thinner “write this string to stdout” helper is needed later, it
belongs in **std** (Log or future Io), not the resolver builtin table.

---

## 5. Conformance, examples, and harness

### 5.1 Harness unchanged

`cargo make test` continues to compare **stdout** to sibling `.out` files.
Default Log sink is **stderr**, so unconfigured Log does **not** affect
goldens.

### 5.2 Migrating tests that used `println` for expected output

Typical preamble in `main` (or module init pattern once available):

```thx
import { info, toStdout, setFormat, formatPlain } from "std/Log"

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
- Tests **about Log itself** may assert full format with a **fixed** message
  and either plain mode or sink stdout without checking the clock portion —
  prefer testing filter/sink/Trial behavior over timestamp text.

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
`warn` / `error` (stderr, full format by default once Log is default-config).

### 5.4 New gates (minimum)

| Gate | Intent |
|------|--------|
| `log_levels` (name flexible) | filter: debug silent at Info; info/warn/error emit |
| `log_sink` / file case | `toFile` Pass writes; bad path Fail; prior sink unchanged |
| Existing suites | green after println migration |

---

## 6. Implementation slices (suggested order)

1. **RT + Log skeleton** — fd write; `std/Log` with defaults, level filter,
   stderr/stdout sinks, full/plain format; unit/conformance for Log only.
2. **File sink** — `toFile` / path `setSink` → `Trial`; append writes.
3. **Env bootstrap** — `THOXA_LOG_LEVEL` / `THOXA_LOG_SINK`.
4. **Remove builtins** + **mass migrate** conformance/examples/std/session —
   one coordinated change so tree never half-depends on ambient print.
5. **Handbook + capabilities shipped**; freeze Implementation → shipped.

Slices 1–3 may land before 4 if Log is additive; **slice 4 is atomic**
(println removal + rewrites together).

---

## 7. Non-goals / anti-patterns

- **`debug(...)?` or `info(...)?`** — rejected for v0 emit API.
- **Panicking on log write failure** — rejected.
- **Default sink stdout** — rejected (pipelines / data tools).
- **Keeping `println` as deprecated alias** — rejected; delete the one-off.
- **Ambient inject of `std/Log`** — rejected (same spirit as D123). Import
  required. Session may import Log from Shell / step sources explicitly.

---

## 8. Acceptance

This freeze is **done** when:

1. `std/Log` matches §3 with defaults §3.6.  
2. Ambient `println` / `print` are gone; no MIR special case.  
3. `cargo make test` green with migrated goldens.  
4. Examples and `std/Shell` compile without ambient print.  
5. Capabilities `std-log` → **shipped**; handbook documents Log; language
   handbook no longer teaches ambient `println`.  
6. D125 recorded as shipped in `docs/decisions.md`.

---

## 9. Capability / docs map (on ship)

| Doc | Update |
|-----|--------|
| `docs/capabilities.md` | add/ship `std-log`; note println removal under language/I-O if listed |
| `docs/handbook/std.md` | `std/Log` module |
| `docs/handbook/language.md` | drop ambient println; show Log / plain stdout pattern |
| `docs/architecture.md` | RT write / drop print builtins if map mentions them |
| `CURRENT.md` | clear this freeze from **Now** when shipped |
