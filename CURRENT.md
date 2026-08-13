# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-13 (**branch `experiment/implicit-async`** — colorless
wait + `start`; Crema-lite and D131 remain as on master)

---

## Now

**Active experiment — implicit wait + `start`** (this branch). User model:
JS with `async`/`await` erased; calls stay sequential; `start name(args)` is
the omitted `await`. No Future values, no `run()`, no FCF. Living review
(gitignored, run locally):

```bash
cargo run -p thoxa -- run scratch/async.thx
```

**Hard rules for this experiment:** keep the review in **one** `scratch/async.thx`
(flip TARGET → live there; do not add a second demo). Do **not** grow `std/`
until the scratch file forces it. No D-id / freeze / handbook until we say so.

**Parked on this branch:** Crema-lite Phase D+ (still example-only on master).
**Session / shell remains deferred.**

**Shipped on master (unchanged):** **multi-clause functions** (**D131**);
Crema-lite **Phase A–C**; JSX `Node` + route clauses (not freeze); **D130**
match arms; **D129** return inference; **D128** diverging `loop`; **D127**
combined import; **D126** Io/Net/Http; **D125** Logging; **D124** diagnostics.

**On master, not product:**

- Type-body `GET "/…"` desugar + LSP (idea
  [`docs/ideas/2026-08-12-router-clause-syntax-experiment.md`](docs/ideas/2026-08-12-router-clause-syntax-experiment.md);
  capability `route-clauses` **partial**). No D-id / handbook. Crema dogfoods
  the clauses (example-only).
- Tag literals + builtin **`Node`** (idea
  [`docs/ideas/2026-08-13-jsx-node-syntax-experiment.md`](docs/ideas/2026-08-13-jsx-node-syntax-experiment.md);
  capability `jsx-node` **partial**). No import; `.html()`; gate `jsx`. No
  D-id / handbook. Crema pages dogfood it (`app/Layout.thx` `Page` +
  `app/Pages.thx`).

**In progress / next:**

1. **Implicit wait + `start`** — sections 1–4 of `scratch/async.thx` are
   **live**. `std/Net` accept/connect/receive/send park the current task
   (same face as before). Next: dogfood Crema `start serveOne` only if we
   want the example server to drop the 200ms recv timeout.  
2. Crema-lite Phase D+ only if useful (parked while this branch is Now).  
3. Editor only when DX forces it.

### Proceed order

1. **This experiment** — next slice is whatever makes the next TARGET
   block in `scratch/async.thx` live. Scratch-only until that file needs
   `std/` or a wait primitive in `thoxa_rt`.  
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
conformance green-ups. **`std/` growth is on hold** for Crema *and* for this
async experiment — helpers stay in `scratch/async.thx` (or `examples/http/`
for Crema) until CURRENT allows otherwise.

**Explicit holds:**

- **`std/` growth** for Crema-lite and for this async experiment (scratch-only
  until the review file forces a primitive)
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
| Logging / print | **`std/Logging` only** (**D125**); ambient **`println`/`print` removed**; app face `import logging, { info, … }` (**D127**); no ambient inject; multi-logger later |
| Diverging loop | Bare `loop` with no exiting `break` is **Never** (**D128**) — valid function tail without dummy value |
| Return inference | Unannotated free fns: returns inferred from exits (**D129**); explicit `->` authoritative; no Trial-by-name; recursion / bare Pass/Fail alone need annotate; methods still annotate or default Void; hover uses inferred types |
| Match arms | Bare ctor = tag-only; `=>` single-expr body or `{ }` multi-stmt (**D130**); `->` types only |
| Multi-clause fns | Same name → one fn (match sugar); `name: Type` formals; any-type dispatch; source order; bare literal types (**D131** — **shipped**). No defaults on multi-clause v0; overlap/export mismatch are **errors**. D16 std factories still separate. |
| Combined import | `import ns, { a, b as c } from "…"` (**D127**) — namespace + selective in one clause |
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
6. If this branch: `scratch/async.thx` (living review; gitignored)

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo test -p thoxa --lib
cargo make run examples/http
cargo run -p thoxa -- tests/conformance/param_defaults.thx
printf 'echo hi\nexit\n' | cargo run -p thoxa --
```

---

## Agent notes

- “Next item” / “next rung” → first open item under **Now**, one slice.  
- Do not re-walk the tree or re-derive the ladder at session start.  
- **After each finished checklist / product slice:** update progress docs,
  then **commit and push** (user standing order).  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
