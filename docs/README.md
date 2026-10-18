# Thoxa documentation

This is the **canonical engineering and product documentation** for native
Thoxa (repository root). Historical material and the Python oracle live under
[`../apocrypha/`](../apocrypha/) and are not authoritative for the product.

**User-facing product docs** live in [`handbook/`](handbook/) — **shipped
behavior only**. See [`progress-model.md`](progress-model.md).

## Session boot (agents and humans)

| Step | Document |
|-----:|----------|
| 1 | [`../AGENTS.md`](../AGENTS.md) — working rules + agent autonomy |
| 2 | [`../CURRENT.md`](../CURRENT.md) — **living** priority + locks |
| 3 | [`capabilities.md`](capabilities.md) — as-built maturity for the slice |
| 4 | One freeze under [`specs/`](specs/) if implementing that domain |
| 5 | [`decisions.md`](decisions.md) only when reopening a locked surface |

When you finish real product work, update **`CURRENT.md`** and
**`capabilities.md`** (and handbook / architecture / roadmap as required) in
the same change. Follow
[`.grok/skills/thoxa-progress-docs/SKILL.md`](../.grok/skills/thoxa-progress-docs/SKILL.md).
Do not create one-off handoff files.

**Progress docs are first-class.** Incomplete meta work is incomplete product
work. Full rules: [`progress-model.md`](progress-model.md).

User phrases like **“next item” / “next rung”** mean: implement the **Now**
line in [`CURRENT.md`](../CURRENT.md) (one slice), not redesign the world.

## Document map

| File | Purpose | Kind |
|------|---------|------|
| [`../CURRENT.md`](../CURRENT.md) | Living priority, next moves, locks | **Focus** |
| [`capabilities.md`](capabilities.md) | Capability status + gaps (as-built progress) | **As-built** |
| [`progress-model.md`](progress-model.md) | How focus / as-built / target / handbook fit | Meta |
| [`architecture.md`](architecture.md) | Pipeline, crates, runtime, build products | **As-built** map |
| [`roadmap.md`](roadmap.md) | Phases: done, remaining, unplanned | **Horizon** |
| [`decisions.md`](decisions.md) | Locked D-ids | Decisions |
| [`handbook/`](handbook/) | The manual (introduction, language, std, cli, editor); rendered at thoxa.dev/manual | **Product** |
| [`specs/`](specs/) | Design freezes — **target** architecture | **Target** |
| [`ideas/`](ideas/) | Parked design thoughts (not active until promoted) | Idea |
| [`conventions.md`](conventions.md) | How we write docs and commit meta | Process |

### Active freezes (target design)

| File | Topic | Implementation |
|------|--------|----------------|
| [`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md) | Kernel, value model, ladder | shipped (ladder through R9+) |
| [`specs/2026-07-19-post-r9-http-completion-plan.md`](specs/2026-07-19-post-r9-http-completion-plan.md) | Bounded HTTP server path | shipped (historical plan) |
| [`specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md) | Zed + `thoxa lsp` | partial (phase 1–2 done) |
| [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md) | Sum types, `?`/`!`, May/Trial via `std/Fallible` | shipped (D118–D120, D123) |
| [`specs/2026-08-09-structured-diagnostics.md`](specs/2026-08-09-structured-diagnostics.md) | Diagnostic IR, pretty check, `--json`, rich LSP | shipped (D124) |
| [`specs/2026-08-10-std-log-and-println-removal.md`](specs/2026-08-10-std-log-and-println-removal.md) | `std/Logging` + remove ambient `println`/`print` | shipped (D125) |
| [`specs/2026-08-10-return-type-inference.md`](specs/2026-08-10-return-type-inference.md) | Free-fn return-type inference | shipped (D129; methods D174 / D211; `main` D223) |
| [`specs/2026-08-11-multi-clause-functions.md`](specs/2026-08-11-multi-clause-functions.md) | Multi-clause fn/method = type-driven match sugar | **shipped** (D131) |
| [`specs/2026-08-13-function-pointers.md`](specs/2026-08-13-function-pointers.md) | Function types + free/unbound/bound methods | **shipped** (D132; no closures) |
| [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md) | Swift/Buffer COW + builtin `Map<K, V>` | **shipped** (D133/D134) |
| [`specs/2026-08-22-jsx-node.md`](specs/2026-08-22-jsx-node.md) | Builtin `Node` + tag literals | **shipped** (D147) |
| [`specs/2026-08-22-start-tasks.md`](specs/2026-08-22-start-tasks.md) | `--> name(args)` nowait | **shipped** (D148) |
| [`specs/2026-08-23-type-read.md`](specs/2026-08-23-type-read.md) | `where` refinements + `Type.read` | **shipped** (D149) |
| [`specs/2026-08-23-route-clauses.md`](specs/2026-08-23-route-clauses.md) | Type-body `GET "/…"` | **shipped** (D150) |
| [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md) | Simple-RC correctness (LSan empty, then ASan) | **shipped** (D153 gate 2026-09-03; ASan not default; Perceus not unparked) |
| [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md) | Last-RC `fn drop()` boxes | **shipped** (D155; File/Socket/Db) |
| [`specs/2026-10-16-typed-codecs.md`](specs/2026-10-16-typed-codecs.md) | Typed codecs: encode all, collect-all reads, forms, `Json` literals | **shipped** (D182) |
| [`specs/2026-10-17-conversions.md`](specs/2026-10-17-conversions.md) | Conversions: `as` / `as?`, `from` clauses, implicit `Json`, route body parameters | **shipped** (D191) |

## Related trees (not under `docs/`)

| Path | Role |
|------|------|
| `std/` | Product standard library modules (`std/…` import root) |
| `tests/conformance/` | Definition of done for language features |
| `extras/` | Non-product DX (tree-sitter, Zed) |
| `apocrypha/` | Historical docs + Python oracle (not active product) |
| `.grok/skills/` | Project agent skills (session start + progress docs) |

## Authority order

When documents disagree:

1. **Code that ships** (and tests)
2. **Root `CURRENT.md`** for active priority
3. **`docs/capabilities.md`** for capability maturity
4. This `docs/` suite for product/architecture intent
5. `apocrypha/` — ignore unless hunting history
