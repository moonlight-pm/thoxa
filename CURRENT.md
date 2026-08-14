# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-13 (`-->` nowait **on master**, not freeze / not handbook;
Crema-lite Phase A–C + JSX `Node`; D131 multi-clause)

---

## Now

**Session / shell remains deferred.**

**Shipped recently:** **`-->` nowait** (colorless wait; Net parks; Crema
`--> serveOne` + keep-alive); **multi-clause functions** (**D131**);
Crema-lite **Phase A–C**; **match arm sugar** (**D130**); **return-type
inference** (**D129** + LSP hover); **diverging `loop`** (**D128**);
**combined import** (**D127**); **`std/Io`+`Net`+`Http`** (**D126**);
**`std/Logging`** (**D125**); LSP nav; **D124** diagnostics.

**On master, not product:**

- Colorless wait + **`--> name(args)`** (capability `start-tasks` **partial**).
  No Future / `async` / `run`. Token only (not a word). `--> { }` later.
  No D-id / handbook. Crema dogfoods it. Review: `scratch/async.thx`
  (gitignored).
- Type-body `GET "/…"` desugar + LSP (idea
  [`docs/ideas/2026-08-12-router-clause-syntax-experiment.md`](docs/ideas/2026-08-12-router-clause-syntax-experiment.md);
  capability `route-clauses` **partial**). No D-id / handbook. Crema dogfoods
  the clauses (example-only).
- Tag literals + builtin **`Node`** (idea
  [`docs/ideas/2026-08-13-jsx-node-syntax-experiment.md`](docs/ideas/2026-08-13-jsx-node-syntax-experiment.md);
  capability `jsx-node` **partial**). No import; `.html()`; `{e}` children
  are String / Int64 / Bool / Node / Array<Node>; gate `jsx`. No D-id /
  handbook. Crema pages dogfood it (`app/Layout.thx` `Page` +
  `app/Pages.thx`).

**In progress / next:**

1. **Crema-lite** — Phase D+ only if useful. Still example only; no `std/`.  
2. Editor only when DX forces it.  
3. Sample-driven language polish (no open CURRENT language slice).

### Proceed order

1. **Crema-lite experiment** — next open checklist step in
   [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)
   (all work under `examples/http/`; no `std/`).  
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
conformance green-ups. **`std/` growth is on hold for the Crema experiment** —
helpers go in `examples/http/`. (Other product slices may still grow std when
CURRENT explicitly allows it outside this experiment.)

**Explicit holds:**

- **`std/` growth for the Crema-lite experiment** (user lock — example-only)
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
6. If Crema experiment: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)

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
