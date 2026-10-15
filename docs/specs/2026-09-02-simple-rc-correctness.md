# Freeze: Simple-RC correctness (memory safety)

**Date:** 2026-09-02
**Status:** target (freeze)
**Implementation:** shipped (gate **D151**, match/init **D152**; overwrite-drop; **D154**; if/match result overwrite-drop; `lsan-skip` empty; bounded ASan **2026-09-03**; alloc size wrap; `Socket.close` clears `fd`). Named-later items stay out of this freeze.
**Decision:** D153 (extends D8 / D151 / D152; does not unpark Perceus)
**Gate:** `cargo make test` with empty `tests/conformance/lsan-skip`; documented
ASan pass on non-fiber programs (`cargo make test-asan`)
**Gaps:**
- LSan does not catch use-after-free or most double-frees
- ASan is optional (`cargo make test-asan` / `THOXA_SANITIZE=address`), not the default (ucontext fibers)
- Session / libtcc steps are unsanitized
- Language `drop()` is **D155** (File / Socket / Db); explicit `.close` still valid and inert
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
(D8). The D153 gate does **not** unpark it.

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
| **E. Resource handles** | `File` / `Socket` / `Db` fds; last RC runs `drop()` which closes (**D155**) | Not LSan unless `detect_fd_leaks`; `file_drop` | Closed for those three; no `drop` on unions / C-layout |
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

**Layout.** Heap header is `ThoxaHeader { refcount, type_tag, flags, reserved,
dtor }` in front of the payload (`thoxa_alloc` / `thoxa_dup` / `thoxa_drop`).
`String` / `Bytes` / `Buffer` / `Node` share that header. `Array` / `Map` have
typed dup/drop (heap elems walked on last drop; User elems via `*_ex`
callbacks). Function values use a separate `ThoxaFnBox.rc` (**D132**).
Products with `fn drop()` are `THOXA_TAG_USER` boxes (**D155**). Refcounts are
**non-atomic**. One OS thread; `-->` is stackful ucontext fibers (**D148**).

**Needs drop.** MIR `needs_drop`: String, Bytes, Buffer, Node, Array, Map,
User (always — cgen walks fields, or `thoxa_drop` the box when `has_drop`),
Fn. Scalars and C-layout structs do not. `File` / `Socket` / `Db` `drop()`
closes the handle (**D155**).

**Insert policy (phase A, D8).** Dup on copy (`assign` of `Use` for
`needs_drop`; name-read of a heap binding always dups into a fresh temp).
Drop at function end for noted locals (`insert_function_drops`), except the
returned place (move). Heap params are **owned** (**D154**) — in `drop_places`;
method `_self` stays borrowed. Module `g_*` globals are never
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

### 2. Borrowed params + COW write-back — **shipped (D154)**

Heap params are owned copies: callee drops them; caller forgets the Dup'd temp
and NULLs it after the call. Method `_self` stays borrowed. Gate: `buffer`.

### 3. Extra Dup on constructed / matched heap

D69: phase-A RC may over-dup `Trial<Request>` / `Array<Header>` (LSan noise;
claimed no UAF). Map `.get` of a heap value explicitly Dups twice to feed Has
+ result + end-of-fn drops — easy to over-count. JSON encode/decode and JSX
trees allocate many temps; some are class 1 (loops), some class 3 (missing
note_drop_place).

Leftovers D152 did **not** close — **shipped this slice:**

- **Node catch-all bind** no longer double-Dups (`assign(Use)` is enough).
- **Multi-clause** arm result uses one `assign(Use)` Dup; clause refine
  payload is `note_drop_place`'d. `for` loop vars get fresh C names (nested
  `for name` must not NULL an outer `let name`).
  Gates: `jsx`, `multi_clause`, `sqlite`.

### 4. Nested `Array` / `Map` literals — **shipped**

`emit_array_lit` uses `thoxa_array_from_ptrs` (`HEAP_ELEMS`) for nested
Array/Map, same as String/Bytes/Node. Gate: `arrays_nested` (return a
`Array<Array<String>>` so inner temps drop while the outer lives).

### 5. Fibers and sanitizers

`-->` packs args with an extra retain, starts a fiber, thunk drops args and
the dropped return. Fiber stacks are `ucontext` on mmap'd 512 KiB stacks with a
PROT_NONE guard page (2026-10-14; were `malloc` 128 KiB). ASan + ucontext is why ASan is not the default gate (**D151**).
`start_net` / `start_keepalive` / other `-->` programs are LSan-clean. They
passed the 2026-09-03 ASan run as well, but stay in `asan-skip` because
ucontext fiber stacks are not reliably poisoned.

---

## `lsan-skip` (empty; was 25 before overwrite-drop)

Ratchet: stdout still gated; a skip that becomes LSan-clean **fails** until
the name is removed. If/match result overwrite-drop closed `stdlib`
(`split` / `replaceAll` loop `_if`) and `http_server` (match result in
`for path in paths`).

No names remain.

## Bounded ASan pass (2026-09-03)

`THOXA_SANITIZE=address`, `ASAN_OPTIONS=detect_leaks=0:halt_on_error=1`
(leaks already gated by `cargo make test`). Runtime `.o` unsanitized;
libasan intercepts `calloc`/`free`. Repeatable as `cargo make test-asan`.

| Set | Result |
|-----|--------|
| 88 non-fiber programs | clean (stdout + exit 0) |
| 6 `-->` fiber programs (`http_host`, `maps`, `start_keepalive`, `start_nested`, `start_net`, `start_tasks`) | also clean **this run**; excepted in `tests/conformance/asan-skip` (ucontext — not a ratchet) |

No class-D use-after-free / double-free fired. ASan is **not** the default
`cargo make test`.

---

## Target (this campaign)

**Done when:**

1. `tests/conformance/lsan-skip` is empty and `cargo make test` is green.
2. A documented ASan pass has been run on **non-fiber** conformance
   (`THOXA_SANITIZE=address`). Fiber programs may remain ASan-excepted with
   a listed reason. ASan does **not** become the default `cargo make test`
   until fibers are compatible or excepted in the harness.
3. Handbook RC paragraph still matches shipped policy (dup on copy, drop at
   end of lifetime including overwrite; heap params owned **D154**).

**Gate status:** items 1–3 satisfied 2026-09-03. Named-later work (language
`Drop`, cycle collector, ASan as default) stays out of this freeze.

**Not done when:** Perceus lands, `lsan-skip` is merely shorter, or only
`control_flow` is clean.

---

## Proceed order (one slice at a time)

1. **Overwrite-drop — shipped.** `assign` Drops the previous occupant unless
   the RHS still reads the place; cgen NULLs pointer locals after Drop.
2. **Nested Array/Map literals — shipped.** Gate `arrays_nested`.
3. **COW vs params — shipped (D154).** Gate `buffer`.
4. **Remaining extra-retain — shipped.** Early-return drops from the **final**
   `drop_places` set (`json`, `http_host`, `start_net`, `start_keepalive`).
   If/match result places overwrite-drop (`stdlib` split/replaceAll,
   `http_server` match-in-loop). `lsan-skip` is empty.
5. **Bounded ASan — shipped (2026-09-03).** 88 non-fiber programs clean;
   `cargo make test-asan` excepts 6 `-->` files (`asan-skip`). ASan is not
   the default harness.
6. **Alloc size wrap — shipped.** `thoxa_alloc` / payload / grow helpers
   panic instead of wrapping `calloc`; `thoxa_dup` panics on refcount wrap.
7. **`Socket.close` handle clear — shipped.** Same D107 write-back as
   `File` / `Db`: `fd = -1`; second close is a no-op. Gate: `http_net_err`.
   Not language `Drop`.
8. **Resource `drop` — shipped as D155** (not this freeze). File/Socket/Db.
9. **Named later (not this wave):** cycle collector; fiber stack guard pages
   (shipped 2026-10-14: mmap'd task stacks with a guard page);
   session/libtcc sanitizer (parked with session); Perceus; `drop` on unions /
   C-layout.

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
- Do not start Perceus (D153 gate is done; Perceus stays deferred megawork).
- Product `thoxa run` stays unsanitized (D151) unless a later D-id says otherwise.
