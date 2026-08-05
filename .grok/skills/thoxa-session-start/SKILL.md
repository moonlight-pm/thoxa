---
name: thoxa-session-start
description: >
  Boot a Thoxa engineering session in the cheap path: AGENTS.md, CURRENT.md,
  capabilities for the slice, one freeze if needed. Use at session start, when
  the user says “what next”, “where are we”, or when context was lost after
  compaction. Do not re-walk the tree or re-derive the ladder.
---

# Thoxa session start

## Read order (cheap path)

1. [`AGENTS.md`](../../../AGENTS.md) — rules + commands  
2. [`CURRENT.md`](../../../CURRENT.md) — **Now** + locks  
3. [`docs/capabilities.md`](../../../docs/capabilities.md) — rows for the slice  
4. **One** freeze under `docs/specs/` only if implementing that domain  
5. [`docs/decisions.md`](../../../docs/decisions.md) only when reopening surface  
6. [`docs/handbook/`](../../../docs/handbook/) if changing user-visible behavior  

Full map: [`docs/README.md`](../../../docs/README.md).  
Progress rules: [`docs/progress-model.md`](../../../docs/progress-model.md).

## Do not

- Re-walk the whole repository or re-read every compiler module  
- Re-derive R0–R9 from scratch (ladder is done; see capabilities historical table)  
- Create `STATUS.md` / `HANDOFF.md` / session diaries  
- Treat `docs/status.md` as living (redirect only)  
- Treat `apocrypha/` as product truth  

## “What next”

Implement the first open item under **Now** in `CURRENT.md` (one slice).

## After real product work

Run [thoxa-progress-docs](../thoxa-progress-docs/SKILL.md).
