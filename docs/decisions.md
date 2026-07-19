# ThoxaNative — Decision Log

Living log of load-bearing choices. Newest first within each section when updated.

**Doc layout (keep it small until it hurts):**

| Path | Role |
|------|------|
| `AGENTS.md` | **Session bootstrap** — auto-loaded by Grok every session; keep short |
| `docs/status.md` | Living “where we are / next rung” — agents update after work |
| `docs/decisions.md` | Locked choices + deferred analyses (this file) |
| `docs/specs/` | Full design docs (architecture, rungs, systems) |
| `.grok/config.toml` | Project permission allows (fewer approval prompts) |

No knowledge graph (`docs/graph/`, node databases, etc.) until cross-links and “where did we say X?” pain are real. Prefer prose + IDs here over premature graph tooling. **Serena-like code-intelligence MCPs are optional**; this repo’s handoff is file-based (`AGENTS.md` + `status.md`), not a separate memory server.

---

## 2026-07-19 — Bytes construction sugar (R5 follow-up)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D14 | String → Bytes surface (now) | Keep `Bytes.fromString(s)` | Matches `../Thoxa` SPEC §9.1; R5 ship shape |
| D15 | `"…" as Bytes` | Deferred to conversions | Belongs with `as` / `can` / `is` ([M4] in SPEC §11), not a one-off |
| D16 | Overloaded `Bytes.from(…)` | Reject (for now) | No ad-hoc overloading story; prefer explicit names if more factories appear |
| D17 | Multi-byte / dynamic build | Prefer `Buffer` path later | `append` / `freeze` → `Bytes`; not `from([1,2,3])` as the main story |
| D18 | Extra factories | Only when forced | e.g. `Bytes(n)` zero buffer for FFI (R6 pressure); maybe `fromArray` when arrays exist |

### Analysis (why)

**`mut b = "abc" as Bytes`**

- Fits Thoxa’s conversion triad once it exists: explicit, no implicit coercion, same family as `n as String`.
- String and Bytes are different kinds: immutable UTF-8 text vs fixed **mutable FFI buffer**. `as` should mean “encode/copy payload,” not “rebrand this string.”
- Implementing a single `as` without the rest of M4 is a design leak (special case, no `can`/`is`, no conversion table).
- Reverse path (`Bytes` → `String`) needs a policy for non-UTF-8; that is conversion work, not R5.

**`Bytes.from("abc")` with overloads (`from([1,2,3])`, …)**

- Friendly, but SPEC already names `fromString`. Renaming reopens the oracle.
- Overload sets need either ad-hoc resolution or a `From`-like trait; Thoxa has neither.
- Bare `from` hides source/encoding (UTF-8 text? raw ints? size?).
- Array form needs array literals + element rules — later rungs.
- If multiple sources appear, prefer **distinct names** (`fromString`, `fromArray`, `Bytes(n)`), same spirit as Buffer’s `append` vs `appendBytes`.

**Rule of thumb**

- Domain construction from a specific source → **named static method** (`fromString`).
- Cross-type encoding/reinterpretation between established types → **`as`**, when the conversion system lands.
- Multiple input shapes → **multiple names**, not silent overload, until overloading is a deliberate language feature.

Revisit D15 when native work takes on [M4] conversions; revisit D17 with Buffer / growable-buffer design; revisit D18 when R6 FFI needs a sized empty buffer.

---

## 2026-07-18 — Kernel kickoff

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D1 | Growth model | Example ladder toward HTTP | Small kernel first |
| D2 | Algebraic effects | Deferred | Revisit with async |
| D3 | Error model | `May`/`Trial` + force-to-deal | Not effect-`exn` |
| D4 | Value model | Hybrid (A) | Unboxed scalars; inline small sums; RC heap; C-layout FFI structs |
| D5 | Phase A backend | Compile-to-C | No QBE yet |
| D6 | Compiler host | Rust under `crates/compiler` | |
| D7 | Runtime | C `thoxa_rt` | Headered RC objects |
| D8 | RC ambition day one | Simple correct RC | Perceus later |
| D9 | Docs path | `docs/specs/`, `docs/decisions.md` | No `docs/superpowers`; no graph until needed |
| D10 | Build automation | `crates/make` xtask, `cargo make …` | No project `.sh` scripts |
| D11 | Build products | Single `target/` tree: `rust/`, `c/`, `thoxa/` | No separate `targets/`; Cargo uses `target/rust` via `.cargo/config.toml` |
| D12 | Language oracle | `../Thoxa` SPEC + conformance | Reopen syntax only deliberately |
| D13 | Old QBE tree | Reference only | Do not revive megapass codegen |
```