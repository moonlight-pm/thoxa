# Code review — HTTP filesystem router (last 3 commits)

**Range reviewed:** `1e1beb1..71e6a95` (base `1e1beb1` exclusive → tip `71e6a95` inclusive; i.e. commits `1cd2589`, `366fff3`, `71e6a95`)
**Date:** 2026-07-18
**Method:** xhigh multi-agent review workflow — 6 finder angles + a per-location
adversarial verifier pass + a gap sweep (24 agents; 17 candidates verified, 1
refuted). The full test suite (619 tests) was run independently and **passes**.

## Commits under review

| Commit    | Summary |
| --------- | ------- |
| `1cd2589` | Design doc for the HTTP filesystem router example (docs only). |
| `366fff3` | The router: `Router.thx`, `Http.thx` Request/Response parsing, `Main.thx` server loop, `Net.thx` socket layer, sample `web/` site, language-enhancement backlog doc. |
| `71e6a95` | Language feature: `export name: T` on type constructor params (parser/ast/checker/interp/resolver/lsp), plus migration of the http `Header`/`Request`/`Response` types to it. |

## Headline

The **language feature is clean.** The `export name: T` constructor-param change
across parser/checker/interp/resolver/lsp is coherent, and the http migration
correctly rewrote every positional call site to labeled/punned form
(`Header(name: ..., value: ...)`, `Response(status: 200, reason: "OK", headers, body)`).
The only candidate flagged in the compiler (a defensive `getattr(p, "exported", False)`
in `thoxa/lsp/index.py`) was **refuted** on verification — `.exported` is now a
mandatory dataclass field, so the default can never fire, but it is harmless.

Every real finding is in the **`examples/http/` example code**, mostly introduced
by the router commit (`366fff3`).

Per project guidance ([[thoxa-demo-is-means]] — examples are forcing functions held
to real standards), the findings below are split into genuine regressions worth
fixing vs. acceptable demo simplifications (see Recommendation).

**Remediation preference (follow-up):** prefer **language-level** fixes that prevent
a whole class of bugs over one-off patches in the demo, when the language can
honestly carry the feature. Example-only fixes remain valid stopgaps or for items
that are pure application logic. Language work should be designed under
`docs/specs/` and promoted from (or added to)
`docs/plans/2026-07-18-language-enhancements-from-http.md` when accepted.

---

## Correctness (7)

### 1. fd leak → slow DoS on the headline feature — `examples/http/Router.thx:87` · CONFIRMED
`readFile` runs `let body = file.read()?` (line 87) *before* `file.close()`
(line 88). `open` on a **directory** succeeds, then `read` fails with `EISDIR` —
and that is exactly the directory→`index.html` fallback path that every `/` and
`/about/` request takes. So every home/about page view leaks one fd. After ~1024
requests the process hits its open-file limit; `sysOpen` returns `EMFILE` (all
requests 404) and the accept loop can no longer allocate sockets, so the server
silently stops serving — a slow denial of service on the router's headline
directory-index feature.

**Local fix sketch:** close before propagating, e.g. read into a local, close, then
`?`-check — or ensure `file.close()` runs on the failure path.

**Language-level alternatives (preferred direction):** this is not an HTTP-only bug;
it is the class “`?` / early leave skips explicit cleanup.” The same pattern hits
sockets, terminal restore, locks, temp files — anything with a release step after a
fallible call. Candidate language fixes (not mutually exclusive long-term):

| Approach | Shape | Prevents the class? | Notes |
| -------- | ----- | ------------------- | ----- |
| **A. `defer`** (Go-style) | `defer file.close()` after open | Only if the author remembers `defer` | Small surface; pairs with `?`/`return`; LIFO on function or block exit. |
| **B. Drop / RAII** | Type opts into cleanup when the value dies | Yes, if Drop is automatic for resource types | Aligns with “safety without ceremony” and Perceus-era RC; see timing models below. |
| **C. Scoped resource block** | `with file.open(...) { ... }` always closes | Automatic at the call site | New construct; less flexible mid-function than free-form `defer`. |
| **D. Demo-only** | Close-before-`?` in `readFile` | No — only this call site | Valid stopgap; does not teach or enforce the right pattern elsewhere. |

**Drop timing models** (if pursuing B — the load-bearing design fork; Thoxa has no
ownership/moves today, host GC now, Perceus RC later):

1. **Last reference dies (RC / `deinit`)** — Drop when nothing holds the value.
   Returning or storing a `File` transfers the reference; local + `?` still drops.
   Matches the long-term memory story. Interpreter must approximate (e.g. refcounts
   only for Drop types, or escape-aware scope exit) until real Perceus.
2. **Lexical scope exit + transfer** — Drop at end of the binding’s scope (including
   `?`/`return`); return/store must *transfer* so the callee does not drop. Simpler
   stack discipline, but introduces move-like rules without a borrow checker —
   easy to get use-after-move or double-drop wrong.
3. **Explicit `use` / resource bindings only** — Ordinary `let`/`mut` never Drop;
   only `use mut f = File(...)` does. Smallest blast radius, but opt-in, so the
   original bug can still be written with plain `mut`.

**Cases any Drop design must handle:** (A) local open + `?` Fail must not leak;
(B) return a live `File`/`Socket` must not close in the callee; (C) store into a
longer-lived structure must keep the resource alive; (D) explicit `close()` then
scope end must be idempotent (no double `sysClose`).

**Recommendation lean (open):** prefer **B with model 1 (last-reference / deinit)**
as the end state, possibly with `defer` (A) as a lighter interim or complement.
`drop`/`deinit` should return `Void` (not `Trial`) so unwind paths stay simple;
explicit `close` leaves the value inert so Drop is a no-op. Promote to a design
spec under `docs/specs/` before implementing — not yet on the HTTP language
backlog.

### 2. Single `recv` assumes one read == one request — `examples/http/Net.thx:93` (also `Main.thx:19`) · CONFIRMED
`receive()` does one 4096-byte `sysRecv` and treats the result as the whole
request. Any request split across TCP segments (slow clients, Nagle) or exceeding
4096 bytes (large `Cookie`/`Authorization` headers) reaches `parseRequest`
truncated, cannot find the `\r\n\r\n` terminator, and is answered `400 Bad
Request` / `"incomplete request"` instead of the requested page. Newly
consequential now that the buffer is actually parsed.

**Fix sketch:** loop `recv` accumulating into a buffer until `\r\n\r\n` is seen
(then, for bodies, until `Content-Length` is satisfied).

### 3. Accept loop dies on any transient error — `examples/http/Main.thx:18` · CONFIRMED
`let client = server.accept()?` propagates *any* `accept` failure out of `main()`.
A client that sends SYN then RST before `accept` (`ECONNABORTED`), or an `EINTR`,
terminates the whole server instead of skipping that one connection. Under real
traffic with early hangups the server exits unpredictably.

**Fix sketch:** match on the `accept` result inside the loop; log and `continue`
on error rather than `?`-propagating.

### 4. Swallowed IO error → lost diagnostic — `examples/http/Router.thx:113` · CONFIRMED
`handle`'s catch-all `Fail {` arm funnels every open/read error into `notFound()`,
discarding the error text. The previous `serve()` propagated it via `?` and logged
`serve error: {e}`. Net effect: the `Fail(e) { println(...) }` arm in `Main.thx:23`
is now **unreachable**, and a misconfigured web root (wrong cwd, permission denied)
is indistinguishable from a genuinely missing page — bare 404 to the client,
nothing on the console.

**Fix sketch:** distinguish "not found" from other IO errors, or log the error
text in the `Fail` arm before returning `notFound()`.

### 5. Header OWS stripping only handles a single space — `examples/http/Http.thx:90` · CONFIRMED
`if line.get(valStart) == b' '` strips exactly one leading space — not a tab
(valid HTTP OWS) and not multiple spaces. `Host:\texample.com` or
`Host:  example.com` yields a `Header` value with embedded leading whitespace, so
any consumer that compares/looks up a header by exact string silently mismatches.

**Fix sketch:** skip all leading `b' '`/`b'\t'` before the value (and ideally trim
trailing OWS too).

### 6. `send` doesn't loop on partial writes — `examples/http/Net.thx:104` · PLAUSIBLE
`sysSend` is issued once and its return count discarded. A response larger than the
kernel send buffer is truncated below its advertised `Content-Length`, then the
connection closes — a corrupt response. Write-side twin of #2, newly relevant now
that arbitrary-size static files are served. Marked PLAUSIBLE: whether it triggers
depends on payload vs. socket-buffer size; the demo's tiny files won't hit it, but
the code and comments claim "send in full."

**Fix sketch:** loop `send` from an offset until `data.count` bytes are written.

### 7. Dead `empty request` guard — `examples/http/Http.thx:55` · CONFIRMED
`splitBytes` always appends a final slice, so it never returns 0 elements —
`if lines.count == 0 { Fail("empty request") }` is unreachable. An empty header
block instead falls through to `reqLine = ""` and is rejected as
`"malformed request line"`. Vestigial guard; the intended diagnostic never fires.

**Fix sketch:** drop the dead branch, or check the real empty condition
(`reqLine.count == 0`).

---

## Cleanup / efficiency (5)

### 8. Duplicated response-header construction — `examples/http/Http.thx:110/121/132` · cleanup
`ok`/`notFound`/`badRequest` copy-paste the identical Content-Type/Content-Length/
Connection header list; `notFound` and `badRequest` differ only in status, reason,
and body. A `textResponse(status!, reason!, contentType!, body!) -> Response`
helper collapses all three into one-liners and removes the risk of updating one
site and missing another.

### 9. `pathSegments` re-implements `splitBytes` — `examples/http/Router.thx:9` · cleanup
`pathSegments` hand-rolls the same scan-for-delimiter/slice/append loop that
`Http.thx`'s `splitBytes` already provides (single-byte delimiter, drop empties,
`toString`). Two split implementations must be kept correct in parallel. Exporting
`splitBytes` and filtering empties in the caller reuses one implementation.

### 10. `Bytes.fromString(":")` re-allocated per header line — `examples/http/Http.thx:81` · efficiency
The header-parse loop rebuilds the constant `":"` delimiter Bytes on every
iteration — N redundant conversions for N headers. Hoist it to a single binding
before the loop.

### 11. `joinPath` does O(n²) string concatenation — `examples/http/Router.thx:44` · efficiency
`path = "{path}/{s}"` re-copies the whole accumulated path on each segment — the
exact quadratic-concat pattern `writeResponse` deliberately avoids with a growable
`Buffer`. Negligible for demo paths, but re-introduces the anti-pattern; use a
`Buffer` or a single join for linear behavior.

### 12. `Request` publishes unused / always-empty fields — `examples/http/Http.thx:14` · cleanup · PLAUSIBLE
`Request` exports `method`/`version`/`headers`/`body`, but only `path` is ever
read, and `body` is a documented always-empty stub. Four unused/empty members
widen the public API readers must reason about. Marked PLAUSIBLE — depends on the
intended forward API surface; may be deliberate for when body/header reading lands.

---

## Recommendation

**Genuine regressions worth fixing** (high-value, introduced by these commits):

- **#1 fd leak** — fix first, preferably via a **language** feature (Drop/RAII and/or
  `defer`; see alternatives under #1), not only a local close-before-`?` patch.
  Example-only fix is an acceptable interim if language work is staged.
- **#4 swallowed diagnostic** — restore the operator-visible error signal (mostly
  application logic; richer `Trial` error unions are a related language backlog item).
- **#5 OWS** and **#7 dead guard** — correctness nits worth taking since `Http.thx`
  is presented as a reusable protocol module. #5 also pressures prelude
  `trim` / OWS helpers (see language backlog §4).

**Cheap, instructive polish** the example should model: **#8–#11** (reuse +
efficiency). Several map to language/stdlib pressure already on the backlog:
**#9** → `split` for Bytes/String; **#11** → growable string / `Buffer`-as-builder
story; **#10** is pure local hoist.

**Classic toy-server simplifications** — reasonable to leave *if called out*:
**#2** (read-until-terminator), **#3** (per-connection error recovery), **#6**
(write drain loop). Suggest a one-line comment at each site acknowledging the
shortcut so the example doesn't quietly teach the wrong pattern — or park them in
`docs/plans/2026-07-18-language-enhancements-from-http.md` alongside the backlog
already captured in these commits. These are primarily I/O *shape* in the example;
language help is better buffers / partial-read loops, not async.

**Suggested order when remediating with a language-first bias:**

1. Design + ship resource cleanup (**#1** → Drop/`defer` spec).
2. Example correctness that is not a language feature: **#4**, **#5**, **#7**.
3. Example polish **#8–#11**, promoting shared helpers into prelude/stdlib when
   they clearly belong there (**#9**, trim for **#5**, builder for **#11**).
4. Document or implement **#2 / #3 / #6** as intentional server-loop quality.

If staying example-only for a first commit: **#1 + #4** together (both small, both
real regressions) remains a valid interim.

---

## Verification stats

- Level: xhigh · Finders: 6 · Candidates: 17 · Verifier agents: 15 · Verified: 17
  · Refuted: 1 · Reported (deduped): 12
- Independent check: `uv run pytest` → **619 passed**.
- Refuted candidate: `thoxa/lsp/index.py:100` defensive `getattr` — harmless, no change needed.
