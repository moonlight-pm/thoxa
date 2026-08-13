# Thoxa — ideas (parking lot)

Uncommitted language / product sketches. **Not decisions.** Sleep on them;
promote to `docs/specs/` + a D-id in `docs/decisions.md` only when something is
locked and (if needed) scheduled. Living priority never lives here — see root
[`CURRENT.md`](../../CURRENT.md) and [`../capabilities.md`](../capabilities.md).

| Idea | Status | Notes |
|------|--------|--------|
| [having / else (May)](2026-07-20-having-else-may.md) | sleeping | Postfix match sugar; happy path first; no parent-scope bind |
| [immutable values / structural sharing](2026-07-21-immutable-values.md) | sleeping | No refs; COW/persistent DS; peers (Clojure/Swift/…); impact + design spectra vs D4/D54/D50/D8 |
| [webapp SSR / components](2026-07-21-webapp-ssr-components.md) | sleeping | HTML `Node` tree + optional tag sugar; SSR-first; sum not `is`; desugar to library |
| [agent DX from Zerolang](2026-08-09-agent-dx-from-zerolang.md) | sleeping | Parked agent patterns; **promoted** slice is structured diagnostics freeze (D124) |
| [examples/http Crema-lite](2026-08-10-examples-http-crema-experiment.md) | **active experiment** | `server.serve(routes.app())`; `framework/` + `app/`; Phase A–C; no product `std/` growth; living checklist |
| [FCF + closures + ORC](2026-08-11-first-class-functions-and-orc.md) | sleeping | Full fn values / JS-like capture / cycle collector **parked**; prefer traits, defunctionalize, typed fn pointers first |
| [router clause syntax](2026-08-12-router-clause-syntax-experiment.md) | **on master** (not freeze) | Type-body `GET "/…"` → handlers + `route(req) -> Response`; LSP; no D-id / handbook |

**Rules of thumb**

- One idea per file; date prefix + short slug.
- Record motivation, surface, open questions — not implementation plans unless the idea *is* a plan.
- No conformance, no compiler work from these files until promoted.
