# Thoxa — ideas (parking lot)

Uncommitted language / product sketches. **Not decisions.** Sleep on them; promote to `docs/specs/` + a D-id in `docs/decisions.md` only when something is locked and (if needed) scheduled.

| Idea | Status | Notes |
|------|--------|--------|
| [having / else (May)](2026-07-20-having-else-may.md) | sleeping | Postfix match sugar; happy path first; no parent-scope bind |
| [immutable values / structural sharing](2026-07-21-immutable-values.md) | sleeping | No refs; COW/persistent DS; peers (Clojure/Swift/…); impact + design spectra vs D4/D54/D50/D8 |
| [webapp SSR / components](2026-07-21-webapp-ssr-components.md) | sleeping | HTML `Node` tree + optional tag sugar; SSR-first; sum not `is`; desugar to library |

**Rules of thumb**

- One idea per file; date prefix + short slug.
- Record motivation, surface, open questions — not implementation plans unless the idea *is* a plan.
- No conformance, no compiler work from these files until promoted.
