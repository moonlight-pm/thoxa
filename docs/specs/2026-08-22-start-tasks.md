# Freeze: `--> name(args)` nowait (D148)

**Date:** 2026-08-22
**Status:** target (freeze)
**Implementation:** shipped
**Decision:** D148 (panic in a task: **D166**)
**Gates:** `start_tasks`, `start_nested`, `start_net`, `start_keepalive`,
`http_host`, `task_panic`
**Gaps:**
- No `Future` / `async` / `run` / `Task` type
- `--> { … }` is a parse error (later)
- `-->` of a function pointer or bound method is out (D132)
- No join / await / read of the callee’s return
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D148 in
[`decisions.md`](../decisions.md)

---

## Intent

Begin a call without waiting for it. The process is **colorless**: there is no
`async` keyword and no future value. Parking happens inside the runtime (Net
syscalls). `-->` is a **token**, not a word.

```thx
--> tick("a")
--> serveOne(peer, handle)
```

`std/http/server.serve` starts one `serveOne` per accept so an idle keep-alive
peer can park in `recv` without stalling the accept loop.

---

## Locks (v0)

### 1. Syntax

- Token `-->` (three characters). Distinct from `->` (types) and `=>` (match).
- Form: **`--> name(args)`** — one ident in scope, then a call.
- `--> { … }` is **not** implemented (clear parse error).
- Not a word: no `go`, `spawn`, `async`, `start`.

### 2. Callee

A **named free Thoxa function** (same lookup as a call of that name, including
defaults and multi-clause dispatch).

**Not:**

| Form | Why |
|------|-----|
| `--> f(x)` when `f` is a function value | D132 — `-->` is not of a pointer |
| `--> instance.method(…)` / `--> Type.method(…)` | Callee is one ident, not a member |
| `--> ns.fn(…)` | Import the name (`import { fn }`) and start that ident |
| `-->` of `extern` | Thoxa function only |
| `-->` of a type name | Not a function |

### 3. Type and result

The `-->` expression is **`Void`**. The callee’s return is **dropped** (RC drop
if heap). There is no handle to join or read.

Use it as a statement. A function whose last expression must be `Int64` still
needs a value after the starts (`0` in the gates).

### 4. Scheduler

One OS thread. Stackful fibers (`ucontext`). `-->` packs arguments, dups heap
args into the pack, and enqueues a thunk.

- If the callee **never parks**, the thunk runs to completion before `-->`
  returns (ready queue is pumped from `thoxa_task_start`).
- If the callee **parks** (Net accept / connect / recv / send on EAGAIN, or
  `thoxa_wait_ms` / `thoxa_wait_fd` in the runtime), the starter continues.
- Generated `main` calls **`thoxa_sched_drain`**
  after the Thoxa entry so the process waits until idle.

RC stays **non-atomic**. Not OS threads. Not a language `spawn`.

### 5. Parking (colorless)

User code does not write `await`. `std/net` socket ops go through `thoxa_*`
and park the current fiber. There is no product `std/` sleep yet;
`thoxa_wait_ms` is runtime-only.

### 6. LSP / color

`-->` is a keyword token (Tree-sitter + LSP semantic tokens). Not a function
name.

---

## Non-goals (punt)

| Item | Why |
|------|-----|
| `Future` / `async fn` / `run` | Colorless v0; a type would be a new surface |
| `--> { … }` blocks | Named call is enough for HTTP + the gates |
| Join / select / cancel | No handle |
| `-->` of a pointer or method | D132; named free fn only |
| Language-level sleep / `wait` | Grow from a sample |
| Atomic RC / multi-thread | Phase A single-threaded |
| Effects as the wait story | Deferred megawork |

---

## Dogfood

`std/http/server.serve` → `--> serveOne(peer, handle)`. Conformance is the
`start_*` gates plus `http_host`, not the cafe.
