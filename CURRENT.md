# CURRENT — living product focus

**Only session handoff.** Update when priority or next slice changes. Read after
`AGENTS.md`. Full model: [`docs/progress-model.md`](docs/progress-model.md).
Capability maturity: [`docs/capabilities.md`](docs/capabilities.md).

**As of:** 2026-10-19 (**D228**: traits, freeze below; all eight slices shipped (`trait`, `conform`, `Resource`, `self`, provided members, properties, declared comparison, generic functions and bounds, declared `Show` / `Component` / codecs, `mixed Trait`, associated types and `for` over user types), and slice 9, conformances of generic and built-in types with members (`Array` is a `Sequence`); next is the rest of the gaps, below; **D227**: the shell and the session are removed, below, and thoxa.dev redeployed without them; gap pass D207–D226, below; D202–D206: label punning, built-in labels, `Element`
field names, `insert(item, at:)`, fields passed `mut`; below). 2026-10-17: **thoxa.dev rewritten (D189)** at Joshua's request: a
home page that states the design rationale, features, status, and build steps
and shows nine sample programs (run by `cargo make test`), and `/manual`, the
handbook rendered as one page. The handbook was rewritten as a reference
manual for it. `containers/` is now `workloads/`; the repository moved to
`github.com/moonlight-pm/thoxa`. Fixes found writing the site: `and` / `or`
short-circuit (D183), array literals take the expected element type (D184), a
refined value's heap base is owned when read (D185, a use-after-free), `let`
binds a Void value (D186), `Float64` text in full between 1e-7 and 1e21
(D187), a session line that starts a statement is never a command (D188).
Then **D190**: every `extern fn` gets its own C prototype (asm-labelled
private name), so any linked C function can be called and `Address<T>`
fits a C struct pointer parameter. Then **D191** (conversions: `as` /
`as?`, `from` clauses, implicit encoding to `Json`, typed route body
parameters) shipped in six slices, with D192 between them and D193
after. Then, at Joshua's request, **D195 slice 1**: methods on String
(`s.contains(x, ignoreCase: true)`, `toLower`, `trim`, `split`, …) with
Unicode case folding, case mapping, and white space from generated Unicode
17.0 tables, replacing most of `std/str`; it needed **D194**, positional
arguments then named ones in one call (the oracle SPEC's rule). Then **D195
slice 2**: `.count` and positions count Unicode scalars (`.byteCount` for
bytes; the count is cached, so `.count` stays O(1)), every String is valid
UTF-8 (U+FFFD repair), and `indexOf` / `substring` / `truncate` / `padLeft` /
`reverse` are methods. Then **D196** (Joshua: Thoxa should self-host, so
write std in Thoxa): String's methods are Thoxa in `std/string`, attached with
`extend String as text { … }` (std modules only) over three runtime byte
primitives (`byteAt`, `byteSlice`, `byteIndexOf`); `std/string` is in every
package; `std/str` is gone (`splitOnce` is a method). AGENTS.md now carries
Joshua's standing orders: write std in Thoxa, API before performance, the
oracle is a light reference, and raise language improvements even against
freezes.
2026-10-16: **Language pass (D173–D181):** `main`'s return is
the exit status, 0–255 (D173); unannotated methods infer their return type,
and `x?` then `Pass(value)` infers `Trial<T, E>` (D174); structural `==` on
products, unions, and arrays (D175, D177, D178); strings inside interpolation
holes (D176); user types as Map keys (D179); collection writes never vanish
(D180); `remove` keeps a Map's insertion order (D181). Editor: Tree-sitter and
LSP colours read `<` as the compiler does (`May<String> {` is not a tag) and
parse string holes as code. Earlier the same day:
**D172** the sites send security headers and
ETags (images and fonts cached a day; review finding 10). **D171** `std/http/server` answers pipelined
requests in order (a request without `Content-Length` has no body; review
finding 9). **D170** a program that is PID 1 ends on SIGTERM /
SIGINT; the site images build their own C from digest-pinned bases and run
as nobody (review findings 8 and 57). Review follow-ups: **D161** defined shifts, **D162** a
line starting `-x` is a new expression, **D163** mutating methods need a
writable receiver (and user method calls write back), **D164** `return` is
Never, **D165** HTTP host deadlines + connection cap + strict framing, **D166** a
panic in a `-->` task ends only that task (the server keeps serving), **D167**
implicit integer narrowing panics when the value does not fit
(`T(truncating: x)` keeps the low bits), **D168** resource constructors
are private to their module and resources live in module state
(`server.peerOf` for SSE), **D169** session `let` / `mut` initialize once and
keep their values (steps link against one base image per declaration set);
struct-field write-back use-after-free fixed; `Address<T>` parameters
compile; `tests/conformance/fail/`
expected-rejection tests; type / signature errors name their file;
`extras/shell` and the `examples/http` store work again; site Containerfiles
pass `-fwrapv`. Correctness pass from the 2026-10-14 project review
(committed): shadowing, method field collection write-back,
fixed-array bounds, string literal emission, **D157** wrapping Int64 + checked
`/` `%`, **D158** else-less `if` is Void, **D159** ASCII `\xHH`, **D160**
session panic/fault recovery + child signals; guarded task stacks, JSON depth
cap, Map tombstones, amortized append, `send` without SIGPIPE, EMFILE backoff,
LSP malformed messages; `cargo make test` also runs the Rust unit tests.
2026-09-04: Interactive REPL: emacs line editor + Tab complete
(**D156**); no startup banner; default prompt `§ ` (D96). **D155** resource
`drop()` on last RC (`File` / `Socket` /
`Db`). **D153** simple-RC **gate is done** (empty `lsan-skip` +
documented ASan). Alloc size wrap panics; `Socket.close` clears `fd`.
**Not Perceus.** Overwrite-drop, nested `Array`/`Map`
`HEAP_ELEMS`, heap params owned (**D154**), early-return final `drop_places`,
if/match result overwrite-drop. Language harvest **D147–D150** shipped.
**D151** LeakSanitizer is the default leak gate; **D152** match/init RC.
`workloads/landing` = thoxa.dev; `workloads/sola` = sola.computer;
`workloads/oath` = oath.wicket.cloud.
Crema Phase **E** through **E5**; **E6** WebSockets parked. **`having`/`else` parked indefinitely.**

---

## Now

**Focus: the core language** (Joshua, 2026-10-16): compiler, type system,
codegen, runtime semantics. Site and hosting work is paused (see holds); the
2026-10-17 thoxa.dev rewrite was an explicit request, not a change of focus.
One language slice at a time, each with conformance.

**Next: traits (D228, Joshua 2026-10-18), freeze
[`docs/specs/2026-10-18-traits.md`](docs/specs/2026-10-18-traits.md).**
Declared conformance (`conform Type to Trait`), traits that carry shared
code, getters and setters, `self`, built-in abilities as declared traits
(`Equatable`, `Hashable`, `Ordered`, `Show`, `Encodable` / `Decodable`,
`Component`, `Resource`), generic functions with bounds, `mixed Trait`.
Eight slices. **Slice 1 shipped**: `trait` with required methods and
parents, `Self`, `conform`, and `Resource` (std/core) replacing a `drop`
method found by name. **Slice 2 shipped**: `self` everywhere (`extend`
drops `as name`) and provided members, checked once at the trait.
**Slice 3 shipped**: getters, setters, `field`, trait property
requirements. **Slice 4 shipped**: `Equatable`, `Hashable`, `Ordered`,
`Order`, declared (`==`, Map keys, `<` need them), conformances with bounds,
std's conformances. **Slice 5, part 1 shipped**: generic functions with
bounds, inferred type arguments, bare trait parameters. **Part 2
shipped**: generic `extend Array<T: Bound>`, std/array (`contains`,
`indexOf`, `sorted` in Thoxa). **Part 3 shipped**: a generic constructor
infers its type arguments (`Shelf(items: [1, 2])`), its instance made on
first use. **Part 4 shipped**: a hand-written `hash` (`Hasher`), used by
Maps; generic functions infer their return type. **Part 5 shipped**:
bounds on generic types' parameters; `Array.map` / `filter`. Open for
Joshua: check a generic type's methods against its bounds (`Counter<T>`
adds `T`s; the freeze rules out arithmetic traits). **Slice 6, parts 1–2
shipped**: `Component` (tag literals need it), `Show` (`"{x}"` of user
types). **Part 3 shipped**: `Encodable` / `Decodable`, declared, in
std/json. **Slice 7 shipped**: `mixed Trait`, values of several types
together. **Slice 8 shipped** (2026-10-19): associated types (`type Item`;
`type Item = Cursor.Item` fixed by the trait), `Iterator` and `Sequence` in
std/core, `for item in value` over them (from a copy), `S.Item` and `where`
in generic functions. **Slice 9, part 1 shipped** (2026-10-19): a generic
type's conformance has members and associated types (`conform Stack<T> to
Sequence { type Cursor = StackCursor<T> … }`); with bounds, its members exist
only for the instances that meet them. **Part 2 shipped**: a built-in type's
conformance has members and associated types, written like `extend` methods
(a program's trait may be given to `Int64` in the trait's module); `Array<T>`
is a `Sequence` (`ArrayCursor<T>`), so generic code over sequences takes
arrays. Joshua: "lean toward long-term correct
design rather than short term stop gaps."

**Next: the rest of D228's gaps** (Joshua 2026-10-19: "take whatever makes
sense and keep working through the rest"; all in the `traits` row of
[`docs/capabilities.md`](docs/capabilities.md)), each a slice: a declared
type's methods with type parameters; same-type constraints (a
sequence's `Item` written without its cursor); `UInt64` constructor fields
(`Hasher`'s state). Waiting on Joshua: whether a generic type's methods are
checked once against its bounds, as generic functions are (recommended:
yes, as freeze §5 says; `Counter<T>`'s `value + d` then needs a trait with
a method, which `Int64` can now conform to).

**Shell and session removed (D227, Joshua 2026-10-18: "drop the shell feature
entirely. This is scope creep.")** `thoxa` is a compiler CLI; every program
has `fn main`, a top-level statement is `THX005`, and bare `thoxa` prints
usage. Gone: the REPL, `-c` / `-l`, session scripts, command sugar, libtcc
and vendored TinyCC, the line editor, `std/shell`, `std/prompt`,
`extras/shell`. Recover from `eefdac4`. A future shell would be a Thoxa
program.

**Strings (D195, D196) shipped**, freezes
[`docs/specs/2026-10-17-string-methods.md`](docs/specs/2026-10-17-string-methods.md),
[`docs/specs/2026-10-17-std-extend.md`](docs/specs/2026-10-17-std-extend.md):
methods on String written in Thoxa, Unicode case and white space, scalar
counts and positions, always-valid UTF-8. Revisit (Joshua): positions in long
non-ASCII text are O(n); the freeze §4 lists what is left to try.

**Language follow-ups shipped (Joshua, "Hit all 5 in order", 2026-10-17):**
**D197** only reachable functions are emitted (hello world 2,824 lines of C,
0.19 s); **D198** declaration keywords (`type`, `from`, `in`, …) are names
wherever a name goes; **D199** `mut` parameters (`f(mut binding)`); **D200**
calls follow declarations (`!` by position, the rest by label); **D201** a
match arm may be one assignment.

**Calls and built-ins shipped (D202–D206, Joshua, 2026-10-18):** a bare name
fills the labelled parameter it names, `connect(host, deadline)` (**D202**);
built-in methods and constructors follow D200, positional except their
options (`find(needle, from: 3)`, `Buffer(capacity:)`, `Element(tag:,
attributes:, children:)`), so they pun too (**D203**); `Element`'s lists
default to empty, `Element(tag: "br")` (**D204**); `items.insert(item, at:
index)` replaces `insertAt` (**D205**); a method passes its own field to a
`mut` parameter, `addLine(mut lines, …)` (**D206**).

**Typed codecs (D182) shipped**, freeze
[`docs/specs/2026-10-16-typed-codecs.md`](docs/specs/2026-10-16-typed-codecs.md):
`toJson` / `encode` on every encodable type (unions tagged by `"type"`); reads
report every issue as `Trial<T, Array<Issue>>`; `T.readForm` reads forms and
query strings into the same types; `{…}` / `[…]` literals where a `Json` is
expected, with `...` spreads; reads of unions, `Map` / `Json` fields, and
constructor defaults; a type's own `toJson` wins. Its non-goals (renaming,
generic types, codecs for `std/` types) stay parked; D191 delivered top-level
`Array` reads, custom decoding, and route body binding.

**Conversions (D191) shipped**, freeze
[`docs/specs/2026-10-17-conversions.md`](docs/specs/2026-10-17-conversions.md):
§1 `x as T` with the built-in conversion table, §2 `x as? T`, §3 `from`
clauses (a `from(json: Json)` clause is the type's reader), §4 implicit
encoding where a `Json` is expected (and `match` takes its expected type), §5
typed body parameters in route clauses, §6 `T.from` / `T.fromJson` removed.
Also on the way: D192 (`return` / `=` convert to the expected type) and D193
(route clauses import only what they write; generated code names `std/http`
itself). thoxa.dev runs `09f76de` (redeployed 2026-10-18 at Joshua's go-ahead; no shell, its manual includes D207–D227).

**Language gap pass (Joshua, 2026-10-18: "cover all these … pick your
order"):** done; each slice with conformance and docs, decisions on the
design questions from Joshua. Shipped: **D207** a field indexes as a local
(`row.cells["name"]`); **D208** a method calls a sibling by bare name
(`double()`), private methods are callable inside their type; **D209**
`thoxa -c` splits its chunk into declarations and statements (an `import`
first line ran nothing; removed with the session by D227); **D210** an LSP rename writes a pun's label out
(`connect(host, deadline: limit)`), and renames method and constructor labels;
**D211** multi-clause methods and generic types' methods infer their return
types; **D212** `==` on maps (any order) and on types that contain
themselves (`Json`, trees); **D213** an expected `Map<K, V>` types a map
literal (`Sheet(cells: {"a": [1], "b": []})`); **D214** Map keys may hold
arrays and Float64, and be Float64 (SameValueZero: NaN finds NaN); **D215**
`\u{…}` string escape (`"caf\u{e9}"`); **D216** `toLower` applies final sigma
(`"ΟΔΟΣ"` → `"οδος"`); **D217** generic types declare `from` clauses
(`[1, 2] as Pair<Int64>`); **D218** a route body parameter may be an
`Array<T>`, read from a JSON array; **D219** `UInt64` arithmetic over its
whole range (wrapping, unsigned `/ % < >>`; mixing with a signed value is an
error); **D220** multi-clause methods write their fields; **D221** a
multi-clause body may call unannotated functions (it is inferred in the same
rounds); **D222** (Joshua's call) `where` ranges mean what `for` ranges do,
`0..24` stops before 24 and `1..=64` includes 64; **D223** (Joshua's call) a
`main` without `->` that returns nothing exits 0 (`fn main() { info("hi") }`);
**D224** (Joshua's call) an implicit `Int64` → `Float64` must be exact (it
panics past 2^53 when it would round), `x as Float64` is a `Trial`, and
`Float64(rounding: x)` rounds; **D225** an LSP rename that would turn a
positional argument into a pun is refused; **D226** `std/normalize`
(`nfc`, `nfd`, `nfkc`, `nfkd`), opt-in, passes all of Unicode 17's
NormalizationTest.

**Language gaps to pick from** (as built; see `docs/capabilities.md`):
From D206: a heap field passed `mut` is copied on the callee's
first write (it is shared with the receiver until the write-back; Joshua
2026-10-18: leave it). Found in
the 2026-10-18 pass: multi-clause parameters cannot join `Int64` and `Float64`
(no union of built-in scalars); a `UInt64` above `Int64` max cannot be
written to JSON.

**To discuss (Joshua, 2026-10-18):** TypeScript-style destructuring as
arguments. Field access does not pun (`options.deadline` stays positional;
only a bare name fills the parameter it names, D202).

**D155** resource `drop` shipped. D153 simple-RC
gate is done. **Not Perceus.** Editor only when DX forces it. Crema Phase
**E** through **E5**; **E6 WebSockets stay parked.** Do **not** vendor HTMX.
Crema leftovers (product migrate, file-based routing) stay parked.

Freezes:
[`docs/specs/2026-09-03-resource-drop.md`](docs/specs/2026-09-03-resource-drop.md)
(**D155**);
[`docs/specs/2026-09-02-simple-rc-correctness.md`](docs/specs/2026-09-02-simple-rc-correctness.md)
(**D153**).

**Shipped recently:** D228 slices 1–8 (`trait`, `conform`, `Resource`, `self`, provided members, properties, declared comparison, generic functions, declared `Show` / `Component` / codecs, `mixed`, associated types) and slice 9 (generic and built-in types' conformances with members, `Array` a `Sequence`), D202–D206 (label punning, built-in labels, `Element` field names, `insert(item, at:)`, fields passed `mut`), D197–D201 (reachable-only emit, soft keywords, `mut` parameters, label rules, assignment arms), D196 (String's methods in Thoxa via `extend`), D195
(String methods, Unicode case, scalar counts and positions, valid UTF-8; two
slices), and D194 (positional then named arguments). D191 conversions (six slices) with D192 and D193, and
the 2026-10-17 fixes D183–D190, each with conformance and rejection gates;
`cargo make test` 186 programs, 122 rejections, 8 site samples, 220 unit tests
after D228 slice 9 (2026-10-19); `cargo make test-asan` 174 clean, 10 excepted (2026-10-19). Before them: the
2026-10-16 language pass (D173–D181, above). Before it:
**D155** `fn drop()` last-RC boxes (`file_drop`; File /
Socket / Db); `thoxa_alloc` size wrap + `Socket.close` clears `fd`
(`http_net_err`); bounded ASan pass (88 non-fiber clean on 2026-09-03, 109 on
2026-10-15, 146 on 2026-10-17; 10 task programs excepted in `asan-skip`); if/match result overwrite-drop (`lsan-skip` empty);
early-return drops from the final `drop_places` set; extra-retain `jsx` /
`multi_clause` / `sqlite`; **D154** callee-owned heap params; nested Array/Map
lits; overwrite-drop; **D152** match/init RC.

**On master, not product:** none.

**Review follow-ups (open, from 2026-10-14):** no CI (R8, deferred); the
session items (Ctrl-C in a loop, the command-or-Thoxa guess, redefining a
session `fn`) went with D227. Paused with
the site work: the event loop's wake scan and blocking log writes (finding
14) and the copied site templates (finding 38). thoxa.dev runs `09f76de` (D227; redeployed 2026-10-18); sola.computer and oath.wicket.cloud still run `e0c0bcf` (D170–D172, redeployed 2026-10-16).

### Proceed order

1. **Editor depth** — only when DX forces it. Freezes:
   [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](docs/specs/2026-07-20-native-editor-lsp-plan.md)
   (recovery/nav residual);
   [`docs/specs/2026-08-09-structured-diagnostics.md`](docs/specs/2026-08-09-structured-diagnostics.md)
   (**D124** shipped — polish codes opportunistically).
2. **Deferred megawork** — M4, Perceus, effects, named `ffi` root, cycle
   collector. Multi-logger (`initLog`) is trajectory under D125, not a
   CURRENT line until samples force it. Perceus is **not unparked**.
   `drop` on unions / C-layout is out of D155 v0.

**Always allowed:** pure safety/doc fixes; tests; progress-doc maintenance;
conformance green-ups; shrinking `lsan-skip` (LSan-clean a listed test, then
delete its line). Further **`std/` growth is sample-driven** (D97/D136).
Crema **domain** (pages, models, cookies policy) stays in `examples/http/`.
Public sites are **`workloads/landing`** (thoxa.dev),
**`workloads/sola`** (sola.computer), and **`workloads/oath`**
(oath.wicket.cloud) — not `examples/`, not `extras/`.

**Explicit holds:**

- **Site and hosting work** (paused 2026-10-16 at Joshua's request; the focus
  is the core language): `workloads/`, `std/http/server` tuning, deploys
- **`having` / `else` May sugar** (parked indefinitely — `match` is enough)
- **Crema domain in `std/`** (pages/models/layout — example-only)
- **A shell** (removed by **D227**; one would be a Thoxa program, not a compiler feature)
- Tree-walk product interpreter or QBE path (phase A)
- Claiming full oracle SPEC coverage
- Treating “expand std/” as its own priority line
- **`"…" as Bytes`** or cast-as-encode for String→Bytes (use `Bytes.fromString`;
  D15 **rejected**)
- **Perceus** (deferred megawork — D153 gate done, not unparked)

Horizon: [`docs/roadmap.md`](docs/roadmap.md). Matrix:
[`docs/capabilities.md`](docs/capabilities.md).

---

## Locked product models (do not re-litigate)

| Topic | Rule |
|-------|------|
| Backend (phase A) | **Compile-to-C** + `thoxa_rt`; no QBE product path |
| Value model | Hybrid A; simple single-threaded RC; Perceus later |
| Errors | `May` / `Trial` from `std/fallible` (import) + `?` / `match`; no May↔Trial mix via `?` |
| Programs | Every program has `fn main`; no REPL, no session scripts, no shell (**D227**) |
| Batch / RT C | System **`cc`** (+ `ar` for runtime) unchanged |
| Host platforms | **Linux** x86_64 product track; Windows out of scope |
| `std/` root | Named `std/…` (D97); **thin**; grow only from sample-driven candidates; Io/Net/Http/HttpStatic shipped (**D126**); Http helpers + **`std/http/server`** (**D136**); hosts under **`std/http/`** (**D139**); Str parse + remaining Http wrappers (**D137**); **`std/json`** (**D141**); **`std/sqlite`** (**D143**); fragments + ETag (**D145**); SSE takeover (**D146**); `ffi` deferred |
| Logging / print | **`std/logging` only** (**D125**); ambient **`println`/`print` removed**; app face `import logging, { info, … }` (**D127**); no ambient inject; multi-logger later |
| Diverging loop | Bare `loop` with no exiting `break` is **Never** (**D128**) — valid function tail without dummy value |
| Iteration | `for item in value` over a range, an Array, a Map (`for k, v`), or a value that conforms to std/core's `Iterator` (`type Item`, `next() -> May<Item>`) or `Sequence` (`iterate() -> Cursor`) (**D228** §7); an iterator is read from a copy, so the value does not move |
| Return inference | Unannotated free fns and methods: returns inferred from exits (**D129**, **D174**); explicit `->` authoritative; no Trial-by-name; recursion / bare Pass/Fail alone need annotate; `x?` then `Pass(value)` refines the success type; `drop` is Void; multi-clause methods join their clauses and generic types' methods infer per instance (**D211**); hover uses inferred types |
| Match arms | Bare ctor = tag-only; `=>` single-expr body or `{ }` multi-stmt (**D130**); `->` types only |
| Multi-clause fns | Same name → one fn (match sugar); `name: Type` formals; any-type dispatch; source order; bare literal types (**D131** — **shipped**). No defaults on multi-clause v0; overlap/export mismatch are **errors**. D16 std factories still separate. |
| Function pointers | `(a: T) -> R` values: free fn, unbound `Type.method`, bound `instance.method` (**D132** — **shipped**). Receiver+code only; no lexical closures. Module store yes; fields/arrays no (D48 / D135). |
| Nowait | Token **`--> name(args)`** starts a named free Thoxa function (**D148**). `Void`; return dropped. Colorless: Net parks; `main` drains. No Future / `async` / `--> { }` / pointer `-->`. |
| Value collections | **D133 shipped** — Array/Map writes through `mut` places (Swift/Buffer COW). `let` aliases do not see later writes. Mutating methods return `Void`. An element of a `mut` Array is a place (`grid[0].append(x)`); a field from outside its type and a temporary are not, and a write on them is an error (**D180**). |
| Equality | Declared (**D228**): `==` needs `conform T to Equatable`, `<` `Ordered`, a Map key `Hashable`; the default compares field by field. Structural: `==` / `!=` compare products field by field, unions by arm and payload, arrays by count and elements, maps by keys and values in any order (**D175**, **D177**, **D178**, **D212**); never by RC pointer. A type that contains itself compares through a generated function. No ordering on them. |
| Map | **D134 shipped** — builtin `Map<K, V>`; keys String/Int64/Float64/Bool (Float64 SameValueZero, **D214**); values include **Float64 (D140)** and User sums (`Json`, **D141**). `.has` / `May` get; `{k:v}` lits; `for k, v in m`. User products / unions are keys too (**D175** `==` + a generated hash, **D179**; arrays inside them, **D214**; not traits). `remove` keeps insertion order (**D181**). No `std/http` header flip. |
| Ctor fields | Int64, **Float64**, Bool, String, Bytes, Node, Array, Map, constructor-param User (including sums) (**D135** / **D140**). No C-layout, no fn types, no recursive User fields (use Array/Map). |
| HTTP request | `Request.query` kept (no leading `?`); path still stripped. `parseRequest` body empty; host `readBody` fills it (**D136**). Headers stay `Array<Header>`. `queryParam` / `formField` / `cookie` / `htmlResponse` (**D137**). `Cookie` / `withCookie` / `clearCookie` (**D142**); `htmlFragment` / `etagOf` / `cached` (**D145**). **`peerFd` + `streamed()`** (**D146**) for SSE takeover; the handler gets
its Socket from `server.peerOf(request)` (**D168**). Cookie and `Cache-Control` *policy* stay in the app. |
| SQLite | Amalgamation in `libthoxa_rt` (**D143**). Write `sql"… {value} …"` (**D144**); holes quote themselves. `exec`/`query` take `Sql`. No named `ffi` root. No product `thoxa migrate` — apps own schema files. |
| Combined import | `import ns, { a, b as c } from "…"` (**D127**) — namespace + selective in one clause |
| Module files | A `.thx` file is a **module** (scope), not a type (**D138**). Non-entry stems are kebab-case (`http.thx`, `math-utils.thx`). Types stay PascalCase; namespace bindings stay camelCase. Directory entry is `main.thx` (D68 / D138). Module ids for C mangling are path-qualified (`std/http/server` → `std_http_server`). HTTP hosts live under `std/http/` (**D139**). No user-facing `Module` type |
| Node / tag literals | Builtin **`Node`** (`Element \| Text \| Fragment`) + JSX tags (**D147**). No import. PascalCase is `Type(…).view(children:)` on a `Component` (**D228**). `{e}` children/attrs stringify String / Int64 / Float64 / Bool; `{for}` collects `Array<Node>`. `.html()` is the markup codec — not generic `toString`. Comparison `<` needs a space (`a < b`). |
| Refinements / `Type.read` | `where` refinements are real types (**D149**). `n as Qty` checks a base value and `json as T` reads a `Json`, both `Trial<T, Array<Issue>>`; `T.read` / `T.readForm` parse text into the same, every issue reported (**D182** §2, **D191**; `Issue` in `std/json`). `T.from` / `T.fromJson` are removed (**D191** §6). Literal unions <: String. Qty→base implicit. Tree `decode` stays D141. No cross-field `where`; write `May<Qty>`. |
| Conversions | `x as T` converts and its type says whether it can fail; `x as? T` is `(x as T)?` (**D191**). At most one conversion per pair of types: built in, or a `from` clause on the target (the target alone declares conversions into itself; `toJson` is the source's way into `Json`). Only infallible conversions are implicit, and encoding to `Json` where a `Json` is expected is the only one D191 added; decoding always needs `as`. Like integer narrowing (D167), an implicit `Int64` / `UInt64` → `Float64` is checked and panics when inexact, and its `as` is a `Trial` (**D224**). Text is parsed, not converted (`T.read`). `return` and `=` convert to the expected type (**D192**). No traits. |
| Route clauses | Type-body `GET "/…"` / `POST` / … on a nullary type → `route(req) -> Response` (**D150**). String wrap `ok`; miss `notFound()`; `Int64`/`String` captures. A parameter that is neither a capture nor the `Request` binds the body (query for GET/HEAD, JSON or form by `Content-Type`, else 415); a failed read is a 400 unless the parameter is `Trial<T, Array<Issue>>` (**D191** §5). The module imports only what its clauses write (**D193**). No fallthrough. |
| String → Bytes | **`Bytes.fromString` only** — no cast sugar (D15 **rejected**) |
| Resource `drop` | `conform T to Resource { fn drop() { … } }` runs `drop` on last RC (**D155**, **D228**); a type's own `fn drop()` is an ordinary method. Heap box; copies share. `File` / `Socket` / `Db` close in `drop`. Explicit `close` stays inert. Only the defining module calls the constructor (**D168**). No unions / C-layout / multi-clause `drop`. Not Perceus. |
| Leak gate | GCC/Clang LeakSanitizer on batch `cc` (**D151**). `cargo make test` always sets `THOXA_SANITIZE=leak`. Match-bind / union-match / unused-global RC (**D152**). Heap params owned (**D154**). If/match result places overwrite-drop. `tests/conformance/lsan-skip` is empty (ratchet). Bounded ASan (**D153**, 2026-09-03): 88 non-fiber programs clean (140 on 2026-10-17); `cargo make test-asan` is optional, not the default (ucontext fibers in `asan-skip`). `thoxa_alloc` panics on size wrap; `Socket.close` clears `fd`. Resource `drop` **D155**. Not Perceus. Session/libtcc unsanitized. |
| Done means | Conformance `.thx` + `.out` under `cargo make test` |
| Progress docs | Meta first-class; capabilities + CURRENT (+ handbook when shipped) |
| Diagnostics | Structured IR → pretty check + `--json` + rich LSP (**D124** shipped); agent-only extras parked |
| Oracle | `apocrypha/old/ThoxaPython` reference only — not the product |

More D-ids: [`docs/decisions.md`](docs/decisions.md).

---

## Session start checklist

1. [`AGENTS.md`](AGENTS.md)  
2. **This file** (Now + locks)  
3. [`docs/capabilities.md`](docs/capabilities.md) — rows for the slice  
4. One freeze if building that domain  
5. [`docs/decisions.md`](docs/decisions.md) only when reopening surface  
6. If D132 / D148: [`docs/specs/2026-08-13-function-pointers.md`](docs/specs/2026-08-13-function-pointers.md)
   + [`docs/specs/2026-08-22-start-tasks.md`](docs/specs/2026-08-22-start-tasks.md) (`-->`)  
7. If D133/D134: [`docs/specs/2026-08-14-map-and-value-collections.md`](docs/specs/2026-08-14-map-and-value-collections.md)  
8. If D135 / JSX `Node`: [`docs/decisions.md`](docs/decisions.md) (**D135**
   fields, **D147** tags) + freeze
   [`docs/specs/2026-08-22-jsx-node.md`](docs/specs/2026-08-22-jsx-node.md)  
9. If D138 / module files: [`docs/decisions.md`](docs/decisions.md) (D138) — kebab-case stems, path-qualified ids; file-based routing parked in [`docs/ideas/2026-08-15-file-based-routing.md`](docs/ideas/2026-08-15-file-based-routing.md)
10. If Crema / HTTP std: [`docs/ideas/2026-08-10-examples-http-crema-experiment.md`](docs/ideas/2026-08-10-examples-http-crema-experiment.md)
   + **D136** / **D137** / **D139** / **D142** / **D145** / **D146** / **D150** in [`docs/decisions.md`](docs/decisions.md)
    + Phase E protocol [`docs/ideas/2026-08-18-html-over-the-wire.md`](docs/ideas/2026-08-18-html-over-the-wire.md)
    + route freeze [`docs/specs/2026-08-23-route-clauses.md`](docs/specs/2026-08-23-route-clauses.md)
11. If JSON / `Type.read`: [`docs/decisions.md`](docs/decisions.md) (**D141** tree,
    **D149** refinements) + freeze
    [`docs/specs/2026-08-23-type-read.md`](docs/specs/2026-08-23-type-read.md)
    + **D140** (`Float64`)
12. If SQLite: [`docs/decisions.md`](docs/decisions.md) (**D143** / **D144**) —
    amalgamation in `vendor/sqlite`; `sql"… {value} …"`. Crema persist is
    example-only (`examples/http` `/tmp/crema.db`); no product migrate.
13. If RC / leaks / memory safety: freeze
    [`docs/specs/2026-09-02-simple-rc-correctness.md`](docs/specs/2026-09-02-simple-rc-correctness.md)
    + [`docs/decisions.md`](docs/decisions.md) (**D153** campaign gate done,
    **D151** LSan gate, **D152** match/init balance, **D155** resource `drop`)
    + freeze [`docs/specs/2026-09-03-resource-drop.md`](docs/specs/2026-09-03-resource-drop.md)
    + `tests/conformance/lsan-skip` + `tests/conformance/asan-skip`.

**End of slice:** [`.grok/skills/thoxa-progress-docs/SKILL.md`](.grok/skills/thoxa-progress-docs/SKILL.md).  
**No** one-off handoff files.

---

## Useful commands

```bash
cargo make test
cargo make test-asan
cargo test -p thoxa --lib
cargo make run examples/http
cargo make run workloads/landing
cargo make run workloads/sola
cargo make run workloads/oath
cargo run -p thoxa -- tests/conformance/param_defaults.thx
printf 'echo hi\nexit\n' | cargo run -p thoxa --
```

---

## Agent notes

- “Next item” / “next rung” → first open item under **Now**, one slice.  
- Do not re-walk the tree or re-derive the ladder at session start.  
- **After each finished checklist / product slice:** update progress docs,
  then **commit and push** (user standing order).  
- Prefer small diffs; don’t drive-by-refactor unrelated surfaces.  
- **Full-word names** — no single-letter locals; `req`/`res` → `request`/`response`. Especially `examples/http`.  
