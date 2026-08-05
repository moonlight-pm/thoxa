---
name: thoxa-progress-docs
description: >
  Mandatory Thoxa documentation and progress maintenance. Update CURRENT.md,
  docs/capabilities.md, specs headers, architecture.md, roadmap, decisions, and
  docs/handbook whenever product behavior or capability status changes.
  Meta work is first-class: incomplete docs mean incomplete product work.
  Use when finishing a slice, shipping code, changing status or priority,
  architecture, freezes, roadmap phases, or user-visible surface; before
  claiming work complete; when committing real product progress;
  on /thoxa-progress-docs; or whenever AGENTS.md / progress-model requires
  handoff updates. Auto-invoke after any non-trivial Thoxa product change.
---

# Thoxa progress docs (mandatory)

Describing the system and its progress is **paramount**. Do not treat this as
optional cleanup after code.

Full model: [`docs/progress-model.md`](../../../docs/progress-model.md).

## When this skill applies

**Always** after any change that affects:

- Product behavior (language, runtime, CLI, session, std, LSP)
- Capability maturity (shipped / partial / gaps)
- Priority or “what next”
- System map (pipeline, crates, build products)
- Spec decisions or roadmap phase status
- User-visible handbook content

If you only reformatted code with no behavior change, skip. If you are unsure,
**update the docs**.

## Status vocabulary (capabilities)

| Status | Meaning | Handbook? |
|--------|---------|-----------|
| shipped | In code; conformance / solid unit gate | yes |
| partial | Subset; **list gaps** | limited only |
| spec’d | Freeze, little code | no |
| planned | Roadmap, no freeze | no |
| idea | `docs/ideas/` only | no |

Partial **without** gaps is invalid.

## End-of-slice checklist (same change as code)

Run through every item. Mark N/A only with a reason.

1. **[`docs/capabilities.md`](../../../docs/capabilities.md)**  
   - Update row(s): Status, Gaps, Gate, Handbook column.  
   - Add a row if a new capability appeared.

2. **[`CURRENT.md`](../../../CURRENT.md)**  
   - **Now** if priority or next moves changed.  
   - **Locked models** only if a product lock was decided.  
   - Keep CURRENT a **dashboard** — do not dump long history here.

3. **[`docs/handbook/`](../../../docs/handbook/)**  
   - Only if user-visible **shipped** (or honestly limited partial) behavior
     changed.  
   - Never document aspirational roadmap as fact.

4. **[`docs/architecture.md`](../../../docs/architecture.md)**  
   - If as-built system map changed (pipeline, crates, session host, targets).

5. **Active freeze under `docs/specs/`**  
   - Refresh Implementation / Gaps header if the freeze’s implementation
     status changed.  
   - Do **not** rewrite freezes into changelogs.

6. **[`docs/roadmap.md`](../../../docs/roadmap.md)**  
   - Only if a **phase-level** status flipped.

7. **[`docs/decisions.md`](../../../docs/decisions.md)**  
   - If a D-id locked, superseded, or deferred stance changed.

8. **Commit**  
   - Only when the user asks. Prefer one commit that includes code + meta.

## Session start (quick)

1. `AGENTS.md` → `CURRENT.md` → `docs/capabilities.md` for the slice  
2. One freeze if needed  
3. Do not create one-off handoff files  

## Hard rules

- **No second living handoff** (`docs/status.md` is a redirect only; no
  `STATUS.md` / `HANDOFF.md` / session diaries).
- **Code wins** over stale docs — then fix the docs immediately.
- **Handbook = as shipped** (progress model).
- **One slice at a time**; conformance is definition of done for language
  features.
- **Meta is not deferred** for shipped work. Deferred meta *improvements*
  live only in
  [`docs/progress-model.md` deferred section](../../../docs/progress-model.md#deferred-meta-work).

## Before claiming “done”

You may not claim a slice complete unless:

- Capability row(s) reflect new truth  
- CURRENT reflects new priority if it changed  
- Handbook updated if user-facing shipped behavior changed  
- Tests appropriate to the code change  

If the user asks to skip docs, still list what would have been updated and
update unless they explicitly forbid it for a throwaway experiment.
