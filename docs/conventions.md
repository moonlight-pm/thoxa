# Conventions

## Documentation

1. **`docs/` is canonical engineering prose.** Root `apocrypha/` is history, not
   truth. User product docs are [`handbook/`](handbook/) (shipped-only).
2. **Root `CURRENT.md` is the only living handoff** (focus). Capability maturity
   lives in [`capabilities.md`](capabilities.md). Full model:
   [`progress-model.md`](progress-model.md).
3. **Progress meta is first-class.** Update capabilities + CURRENT (+ handbook
   when user-visible shipped behavior changes) in the **same change** as product
   progress. Incomplete docs = incomplete work.
4. **No second handoff files** (`docs/status.md`, `STATUS.md`, `HANDOFF.md`,
   session diaries).
5. Prefer short, scannable docs over stage essays.
6. **Never mix is and will-be** without labels. Specs = target; architecture +
   capabilities = as-built; handbook = shipped; CURRENT = focus.
7. Spec freezes must carry Implementation / Gaps headers (see progress model).
8. **Conformance is the definition of done** for language features (`.thx` +
   `.out` under `cargo make test`).
9. Prefer prose + D-ids over premature knowledge graphs or memory servers.
10. **Doc truth:** do not document aspirational surface as fact; fix dogfood
    READMEs when they contradict the product face.

## Agent sessions (Grok)

1. Read `AGENTS.md` → `CURRENT.md` → `docs/capabilities.md` → relevant freeze.
2. Skills: `thoxa-session-start`, `thoxa-progress-docs` (project `.grok/skills/`).
3. Routine edit/test is allowed; do **not** commit unless the user asks.
4. Do not create random `NOTES.md` / handoff files.
5. When finishing a slice: run the **thoxa-progress-docs** checklist.

## Code

- Prefer `cargo make …` over ad-hoc scripts (D10).
- Build products under `target/{rust,c,thoxa}/` only.
- One rung / one slice at a time; match oracle surface; don’t invent syntax
  without checking SPEC / decisions.
- **No QBE / no tree-walk product interpreter** in phase A.
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.

## Git

- Small logical commits with complete-sentence messages **when the user asks**.
- Prefer including doc/meta updates in the same commit as the product change.
- Do not force-push shared history without explicit ask.

## Naming (product speech)

| Prefer | Avoid as product face |
|--------|------------------------|
| `std/module` imports (incl. `std/fallible`) | inventing a second ambient prelude language |
| conformance fixture names | informal “it works on my machine” |
