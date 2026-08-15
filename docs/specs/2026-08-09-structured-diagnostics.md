# Structured diagnostics — IR, CLI, human form, LSP

**Date:** 2026-08-09  
**Status:** target (freeze)  
**Implementation:** shipped  
**Gaps:** mid-expression parse recovery still limited (editor plan); not every free-form message has an explicit emit-site code (message classifier covers the high-traffic batch); build/run still primary-error only; no `explain` / code actions (parked ideas); first warning lint is unused import (`THX010`) — more lints later 

**As-built:** [`../capabilities.md`](../capabilities.md) ·
[`../architecture.md`](../architecture.md) · focus
[`../../CURRENT.md`](../../CURRENT.md)  

**Related freezes:**
[`2026-07-20-native-editor-lsp-plan.md`](2026-07-20-native-editor-lsp-plan.md)
(editor / multi-diag recovery — **complementary**; this freeze owns diagnostic
*shape* and presentation).  

**Parked (not this freeze):**
[`../ideas/2026-08-09-agent-dx-from-zerolang.md`](../ideas/2026-08-09-agent-dx-from-zerolang.md)
(explain catalog, fix plans, skills get, scoped view, agent triage protocol, …).

**Decision:** **D124**.

---

## 0. Product thesis

Diagnostics should be **clear and actionable** for humans editing Thoxa —
especially in **Zed via `thoxa lsp`** and at the **`thoxa check`** CLI.

**As-built (shipped):** one structured IR drives pretty CLI, `--json`, and LSP
(codes, help, expected/actual when parseable, related labels). Multi-diag
recovery still finds multiple issues in one check pass. Residual free-form
emit sites are classified by message pattern when no explicit code was set.

**One sentence:**  
One structured diagnostic IR → pretty human CLI + optional JSON + rich LSP.

**Priority of this freeze:** **editor and human DX first.** The same foundation
is useful later for tooling and agents, but this freeze does **not** schedule
agent-first surfaces (skills server, auto-fix, graph edits, etc.). Those stay
parked under ideas until the language is more settled and CURRENT elevates them.

---

## 1. Scope (this freeze = trajectory items 1–4)

| # | Deliverable | Purpose |
|---|-------------|---------|
| 1 | **Diagnostic IR** | Shared structure: code, severity, message, primary span, labels, help, optional expected/actual |
| 2 | **`thoxa check --json`** | Stable machine view of the same IR (+ compiler identity) |
| 3 | **Pretty human CLI output** | Actionable terminal presentation (codes, underlines, help) |
| 4 | **LSP mapping** | `code` + `relatedInformation` (+ message) from the same IR |

**In scope**

- Grow `Error` / check pipeline into a first-class `Diagnostic` (or equivalent)
  used by `thoxa check` and `thoxa lsp`.
- Stable diagnostic **codes** for errors we emit (phased catalog — not every
  string must have a code on day one, but new work prefers codes).
- Human-readable default CLI for `thoxa check` that shows code, location,
  snippet/caret when span known, and help when present.
- `thoxa check --json` emitting a documented schema.
- LSP `publishDiagnostics` carrying `code` and `relatedInformation` when the
  IR has them; keep `source: "thoxa"`.
- Unit tests for convert/format/JSON; multi-diag behavior preserved.

**Explicitly out of scope (this freeze)**

| Item | Where it lives |
|------|----------------|
| `thoxa explain <code>` encyclopedia | ideas (later) |
| Auto-fix / code actions / fix-safety labels | ideas |
| Compiler-served `thoxa skills get …` | ideas |
| Scoped CLI inspect (`outline` / `view --fn`) | ideas |
| Agent triage protocol as a product skill | ideas |
| Graph / patch / dual source-of-truth | **rejected** as product direction |
| Changing language fallibility (`May`/`Trial`) | already locked (D3/D118/D123) |
| Making `build`/`run` multi-diag | optional later; check/LSP remain multi-diag owners |
| Handbook full rewrite before ship | handbook updates only when slices ship |

---

## 2. Diagnostic IR (normative)

### 2.1 Fields

```text
Diagnostic {
  code:       Option<DiagCode>,   // stable id when catalogued
  severity:   Error | Warning | Note,  // v0: Error primary; Warning optional later
  message:    String,             // one-line summary (no trailing period required)
  span:       Option<Span>,       // primary byte span into the relevant source
  path:       Option<PathBuf>,    // file path / buffer label when known
  labels:     Vec<Label>,         // secondary spans (“defined here”, …)
  help:       Vec<String>,        // concise next actions (0..n)
  expected:   Option<String>,     // structured mismatch (types, arity, …)
  actual:     Option<String>,
}

Label {
  span:    Span,
  path:    Option<PathBuf>,       // if different file from primary
  message: String,                // short label text
}
```

**Rules**

- **Primary message** stays short and plain (no session jargon; see existing
  multi-diag wording rules).
- **`help`** is optional but preferred for actionable classes (undefined name,
  missing import, May/Trial misuse, top-level stmt with `main`, …).
- **`expected` / `actual`** when the failure is a mismatch; they must also be
  readable if inlined into message for consumers that only show `message`.
- **`labels`** map to human “note” lines and to LSP `relatedInformation`.
- **Codes** are stable once published; message wording may improve without
  changing the code.
- Multi-diag remains a **list of independent `Diagnostic`s** (today’s
  `Error.more` flattens into that list). Soft-recovered AST is orthogonal
  (check engine), not a diagnostic field.

### 2.2 Code naming

- Form: **`THX` + category letters + number**, displayed as e.g. `THX001` or
  `error[THX001]` in pretty CLI.
- Categories (initial, not exhaustive):

| Prefix cluster | Domain (examples) |
|----------------|-------------------|
| parse / lex | unexpected token, unclosed delimiter |
| name | undefined name, already defined, not in scope |
| type | type mismatch, arity, missing return |
| import | bad specifier, missing module, selective import miss |
| fallible | `?` residual mismatch, May/Trial misuse |
| program | top-level stmt with `main`, missing main when required |
| io / driver | cannot read file, bad CLI usage (may stay code-less at first) |

Exact code table is filled as emitters are migrated. Unknown/legacy free-form
errors may omit `code` until converted (`code: null` in JSON).

### 2.3 As-built migration

| Today | Target |
|-------|--------|
| `Error { message, span, more, recovered_module }` | Keep recovery plumbing; presentation path builds `Vec<Diagnostic>` |
| `CheckDiagnostic { message, range, span, filename }` | Built from IR (add code, related, help) |
| `path:line:col: message` CLI | Pretty form (§4); compact one-liner optional via flag if useful |
| LSP message-only | §5 |

Prefer **one conversion site** (e.g. `error` / `diagnostic` module +
`lsp/convert`) so CLI and LSP cannot diverge.

---

## 3. `thoxa check --json` (normative)

### 3.1 Invocation

```bash
thoxa check path.thx           # pretty human (default)
thoxa check --json path.thx    # machine contract
thoxa check path.thx --json    # same (flag position flexible)
```

- Exit code: **0** if no diagnostics of severity Error; **1** if any Error
  (warnings alone → 0 when warnings exist later).
- JSON is for tools and tests; humans use the default pretty form.

### 3.2 Schema (v0)

```json
{
  "ok": false,
  "compiler": {
    "version": "0.1.0+<git>[.dirty]",
    "path": "/abs/or/reported/binary"
  },
  "diagnostics": [
    {
      "code": "THX001",
      "severity": "error",
      "message": "undefined name `fooo`",
      "path": "main.thx",
      "byte_start": 120,
      "byte_end": 124,
      "line": 12,
      "column": 9,
      "end_line": 12,
      "end_column": 13,
      "help": ["did you mean `foo`?"],
      "expected": null,
      "actual": null,
      "related": [
        {
          "path": "main.thx",
          "line": 3,
          "column": 4,
          "byte_start": 10,
          "byte_end": 13,
          "message": "`foo` defined here"
        }
      ]
    }
  ]
}
```

**Rules**

- `line` / `column` are **1-based** in JSON (human-aligned); document that
  clearly. LSP remains 0-based UTF-16 ranges internally.
- Omit or null optional fields consistently; schema stability > clever brevity.
- `compiler.version` reuses the same identity as `thoxa version` / LSP stamp.
- Snapshot / unit tests own the contract for at least one golden example per
  severity class we care about.

### 3.3 Non-goals for JSON v0

- SARIF export  
- Full explain bodies embedded in each diagnostic  
- Fix / repair objects  
- Streaming multi-file workspace check protocol  

---

## 4. Pretty human CLI (normative)

Default `thoxa check` output should be **scannable and actionable**, rustc-like
in spirit without requiring full rustc parity.

### 4.1 Shape

```text
error[THX001]: undefined name `fooo`
 --> main.thx:12:9
  |
12 |     let x = fooo()
  |             ^^^^ not found in this scope
  |
  = help: did you mean `foo`?
  = note: `foo` defined here
 --> main.thx:3:4
```

Minimum viable pretty (acceptable v0 if snippet plumbing is heavy):

```text
error[THX001]: undefined name `fooo`
  --> main.thx:12:9
   = help: did you mean `foo`?
```

### 4.2 Rules

- Always print **path:line:col** when span maps cleanly (same data as today).
- Print **`error[CODE]:`** when code present; otherwise `error:`.
- Multiple diagnostics: print each fully; blank line between.
- Do **not** require color in v0; color is optional polish if `stderr` is a TTY.
- Optional later: `--compact` one-liner for greppable scripts
  (`path:line:col: CODE: message`) — not required for freeze completion if
  pretty + JSON cover human and machine needs.

### 4.3 `build` / `run`

Unchanged for this freeze unless a tiny shared formatter is reused for the
primary error. Multi-diag remains a **check/LSP** concern.

---

## 5. LSP mapping (normative)

Same IR → `textDocument/publishDiagnostics`.

| IR | LSP |
|----|-----|
| primary span | `range` (existing UTF-16 conversion) |
| message | `message` (primary text; may include expected/actual if helpful) |
| code | `code` (string, e.g. `"THX001"`) |
| severity | `severity` (Error; Warning when used) |
| labels | `relatedInformation[]` (`location` + `message`) |
| help | Prefer appending to `message` as a second sentence/line **or** a short
  trailing `help: …` in the message when the client does not surface a
  separate help field (LSP has no standard `help`; do not invent nonstandard
  fields for Zed) |
| source | `"thoxa"` (unchanged) |

**Zed-focused acceptance**

1. Squiggle on the primary range.  
2. Hover/problems panel shows **code + message**.  
3. Related locations navigable when labels exist.  
4. Multi-diag recovery behavior from the editor plan **preserved**.

No toast spam; identity remains `thoxa version` / logMessage (already shipped).

---

## 6. Implementation slices (suggested PR order)

Implement **one slice at a time**; gate: `cargo test -p thoxa` +
`cargo make test`. Update capabilities + handbook when user-visible behavior
ships.

| PR | Slice | Done when |
|----|--------|-----------|
| **A** | Diagnostic IR type + flatten `Error` → `Vec<Diagnostic>`; check/LSP still message-equivalent | Unit tests; no user-visible regression |
| **B** | Assign codes + help for a **first batch** of high-traffic errors (undefined name, type mismatch, import fail, top-level with main, missing Fallible import if distinct) | Codes appear in CLI/LSP for those paths |
| **C** | Pretty `thoxa check` default output | Manual + unit on formatter |
| **D** | `thoxa check --json` + schema tests + version stamp in payload | Unit/golden JSON |
| **E** | LSP `code` + `relatedInformation` (+ help-in-message) | Unit on convert; manual Zed |

PR order may merge A+B or C+D if small; do not ship JSON/LSP fields before the
IR exists.

**Catalog growth:** after the first batch, migrate remaining free-form sites
opportunistically when touching those emitters — not a separate megasprint.

---

## 7. Quality bar (definition of done for this freeze)

- [x] One IR drives **check pretty**, **check --json**, and **LSP**.  
- [x] At least one golden path per: undefined name (with help), type mismatch
      (expected/actual when applicable), multi-diag still reports ≥2 issues.  
- [x] JSON includes compiler identity.  
- [x] LSP diagnostics include `code` when IR has a code.  
- [x] Handbook (`cli.md`, `editor.md`) updated for shipped behavior.  
- [x] Capability row `diag-structured` shipped.  
- [x] No graph, explain CLI, or auto-fix required.

---

## 8. Inspiration and non-copy

Informed by reading [vercel-labs/zerolang](https://github.com/vercel-labs/zerolang)
diagnostics (stable codes, dual prose/JSON, help, related facts) and by
rustc-style human presentation.

**We adopt:** structured failure, dual human/machine presentation, rich LSP.  
**We do not adopt:** graph-as-program-database, patch/hash edit loop, or
agent-primary product positioning in this freeze.

---

## 9. Locks (summary → D124)

1. Diagnostics are a **shared IR**, not ad-hoc strings per consumer.  
2. **`thoxa check`** defaults to **pretty human** output; **`--json`** is the
   machine contract.  
3. **LSP** publishes **code** and **relatedInformation** from that IR.  
4. **Editor/human clarity** is the ship criterion; agent-only surfaces wait.  
5. Text remains the program source of truth (no diagnostic dual-store).
