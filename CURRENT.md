# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-10 (`std/Io`+`Net`+`Http`+`HttpStatic` **D126**; session deferred)

---

## Now

**Session / shell is deferred** — do not resume residual session polish unless
this file says so. Prefer **finishing started frozen surfaces** over free
redesign or expand-std sprints.

**Shipped recently:** **`std/Io` + `std/Net` + `std/Http` + `std/HttpStatic`**
(**D126**, sample-driven from http); **`std/Logging`** (**D125**); **LSP
call-arg hover**; **typed members after `?`**; **THX010**; **D124** diagnostics;
**`thoxa version`**; **D123** Fallible import; **`examples/kilo`** /
**`examples/http`** (now Main+web only). Session residual deferred.
Agent-only ideas parked:
[`docs/ideas/2026-08-09-agent-dx-from-zerolang.md`](docs/ideas/2026-08-09-agent-dx-from-zerolang.md).

**In progress / next:** language gaps forced by samples; sample-driven std when
forced; editor only when DX forces (nav residual / recovery depth).

### Proceed order

1. **Language / I/O / std** — next matrix gap (non-session); not free redesign.  
2. **Editor depth** — only when DX forces it. Freezes:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md)
   (recovery/nav residual);
   [`docs/specs/2026-08-09-structured-diagnostics.md`](docs/specs/2026-08-09-structured-diagnostics.md)
   (**D124** shipped — polish codes opportunistically).  
3. **Deferred: all session / shell residual** — redir polish, line editor,
   Shell import-first, libtcc cache/license, etc. Freeze parked:
   [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
4. **Deferred megawork** — M4, Perceus, effects, named `ffi` root — not default
   until CURRENT says so. Multi-logger (`initLog`) is trajectory under D125, not
   a CURRENT line until samples force it.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups; **opportunistic `std/` helpers** proven by a real sample
in the same change (not a free-floating “grow std” sprint).

**Explicit holds:**

- **All session / shell residual** (parked by user direction)
- Inventing shell dialect (`if`/`for`/full `$` forest)
- Tree-walk product interpreter or QBE path (phase A)
- Claiming full oracle SPEC coverage
- Windows / non-Linux session backends
- Treating “expand std/” as its own priority line
- **`"…" as Bytes`** or cast-as-encode for String→Bytes (use `Bytes.fromString`;
  D15 **rejected**)

Horizon: [`docs/roadmap.md`](docs/roadmap.md). Matrix:
[`docs/capabilities.md`](docs/capabilities.md).

---

## Locked product models (do not re-litigate)

| Topic | Rule |
|-------|------|
| Backend (phase A) | **Compile-to-C** + `thoxa_rt`; no QBE product path |
| Value model | Hybrid A; simple single-threaded RC; Perceus later |
| Errors | `May` / `Trial` from `std/Fallible` (import) + `?` / `match`; no May↔Trial mix via `?` |
| Session product face | Bare `thoxa` / `thoxa repl`; no-`main` = session script |
| Session value | `type Shell(status: Int64)` + methods; real Thoxa; `std/Shell.thx` |
| REPL step backend | **libtcc in-process only** (`TCC_OUTPUT_MEMORY`) → call `thoxa_session_step` (D105); no step `.so` |
| Batch / RT C | System **`cc`** (+ `ar` for runtime) unchanged |
| Host platforms | **Linux** product track (x86_64 libtcc); Windows out of scope |
| Command sugar | Thoxa-first; flags like `ls -al` desugar; pure `1 - 2` stays Thoxa |
| `extras/shell` | Dogfood only; product face is bare `thoxa` |
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; Io/Net/Http/HttpStatic shipped (**D126**); `ffi` deferred |
| Logging / print | **`std/Logging` only** (**D125** shipped); ambient **`println`/`print` removed**; app face `import logging` + `import { info, … }`; no ambient Logging inject; multi-logger later |
| String → Bytes | **`Bytes.fromString` only** — no cast sugar (D15 **rejected**) |
| Done means | Conformance `.thx` + `.out` under `cargo make test` |
| Progress docs | Meta first-class; capabilities + CURRENT (+ handbook when shipped) |
| Diagnostics | Structured IR → pretty check + `--json` + rich LSP (**D124** shipped); agent-only extras parked |
| Oracle | `apocrypha/old/ThoxaPython` reference only — not the product |

More D-ids: [`docs/decisions.md`](docs/decisions.md).

---

## Session start checklist

1. [`AGENTS.md`](AGENTS.md)  
2. **This file** (Now + locks)  
3. [`docs/capabilities.md`](docs/capabilities.md) — rows for the slice  
4. One freeze if building that domain  
5. [`docs/decisions.md`](docs/decisions.md) only when reopening surface  

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo test -p thoxa --lib
cargo run -p thoxa -- tests/conformance/param_defaults.thx
cargo run -p thoxa -- tests/conformance/user_rc.thx
printf 'echo hi\nexit\n' | cargo run -p thoxa --
```

---

## Agent notes

- “Next item” / “next rung” → first open item under **Now**, one slice.  
- Do not re-walk the tree or re-derive the ladder at session start.  
- **After each finished checklist / product slice:** update progress docs,
  then **commit and push** (user standing order).  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
