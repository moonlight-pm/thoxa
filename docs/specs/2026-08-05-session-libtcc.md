# Session step backend — in-process libtcc

**Date:** 2026-08-05  
**Status:** shipped  
**Implementation:** PR0–PR3 landed; **libtcc only** for session steps (no
`cc`+dlopen escape)  
**Gaps:** none for this freeze’s product path. Later polish (not CURRENT):
base-object cache, license packaging — §11  

**As-built:** [`../capabilities.md`](../capabilities.md) ·
[`../architecture.md`](../architecture.md) · parent session freeze
[`2026-07-24-thoxa-session-repl.md`](2026-07-24-thoxa-session-repl.md)

> **For agentic workers:** Implement **one PR at a time**. Gate:
> `cargo test -p thoxa` + `cargo make test` (session unit gates below). Update
> `CURRENT.md` + `docs/capabilities.md` + `docs/decisions.md` when a PR finishes.
> Do **not** change batch program semantics or invent a second language runtime.

**Parent:** session product model D88–D104.  
**Locks this freeze:** D105 (draft → promote when first code lands or when this
freeze is accepted as product intent — already accepted in CURRENT).

---

## 0. Product thesis

Interactive session steps must stay **honest compile-to-C** (phase A: no
tree-walk product interpreter, no QBE path). Latency of “type a line → see
result” is dominated today by **spawning system `cc`** and **writing/loading
step `.so` files**, not by Thoxa frontend cost alone.

**Chosen backend for REPL / session-step execution:**

> **cgen → libtcc in-process (`TCC_OUTPUT_MEMORY`) → relocate → call
> `thoxa_session_step` — no compiler process spawn, no step `.so` on the happy
> path.**

Batch programs, `libthoxa_rt` builds, and session **scripts** that produce a
normal executable keep system `cc` + normal link. This freeze is **session-host
step compile only** (REPL chunks, and the same step path used for `prompt()` /
`path()` hooks).

**One sentence:**  
Vendored **libtcc** is the product C backend for interactive session steps;
system **`cc`** remains the product C backend for everything else.

---

## 1. Product axioms (scope constraints)

These are **project locks for this freeze** (not temporary experiment knobs):

| # | Axiom | Consequence |
|---|--------|-------------|
| A1 | **No Windows.** Windows is out of scope forever for this product track. | Vendor and build TinyCC for Linux host targets only (x86_64 first; aarch64 only if TinyCC is proven later). |
| A2 | **Licensing is deferred.** Do not block implementation on LGPL distribution policy. | Static link of libtcc into `thoxa` is allowed for now; third-party notice cleanup is a later packaging slice. |
| A3 | **In-process is the goal.** Prefer libtcc memory output over CLI `tcc` or system `cc`. | Happy path: zero compiler spawns, zero step `.so` files. |

Optional system-`tcc` CLI on PATH is **not** the product design.

---

## 2. As-built (this freeze, after PR3)

| Area | Status | Notes |
|------|--------|-------|
| Session scripts | shipped | Splice + synthetic `main` + system `cc` (D94) |
| REPL pure step | shipped | All commands via step path (D103) |
| Step C backend | shipped | libtcc memory only (D105); no `cc`+dlopen |
| `std/Shell.thx` | shipped | D104 |
| Pipes / redirs | shipped | D101 / D102 |

Does not reopen desugar, splice, Shell API, or status threading.

---

## 3. Chosen approach

### 3.1 What we build

| Component | Role |
|-----------|------|
| Vendored TinyCC sources | Pinned under `vendor/tinycc` |
| Build of `libtcc` (+ libtcc1) | Host Linux; produced into `target/` or linked by `crates/compiler` |
| Session step compiler | Rust host API: C strings (and optional files) → relocated entry |
| REPL / prompt / path | Call that API instead of `cc`+`dlopen` |

### 3.2 Target lifecycle (happy path)

```text
chunk
  → desugar / prepare / MIR / cgen          (unchanged)
  → combined C string (base + step; shipped)
  → tcc_new()
  → tcc_set_output_type(TCC_OUTPUT_MEMORY)
  → include path: runtime/include
  → resolve thoxa_rt (archive and/or host symbols)
  → tcc_compile_string(base_c) / tcc_compile_string(step_c)
  → tcc_relocate(TCC_RELOCATE_AUTO)
  → fn = tcc_get_symbol("thoxa_session_step")
  → status = fn(&ThoxaShellState{ status: prev })
  → keep TCCState alive until superseded; then tcc_delete
```

**Non-goals for happy path:**

- Spawning `cc` or `tcc` CLI for each chunk  
- Emitting `session_step_*.so` for each chunk  
- `dlopen` of step libraries  

**Allowed during debug:**

- Write `.c` under `target/thoxa/repl/` when `THOXA_REPL_KEEP_C=1`

### 3.3 Batch path (explicitly unchanged)

| Mode | Backend |
|------|---------|
| `thoxa path.thx` with `main` / `Main.thx` | system `cc` + executable |
| Session **script** one-shot (no `main`) | system `cc` + executable (D94) |
| `libthoxa_rt.a` | system `cc` / `ar` as today |
| Conformance gate | system `cc` (definition of done for language) |

Scripts may later optionally share libtcc for curiosity; **not** required by
this freeze. Default script path stays `cc`.

### 3.4 Symbol resolution for `thoxa_rt`

Generated step C calls into `thoxa_rt`. Preferred order of attempts in
implementation:

1. `tcc_add_file` / library path for `libthoxa_rt.a` (PIC as today).  
2. Link `libthoxa_rt` into the `thoxa` host and resolve against the process.  
3. Selective `tcc_add_symbol` for stragglers.

Also provision **libtcc1** (TinyCC helper objects) per upstream requirements.

### 3.5 Preamble / decls cache

**Must keep** the existing idea: stable decls should not redo full frontend work
unnecessarily.

| Layer | Target |
|-------|--------|
| Decls key | Unchanged (`SessionBuildCache`) |
| Base C | Re-cgen only when decls change |
| Base feed to libtcc | Recompile combined base+step each chunk (shipped) |
| Step C | Always re-cgen for the chunk |

Combined recompile is **shipped and sufficient**. Caching a separate base
object is **later polish** only (§11) — not an open implementation task.

### 3.6 Multiple live images

Today: separate `.so` for step vs prompt. Target: **separate `TCCState`s** (or
clear rebuild rules) so:

- Primary step state for user chunks  
- Prompt step state for `fn prompt()`  
- Path step state for `fn path(...)`  

Never `tcc_delete` a state while a function pointer from it might still run.
Single-threaded REPL assumed (as today).

### 3.7 Platform

| Support | Policy |
|---------|--------|
| Linux x86_64 | **Required** for the freeze |
| Linux aarch64 | Optional later; not blocking |
| Windows / macOS | **Out of scope** (A1); no CI or design work |

---

## 4. Non-goals

| Item | Why |
|------|-----|
| Replacing system `cc` for programs / runtime | Correctness and portability of the batch path |
| Tree-walk interpreter | Phase A lock |
| QBE / LLVM product path | Phase A lock |
| CLI-only `tcc` as the product design | Spawns remain; weaker than libtcc memory |
| Full JIT of MIR without C | Out of scope; cgen stays the IR→C surface |
| Windows TinyCC builds | A1 |
| Packaging / license polish | A2 — later |

---

## 5. Phased delivery

Implement **one PR at a time**. Each phase leaves `cargo make test` green.

### PR0 — Vendor + smoke libtcc

**Goal:** TinyCC builds and links into the host; prove memory compile.

- Pin sources under `vendor/tinycc`.  
- Build `libtcc` (+ libtcc1) for host Linux (xtask / `build.rs` / `cargo make`).  
- Link into `thoxa` (or a small internal crate).  
- Unit smoke: compile `int f(void){return 42;}` in memory, call, assert.

**Exit:** no session path change yet; bisectable vendor build.

### PR1 — Session step via libtcc memory ✅

**Goal:** `build_session_step` happy path uses libtcc; no `.so` / no `cc` spawn.

### PR2 — Prompt + path hooks ✅

Same backend for D96 / D100 step images.

### PR3 — Cleanup (libtcc only) ✅

- Removed legacy `cc -shared` + `dlopen` and `THOXA_REPL_BACKEND` escape.  
- Session steps are libtcc-only on the product track (Linux x86_64).

---

## 6. Key decisions

| ID | Decision | Choice | Rationale |
|----|----------|--------|-----------|
| **D105** | REPL step C backend | **In-process libtcc only** (`TCC_OUTPUT_MEMORY`); no step `.so` | Latency; A1/A2/A3 |
| D105.a | Batch / RT C backend | System **`cc`** unchanged | Correctness gate; conformance |
| D105.b | TinyCC distribution | **Vendored source**, built with product | No user install; reproducible |
| D105.c | Host platforms | **Linux only** (x86_64 first) | A1 |
| D105.d | License packaging | **Deferred** | A2 |
| D105.e | Escape hatch | **Removed** — was temporary `THOXA_REPL_BACKEND=cc` | Product choice: libtcc only |

Promote D105 into [`../decisions.md`](../decisions.md) as a locked row when
this freeze is accepted (same change as docs that point at it).

---

## 7. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| `libthoxa_rt.a` + relocate fails | PIC objects; host-linked RT / `tcc_add_symbol` if needed |
| libtcc rejects generated C | Keep cgen boring C11; fix cgen, don’t fork dialect |
| Lifetime bugs (`tcc_delete` early) | Own state in host; delete only after supersede; no async |
| Prompt vs step state collision | Separate `TCCState` per image role |
| Debug stacks useless | `THOXA_REPL_KEEP_C=1` writes `.c` for inspection |
| Vendor build flaky in CI | Pin commit; document host deps |

---

## 8. Verification

| Gate | What |
|------|------|
| Every PR | `cargo test -p thoxa` + `cargo make test` |
| PR0 | libtcc memory smoke unit (`tcc::memory_compile_returns_42`) |
| PR1+ | `repl::pure_step_*`; manual `printf 'echo hi\nstatus\nexit\n' \| thoxa` |
| PR2 | Existing prompt / path unit tests (same `build_session_step`) |
| Perf (soft) | See §8.1 below |
| Negative | No step `.so` under happy path (`target/thoxa/repl/*.so` absent after piped REPL) |

### 8.1 Soft perf baseline (2026-08-05, documented once)

Informal wall-clock on a Linux x86_64 host, **debug** `thoxa`, piped non-interactive
REPL (N lines + `exit`), warm once then measure. Historical compare of libtcc
vs the removed `cc`+dlopen path (escape hatch deleted after PR3).

| Workload | Backend | ≈ total | ≈ per step |
|----------|---------|--------:|-----------:|
| 20× `echo lineN` | libtcc | 280 ms | **14 ms** |
| 20× `echo lineN` | cc+dlopen (removed) | 2.2 s | **109 ms** |
| 50× `echo lineN` | libtcc | 590 ms | **12 ms** |
| 50× `echo lineN` | cc+dlopen (removed) | 5.0 s | **101 ms** |

**Roughly 7–9× faster** than former `cc`+dlopen. Not a CI gate.

---

## 9. Relationship to other docs

| Doc | Role |
|-----|------|
| [`2026-07-24-thoxa-session-repl.md`](2026-07-24-thoxa-session-repl.md) | Parent session product freeze; this is the **step backend** sub-freeze |
| [`../architecture.md`](../architecture.md) | As-built map — update when code lands, not when this freezes alone |
| [`../capabilities.md`](../capabilities.md) | `session-libtcc` maturity |
| Root [`CURRENT.md`](../../CURRENT.md) | Now line until shipped |
| Handbook | **No** aspirational docs; update only if user-visible behavior changes at ship |

---

## 10. Agent handoff

- **Shipped:** PR0–PR3 — libtcc-only session steps; no `THOXA_REPL_BACKEND`.  
- **Do not** put base-object cache or license packaging on the CURRENT proceed
  queue — see §11.  
- Do not re-litigate A1–A3 or reopen QBE / interpreter.  
- Do not invent shell dialect.

---

## 11. Later polish (not CURRENT)

These are **documented so they are not forgotten**, and so agents do **not**
treat them as “next slice.” Promote only if CURRENT explicitly says so (e.g.
after profiling or packaging work).

### 11.1 Base-object cache (performance)

**What:** When decls are stable, compile the Shell/prelude “base” once and only
recompile each chunk’s `Shell.step` body, linking against the cached base
(memory and/or a libtcc-addable object). The former `cc` path had
`session_base_*.o`; libtcc currently rebuilds **combined** C every chunk.

**Why later:** Combined libtcc is already ~7–9× faster than old `cc`+dlopen on
tight loops (§8.1). Frontend + cgen still dominate many lines. Extra complexity
(invalidation on decl change, object lifetime, two-part compile) is not justified
by default.

**When to consider:** Profiling shows large stable decls + many steps still
bottlenecked on recompiling unchanged base C.

### 11.2 TinyCC license packaging notice

Third-party / LGPL notice cleanup for vendored TinyCC (D105.d / axiom A2).
Packaging work only — does not change session behavior.
