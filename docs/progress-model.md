# Progress model — how Thoxa documents itself

**This system is first-class.** Describing the language, tools, and progress is
not optional polish. Any session that changes product behavior, capability
status, or priority must update the matching docs in the **same change**.
Incomplete meta work is incomplete product work.

Read this once; then follow the session checklist in
[`../AGENTS.md`](../AGENTS.md) and the skill
[`.grok/skills/thoxa-progress-docs/SKILL.md`](../.grok/skills/thoxa-progress-docs/SKILL.md).

---

## Four document kinds

Never mix “is” and “will be” in the same paragraph without an explicit label.

```text
┌─────────────────────────────────────────────────────────┐
│  PRODUCT (user)              docs/handbook/             │
│  Audience: people writing Thoxa.                        │
│  “What you can do.” AS-SHIPPED only. No roadmap as fact.│
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  AS-BUILT (engineering)      capabilities + architecture│
│  Audience: humans + agents building.                    │
│  “What exists in code and conformance.”                 │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  TARGET (engineering)        specs + roadmap remaining  │
│  Audience: design + implementation.                     │
│  “What we decided; what’s still left.”                  │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│  FOCUS (session)             CURRENT.md only            │
│  Priority, next slice, locks not to re-open             │
└─────────────────────────────────────────────────────────┘
```

| Kind | Canonical home | Update when |
|------|----------------|-------------|
| **Focus** | Root [`CURRENT.md`](../CURRENT.md) | Priority or next moves change |
| **As-built progress** | [`capabilities.md`](capabilities.md) | Capability status or gaps change |
| **As-built system map** | [`architecture.md`](architecture.md) | Pipeline, crates, RT layout, build products change |
| **Target design** | [`specs/`](specs/) freezes | Decisions lock or amend (not a changelog) |
| **Horizon backlog** | [`roadmap.md`](roadmap.md) | Phase-level status flips |
| **Locked choices** | [`decisions.md`](decisions.md) | New D-id or supersession |
| **Parked** | [`ideas/`](ideas/) | Thought captured; not active until promoted |
| **Product user docs** | [`handbook/`](handbook/) | User-visible **shipped** surface changes |
| **History** | [`../apocrypha/`](../apocrypha/) | Never authoritative |

### Authority order

When documents disagree:

1. **Code that ships** (and conformance / unit tests)
2. **Root `CURRENT.md`** for active priority
3. **`docs/capabilities.md`** for capability maturity
4. **This `docs/` suite** for product/architecture intent
5. **`apocrypha/`** — ignore unless hunting history or oracle SPEC

---

## Capability status vocabulary

Use these statuses on **capabilities** (product surfaces), not only roadmap
phases:

| Status | Meaning | Handbook may document? |
|--------|---------|------------------------|
| **shipped** | In code; has conformance and/or solid unit gate | **Yes** — as fact |
| **partial** | Scaffold or subset; **gaps must be listed** | Only with honest limits |
| **spec’d** | Freeze exists; little or no code | **No** |
| **planned** | On roadmap; no freeze yet | **No** |
| **idea** | [`ideas/`](ideas/) only | **No** |

**Partial without listed gaps is invalid.** Always name what remains.

Roadmap phases may use done / partial / active / next / planned / unplanned —
those are program-horizon labels. Capabilities are the progress truth agents
use day to day.

**Definition of done (language/features):** a checked-in `.thx` compiles, runs,
and matches `.out` under `cargo make test` (plus unit tests where noted).

---

## Spec freeze headers (required)

Every active freeze under `docs/specs/` must open with a machine-scannable
block:

```markdown
**Date:** YYYY-MM-DD
**Status:** target (freeze) | historical
**Implementation:** shipped | partial | not started
**Gaps:** (bullets or “none for this freeze”)
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
```

- Freezes stay **normative desired shape**. Do not rewrite them into changelogs.
- When implementation catches the freeze, set **Implementation: shipped** and
  list remaining polish under Gaps or “none for this freeze”.
- Implementation plans may live in the same file as a freeze when the set is
  small; completed plan sections stay historical. Prefer not inventing a
  parallel living tracker outside CURRENT + capabilities.

---

## Product handbook rule (hard)

[`handbook/`](handbook/) documents **user-visible, working behavior only**.

- If status is **shipped** → document it as fact.
- If status is **partial** → omit, or document only what works, labeled as
  limited.
- Never write roadmap aspiration (“Perceus will…”) as product fact.
- Engineering desire lives in specs + roadmap + capabilities — not the handbook.
- When user-visible behavior ships, update the handbook **in the same change**
  as code.
- Oracle SPEC under `apocrypha/old/ThoxaPython/spec/` is **reference**, not the
  product handbook (much of it is not implemented here).

---

## `CURRENT.md` is a dashboard

**Keep:**

- **Now** — ordered next moves (about 3–7 items)
- **Locked product models** — do-not-re-litigate
- **Pointers** into capabilities, specs, roadmap
- Short agent ops notes / useful commands as needed

**Do not grow into:**

- Full ladder / phase inventory (→ capabilities + roadmap)
- Long ship history (→ git log)
- Design essays (→ specs)
- Duplicate of AGENTS “already known” prose (→ capabilities)

If it is not needed to **choose the next action** or **avoid re-litigating a
lock**, it does not belong in `CURRENT.md`.

**No second living handoff.** No `docs/status.md`, `STATUS.md`, `HANDOFF.md`,
session diaries, or root notes that duplicate CURRENT. (Historical pointer
stubs may redirect only.)

---

## Session loop (mandatory)

### Start

1. [`AGENTS.md`](../AGENTS.md)
2. [`CURRENT.md`](../CURRENT.md) — Now + locks
3. [`capabilities.md`](capabilities.md) — rows for the slice
4. One freeze if building that domain
5. Only other docs needed for the slice

### End (when real product progress — same change)

1. Code + tests as usual (`cargo make test` / unit as appropriate)
2. Update **capability row(s)** (status and/or gaps)
3. Update **CURRENT** Now if priority changed
4. **Handbook** only if user-visible shipped behavior changed
5. **architecture.md** if system map changed
6. **roadmap.md** only if phase-level status flipped
7. **decisions.md** if a D-id locked or superseded
8. Refresh freeze **Implementation / Gaps** header if that freeze’s
   implementation status changed
9. Prefer committing code + meta together when the user asks for a commit

Skipping capability / CURRENT / handbook updates while shipping behavior is a
**process failure**, not a minor omission.

---

## What agents must not do

| Anti-pattern | Why |
|--------------|-----|
| Second living handoff | Both rot; agents pick the wrong one |
| Freezes as progress trackers | Desired state becomes a stale changelog |
| Handbook tracks the roadmap | Users get aspirational docs |
| CURRENT as full program of record | Unreadable; people stop updating |
| “Partial” with no gaps | Status becomes meaningless |
| Per-feature markdown forests without the matrix | Every session rediscovers the world |
| One-off handoff files under root or `docs/` | Forbidden; use CURRENT + capabilities |
| Invent syntax without SPEC / decisions | Conformance and oracle discipline |

---

## Deferred meta work

Tracked here so we do not pretend it is free. Do **not** start these unless
priority says so.

| Item | Why deferred | When to revisit |
|------|----------------|-----------------|
| Generate capabilities from conformance names | Hand matrix is small and accurate | When row count exceeds ~40 or drift hurts |
| Auto-check: handbook claims vs capabilities shipped | Needs a small validator | After handbook pages stabilize |
| Split `docs/specs/active/` vs archive | Date prefixes + Implementation field enough | When active freezes exceed ~12 |
| Formal `supersedes:` graph on freezes | D-ids + headers work | When freeze chain is hard to scan |
| Publish engineering docs as a public site | Repo is enough for contributors | External contributor onboarding |
| Capability IDs in code comments | Nice for cross-links; high churn early | After matrix stabilizes ~1 month |

When deferring further meta ideas, **add a row here** rather than inventing a
parallel tracker.
