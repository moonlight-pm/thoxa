# Freeze: Simple-RC correctness (memory safety)

**Date:** 2026-09-02
**Status:** target (freeze)
**Implementation:** partial (gate **D151**, match/init **D152**; overwrite-drop **shipped**; campaign **D153** open)
**Decision:** D153 (extends D8 / D151 / D152; does not unpark Perceus)
**Gate:** `cargo make test` with empty `tests/conformance/lsan-skip`; then a
bounded ASan pass (see §4)
**Gaps:**
- 10 conformance programs still listed in `tests/conformance/lsan-skip`
- LSan does not catch use-after-free or most double-frees
- ASan is optional (`THOXA_SANITIZE=address`), not the default (ucontext fibers)
- Session / libtcc steps are unsanitized
- No language `Drop` for File / Socket / Db
- No cycle collector
- Perceus precision is **not** this freeze
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D153 in
[`decisions.md`](../decisions.md)
**Related:** kernel [`2026-07-18-thoxa-native-kernel-design.md`](2026-07-18-thoxa-native-kernel-design.md)
(D4 / D8); LeakSanitizer **D151**; match/init **D152**; HTTP over-dup **D69**;
module globals **D122**; for-in **D62**; value collections
[`2026-08-14-map-and-value-collections.md`](2026-08-14-map-and-value-collections.md)

---

## Intent

Simple single-threaded RC must be **correct** before it is **precise**.

Correct means: every heap object that the compiler claims to own is retained
exactly as many times as it is dropped, and a drop at refcount 0 frees nested
heap (and only then). Leaks, extra retains, missing overwrite-drops, and
drop-on-zero panics are all simple-RC bugs. Perceus reuse is a later MIR pass
(D8) and is forbidden while `lsan-skip` is non-empty.

This freeze is the campaign, not a new value model.

---

## Threat model (as-built)

Label every finding. Do not call a leak “undefined behavior,” and do not call
an empty `lsan-skip` “memory safe.”

| Class | What it is | How we see it today | Severity |
|-------|------------|---------------------|----------|
| **A. Missing overwrite-drop** | Heap place reused (loops, re-bind) without dropping the previous occupant | LSan; systematic in MIR `assign` | High — many skips |
| **B. COW vs borrowed param** | Mutating Buffer/Array/Map replaces a param the callee never drops | LSan; `buffer` can **panic** (drop-on-zero), not only leak | High — closer to UB |
| **C. Extra retain** | Dup without a matching Drop (`Trial<Request>`, `Array<Header>`, JSON/Node temps) | LSan; D69 already named HTTP | Medium — leak, documented no UAF |
| **D. Use-after-free / double-free** | Unbalanced Drop, field walk after free, memcpy of User | Runtime `drop on zero refcount` panic; **ASan not default** | High if present — **under-tested** |
| **E. Resource handles** | `File` / `Socket` / `Db` are `Int64` fds; User drop does not close | Not LSan (unless `detect_fd_leaks`) | Medium — not heap RC |
| **F. Cycles** | `Array`/`Map` of User can retain a cycle | No collector | Low today; real if graphs appear |
| **G. Precision** | Unique objects cloned instead of reused | Perceus | Out of scope |

**What the gate actually is:** GCC/Clang LeakSanitizer on batch `cc`
(`THOXA_SANITIZE=leak`, `target/thoxa/bin-leak/`, **D151**). Product
`thoxa run` is unsanitized. Session/libtcc cannot link liblsan. Valgrind is
not a host dep. ASan exists (`THOXA_SANITIZE=address`) and is **not** CI.

LSan-clean ≠ no UAF. Historical double-frees that *did* fire: D122 (module
global dropped at fn end — kilo), D62 (for-in dropped the last elem twice).
Those panicked or crashed. The current default suite would miss a silent UAF
in a fiber or an HTTP keep-alive path.

---

## As-built RC (do not re-derive)

**Layout.** Heap header is `ThoxaHeader { refcount, type_tag, flags, reserved }`
in front of the payload (`thoxa_alloc` / `thoxa_dup` / `thoxa_drop`).
`String` / `Bytes` / `Buffer` / `Node` share that header. `Array` / `Map` have
typed dup/drop (heap elems walked on last drop; User elems via `*_ex`
callbacks). Function values use a separate `ThoxaFnBox.rc` (**D132**).
Refcounts are **non-atomic**. One OS thread; `-->` is stackful ucontext fibers
(**D148**).

**Needs drop.** MIR `needs_drop`: String, Bytes, Buffer, Node, Array, Map,
User (always — cgen walks fields), Fn. Scalars and C-layout structs do not.
`File` / `Socket` / `Db` are User-with-Int64: dropping them does **not** close
the fd.

**Insert policy (phase A, D8).** Dup on copy (`assign` of `Use` for
`needs_drop`; name-read of a heap binding always dups into a fresh temp).
Drop at function end for noted locals (`insert_function_drops`), except the
returned place (move). Params are **borrowed** — not in `drop_places` (early
return / `?` must not drop caller-owned args). Module `g_*` globals are never
end-of-fn dropped (**D122**); init temps are. Unused heap/User globals are
`__attribute__((used))` so `-O1` DSE cannot delete the store (**D152**).

**Already closed (D152).** Value-match bare bind no longer double-Dups.
Union/Node match result places are dropped (inner `readFile` Trials free).
Seven more programs LSan-clean, including `match_bind`, `file_io*`,
`logging_sink`, `strings_multiline`.

**C locals.** Heap pointer locals are `= NULL`; User/Fn are `{0}`. First Drop
on a place is a no-op. That makes overwrite-drop safe.

**COW.** Buffer/Array/Map writes `ensure_unique` when `refcount > 1`, clone,
drop the shared ref, return the unique pointer. MIR mutating methods assign
that pointer back onto the binding **without** going through `assign`’s Dup
(correct). If the binding is a **param**, the unique clone is never dropped.

---

## Systematic holes (evidence)

### 1. `assign` overwrites without dropping — **shipped**

`LowerCtx::assign` now Drops the previous `needs_drop` occupant unless the RHS
still reads that place (COW MapSet / Array append write-back). Cgen NULLs
pointer locals after Drop so a reused temp (module-init `_s0` across globals)
is not dropped twice. `control_flow`, `maps`, `http_parse`, session scripts,
and others left `lsan-skip`.

### 2. Borrowed params + COW write-back

Caller name-read Dups the argument (pass-by-value copy). Callee does not drop
the param. A mutating method (`Buffer.append`, `Array.insertAt`, `Map.set`)
may COW-replace the param with a unique object. That unique is abandoned.

`tests/conformance/buffer.thx` `grown(seed: Buffer)` is this path. Docs record
that `buffer` **panics** under LSan — treat as RC imbalance, not hygiene.

### 3. Extra Dup on constructed / matched heap

D69: phase-A RC may over-dup `Trial<Request>` / `Array<Header>` (LSan noise;
claimed no UAF). Map `.get` of a heap value explicitly Dups twice to feed Has
+ result + end-of-fn drops — easy to over-count. JSON encode/decode and JSX
trees allocate many temps; some are class 1 (loops), some class 3 (missing
note_drop_place).

Leftovers D152 did **not** close (same “Dup then `assign(Use)`” shape):

- **Node catch-all bind** (`lower_match_node`): `assign(Use)` already Dups,
  then a second `Stmt::Dup` on `sbind`. Value-match bind was fixed; Node was
  not. `jsx` is in `lsan-skip`.
- **Multi-clause return**: explicit Dup of the arm value, then
  `assign(Use)` Dups again. `multi_clause` is in `lsan-skip`.

### 4. Nested `Array` / `Map` literals — **shipped**

`emit_array_lit` uses `thoxa_array_from_ptrs` (`HEAP_ELEMS`) for nested
Array/Map, same as String/Bytes/Node. Gate: `arrays_nested` (return a
`Array<Array<String>>` so inner temps drop while the outer lives).

### 5. Fibers and sanitizers

`-->` packs args with an extra retain, starts a fiber, thunk drops args and
the dropped return. Fiber stacks are `malloc` + `ucontext` (`THOXA_FIBER_STACK`
128 KiB). ASan + ucontext is why ASan is not the default gate (**D151**).
`start_net` / `start_keepalive` are in `lsan-skip`; they are also the programs
ASan is most likely to mis-handle.

---

## `lsan-skip` (10 of ~93 programs; was 25 before overwrite-drop)

Ratchet: stdout still gated; a skip that becomes LSan-clean **fails** until
the name is removed.

| Skip | Likely class | Confidence |
|------|--------------|------------|
| `buffer` | B (COW vs borrowed param) | high |
| `multi_clause` | C (arm Dup + `assign(Use)` Dup) | high |
| `jsx` | C (Node catch-all extra Dup + tree) | med |
| `json` | C (parse/encode temps) | med |
| `http_server` / `http_host` | C (D69 leftover) + fiber | med |
| `sqlite` | C (sql interpolator temps) | med |
| `start_net` / `start_keepalive` | C (task pack) + fiber | med |
| `stdlib` | C (heap arrays / helpers) | med |

---

## Target (this campaign)

**Done when:**

1. `tests/conformance/lsan-skip` is empty and `cargo make test` is green.
2. A documented ASan pass has been run on **non-fiber** conformance
   (`THOXA_SANITIZE=address`). Fiber programs may remain ASan-excepted with
   a listed reason. ASan does **not** become the default `cargo make test`
   until fibers are compatible or excepted in the harness.
3. Handbook RC paragraph still matches shipped policy (dup on copy, drop at
   end of lifetime including overwrite; params borrowed unless a later D-id
   changes that).

**Not done when:** Perceus lands, `lsan-skip` is merely shorter, or only
`control_flow` is clean.

---

## Proceed order (one slice at a time)

1. **Overwrite-drop — shipped.** `assign` Drops the previous occupant unless
   the RHS still reads the place; cgen NULLs pointer locals after Drop.
2. **Nested Array/Map literals — shipped.** Gate `arrays_nested`.
3. **COW vs params.** Either drop unique objects written back onto params, or
   stop treating a COW-mutated param as borrowed. Must not reintroduce D122
   (do not drop module globals) or double-drop caller args. Gate: `buffer`
   LSan-clean **and** no panic.
4. **Remaining extra-retain.** Node catch-all leftover Dup; multi-clause
   Dup+`assign(Use)`; HTTP D69 (`Trial<Request>` / `Array<Header>`); then
   JSON, JSX trees, sqlite interpolator, session Bytes, start-task packs.
   Each skip that goes clean is deleted in the same change.
5. **Bounded ASan.** After (1)–(4) have shrunk the skip list: run
   `THOXA_SANITIZE=address` on non-fiber programs; file what it finds as
   simple-RC bugs, not as a reason to start Perceus.
6. **Named later (not this wave):** language `Drop` for File/Socket/Db
   (`Socket.close` today does not set `fd = -1`); cycle collector;
   `thoxa_alloc` size wrap; fiber stack guard pages; session/libtcc
   sanitizer (parked with session); Perceus.

---

## Explicit non-goals

- Perceus / precise RC / reuse of unique objects
- Atomic RC / threads
- Making ASan the default harness while ucontext fibers remain
- Sanitizing libtcc session steps
- Valgrind as a host dependency
- Claiming oracle-SPEC completeness
- Inventing `Drop` / linear types in this freeze (resource Drop is a later D-id)

---

## Working rules

- Conformance `.thx` + `.out` + LSan-clean is done for a skip.
- Prefer one hole class per slice.
- Runtime `drop on zero refcount` panics are failures, not “LSan noise.”
- Do not start Perceus while `lsan-skip` is non-empty.
- Product `thoxa run` stays unsanitized (D151) unless a later D-id says otherwise.
