# Post-R9 HTTP completion — remaining work plan

**Date:** 2026-07-19  
**Status:** historical (plan complete)  
**Implementation:** shipped  
**Gaps:** none for this freeze (bounded multi-request static server + Main.thx path)  
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)

> **Historical plan.** Bounded HTTP path shipped. Living priority:
> [`../../CURRENT.md`](../../CURRENT.md). Progress updates:
> capabilities + CURRENT (+ handbook when user-visible), not this file as a changelog.

**Goal:** Close the gap from today’s subset to a **bounded multi-request static HTTP server** that matches the oracle `examples/http` *shape* (Net + Http + Io + Router + Main), without taking on deferred language megawork (M4, Perceus, effects, full stdlib roots).

**Architecture:** Keep the phase-A pipeline (`.thx` → MIR → cgen → `cc` + `libthoxa_rt`). Grow only what the oracle HTTP example forces. Each PR ships a conformance fixture (`.thx` + `.out`) or extends an existing one. Prefer small language surface + library modules under `tests/conformance/httplib/` and `iolib/`.

**Tech stack:** Rust compiler (`crates/compiler`), C runtime (`runtime/`), `cargo make test` gate.

---

## 0. Current baseline (do not re-implement)

| Area | Status | Conformance |
|------|--------|-------------|
| Driver / C emit / RC String/Bytes | ✅ | `hello`…`bytes` |
| Trial monomorphs, `?`, `match` | ✅ | `trials`, `trials_mono` |
| Modules `./` + `~/` | ✅ | `modules`, `namespaces`, `modstate` |
| Extern `"c"`, C-layout, `Address`, fixed `Array<T,N>` | ✅ | `extern_libc`, `cstruct_ffi` |
| Net sockets + one-shot wire HTTP | ✅ | `http`, `http_wire` |
| Loops + for-in Array | ✅ | `control_flow`, `loops`, `arrays` |
| Buffer COW | ✅ | `buffer` |
| `readFile` | ✅ | `file_io` |
| String `==`/`!=`, static path router | ✅ | `router` |

**Known soft spots (plan addresses or documents):**

1. **User+String inside Trial / multi-copy RC** — free `readFile` avoids it; File-typed API blocked until PR 2.
2. **No early `return`** — Router uses nested `if`/`match`; oracle uses `return`.
3. **No `Array<User>`** — blocks oracle `Array<Header>` and full `Request`/`Response`.
4. **No infinite accept loop in conformance** — CI needs a **bounded** N-request server (oracle `Main` is infinite).
5. **No named `ffi` root** — errno/strerror deferred or faked with static Fail strings.
6. **No early `return`, byte lits, May, default params, `Fail { }` ignore** — force only when a later PR needs them.

---

## 1. Target (definition of “HTTP complete” for this plan)

A program that:

1. Creates a listening socket on ephemeral port `0`.
2. Accepts **N** sequential clients (N fixed in the test, e.g. 3).
3. For each: `recv` → `parseRequest` → `Router.handle` → `writeResponse` → `send` → `close`.
4. Serves files from `tests/conformance/io_data/web/` (index, nested about, css, 404, traversal reject).
5. Exits 0 with deterministic stdout for conformance.

**Out of scope for this plan (explicitly deferred):**

| Item | Why deferred |
|------|----------------|
| Algebraic effects / async | D2 |
| Full Perceus RC precision | D8 |
| M4 conversions (`as`/`can`/`is`) | D15 |
| Explicit `address()`, struct value-copy, `sizeof` | D23–D25 |
| Named roots `std/` / full `ffi` package | D30–D32 (partial errno only if forced) |
| Buffer consume-on-freeze | D51 |
| Array `insertAt` / `removeAt` / full prelude methods | Not forced by server |
| Infinite accept loop as product binary | Conformance uses bounded N |
| HTTPS, body parsing, keep-alive, chunked | Oracle v1 is one-shot static |

---

## 2. Work inventory → PR mapping

| # | Remaining item | PR(s) | Force |
|---|----------------|-------|-------|
| L1 | Early `return` in fn/method | PR 1 | Oracle control flow; cleaner Router/server |
| L2 | User field-RC on copy + Trial&lt;User+heap&gt; | PR 2 | Unblocks File type / Request with Strings |
| L3 | `Array&lt;User&gt;` element support | PR 3 | `Array&lt;Header&gt;`, full Request |
| L4 | Match ignore bind `Fail(_)` or empty | PR 4 | Oracle `Fail { }` / discard err |
| L5 | Byte literal `b'x'` (optional if using int codes) | PR 4 or skip | Nice for parsers; ASCII ints OK |
| L6 | `Request` / `Response` / `Header` + parse + write | PR 5 | Protocol surface |
| L7 | Router type + `handle(Request) -> Response` | PR 6 | Oracle shape |
| L8 | `File` type or keep free `readFile` | PR 2 then optional polish | Free fn is enough if RC fixed |
| L9 | Bounded multi-request server | PR 7 | Closes HTTP ladder |
| L10 | `Main.thx` directory entry | PR 8 | Oracle `thoxa run dir` |
| L11 | OpenMode / better Io errors | PR 9 (optional) | Nice-to-have; static Fail tags work |
| L12 | Local errno/strerror without full ffi root | PR 9 (optional) | Same |
| L13 | String `.count` / index | only if forced | Prefer Bytes path |

---

## 3. Key Decisions

| ID | Decision | Choice | Rationale |
|----|----------|--------|-----------|
| P1 | Conformance server shape | Bounded accept loop (`for i in 0..N`) + ephemeral port | Infinite loop is not CI-safe; still proves accept/serve/close |
| P2 | Request headers | Ship `Array&lt;Header&gt;` (User elements) | Matches oracle; forces PR 3 |
| P3 | May / find | Keep `Bytes.find` → Int64 `-1` | Do not block on May&lt;T&gt; |
| P4 | Early return | Statement `return expr` (or bare `return` for Void) | Nested if trees are unmaintainable for server |
| P5 | Match discard | Allow `Fail(_)` bind name `_` that is not defined, or no-bind `Fail` if parser easy | Prefer `Fail(_)` first |
| P6 | Io surface | Keep `readFile`; add `File` only after PR 2 green | Avoid reintroducing UAF |
| P7 | Module layout | Stay under `tests/conformance/{httplib,iolib}/` with `./` + `~/` | Named roots still deferred |
| P8 | Response write | Serialize to Bytes via Buffer or String build | Buffer already shipped |
| P9 | Docs | Every PR updates status Done row + decision if locking surface | AGENTS.md “next” only on last PR |

---

## 4. Dependency graph (DAG)

```text
PR1 early return ─────────────────────────────┐
PR2 User+heap RC ──┬── PR3 Array<User> ── PR5 Request/Response ── PR6 Router type ── PR7 bounded server
                   │                              │
PR4 match discard ─┴──────────────────────────────┘
PR8 Main.thx dir entry (depends PR7 optional; can follow PR7)
PR9 Io polish (optional, parallel after PR2)
```

**Levels (parallelism):**

| Level | PRs |
|-------|-----|
| 0 | PR1, PR2, PR4 (independent language fixes) |
| 1 | PR3 (needs PR2) |
| 2 | PR5 (needs PR3, PR4; benefits from PR1) |
| 3 | PR6 (needs PR5) |
| 4 | PR7 (needs PR6, PR1) |
| 5 | PR8 (needs PR7) |
| opt | PR9 after PR2 |

---

## 5. Global constraints (every implementer)

1. Read `AGENTS.md` + `CURRENT.md` + `docs/capabilities.md` first.
2. One PR = one focused diff; no drive-by refactors outside listed files.
3. After substantive compiler/runtime changes: `cargo make test` must pass (all existing + new).
4. Do **not** invent syntax without checking SPEC (`apocrypha/old/ThoxaPython/spec/SPEC.md`) or decisions.
5. Do **not** commit unless the orchestrator/user asks (orchestrator owns git for execute-plan).
6. Prefer matching oracle names when surface is public (`parseRequest`, `writeResponse`, `Header`, …).
7. Heap RC: dup on copy, drop at end; for-in heap elems already fixed (D62) — do not regress.
8. All build products under `target/`; use `cargo make …`.

---

## 6. File map (where work lives)

| Path | Role |
|------|------|
| `crates/compiler/src/parser.rs` | `return`, match patterns, optional byte lits |
| `crates/compiler/src/ast.rs` | AST nodes |
| `crates/compiler/src/mir.rs` | Lowering, RC, Array&lt;User&gt;, Trial |
| `crates/compiler/src/cgen.rs` | Emit C for new ops; includes |
| `runtime/include/thoxa_rt.h`, `runtime/src/thoxa_rt.c` | Only if new runtime helpers |
| `tests/conformance/*.thx` + `*.out` | Gate |
| `tests/conformance/httplib/*` | Net, Http, Router |
| `tests/conformance/iolib/*` | Io |
| `tests/conformance/io_data/web/*` | Static fixtures (exists) |
| `CURRENT.md`, `docs/capabilities.md`, `docs/decisions.md`, `AGENTS.md` | Progress |

---

## PR Plan

### PR 1: Early `return` statement

- **Description:** Parse and lower `return` / `return expr` inside functions and methods. Type-check against enclosing return type. Emit C `return`. Support `return` of Void (no value) and early exit from loops. Conformance: early returns in nested if/for/match.
- **Files/components affected:** `crates/compiler/src/ast.rs`, `parser.rs`, `mir.rs`, `cgen.rs` (if needed), `tests/conformance/early_return.thx`, `early_return.out`, `CURRENT.md`, `docs/capabilities.md`, `docs/decisions.md`
- **Dependencies:** None
- **Acceptance:**
  - [ ] `return 1` from mid-function changes result
  - [ ] `return` inside `for` / `while` / `loop` exits function (not just loop)
  - [ ] Type mismatch on return is a compile error
  - [ ] `cargo make test` green; new `early_return` fixture
- **Implementation notes:**
  - Add `BlockItem::Return { value: Option<Expr> }` or statement form consistent with existing block parsing.
  - MIR already has `Stmt::Return { value: Place }` used by `?` — reuse.
  - Ensure end-of-fn drops still run on early return **or** document that RC on early return is best-effort for phase A; prefer inserting drops before Return (collect live heap locals) so we do not reintroduce leaks/UAF. **Minimum:** drop all `needs_drop` locals defined so far (same list as `insert_function_drops`, excluding moved return value).
  - Decision id suggestion: D63 early return.

### PR 2: User+heap field RC hardening

- **Description:** Fix reference counting when User types with String/Bytes fields are copied, passed, stored in Trial ok/err, extracted via match/`?`, and dropped. Today free `readFile` avoids `Trial<File,…>`; this PR makes that pattern safe.
- **Files/components affected:** `crates/compiler/src/mir.rs` (`assign` Dup/Use for User, TrialMake, TrialOk extract, `insert_function_drops`, method receiver), `crates/compiler/src/cgen.rs` (`emit_rc` for User — already walks fields; verify call sites), `tests/conformance/user_rc.thx`, `user_rc.out`, optionally extend `string_fields`, `docs/*`
- **Dependencies:** None
- **Acceptance:**
  - [ ] `type Box(s: String)` in `Trial<Box, String>`: Pass, match, `?` rewrap, no ASan UAF under two sequential calls
  - [ ] Assign `let a = b` for Box dups String; drop both safe
  - [ ] Method call on Box that returns Box does not double-free path field
  - [ ] Run ASan on the new fixture binary (document command in PR summary):  
    `cc -fsanitize=address …` against emitted C + runtime
  - [ ] `cargo make test` green
- **Implementation notes:**
  - Audit every `RValue::Use` of `Ty::User`: Dup must walk heap fields (already in `emit_rc`); ensure MIR always inserts Dup and Drop pairs balanced.
  - Trial ok extract: currently dups heap ok; User ok must field-dup once, not thrice (see historical `thoxa_dup` spam in generated C for File).
  - Prefer structural fix over per-call hacks.
  - Decision: D64 User+heap RC.

### PR 3: `Array<User>` elements

- **Description:** Allow dynamic `Array<T>` where T is a constructor-param User type (e.g. `Header`). Runtime: store User structs inline in a growable buffer **or** store as opaque blob with elem_size + cgen-generated dup/drop loops. Recommend: **monomorphized helpers per User type used as array elem** or generic `elem_size` + compiler-emitted walk. Simpler path: represent User array as `ThoxaArray` of pointers to **heap-boxed** User values — heavier. Prefer **inline values** with `elem_size = sizeof(C struct)` and cgen loops for element Dup/Drop matching `emit_rc`.
- **Files/components affected:** `mir.rs` (`is_array_elem_ty`, append/get/set/for-in), `cgen.rs` (ArrayLit, DynIndexAssign, get/set builtins or inline), `runtime/*` if generic byte-slot arrays need expand, `tests/conformance/array_user.thx`, `array_user.out`, `docs/*`
- **Dependencies:** PR 2
- **Acceptance:**
  - [ ] `type H(name: String, value: String)` + `mut xs: Array<H> = []` + append + for-in + index get
  - [ ] Display optional; at least count and field access
  - [ ] No UAF under ASan on fixture
  - [ ] `cargo make test` green
- **Implementation notes:**
  - Extend `is_array_elem_ty` to `Ty::User(_)` when User has only supported field types.
  - get/set cannot use `int64_t` slots if sizeof(User) > 8 — **must** change representation for large elems: either flexible `uint8_t data[]` with `elem_size` or monomorphized `ThoxaArray_Header`.
  - Recommended design (lock as D65): `ThoxaArray` grows to `{ len, cap, flags, elem_size, /* data */ }` with data as `uint8_t[cap * elem_size]`; cgen emits typed load/store + RC walk for User.
  - Int64/String arrays must keep working (regression).

### PR 4: Match discard binding `_`

- **Description:** Allow `Fail(_)` / `Pass(_)` where `_` does not introduce a binding (or is immediately unused). Optionally `Fail` with no paren if easy — prefer `_` only to minimize grammar churn.
- **Files/components affected:** `parser.rs`, `ast.rs`, `mir.rs` (match arm bind), `tests/conformance/match_discard.thx`, `match_discard.out`, `docs/*`
- **Dependencies:** None
- **Acceptance:**
  - [ ] `match t { Pass(v) { … } Fail(_) { println("x") } }` compiles
  - [ ] Payload of discarded side is still dropped (RC)
  - [ ] `cargo make test` green
- **Implementation notes:** Decision D66.

### PR 5: Request / Response / Header + parse + write

- **Description:** Grow `httplib/Http.thx` toward oracle: `Header`, `Request`, `Response`, `parseRequest(raw) -> Trial<Request, String>`, `writeResponse(res) -> Bytes`, `ok`/`notFound`/`badRequest` producing `Response` (or keep Bytes helpers and add typed variants). Prefer typed API + `writeResponse`.
- **Files/components affected:** `tests/conformance/httplib/Http.thx`, new `tests/conformance/http_parse.thx` + `.out`, possibly `http.thx` / `http_wire.thx` updates if signatures change (keep backward compat wrappers if needed), `docs/*`
- **Dependencies:** PR 3, PR 4 (PR 1 recommended for cleaner parse)
- **Acceptance:**
  - [ ] Parse `GET /x HTTP/1.1\r\nHost: h\r\n\r\n` → method/path/version; headers optional v1 (may be empty array if header parse deferred one step — **prefer parse at least Host**)
  - [ ] Malformed → Fail string
  - [ ] `writeResponse` round-trips status/body Content-Length
  - [ ] Existing `summarizeRequest` / `ok(Bytes)` either preserved or updated with adapters so `http` + `http_wire` still pass
  - [ ] `cargo make test` green
- **Implementation notes:**
  - Use `Bytes.find` (-1) not May.
  - Split lines with loops + find `\r\n` (no need for full `splitBytes` if painful).
  - Build response with Buffer or interpolation + `Bytes.fromString`.
  - Header type: `type Header(name: String, value: String)`.
  - Request: method, path, version, headers: Array&lt;Header&gt;, body: Bytes (empty).
  - Response: status: Int64, reason: String, headers: Array&lt;Header&gt;, body: Bytes.

### PR 6: Router type + handle(Request) → Response

- **Description:** Refactor `httplib/Router.thx` to oracle-ish `type Router(root: String) { export fn handle(req: Request) -> Response }`. Keep free `handle(root, path)` as thin wrapper **or** replace call sites. Use early return if PR 1 landed.
- **Files/components affected:** `tests/conformance/httplib/Router.thx`, `tests/conformance/router.thx` + `.out`, `docs/*`
- **Dependencies:** PR 5
- **Acceptance:**
  - [x] Same behavioral coverage as current router fixture (index, about, css, traversal, missing, non-absolute)
  - [x] Returns Response values; test asserts via `writeResponse` status line
  - [x] `cargo make test` green
- **Implementation notes:** Import `readFile` from `~/iolib/Io` (no `..`). Content-Type header on Response. **Shipped (D70):** free `handle` removed; call sites use `Router` + `writeResponse`.

### PR 7: Bounded multi-request static server

- **Description:** Conformance program that binds ephemeral port, connects N clients (or one client reconnecting N times), each GETs a path, server accept/serve/close per connection, then exits. Integrate Net + Http + Router. Deterministic stdout (status lines / paths only).
- **Files/components affected:** `tests/conformance/http_server.thx`, `http_server.out`, possibly small helpers in `httplib/`, `CURRENT.md`, `docs/capabilities.md`, `docs/decisions.md`, `AGENTS.md`
- **Dependencies:** PR 6, PR 1
- **Acceptance:**
  - [ ] N≥2 sequential connections succeed
  - [ ] At least one 200 file hit and one 404
  - [ ] No infinite loop; process exits
  - [ ] `cargo make test` green including ASan spot-check of this binary recommended
- **Implementation notes:**
  - Pattern from `http_wire.thx`: server bind(0), client connect(localPort).
  - For N requests: loop with counter; server accept each time; client reconnect or multiple clients.
  - Do **not** require threads.
  - Decision D67 bounded server conformance.

### PR 8: Directory entry `Main.thx`

- **Description:** `thoxa run <dir>` resolves `<dir>/Main.thx`; project root for `~/` is that directory. Conformance: small multi-file package under `tests/conformance/pkgmain/` with `Main.thx` + helper, invoked via CLI.
- **Files/components affected:** `crates/compiler/src/main.rs` and/or `driver.rs` / `loader.rs`, `crates/make/src/main.rs` if test harness needs dir mode, `tests/conformance/…`, `docs/*`
- **Dependencies:** PR 7 (so server package can be dir-shaped) — **or** None if only a tiny package is used; prefer soft dep on PR 7 only for optional demo layout
- **Acceptance:**
  - [x] `cargo run -p thoxa -- run tests/conformance/pkgmain` (or make target) runs Main
  - [x] `~/` imports resolve from package dir
  - [x] File mode `run path.thx` unchanged
  - [x] `cargo make test` green
- **Implementation notes:** D31 already says `~/` = entry file’s directory; for dir entry, entry file is `Main.thx`. Decision D68.

### PR 9 (optional): Io OpenMode + errno strings

- **Description:** If time/force: `open` flags as Int32 constants module-level; optional `extern` for strerror via local `"c"` without full `ffi` root (`extern fn strerror…`). Improve Fail messages. Not required for PR 7.
- **Files/components affected:** `tests/conformance/iolib/Io.thx`, `file_io.*`, `docs/*`
- **Dependencies:** PR 2 (if File type returns)
- **Acceptance:** Documented; tests still pass.
- **Shipped (D80–D82):** `oRdonly`/`oWronly`/`oRdwr`; `thoxa_errno` + `strerror` extern; path+strerror Fail; cgen String return marshal. File type deferred (method mut-field write-back).

---

## 7. Suggested execution order (linear stack)

For `/execute-plan` linearization (also respects parallel levels):

1. PR1 early return  
2. PR2 User+heap RC  
3. PR4 match discard  
4. PR3 Array&lt;User&gt;  
5. PR5 Request/Response  
6. PR6 Router type  
7. PR7 bounded server  
8. PR8 Main.thx  
9. PR9 optional Io polish  

**Critical path to “HTTP complete”:** PR2 → PR3 → PR5 → PR6 → PR7 (PR1/PR4 feed quality, PR1 required for clean server).

---

## 8. Verification matrix

| After PR | Command | Expect |
|----------|---------|--------|
| Each | `cargo make test` | All previous + new fixtures PASS |
| PR2, PR3, PR7 | ASan on new binary | No heap-use-after-free |
| Final | `cargo make test` | Full suite green; CURRENT points past HTTP ladder |

---

## 9. Docs updates (final PR of critical path)

When PR 7 merges (or last executed critical PR):

- `CURRENT.md` + `docs/capabilities.md`: HTTP rows **shipped** (historical — already done).
- `docs/decisions.md`: D63–D68 (as assigned).
- Handbook / architecture only if user-visible or map changed.
- Kernel design ladder table: post-R9 HTTP ✅.

---

## 10. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Array&lt;User&gt; representation blows scope | Stick to elem_size + byte payload; reuse emit_rc walks |
| Early return skips drops | Explicit drop-before-return in MIR |
| Request parse too large | v1: request-line + optional headers; body empty |
| Server flaky ports | Ephemeral 0 + getsockname (already in Net) |
| Subagent context loss | This doc is self-contained; each PR lists files + acceptance |

---

## 11. Open questions (defaults if user silent)

| Q | Default for execution |
|---|------------------------|
| Ship full header parse in PR5? | Yes, simple `Name: value` lines until blank |
| File type in Io or free fn only? | Free `readFile` unless PR2 makes File trivial |
| PR8/PR9 in first execute-plan run? | Include PR8; mark PR9 optional/skip unless time |
| Commit policy | Orchestrator commits per PR; no force-push to main |

---

## 12. Handoff for session clear

**Start command (after session clear):**

```text
/execute-plan docs/specs/2026-07-19-post-r9-http-completion-plan.md
```

Or manual: “Implement PR 1 from `docs/specs/2026-07-19-post-r9-http-completion-plan.md`.”

**Bootstrap for every subagent:**

1. Read `AGENTS.md`, `CURRENT.md`, `docs/capabilities.md`, this plan’s **Global constraints** + target PR section.  
2. Implement only that PR’s files.  
3. `cargo make test`.  
4. Update CURRENT / capabilities / decisions if the PR says so.  
5. Stop.

**Do not** re-walk the whole tree or re-derive the ladder; this plan is historical
(HTTP path shipped). Living queue: root `CURRENT.md`.
