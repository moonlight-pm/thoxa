# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-08-05 (Thoxa-side pipes + redirs shipped; session polish continues)

---

## Now

**Kernel + HTTP + editor phase 1–2 + session core + pipes + file redirs are shipped.**
Default work is remaining **session polish** and thin `std/` growth — not
re-opening the ladder.

### Proceed order

1. **Session polish** — prelude out of compiler embed; drop host `status`/`cd`/`|`
   when pure step path is enough; optional faster REPL C (`tcc`).  
   Plan: [`docs/specs/2026-07-24-thoxa-session-repl.md`](docs/specs/2026-07-24-thoxa-session-repl.md).
2. **Grow `std/`** — more Str/Path/Io helpers; `ffi` remains deferred.
3. **Editor depth** — only when DX forces it (MIR on shared resolver, multi-diag,
   typed-member nav). Freeze:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md).
4. **Deferred megawork** — M4, Perceus, effects, File type / method mut-field
   write-back, Net errno strings — not default until CURRENT says so.

**Explicit holds:**

- Inventing shell dialect (`if`/`for`/full `$` forest)
- Tree-walk product interpreter or QBE path (phase A)
- Claiming full oracle SPEC coverage
- Breadth language features ahead of session polish unless human redirects

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups.

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
| Session value | `type Shell(status: Int64)` + methods; real Thoxa |
| REPL execution | Host-held status + **dlopen** `thoxa_session_step`; preamble `.o` cache |
| Command sugar | Thoxa-first; flags like `ls -al` desugar; pure `1 - 2` stays Thoxa |
| `extras/shell` | Dogfood only; product face is bare `thoxa` |
| `std/` root | Named `std/…` (D97); thin modules; `ffi` deferred |
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
cargo test -p thoxa --lib repl::
cargo test -p thoxa --lib command::
printf 'echo hi\nls -al\nexit\n' | cargo run -p thoxa --
cargo make shell --login
cargo run -p thoxa -- tests/conformance/stdlib.thx
cargo run -p thoxa -- tests/conformance/param_defaults.thx
```

---

## Agent notes

- “Next item” / “next rung” → first open item under **Now**, one slice.  
- Do not re-walk the tree or re-derive the ladder at session start.  
- Do not commit unless the user asks.  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
