# Idea: Agent DX patterns from Zerolang (parked)

**Status:** sleeping — capture only; **not** scheduled.  
**Sparked by:** review of [vercel-labs/zerolang](https://github.com/vercel-labs/zerolang)
(agent-first language: graph program DB, checked patches, structured diagnostics).  
**Related freeze (promoted + shipped):**
[`../specs/2026-08-09-structured-diagnostics.md`](../specs/2026-08-09-structured-diagnostics.md)
(**D124**) — diagnostic IR, `check --json`, pretty CLI, rich LSP.  
**Related:** editor plan
[`../specs/2026-07-20-native-editor-lsp-plan.md`](../specs/2026-07-20-native-editor-lsp-plan.md);
version stamp (`thoxa version`); Fallible D123.

---

## Why this file exists

Zerolang optimizes the **agent edit loop**: query semantic structure, apply
checked patches, fail invalid/stale edits early, dual prose/JSON diagnostics,
version-matched skills served by the compiler.

Thoxa is a **text-first systems language** (compile-to-C). We are **not**
pivoting to a graph program database. We **are** taking the diagnostics
foundation (human + machine structure) into a freeze for editor DX.

Everything below is **parked** until the language is more settled and
`CURRENT.md` elevates agent tooling. Do not implement from this file alone.

---

## Already promoted (do not re-park)

| Item | Home |
|------|------|
| Diagnostic IR (codes, help, labels, expected/actual) | structured-diagnostics freeze |
| `thoxa check --json` + version in payload | same |
| Pretty human `thoxa check` | same |
| LSP `code` + `relatedInformation` | same |

---

## Parked ideas worth revisiting later

### 1. `thoxa explain <code>`

Compiler-served short encyclopedia for a diagnostic code (prose + optional
`--json`). Agents and humans get progressive depth without growing every
diagnostic message into an essay.

**Depends on:** stable codes from D124.

### 2. Fix plans / code actions with safety labels

Zerolang’s `fix --plan` safety ladder:

- format-only  
- behavior-preserving  
- api-changing  
- target-changing  
- requires-human-review  

Thoxa shape: LSP code actions or CLI suggestions (e.g. insert
`import { … } from "std/Fallible"`, “did you mean”) — **never** silent
API-changing rewrites. Only after codes are stable.

### 3. Version-matched compiler-served skills / docs

```text
thoxa skills
thoxa skills get diagnostics
thoxa skills get language
thoxa skills get std --topic Fallible
```

Bootstrap skill stays thin; binary serves content matching its version (avoids
handbook/master vs old binary drift). Alternate: `thoxa doc …` from the same
stamp.

### 4. Token-efficient inspect (text-native, not graph)

Reuse resolver/index without a program graph:

- `thoxa outline path.thx` — signatures + docs  
- `thoxa view --fn name` — one function body  
- optional symbols JSON for tools  

Reduces full-file context for humans and future agents.

### 5. Command contracts / CLI snapshot suite

Beyond unit tests for JSON schema: fixtures that lock CLI flag behavior and
diagnostic field presence so tools can treat `thoxa` as an API.

### 6. Agent triage protocol (docs/skill only)

Process, not a language feature:

1. Prefer `thoxa check` / LSP over guessing.  
2. Use `--json` when a tool needs exact fields.  
3. Fix earliest root cause; re-run the same gate.  
4. Small edits near the span; don’t rewrite the file for one name.  
5. Don’t invent syntax — handbook / skill.

Cheap to codify in `AGENTS.md` / a skill when agents become a first-class
audience.

### 7. Structured type-mismatch fields everywhere

D124 allows `expected` / `actual`; a later pass can make **all** type/arity
errors populate them consistently for repair-oriented tooling.

### 8. Soft recovery depth

Editor plan already owns multi-diag recovery. Further mid-expression recovery
is DX-forced only — not an agent feature per se.

### 9. Agent-loop eval harness (research)

Measure “fix this diagnostic in N steps” against golden broken `.thx` files.
Useful meta; not product until agent use is intentional.

---

## Explicitly rejected as Thoxa product core

| Zero pattern | Why rejected |
|--------------|--------------|
| Graph as program source of truth | Thoxa is text + compile-to-C |
| Checked graph patch / expect-hash as primary edit | Agents and humans edit `.thx` text; invest in diagnostics + LSP |
| `.0` projection dualism + import/export reconcile | Second SoT and drift classes we do not want |
| `World` / `raises` / `check`/`rescue` surface as replacement for May/Trial | Language fallibility already locked (D3/D118/D123) |
| Graph-native test registration as definition of done | Conformance `.thx`+`.out` remains done |

Steal **contracts and presentation discipline**, not representation.

---

## Open questions (when revisiting)

1. When is agent DX first-class enough to leave “sleeping”?  
2. Skills-from-binary vs always-repo skills — which fails less often?  
3. Is `explain` worth a CLI before handbook deep-links from LSP `codeDescription`?  
4. Any package fingerprint (weaker than graph hash) useful for multi-file tools?

---

## Promotion rule

Promote a bullet only by:

1. A freeze under `docs/specs/` (or extending D124’s freeze if still diagnostic),  
2. A D-id if locking product surface,  
3. `CURRENT.md` scheduling the slice.

Until then: **sleeping**.
