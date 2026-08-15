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
| [examples/http Crema-lite](2026-08-10-examples-http-crema-experiment.md) | **active experiment** | `std/http-server.serve` (D132/D136); type-body `GET`/`POST`; Phase A–C; domain stays example-only; living checklist |
| [FCF + closures + ORC](2026-08-11-first-class-functions-and-orc.md) | sleeping | Full fn values / JS-like capture / cycle collector **parked**. **C** (typed fn pointers + bound methods) **promoted** → **D132** / [`../specs/2026-08-13-function-pointers.md`](../specs/2026-08-13-function-pointers.md) |
| [router clause syntax](2026-08-12-router-clause-syntax-experiment.md) | **on master** (not freeze) | Type-body `GET "/…"` → handlers + `route(req) -> Response`; LSP; no D-id / handbook |
| [JSX / tag literals + `Node`](2026-08-13-jsx-node-syntax-experiment.md) | **on master** (not freeze) | Builtin `Node` + `.html()`; `{e}` children/attrs; spreads; no D-id / handbook |
| [`toString` / `repr` / interpolation](2026-08-15-tostring-repr-and-traits.md) | sleeping | Do not replace `.html()` with generic `toString`; Display is not a traits forcing function |
| [file-based routing](2026-08-15-file-based-routing.md) | **parked** | Path-from-tree for Crema; captures/layouts as filenames deferred. Kebab-case modules split out → **D138** |

**Rules of thumb**

- One idea per file; date prefix + short slug.
- Record motivation, surface, open questions — not implementation plans unless the idea *is* a plan.
- No conformance, no compiler work from these files until promoted.
