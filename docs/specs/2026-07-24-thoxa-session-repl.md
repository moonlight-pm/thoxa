# Thoxa session model — shell / REPL / no-main scripts

**Date:** 2026-07-24  
**Status:** target (freeze)  
**Implementation:** partial  
**Gaps:** no `2>` / redir-on-pipe-stages; Shell still auto-injected (not import-first); optional libtcc base cache / license packaging  
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md) · handbook [`../handbook/session.md`](../handbook/session.md)  
**Step backend:** [`2026-08-05-session-libtcc.md`](2026-08-05-session-libtcc.md) (D105, core shipped)

> **For agentic workers:** Implement **one PR at a time**. Gate: `cargo test -p thoxa` + `cargo make test`. Update `CURRENT.md` + `docs/capabilities.md` + `docs/decisions.md` (+ handbook when user-visible) when a PR finishes. Do **not** invent a second shell language.

**Shipped core:** PR0–PR8 + D93 v2; `prompt()` / `path()`; pipelines (D101); redirs (D102); pure-step (D103); `std/Shell.thx` (D104); libtcc steps (D105). Decisions D88–D105.  
**Open polish:** residual redir/import polish; libtcc packaging/cache.  
**Baseline:** Native ladder + session scripts + command sugar + bare `thoxa` REPL.

**Goal:** Make **Thoxa the session language**. Interactive and no-`main` scripts share one model: a live **`Shell`** value, user text desugared into real Thoxa, compiled, and executed as a method body on that value. Familiar command skin stays thin; programming is always Thoxa.

---

## 0. Product thesis

| Invocation | Mode | Behavior |
|------------|------|----------|
| `thoxa` (no args) | **REPL** | Long-lived host; read chunk → desugar → compile step → run against live `Shell` |
| `thoxa path.thx` (no `fn main`) | **Session script** | One-shot: wrap file as session unit → compile → run once → exit with status |
| `thoxa path.thx` (has `main`) or `thoxa dir/` → `Main.thx` | **Program** | Today’s batch compile-and-run (unchanged semantics) |
| `thoxa run\|build\|check\|lsp\|…` | **Tools** | Explicit subcommands stay; CI/editor unchanged |

**One sentence:**  
`thoxa` alone is a Thoxa session whose default value is `Shell`; command-shaped lines desugar to ordinary Thoxa on that value and compile; packages with `main` remain ordinary programs.

**Non-goals (this plan):**

| Item | Why |
|------|-----|
| Bash/zsh script compatibility | Explicitly out; familiarity ≠ dialect clone |
| Shell-language `if` / `for` / functions | Real Thoxa only |
| Full `$` expansion forest | Prefer Thoxa names (`status`, later env APIs) |
| Tree-walk interpreter as product | Phase A stays compile-to-C |
| Infinite quality line editor | Crude `read` OK for a long time |
| Absorbing all of `std/` | Only `Shell` + process helpers forced by the session |

---

## 1. Chosen approach (decision summary)

### 1.1 What we build

**Approach 3 — Thoxa is the REPL**, with these concrete mechanics:

1. **Session value:** `type Shell` (fields + methods). Builtins and process ops are methods (or prelude fns that take/return `Shell`).
2. **Desugar:** Command skin (`echo hi`, `uname -a`) → real Thoxa calling those methods.
3. **Wrap:** A user **chunk** (one REPL unit or whole script) is not eval’d by a second runtime. It is spliced into a **synthetic session module** and compiled with the normal pipeline.
4. **Call:** Host obtains an entry (`step` / `run` on `Shell`, plus a thin `main` for one-shot scripts) and **invokes** it against session state.

### 1.2 Wrap model (refined — not fully naive)

User intuition (“body of a function on `Shell`”) is right for **executable statements**. Full files also need **declarations**. Chosen split:

```text
user chunk
  ├── declarations  →  session module scope
  │     import / type / fn / extern / module-level let (as allowed)
  └── statements    →  body of Shell.step(self)   # name TBD; internal
```

```thoxa
## Conceptual splice (host-generated; users do not write this wrapper)

## … prelude / std Shell definition …

## … lifted user imports, types, fns …

type Shell(
    status: Int64,
    ## … session fields …
) {
    ## … fixed API: echo, cd, run, … …

    ## Host replaces this body each chunk (REPL) or once (script).
    fn step() -> Shell {
        ## desugared user statements only
        ## implicit receiver scope: fields + methods as today for methods
    }
}

## One-shot script / test harness only:
fn main() -> Int64 {
    mut shell = Shell(status: 0 ## … defaults …)
    shell = shell.step()
    shell.status   ## process exit code
}
```

**Why not “any function may import/type” as the first language change?**  
That is a broad grammar/scope redesign. Session units get the power via **host splicing** (lift decls to module scope). General in-function `import`/`type` stays **deferred** unless a later language PR wants it for everyone.

**Why method body at all?**  
So `status`, `echo`, `cd`, `run` share ordinary method scope — no magic globals, no second name resolver.

### 1.3 Script vs REPL

| | Script (no `main`) | REPL |
|--|--------------------|------|
| Chunk | Whole file | Line or continued block |
| Declarations | Lifted once | Accumulated into session module across chunks |
| `step` body | All executable statements | **This chunk’s** statements only |
| `Shell` instance | Created once in synthetic `main` | **Host-held** across chunks |
| Compile | Once | Once per chunk (cache later) |

**REPL binding persistence** (required for “real programming” at the prompt):

| User writes | Host does |
|-------------|-----------|
| `let x = 1` at chunk top (session bind) | Promote to session module binding or `Shell` field for later chunks |
| `let` inside `if` / nested block | Stays local to that `step` body (normal Thoxa) |
| Commands / expressions | Run in `step`; may assign `shell = shell.echo(…)` |

Phase 1 REPL may ship **commands + expressions + `Shell` fields** before full `let` promotion; scripts get full `let` in one `step` naturally. **Do not** claim full multi-line lexical REPL until promotion lands.

### 1.4 Host architecture (how compile-to-C stays honest)

There is no tree-walk. Persistence forces a **long-lived host** for REPL.

**Chosen target mechanics:**

```text
┌─────────────────────────────────────────┐
│  thoxa (Rust host)                      │
│  - own Shell instance (C layout / RT)   │
│  - read chunk, desugar, splice module   │
│  - cgen → cc -shared → dlopen           │
│  - call step(shell) → updated shell     │
└─────────────────────────────────────────┘
```

| Strategy | Verdict |
|----------|---------|
| Re-exec full history each line | Reject — redoes `cd`/side effects |
| Serialize `Shell` via files between processes | Emergency only — brittle |
| **dlopen (or equivalent) step .so + stable `Shell` ABI** | **Chosen** for REPL |
| One-shot script: normal link + `main` | **Chosen** for scripts (no dlopen required) |

**Script path (simpler, first):** splice → full executable with synthetic `main` → `cc` → run process → exit. No dlopen.

**REPL path (later):** stable C layout for `Shell` (or opaque `ThoxaShell*` in RT), each chunk builds a small shared object exporting `thoxa_session_step(ThoxaShell*)`, host dlopens and calls. ABI and packing rules get a decision row when implemented (align with existing User/C-layout story).

**Early prototype exception:** `extras/shell` remains a **pure Thoxa** dogfood binary (`thoxa run extras/shell`) that implements the *API shape* of `Shell` without the host. It is a ladder, not the end state.

### 1.5 Command sugar

Only a **thin** skin. No control-flow sugar.

| Prompt / script line | Desugars toward (illustrative) |
|----------------------|--------------------------------|
| `echo a b` | `shell = echo(["a", "b"])` or method form on receiver |
| `cd /tmp` | `shell = cd("/tmp")?` or match on `Trial` |
| `pwd` | print cwd helper / method |
| `uname -a` | `shell = run(["uname", "-a"])` |
| `let x = 1 + 2` | as-is Thoxa (no sugar) |
| `shell.status` | as-is |
| `ls \| wc` (later) | `shell = pipeline([["ls"], ["wc"]])` — still one expression |

**Detection rule (chosen):**

1. Parse chunk as Thoxa statements/decls (session grammar).
2. If that fails **and** the chunk looks like a **command line** (word-split with thoxash v3 quotes), desugar to `run` / builtin method calls and parse again.
3. If both fail → one diagnostic (span if available).

Prefer **parse-Thoxa-first** so `let`, `match`, `fn`, types are never stolen by the command lexer.

Builtins that are session ops (`echo`, `cd`, `pwd`, `exit`) map to `Shell` methods, not external PATH lookup. Unknown first word → `run([…])` PATH search (today’s thoxash v2 behavior).

### 1.6 `Shell` surface (initial)

Minimal API the plan forces (names adjustable at implement time; lock in a D-id when shipped):

```thoxa
type Shell(
    status: Int64,
    ## optional later: last stdout Bytes, env handles, …
) {
    fn echo(args: Array<String>) -> Shell
    fn pwd() -> Shell                 ## print cwd; status 0/1
    fn cd(path: String) -> Shell      ## or Trial; update status on fail
    fn run(argv: Array<String>) -> Shell
    fn step() -> Shell                ## host-filled body; not user-facing forever
}
```

- **`status`:** last command/builtin outcome (0 = success). Process exit of one-shot script = final `status` (or `main` return).
- **No `$?` required** in the language model. Optional later sugar `$?` → `status` if muscle memory wants it; not a design dependency.
- **Pipes/redirs:** later methods (`pipeline`, `redirect`) + sugar; OS pipes via RT, not a shell dialect.

---

## 2. Current baseline (do not re-implement)

| Area | Status | Notes |
|------|--------|-------|
| Pipeline MIR → C → binary | ✅ | `driver`, `cgen`, `libthoxa_rt` |
| `fn main` programs + `Main.thx` dirs | ✅ | D68 |
| Library check without `main` | ✅ | D83 `require_main` |
| Modules `./` `~/`, import/export | ✅ | D30–D35 |
| `thoxa_spawn_wait` | ✅ | D86; argv as NUL-separated `Bytes` |
| thoxash word split + quotes | ✅ | `extras/shell` v3+ |
| Value `match` string/int/bool | ✅ | D87 |
| Dogfood `type Shell(status)` + methods | ✅ | PR0 — `extras/shell` v4 |
| Bare `thoxa` → REPL | ✅ | PR4+; product face |
| No-main run-as-script | ✅ | PR1; D90/D94 |
| dlopen session steps | ✅ | D93 v2 |
| Prelude / std `Shell` type | ✅ | D104; `std/Shell.thx` + embed fallback |
| Command → AST desugar in compiler | ✅ | PR2; D92 |
| Thoxa-side OS pipelines | ✅ | D101; `session_pipe` |
| File redirs `>` / `>>` / `<` | ✅ | D102; `session_redir`; no `2>` yet |
| Pure-step REPL (no host cd/status/\|) | ✅ | D103; `repl::pure_step_*` |

---

## 3. Phased delivery

### Phase 0 — Dogfood the API in pure Thoxa (no host) ✅

**Goal:** `extras/shell` becomes a readable model of `Shell`, not an ad-hoc REPL forever.

- ✅ `type Shell(status: Int64)` with `echo` / `pwd` / `cd` / `run` / `showStatus` / `help` → new `Shell`
- ✅ REPL holds `mut shell`; process exit = `shell.status`
- ✅ Builtin `status` prints field (not bash `$?`)
- ✅ Quotes kept; README desugar table
- Note: ctor params cannot nest User (`EvalResult(shell: Shell, …)` rejected) → `sessionDone` module mut for exit flag

**Exit:** `cargo make shell` works; smoke shows `status` after fail/success; still `thoxa run extras/shell`.

### Phase 1 — Session **scripts** (compiler + driver)

**Goal:** `thoxa run file.thx` when there is **no** `main` runs as a session script.

1. Detect entry mode: has `main` / dir `Main.thx` → program; else → session script.
2. Splice: lift decls; wrap statements in `Shell.step`; emit synthetic `main`.
3. Optional: enable command-sugar desugar for script lines (can land in Phase 1b).
4. Conformance: `tests/conformance/session_*.thx` (+ `.out`) — pure Thoxa statements first; sugar tests second.
5. Decisions: D-ids for session mode, wrap rules, exit status = `shell.status`.

**Exit:** no-main file runs; programs with `main` unchanged; `cargo make test` green.

### Phase 2 — Bare `thoxa` REPL host

**Goal:** `thoxa` with no args enters the session REPL.

1. Rust host loop: prompt (TTY), read chunk (line-first; block continuation later).
2. Per chunk: desugar → splice (accumulate decls) → emit C → **shared library** → dlopen → `step` → keep `Shell`.
3. `exit` / EOF ends process with final status.
4. Wire `cargo make shell` → `thoxa` (or thin wrapper); deprecate parallel personality of extras as the product face.
5. Safety: clear error paths for compile failures (print diag, keep previous `Shell`, set status ≠ 0).

**Exit:** interactive smoke without `thoxa run extras/shell`; failed compile does not kill session state.

### Phase 3 — REPL programming quality

**Goal:** Session feels like programming Thoxa, not only running commands.

- Top-level `let` promotion across chunks.
- Multiline chunk continuation (unclosed `{`, quotes, etc.).
- Optional `$?` → `status` sugar (only if still desired).
- Simple `|` sugar → `pipeline` (RT pipe/dup2 as needed).
- Faster rebuilds (cache preamble object; only recompile `step` body).
- **Step C backend target:** in-process libtcc (D105) — sibling freeze
  [`2026-08-05-session-libtcc.md`](2026-08-05-session-libtcc.md).

**Exit:** multi-chunk `let` + use works; basic pipeline dogfood; still no bash language.

### Phase 4 — Prelude absorption (when earned)

- Move stable `Shell` + process helpers from extras into product prelude / `std` story (respect D30–D32 timing).
- extras/shell shrinks to demos or disappears.
- Document user-facing session in a short handbook section (not full SPEC freeze until stable).

---

## 4. PR plan (ordered)

Each PR is independently reviewable; conformance or explicit smoke is the gate.

| PR | Title | Depends | Deliverable |
|----|-------|---------|-------------|
| **PR0** ✅ | Shell type + status in `extras/shell` | — | dogfood Shell |
| **PR1** ✅ | Session script splice | PR0 | `session_script`, `session_step` |
| **PR2** ✅ | Command sugar + prelude | PR1 | `session_cmd`; `session_prelude.thx` |
| **PR3** ✅ | RT session helpers | PR1 | `ThoxaShellState`, `thoxa_pipe` |
| **PR4** ✅ | Bare `thoxa` REPL | PR2–3 | `repl.rs`; compile-per-chunk (dlopen deferred) |
| **PR5** ✅ | Decl accumulate + multiline | PR4 | `let`/`type`/… persist; brace continuation |
| **PR6** ✅ | Host `\|` pipelines | PR4 | OS pipe in REPL host |
| **PR7** ✅ | Docs + D88–D95 | — | decisions, status, AGENTS |
| **PR8** ✅ | Session rc + `prompt()` (D96) | PR5 | `~/.config/thoxa/shell.thx`; cached prompt binary |

**Parallelism:** PR0 can proceed immediately (dogfood). PR1 is the first compiler PR. PR3 can overlap PR2 after PR1 lands.

**Do not start PR4 until PR1 is solid** — REPL multiplies wrap bugs.

---

## 5. Key decisions (lock when implementing; draft IDs)

| Draft | Decision | Choice | Rationale |
|-------|----------|--------|-----------|
| D88′ | Session product shape | Bare `thoxa` = REPL; no-`main` = session script; `main`/`Main.thx` = program | One binary face; batch path preserved |
| D89′ | Session value | `type Shell` with `status` + methods | Real Thoxa scope; no magic `$?` core |
| D90′ | User code placement | Decls → module; statements → `Shell.step` body | Matches “function on Shell” without general nested modules |
| D91′ | In-function import/type | **Not** required for session; host lifts decls | Smaller language blast radius |
| D92′ | Command sugar | Thoxa-first parse; fallback command lex (v3 quotes) → `run`/builtins | Familiar prompt, language wins conflicts |
| D93′ | REPL execution | Host-held `Shell` + per-chunk compile + **dlopen step** | Honest compile-to-C; state survives |
| D94′ | Script execution | Splice + synthetic `main` + normal executable | No dlopen; proves wrap first |
| D95′ | extras/shell | Prototype / dogfood until Phase 4 | Drives API without blocking compiler |

Promote to real D-ids in `docs/decisions.md` when each ships (renumber as needed).

---

## 6. Desugar examples (normative illustrations)

### 6.1 Script (no main)

User file:

```thoxa
import { helper } from "./Util"

echo "hello world"
let code = run(["false"]).status
```

Conceptual after sugar + splice (shape, not exact surface):

```thoxa
import { helper } from "./Util"
## Shell type from prelude …
type Shell(status: Int64) {
    fn echo(args: Array<String>) -> Shell { … }
    fn run(argv: Array<String>) -> Shell { … }
    fn step() -> Shell {
        mut shell = /* receiver — exact self story per language */
        shell = shell.echo(["hello world"])
        let code = shell.run(["false"]).status
        shell
    }
}
fn main() -> Int64 {
    Shell(status: 0).step().status
}
```

*(Exact `self` / `mut` threading follows existing method conventions at implement time; functional `Shell → Shell` is preferred if it fits RC/value model cleanly.)*

### 6.2 REPL chunks

```text
$ echo hi
## step body: shell = shell.echo(["hi"])

$ let x = 1
## promote x into session module

$ println("{x}")
## step body: println("{x}")
```

### 6.3 Program path unchanged

```thoxa
fn main() -> Int64 {
    println("batch")
    0
}
```

No `Shell` wrap. No command sugar unless we later opt in (default **off** for programs).

---

## 7. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| dlopen / ABI fragility | Script mode first; freeze `Shell` C layout in PR3 with tests |
| Command sugar steals Thoxa syntax | Thoxa-first parse |
| REPL `let` confusion | Document phase limits; PR5 promotion |
| Slow recompile each line | Accept early; PR5+ cache preamble |
| extras/shell diverges from host | README desugar table is shared contract; PR0 aligns API |
| Scope creep into bash | Non-goals table; reject shell control-flow PRs |

---

## 8. Verification

| Mode | Gate |
|------|------|
| Every compiler PR | `cargo test -p thoxa` + `cargo make test` |
| PR0 dogfood | `printf '…' \| cargo make shell` smoke (document cases) |
| Session scripts | Conformance `.thx` + `.out` |
| REPL | Scripted smoke (TTY off): multi-line file piped to `thoxa`; optional `expect`-style later |
| ASan | Spawn/pipeline RT changes as today for HTTP/shell RT |

---

## 9. Agent handoff notes

- **Core shipped** (PR0–PR8 + D93 v2 + D101–D104). Living next work: root
  `CURRENT.md` + **libtcc freeze**
  [`2026-08-05-session-libtcc.md`](2026-08-05-session-libtcc.md) (D105), then
  std growth.
- Do not add shell `if`/`for`.
- Prefer small diffs; update CURRENT + capabilities + decisions after each PR.
- Oracle Python shell (if any) is reference-only — native design above wins.

---

## 10. Open questions (resolved by this doc unless revisited)

| Question | Resolution |
|----------|------------|
| Approach 1/2/3? | **3** — Thoxa is the REPL |
| `$?` vs `status`? | **`status` field**; `$?` optional later sugar only |
| Imports inside functions? | **Host lifts decls**; no general language change in this plan |
| Pipes before REPL? | **After** REPL host (PR6); OS pipeline as `Shell` API |
| Where does `Shell` live long-term? | Prelude/std in Phase 4; extras until then |

---

## 11. Relationship to thoxash v3 “next”

Earlier status line “next: `$?` / pipes / redirections” is **subsumed**:

| Old informal next | Where it goes now |
|-------------------|-------------------|
| `$?` | PR0 `status` on `Shell` |
| quotes | Done (v3) |
| pipes / redirs | PR6 after host exists |
| “more shell dialect” | **Rejected** — desugar to Thoxa instead |
