# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-08 (D106–D119; dead `Ty::May`/`Trial` removed)

---

## Now

**Session / shell is parked** (core + libtcc shipped). Default product work is
**language kernel depth** — one capability gap at a time, with conformance.
**Shipped:** D106–D119; **D118** user unions + generics + **May/Trial prelude**
cutover; builtin `Ty::May`/`Ty::Trial` MIR/cgen paths removed (unions only).

**In progress:** sum/fallible polish — **generic methods** next; residual
type-arg rewrap edge cases only if forced.
Freeze: [`docs/specs/2026-08-07-sum-types-and-fallible.md`](docs/specs/2026-08-07-sum-types-and-fallible.md).

### Proceed order

1. **Language** — generic methods on sum/generic types (sum-types gap), or next
   CURRENT gap; not a free redesign.  
   Also: session residual / editor only if CURRENT reprioritizes.
2. **Editor depth** — only when DX forces it (MIR on shared resolver, multi-diag,
   typed-member nav). Freeze:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md).
3. **Parked: session residual** — redir polish (`2>`, redir-on-pipe), Shell
   import-first, crude line editor — resume only if CURRENT says so. Freeze:
   [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
4. **Deferred megawork** — M4, Perceus, effects, named `ffi` root, Net errno
   strings, TinyCC license packaging, libtcc base-object cache — not default
   until CURRENT says so.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups; **opportunistic `std/` helpers** proven by a real sample
in the same change (not a free-floating “grow std” sprint).

**Explicit holds:**

- Inventing shell dialect (`if`/`for`/full `$` forest)
- Session residual polish as default work (parked)
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
| Errors | `May` / `Trial` + `?` / `match`; no May↔Trial mix via `?` |
| Session product face | Bare `thoxa` / `thoxa repl`; no-`main` = session script |
| Session value | `type Shell(status: Int64)` + methods; real Thoxa; `std/Shell.thx` |
| REPL step backend | **libtcc in-process only** (`TCC_OUTPUT_MEMORY`) → call `thoxa_session_step` (D105); no step `.so` |
| Batch / RT C | System **`cc`** (+ `ar` for runtime) unchanged |
| Host platforms | **Linux** product track (x86_64 libtcc); Windows out of scope |
| Command sugar | Thoxa-first; flags like `ls -al` desugar; pure `1 - 2` stays Thoxa |
| `extras/shell` | Dogfood only; product face is bare `thoxa` |
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; `ffi` deferred |
| String → Bytes | **`Bytes.fromString` only** — no cast sugar (D15 **rejected**) |
| Done means | Conformance `.thx` + `.out` under `cargo make test` |
| Progress docs | Meta first-class; capabilities + CURRENT (+ handbook when shipped) |
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
- Do not commit unless the user asks.  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
