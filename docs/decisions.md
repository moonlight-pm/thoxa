# Thoxa — Decision Log

Living log of load-bearing choices. Newest first within each section when updated.

**Doc layout** (full model: [`progress-model.md`](progress-model.md); map: [`README.md`](README.md)):

| Path | Role |
|------|------|
| `AGENTS.md` | Session bootstrap — keep short |
| `CURRENT.md` | **Only** living focus handoff (priority + locks) |
| `docs/capabilities.md` | As-built capability maturity + gaps |
| `docs/decisions.md` | Locked choices + deferred analyses (this file) |
| `docs/specs/` | Target freezes (desired shape; not changelogs) |
| `docs/handbook/` | User manual — **shipped** behavior only |
| `docs/ideas/` | Parking lot — **not** decisions |
| `.grok/config.toml` | Project permission allows |

No knowledge graph (`docs/graph/`, node databases, etc.) until cross-links and “where did we say X?” pain are real. Prefer prose + IDs here over premature graph tooling. Handoff is file-based (`AGENTS.md` + `CURRENT.md` + `capabilities.md`), not a separate memory server.

---

## 2026-10-18 — Traits (Joshua: "'implicit' traits scattered about like 'drop()' -- which is kind of too informal")

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D228 | Traits, declared conformance, properties, and generic functions | Freeze [`specs/2026-10-18-traits.md`](specs/2026-10-18-traits.md). `trait Name: Parent { … }` declares required members (no body) and provided ones (a body, written once on the trait: "traits over mixins"); a trait adds no fields but may require a property, satisfied by a field or a getter. `conform Type to A, B { … }` is the only way to conform, in the type's or the trait's module, once per package; generic types conform with bounds (`conform Box<T: Equatable> to Equatable`). `Self` is the conforming type, known only through the trait; `self` is the receiver value, everywhere (`extend … as name` goes). Getters and setters (`get name: T`, `set name(value: T)`), uniform with fields; inside an accessor, `field` is the stored value. Built-in abilities become declared traits with structural defaults: `Equatable` (`==`), `Hashable` (Map keys), `Ordered` (`<`), `Show` (`"{x}"`), `Encodable` / `Decodable` (codecs), `Component` (tags), `Resource` (`drop`). Generic functions `<T: A & B>` are checked against their bounds where declared, with inferred type arguments; a bare trait parameter is a hidden type parameter; values of several types are `mixed Trait`. Associated types for iteration later. | Joshua, 2026-10-18, over one discussion: reopen "no traits" for bounds; conformance declared ("we should have to formally declare a type to be ordered or hashable or comparable etc."); "lean toward long-term correct design rather than short term stop gaps"; "rather than impl X for Y i'm thinking 'conform Y to X'"; getters "instead of a 'let x'"; `field` over Swift-style storage inside the type; "just use self.. no need to keep inventing new variable names"; the dynamic form should say "mixed". Supersedes, as each slice ships: the "no traits" lines of D134, D149, D175, D179, D182, D191; bare `fn drop()` (D155); `==` on every type (D175, D177, D178, D212); every type a Map key (D179, D214); `view(children:)` by name (D147); "no `self`" (D196, D208). Eight slices in the freeze. **Slice 1 shipped** (2026-10-18): `trait` with required methods and parents, `Self`, `conform`; `Resource` in std/core replaces finding `drop` by name (a type's own `fn drop()` is an ordinary method), `File` / `Socket` / `Db` conform. Gates: `traits`, `fail/conform_*`, `fail/trait_cycle`, `fail/trait_member_body`, `fail/resource_union`. |

## 2026-10-18 — Shell and session removed (Joshua: "drop the shell feature entirely")

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D227 | The shell and the session are removed | Joshua, 2026-10-18: "Let's drop the shell feature entirely. This is scope creep. Maybe some day we'll write a shell in thoxa." `thoxa` is a compiler CLI only: `run`, `build`, `check`, `emit-c`, `runtime`, `lsp`, `version`, and `thoxa path` for `thoxa run path`. Every program has `fn main`; a file without one is a library module. A statement at the top level of any file is a compile error (`THX005`, `a statement cannot be at the top level`, help ``move the statement into `fn main` ``); an entry file without `main` reports ``program requires `fn main` `` (`THX006`). Bare `thoxa` prints usage and exits 2. | **Shipped.** Why: scope creep away from the core language (CURRENT's focus since 2026-10-16). The session carried a second C backend (libtcc), a vendored LGPL compiler, a line editor, and a command dialect, and Oath's analysis (its `SHELL.md`, 2026-10-16) showed that a usable login shell would also need POSIX `sh -c`, job control, and a static-musl libc story. A future shell would be a Thoxa program, not a compiler feature. Removed: bare `thoxa` REPL, `thoxa repl`, `thoxa -c`, `-l` / `--login`, argv0 `-thoxa`; session scripts (a `.thx` without `fn main`); command sugar, pipelines, redirections; the `Shell` type (`std/shell.thx`) and `std/prompt.thx`; the rc file (`~/.config/thoxa/shell.thx`) and the `fn prompt()` / `fn path()` hooks; the rustyline line editor and history; the libtcc session-step backend (base images, session slots, `thoxa_session_call` crash recovery); `vendor/tinycc`, `vendor/TINYCC_PIN`, and the `rustyline` dependency; runtime `thoxa_spawn_wait`, `thoxa_spawn_wait_io`, `thoxa_pipeline`, `thoxa_pipe`, `ThoxaShellState`, `thoxa_session_slot*`; `cargo make shell`; `extras/shell`; the `session_*` conformance tests; the thoxa.dev `shell` sample; the variables `THOXA_SHELL_RC`, `THOXA_NORC`, `THOXA_HISTFILE`, `THOXA_STATUS`, `THOXA_REPL_KEEP_C`, `THOXA_TCCDIR`, `THOXA_C_INCLUDE`, `THOXA_TCC_LIBPATHS`, `THOXA_TOOLCHAIN_PATH` (`THOXA_REPL_PROFILE` is now `THOXA_PROFILE`: MIR lowering times on stderr). Kept: `std/env`, `std/path`, `std/ansi`, `std/git` (logging, the sites, and `stdlib` use them), `thoxa_local_hms` / `thoxa_hostname`, and D97's `std/` root (`THOXA_ROOT` / `THOXA_STD`). Building needs a Rust toolchain and a C compiler (`make` was for TinyCC). **Recovery revision: `eefdac4`**, the last commit with the shell: `git show eefdac4:<path>`, or `git checkout eefdac4 -- <paths>`. The main paths there: `crates/compiler/src/{repl,session,command,line_edit,tcc}.rs`; the session parts of `driver.rs`, `cgen.rs`, `mir.rs`, `parser.rs`, `lsp/analysis.rs`; the TinyCC build in `crates/compiler/build.rs`; spawn / pipeline / `thoxa_session_call` in `runtime/src/thoxa_rt.c`; `std/shell.thx`, `std/prompt.thx`; `extras/shell/`; `vendor/tinycc/`; `tests/conformance/session_*`; `crates/make` (`cmd_shell`); `docs/handbook/session.md`; `docs/specs/2026-07-24-thoxa-session-repl.md`; `docs/specs/2026-08-05-session-libtcc.md`. Supersedes D86, D88–D90, D92–D97 (D97 only for its session modules), D100–D105, D156, D160, D169, D188, D209. Oath keeps `/bin/sh` as `home`'s login shell. Gates: `fail/top_level_statement`, `fail/program_without_main`; `cargo make test` (170 programs, 59 rejections, 8 site samples, 215 unit tests). |

## 2026-10-18 — Language gap pass (Joshua: "cover all these -- keep going, pick your order"): D207–D226

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D226 | Unicode normalization is the opt-in module `std/normalize` | Joshua, 2026-10-18 ("Opt-in std module"): `import { nfc, nfd, nfkc, nfkd } from "std/normalize"`, written in Thoxa (decompose with full mappings, canonical ordering by combining class, canonical composition by UAX #15's algorithm; Hangul by formula). `cargo make unicode` also writes `std/normalize-data.thx` from UnicodeData and DerivedNormalizationProps (403 class ranges, 2081 canonical / 3849 compatibility decompositions, 961 compositions). A module's tables go into every program that loads it, and std/string is in every program, so normalization is not a String method: only programs that import it carry the tables. | **Shipped.** Gates: `normalize` (LeakSanitizer); checked by hand against all of Unicode 17's `NormalizationTest.txt` (20 034 lines × 20 checks, and the 1 094 978 unlisted code points invariant): 0 failures. Cost for an importing program: about 7.7k lines of C (+0.3 s build). The UTF-8 helpers are copied from std/string, which keeps its private. |
| D225 | An LSP rename never makes a pun | The resolver also records each bare argument passed by position (`Resolution.bare_args`) with the callee's labelled parameters the call leaves unlabelled. Renaming its binding to one of those names is refused (`rename_makes_pun`), since the argument would then fill that parameter (D202). A call that labels the parameter keeps the bare one positional, so it does not count. | **Shipped.** Gates: `lsp::index::tests::rename_refuses_making_a_pun`, `resolver::tests::generated_route_calls_do_not_underflow_spans`. Closes the case D210 left open. Calls the route desugar generates have no source spans; recording their bare arguments underflowed a span (a debug-build panic in `thoxa check` of any file with route clauses, `samples/server`), so generated arguments are skipped. |
| D224 | An implicit `Int64` → `Float64` must be exact | Joshua, 2026-10-18 ("Panic when inexact"): where an `Int64` or `UInt64` becomes a `Float64` implicitly (an argument, a field, a return, `x * 1.5`), the value must round-trip, else the program panics, `9007199254740993 does not fit in Float64 exactly` (D167's rule for lossy conversions; `thoxa_f64_from_i64` / `_u64`). A literal that would round is a compile error with a hint. `Float64(rounding: x)` rounds on purpose (as `T(truncating: x)` keeps low bits). Following D191 (the result type says whether a conversion can fail) and D167 (`as` is how narrowing is handled), `x as Float64` for an `Int64` / `UInt64` is now `Trial<Float64, Array<Issue>>` (generated `__asFloat_*` readers over `__float64Exact`); a literal still gives a `Float64` (`1 as Float64`). `Byte`, `Int32`, `UInt32` convert exactly and stay infallible. | **Shipped.** Gates: `float_exact`, `fail/float_inexact`, `conversions_as` (now shows a checked `as Float64`); `soft_keywords` changed `where` to an `Int32` so its `as Float64` stays a `Float64`. Cost: a compare per implicit conversion. Before, values past 2^53 rounded silently. |
| D223 | A `main` that returns nothing exits 0 | Joshua, 2026-10-18 ("Exit 0"): an unannotated `main` is inferred like any function (D129) instead of being `Int64`: an `Int64` tail is the exit status (D173 unchanged), a statement tail is Void and the generated C `main` returns 0, and any other type is an error (``main` ends with a String value: it returns its exit status (`Int64`) or nothing``). A `main` that returns a status on one path and nothing on another must say `-> Int64`. `fn main() -> Int64` is unchanged. | **Shipped.** Gates: `main_void`, `fail/main_returns_text`; every `-> Int64` test still passes. Before, `fn main() { info("hi") }` was rejected with `expected Int64, found Void` at its last line. The handbook's first program drops its trailing `0`. |
| D222 | A `where` range means what a `for` range means | Joshua, 2026-10-18 ("Half-open, like for"): `where lo..hi` holds for `lo <= v < hi`, `lo..=hi` for `lo <= v <= hi`, `..hi` for `v < hi`, `..=hi` for `v <= hi`, `lo..` for `v >= lo`; on `String` / `Array` they test `.count`. `WherePred::Range` carries `inclusive`. A failed check prints the range as written (`expected integer 0..24`, `0.0..1.0` for floats), and a literal that cannot inhabit the type says so at compile time: `24 does not fit in Hour: expected integer 0..24` (was "expected this refinement, got 24"). | **Shipped.** Gates: `where_ranges` (LeakSanitizer), `fail/where_range_literal`. Before, `where 1..64` included 64, unlike `for 1..64`, and its message printed `1..64` even for `1..=64`. Nothing in std, tests, examples, or sites used a `..` range in `where`; the handbook's `String where 1..64` example became `1..=64`. |
| D221 | Multi-clause bodies infer in the same rounds | An unannotated multi-clause function or method is an `InferTarget::Multi` candidate in D174's rounds (its call order taken from all its clause bodies), probed by `probe_multi_return` with the round's probe context: a clause that calls a candidate not inferred yet waits for the next round, and annotated clauses contribute their annotation. The separate pre-pass (`probe_multi_clause_return`) is gone. | **Shipped.** Gate: `return_infer_multi_calls` (LeakSanitizer). Before, multi-clause bodies were probed before every other candidate, so `fn label(n!: 0) { "zero {twice(0)}" }` saw an unannotated `twice` as Void ("cannot interpolate Void into String"). |
| D220 | Multi-clause methods write their fields | `lower_multi_method` binds fields with `bind_method_fields`, the same code as a single method: each is a writable local with write-back through `_self` (C-layout fields only when `mut`), and sibling calls resolve on the receiver. The method is mutating when a clause writes (D163). | **Shipped.** Gate: `multi_clause_method_writes` (LeakSanitizer). Found during the gap pass: before, a multi-clause method's fields were read-only (`cannot assign to immutable binding count`), unlike a single method's. |
| D219 | `UInt64` arithmetic stays in `UInt64` | When an operand of `+ - * / %`, `& \| ^`, a comparison, or the left of `<< >>` is a `UInt64`, and the other is a `UInt64`, a `UInt32` or `Byte` (widened), or a non-negative literal, the operation happens in `UInt64`: `+ - *` wrap modulo 2^64 (as Int64 wraps, D157), `/ %` divide unsigned and panic on zero (`thoxa_uint_div` / `_rem`), comparisons are unsigned, `>>` is logical, a count of 64 or more shifts every bit out and a negative one panics (`thoxa_uint_shl` / `_shr`, D161's rule), and `~` keeps 64 bits. `-x` on a `UInt64` is a compile error (`0 - x` wraps), and so is mixing a `UInt64` with a signed value: no type holds both (Swift's and Rust's rule), so the program says which conversion it wants (`UInt64(truncating: x)`, `x as? UInt64`). Other integer types still compute in Int64 (D21). | **Shipped.** Gates: `uint64_arith` (LeakSanitizer), `fail/uint64_mixed_signed`. Before, a `UInt64` operand was narrowed to Int64 first, so one above Int64 max panicked in `+` or `<` (D167's note). Nothing in std, the examples, or the sites did UInt64 arithmetic (it appears at C boundaries), so the mixing error broke nothing. Still: a `UInt64` above Int64 max cannot be written to JSON (its numbers are Int64). |
| D218 | A route body parameter may be an `Array<T>` | `POST "/batch" (orders: Array<CreateOrder>)` (or `Trial<Array<T>, Array<Issue>>`) reads a top-level JSON array: the body reader is `readJson(body)? as Array<T>`, so issues carry element paths (`0.quantity`). A form or a query answers `Array<T> is read from JSON only`, as a JSON-only product does. The codec pass fills body readers before it scans `as` targets, so the reader's own `as` gets its generated element reader. | **Shipped.** Gate: `route_body_arrays` (LeakSanitizer). Before, a parameter with type arguments was not a body parameter (D191 §5: named types only). |
| D217 | Generic types declare `from` clauses | A generic type's `from` clause returns the type with its own parameters (`Pair<T>`, or `Trial<Pair<T>, E>`). The conversion desugar checks it but makes no function; each instance makes one: `instantiate_product_mono` queues the clause as a conversion job, and `lower_package` registers and lowers `__conversion_{instance}_N` (substituted with `subst_method`) in the type's module, the name `as` already looks up. An `as` target is now scanned for instances (`Cast` in `collect_expr_type_apps`), and a clause's signature types are queued like a method's. | **Shipped.** Gates: `conversions_from_generic` (LeakSanitizer), `fail/conversion_generic_return`. Before, "generic type `Pair` cannot declare `from` clauses". As with generic methods, a clause body is checked only for instances the program uses. The handbook also lost "methods of a generic type must annotate a non-Void return type", untrue since D211. |
| D216 | `toLower` applies final sigma | `Σ` (U+03A3) lowercases to `ς` (U+03C2) when SpecialCasing's Final_Sigma holds: before it, past case-ignorable scalars, is a cased one, and after it, past case-ignorable scalars, is none (Rust's and ICU's reading). `std/string`'s `mapCase` takes a `finalSigma` flag (`toLower` only; `toUpper` and case folding are unchanged, and `Σ`, `σ`, `ς` all fold to `σ`). `cargo make unicode` now also reads DerivedCoreProperties.txt and writes `casedRanges` (158) and `caseIgnorableRanges` (464) as `first, last` pairs; regenerated from the Unicode 17.0.0 UCD, which reproduces the existing tables byte for byte. | **Shipped.** Gates: `string_final_sigma`; `string_methods` now expects `σίσυφος` (it recorded `σίσυφοσ`). Cost: `std/logging`'s `parseLevel` calls `toLower`, so every program carries `endsWord` and the two tables: hello world's C compiles in about 0.21 s instead of 0.18 s at `-O2` (about 5 ms of it the tables, the rest the code). Accepted per API-first; merging the helpers into one function took it from +33 ms to +26 ms. |
| D215 | `\u{H…}` string escape | One to six hex digits in braces after `\u` write that Unicode scalar (Rust's and Swift's form). A surrogate (`D800`–`DFFF`), a value above `10FFFF`, no digits, more than six, or a missing `}` is a compile error at the escape. A `{` / `}` written this way is text (pushed as `{{` / `}}`), not a hole. The Tree-sitter grammar's `escape_sequence` reads it whole (regenerated with tree-sitter 0.25.10, the version that made the checked-in parser). | **Shipped.** Gates: `string_unicode_escape`, `fail/string_unicode_escape`, the grammar corpus case "Unicode escape is one escape, not a hole". Before, `\u` was "unknown escape \u", so a character outside ASCII had to be typed literally (D159 kept `\xHH` to ASCII). |
| D214 | Map keys may hold arrays and Float64, and be Float64 | A key type may hold `Array<T>` of key-able `T` (hashed as its count, then each element in order; equal when counts and elements are) and `Float64`; `Map<Float64, V>` is allowed. A `Float64` in a key compares **SameValueZero**, as JavaScript's `Map`: any NaN matches NaN and `-0.0` matches `0.0`, so a key can always be found again (IEEE `==` would make a NaN key unreachable and `m[nan] = …` add a new entry each time). Key `==` is otherwise D175's. A `{1.5: "a"}` literal parses as a map. | **Shipped.** Gates: `map_keys_arrays_floats` (LeakSanitizer; also clean under ASan); `fail/map_key_float` became `fail/map_key_map`. cgen emits `thoxa_key_hash_arr_*` / `thoxa_key_eq_arr_*` per element type inside keys; a top-level Float64 key travels as `thoxa_f64_key_bits` (canonical bits, so the runtime's i64 bit equality is SameValueZero). Still refused: a Map inside a key (no map hash), an `Array` as the key itself, Node, `drop` and C-layout types. Proposed difference to note: `Reading(celsius: nan) == Reading(celsius: nan)` is `false`, but the two are one key. |
| D213 | An expected `Map<K, V>` types a map literal | Where a `Map<K, V>` is expected (an annotation, a parameter, a field, a return), each key is lowered against `K` and each value against `V`, then coerced (`coerce_element`, as D184 does for array elements): a union member widens, an integer becomes `Float64`, and an `[]`, `Map()`, or nested `{…}` value takes the type. Without an expected type a literal still takes its first entry's types. | **Shipped.** Gate: `map_literal_expect` (LeakSanitizer). Before, `Sheet(cells: {"a": [1, 2], "b": []})` was "heterogeneous map values: expected Array { elem: Int64 }, found Array { elem: Void }", `{"tea": 4, "milk": 2.5}` could not be a `Map<String, Float64>`, and a map of union values needed every member to be the first one's type. |
| D212 | `==` on maps, and on types that contain themselves | Two maps are equal when they have the same count and every key of the left one is in the right one with an equal value; insertion order does not matter (a map is a set of entries, and D181's order is for iteration). `lower_map_eq` walks the left map's keys and looks each up in both maps; the values are borrowed slots (`MapGet` copies no reference; `declare_borrow` places are never dropped), and the walk stops at the first missing key or unequal value. Equality stays expanded inline in MIR, except that a type met again while its own comparison is being expanded (`Tree` through `Array<Tree>`, `Json` through `JArr` / `JObj`) calls `__eq_{type}(&left, &right)`, a generated function taking both sides as `mut` parameters (by address, nothing moved), lowered after the other bodies until no more are requested. A session step that needs a new one rebuilds the base image. | **Shipped.** Gates: `map_eq` (LeakSanitizer; also clean under ASan), `fail/map_eq_value`; `fail/product_eq_map_field` and `fail/union_eq_map_arm` became `fail/product_eq_node_field` and `fail/union_eq_node_arm`. Before, any product, union, or array holding a map had no `==`, so `Json` values could not be compared. Found on the way: a map literal does not pass its expected value type down, so `[]` in `{"a": [1], "b": []}` is untyped (D213). |
| D211 | Multi-clause methods and generic types' methods infer their return types | A multi-clause method without `->` on some clause is probed like a multi-clause free function (D131 + D129: each clause's exits, joined), with the type's fields in scope. A generic type's methods are inferred per instance: each mono job's `subst_method` body is a candidate in D174's rounds, keyed `Shelf_String__first`. Inside a method, a bare call is also ordered as a possible sibling (D208). Multi-clause methods resolve sibling calls too. | **Shipped.** Gate: `return_infer_generic_multi` (LeakSanitizer). Before, both were Void, so `words.size() + 1` was a type error. Still: a multi-clause body is probed before the rounds, so one calling an unannotated function sees Void (as multi-clause free functions always have). Also: `cargo make test` runs `repl::tests` single-threaded after the other unit tests; they capture fd 1, where libtest prints progress, and failed when a dot landed in a capture. |
| D210 | An LSP rename keeps punned calls compiling | The resolver links labels and puns after every body is walked (a callee's parameters are bound with its body, so a call above its callee had none), for functions, methods (by member), and constructors (labels are fields). Each bare argument that fills a labelled parameter is a `Pun` (argument span, name span, label, parameter, `mut`). `rename_edits`: renaming the argument's binding writes `label: new` (`label: mut new`), renaming the parameter writes `new: name`; a recursive call that passes the parameter itself stays a pun. A `mut name` argument's reference is the name, so a rename edits it. | **Shipped.** Gates: `lsp::index::tests::rename_punned_local_writes_the_label`, `rename_punned_mut_and_field`, `rename_recursive_pun_stays_a_pun` (each checks the renamed text compiles). Before, either rename left a bare name that no longer named the parameter, so the call stopped compiling; renaming a method parameter or a field left its labels, and a `mut total` argument was skipped. Not handled: a rename *into* a parameter's name at a call that passes another value positionally makes a new pun. |
| D209 | `thoxa -c` splits its chunk into declarations and statements | `run_one_shot` goes through `apply_session_source`, the rc file's path: top-level forms that start with `import` / `fn` / `type` / `extern` / `export` / `let` / `mut` are declarations, the others are joined and run once with them in scope. | **Shipped.** Gate: `repl::tests::one_shot_import_then_statement_runs`. Before, the chunk went to `run_chunk` whole, and a first line starting with `import` made all of it one declaration: `thoxa -c 'import …\ninfo("hi")'` compiled the import and ran nothing, exit 0. **Superseded by D227.** |
| D208 | A method calls its siblings by bare name | Inside a method of `T`, a bare call `name(args)` naming one of `T`'s methods is that method on the same receiver (`_self` is passed through). It wins over a module-level function of the same name, as a field does over a module name (the resolver already scoped members this way). The callee may write fields through `_self`, so the caller loads its field locals again after the call. A method that calls a writing sibling on `_self` is itself mutating (D163, a fixpoint over the calls). A private method is callable from `T`'s own methods on any `T` (`other.double()`). Only calls: a bare `double` is not a function value. | **Shipped.** Gates: `method_sibling_calls` (LeakSanitizer), `fail/mutating_sibling_let`, `resolver::tests::sibling_method_call_binds_to_method`. Before, there was no way to call one (no `self`), so a private method could not be called at all and the handbook said to move shared code to a module function. No method in std, the tests, examples, or sites calls a free function named like a sibling, so nothing changed meaning. |
| D207 | A field indexes as a local does | `row.cells["name"]` is a `May<V>` and `row.grid[1][0]` an element, through any chain of fields (`sheet.row.cells[k]`): the field is loaded, then indexed by `lower_index_value`, the code a local `Array` / `Map` uses. A fixed-array field still indexes in place. | **Shipped.** Gate: `field_index` (LeakSanitizer). Before, any field but an `Array` or fixed array was `cannot index non-array field type Map …`; `.get(key)` worked. Writes are unchanged: from outside its type a field is read-only (D180). |

## 2026-10-18 — Label punning, built-in labels (Joshua: "do label punning next", "do the builtin label checks next", "Hit all three proposals"): D202–D206

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D206 | A method may pass its own field to a `mut` parameter | Inside a method, a field is a `mut` binding (D107's local with write-back), so `addLine(mut lines, "x")` passes the local's address; after the call, `emit_call_result` stores it back through `_self` (as `=` and collection writes do). Puns with D202: `tally(4, mut total)` fills a labelled `mut total`. Outside its methods a field is not a binding, so `mut report.lines` is still not an argument. | **Shipped.** Gate: `mut_param_fields` (LeakSanitizer; also clean under ASan), including a field passed `mut` to a call whose argument is another call passing a field `mut`. Replaces D199's "field ... cannot be passed `mut`" error. Cost: the local shares the field's object until the write-back, so a heap field's first write in the callee copies it (copy on write); acceptable per API-first. `resolve_call_args_full` records the fields passed `mut` in `LowerCtx::mut_field_args` for the call it resolves; nested calls in its arguments resolve and emit first, so the list belongs to the next `emit_call_result`. |
| D205 | `insert(item, at: index)` replaces `insertAt(index, item)` | Array `items.insert(item, at: index)` (in place) and Bytes `bytes.insert(byte, at: index)` (a new Bytes): the value is the subject, so it is positional, and the index is labelled, per D200's guidance. `at: items.count` appends. `removeAt(index)` stays (its index is the subject). Panics read `Array.insert out of range` / `Bytes.insert out of range`. | **Shipped.** Gates: `splice`, `array_user`, `collection_element_writes`, `builtin_labels`, `fail/array_insert_label`. Proposed with D203: `insertAt(2, "x")` did not say which argument is the index. Migration: 16 calls (tests, `examples/kilo`, the landing C sample); kilo's `chars.insertAt(at, byte)` puns to `chars.insert(byte, at)`. |
| D204 | `Element`'s fields are `attributes` and `children`, and default to empty | `Element(tag: "p", attributes: […], children: […])`; `Element(tag: "br")` has neither. `Text`'s field is `text`, `Fragment`'s `children` (the names a named pattern or a hover shows; patterns bind by position). | **Shipped.** Gate: `jsx` (`Element(tag: "br")`), `fail/call_label_unknown`. Since D203 they are labels a call writes, so the full-word rule applies; `children` also matches a component's `view(children:)`. Migration: `jsx`, `builtin_labels`, the landing site's `markdown.thx`. |
| D203 | Built-in methods and constructors follow D200 | Every call is now checked against marks. Built-ins' parameters are positional, as if declared `!`: Array `append(item)`, `insertAt(at, item)`, `removeAt(at)`; Map `has` / `get` / `remove(key)`, `set(key, value)`; Bytes `get(index)`, `set(index, byte)`, `slice(start, end)`, `equals(other)`, `insertAt(at, byte)`, `removeAt(at)`; Buffer `append(byte)`, `appendBytes(bytes)`; String `byteAt(index)`, `byteSlice(start, end)`; `Bytes(count)`, `Bytes.fromString(text)`, `Text(text)`, `Fragment(kids)`. Options are labelled: `from:` on `byteIndexOf` and `Bytes.find` (both default 0, as in std/string), `Buffer(capacity:)`, `Element(tag:, attrs:, kids:)` (like `Attr(name:, value:)`, already a checked product). So built-ins pun (D202): `Element(tag, attrs, kids)`. A call through a function value takes no labels (a function type has no parameter names): a label there is an error. A label that names no parameter is reported at the label, listing the parameters (it was "missing argument" for the first unfilled one). | **Shipped.** Gates: `builtin_labels`, `fail/builtin_label_positional`, `fail/builtin_label_missing`, `fail/call_label_unknown`, `fail/call_function_value_label`. Parameter names became full words, since they now appear in errors (`i` → `index`, `b` → `byte`, `n` → `count`, `s` → `text`). Migration: 10 positional `from` arguments (std/string, std/http, two tests) are `from: …`; 27 `Text(s: …)` / `Fragment(kids: …)` calls (the landing site, `jsx`) are `Text(…)` / `Fragment(…)`. `Bytes.find`'s two hand-written arities became one formal with a default. Generated readers (`T.read`) check their marks too. |
| D202 | A bare name fills the labelled parameter it names | An unlabelled argument that is a bare name (or `mut name`) where the callee declares a labelled (non-`!`) parameter of that name is that labelled argument: `connect(host, deadline)` is `connect(host, deadline: deadline)`, `Point(x, y)` is `Point(x: x, y: y)`, `tally(3, mut total)` fills a labelled `mut total`. It counts as labelled for D194's order, so it goes after the positional arguments. A call that also labels the name keeps the bare one positional (`scale(factor, factor: 3)` passes `factor` as `value!`). Only calls checked under D200 pun: user functions, methods, constructors, std's `extend` methods, `-->` starts; not built-in methods or calls through function values. | **Shipped.** Gates: `call_label_pun`, `fail/call_pun_needs_positional`, `fail/call_pun_before_positional`, `lsp::crossmodule::tests::hover_arg_punned_name`. Proposed after D200 left many `deadline: deadline` calls; 93 of them in std, examples, the sites, and the shell now pun (tests keep the long form they test). Considered: an explicit marker (Gleam's `f(deadline:)`, OCaml's `~deadline`), which never needs the declaration to read; rejected because D200 already requires the declaration to read a call, and the name match is the signal (Rust's `Point { x, y }`). The ambiguous case (a binding named like a labelled parameter, meant for a positional one) is an error, not a silent choice, unless the positional parameter has a default and the types agree. Also found: `-->` starts never checked D200's labels; they do now. |

## 2026-10-17 — Language follow-ups (Joshua: "Hit all 5 in order"): D197–D201

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D201 | A match arm may be one assignment after `=>` | `Pattern => target = value` (a name, `name.field`, or `name[index]`, as an assignment statement allows) is a one-assignment block; the arm is Void. | **Shipped.** Gate: `match_arm_assign`. Before, `Has(at) => total = total + at` was a parse error and needed `Has(at) { total = total + at }`. |
| D200 | A call follows its declaration: `!` by position, the rest by label | A parameter marked `!` is passed by position and may not be named; any other parameter is passed as `name: value` (D194 order: positional first). Constructors follow their fields' marks (`Has<T>(value!: T)` is `Has(3)`; `Point(x: Int64)` is `Point(x: 1)`). Built-in methods and calls through function values are not checked. Code the compiler generates passes its own parameters by position (route handlers, `from` clauses, codec readers are `!`). | **Shipped.** Gates: `fail/call_label_missing`, `fail/call_positional_named`; the whole suite. Before, `!` was documentation only, so `s.contains(x, true)` passed for `ignoreCase: true`. Migration: std/fallible's `Has` / `Pass` / `Fail`, std/json's `J*` and std/sqlite's `Sql*` value wrappers became `!`; about 130 declarations across std, tests, examples, and the sites marked their subject parameter `!`; options (`deadline:`, `headOnly:`, `pad:`, `parent:` for the session's `path` hook) are labelled at the call. Many calls now read `f(deadline: deadline)`: label punning (`f(deadline)` for a same-named labelled parameter) is the natural next step. |
| D199 | `mut` parameters | `fn f(mut name: T)` takes one of the caller's bindings: the call writes `f(mut binding)` (or `label: mut binding`) with a `mut` binding of exactly type `T`, and the function reads and writes it in place. Passed by address: the C parameter is `T *`, the body's place is `(*name)`, the callee never drops it, and the caller passes `&slot`. No default; not on constructor, `extern`, or `from` parameters, multi-clause functions, `-->` tasks, or function values. A method field cannot be passed (`field ... cannot be passed mut`). | **Shipped.** Gates: `mut_params`, `fail/mut_param_needs_mut`, `fail/mut_param_immutable`. Found writing std/string (D196): a helper could not append to its caller's `Buffer` without a copy; `appendScalar(mut out, code)` now does, instead of a `Bytes` per scalar. |
| D198 | Declaration keywords are names everywhere else | `as`, `export`, `extend`, `extern`, `from`, `import`, `in`, `type`, `where` are keywords only where they introduce a declaration or clause; anywhere a name goes (argument label, parameter, field, binding, member, map-literal key, a reference in an expression) the parser takes them as names. `and break continue else false fn for if let loop match mut not or return true while` stay reserved everywhere. | **Shipped.** Gate: `soft_keywords`. Found writing std/string (D196): `byteIndexOf(needle, from: 3)` was a syntax error, so the parameter was named `start`; it is `from` again. Also allows `Token(type: …)`, the usual JSON tag field. The tree-sitter grammar still colours them as keywords in name positions. |
| D197 | A program carries only the functions it reaches | After lowering, `mir::retain_reachable` keeps the functions reachable from `main`, the module initializers, and every `Type__drop` (the runtime calls destructors), following calls, `-->` starts, and function values; `compile_to_c` emits only those. Session images keep everything (a later step may call anything). | **Shipped.** Gate: `driver::tests::unreachable_functions_are_not_emitted`; the suite. Hello world: 9,912 lines of C and 0.60 s for `cc -O2` with D196, 2,824 lines and 0.19 s now (4,673 and 0.33 s before D196). Module globals are all kept and initialized (the Unicode tables among them). |

## 2026-10-17 — Strings

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D196 | std writes built-in types' methods in Thoxa (`extend`) | Freeze [`specs/2026-10-17-std-extend.md`](specs/2026-10-17-std-extend.md). `extend Type as receiver { [export] fn … }` declares methods on a built-in type (String, Bytes, Buffer, Int64, Float64, Bool, concrete Array / Map), in a std module only; each becomes a free function `__extend_{Type}_{method}` taking the receiver first, which a method call finds before the runtime primitives. `std/string` (String's methods, `Split`, `parseUInt`) is in every package (`include_core_modules`) without adding names. The runtime keeps the String representation and three byte primitives: `byteAt`, `byteSlice`, `byteIndexOf`. `cargo make unicode` writes `std/unicode.thx`. An all-integer-constant array literal lowers to one C array (`RValue::IntArray`). | **Shipped.** Joshua, 2026-10-17: "Eventually we want thoxa to be self hosting, so we should be writing our code in thoxa when possible", and "write in thoxa unless you can't. If we need to add a feature to write something in thoxa, we can look into that"; performance only where fundamental, for now. Gates: `string_methods`, `string_scalars`, `stdlib`, `fail/extend_outside_std`, `string_methods_without_imports` (check / LSP packages with no imports also get std/string). Cost: hello world went from 4,673 to 9,912 lines of C (0.33 s to 0.60 s for `cc -O2`): every loaded module is emitted whole. |
| D195 | String methods; a String is Unicode text counted in scalars | Freeze [`specs/2026-10-17-string-methods.md`](specs/2026-10-17-string-methods.md). String's methods (first C calls behind a compiler table; Thoxa in `std/string` since **D196**): `contains` / `startsWith` / `endsWith` / `equals` with `ignoreCase: Bool = false`, `isEmpty`, `isAscii`, `toLower`, `toUpper`, `trim` / `trimStart` / `trimEnd`, `removePrefix` / `removeSuffix`, `replace` / `replaceFirst`, `repeat`, `split`, `lines`, `indexOf` / `lastIndexOf` (`May<Int64>`), `substring`, `truncate`, `padLeft` / `padRight`, `reverse`; `join` on `Array<String>`. A refined String takes them. `ignoreCase` compares full Unicode case folds; `toLower` / `toUpper` use the full default mappings; `trim` removes Unicode White_Space (tables in `std/unicode.thx`, Unicode 17.0.0, from `cargo make unicode`). `.count` and positions count Unicode scalars, `.byteCount` bytes. Every String is valid UTF-8: `thoxa_string_new` replaces each maximal ill-formed subsequence with U+FFFD. `std/str` was replaced by `std/string` (D196). | **Shipped** in two slices (§1–§3, then §4). Gates: `string_methods`, `string_scalars`, `string_escapes`, `stdlib`. Joshua, 2026-10-17, on `containsIgnoreCaseAscii` in the cafe's models: "could we not attach such to the string itself … and why are we checking ascii here? we're trying to be end-to-end utf8". On the unit: scalars, but "thoxa should be performant and that O(n) bothers me a little" — the count is cached in the String (O(1)) and an all-ASCII String indexes in O(1); a position in other text walks the UTF-8, O(n). Revisit if programs show it (freeze §4). Gaps: final sigma, normalization, `\u{…}` escapes, non-UTF-8 C strings cannot round-trip. |
| D194 | Positional arguments first, then named | A call may pass leading positional arguments and then named ones (`format("hi", width: 40)`), the oracle SPEC's rule (§4). Positional arguments fill the first parameters in order; a named argument may not name one of them; a positional argument after a named one is an error. | **Shipped.** Gates: `call_mixed_args`, `fail/call_positional_after_named`, `fail/call_named_twice`. Before, any mix was "cannot mix positional and named arguments", so `s.contains(x, ignoreCase: true)` (D195) and `sendAll(data, deadline: …)` could not be written. `!` is still not checked at calls. |

## 2026-10-17 — Conversions

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D193 | Route clauses import only what they write | The generated `route` method and handlers name `std/http`'s `Request`, `Response`, `ok`, `notFound`, and `badRequest` by the module's own name when it defines or imports one, and by the mangled name (`std_http_notFound`) otherwise. Hover shows a mangled `std` type by its source name. | **Shipped.** Joshua, 2026-10-17: "I'm looking at routes.thx and theres a lot of unused imports". Before, a module with route clauses had to import `notFound` and `badRequest` (and `ok`) for code it never saw; the cafe, the three sites, and the server sample now import only what they write. Gates: `route_desugar::tests` (mangled without imports, local with), existing route conformance unchanged. |
| D192 | `return` and `=` convert to the expected type | A `return` value is lowered expecting the function's return type and an assignment's value expecting the binding's type, then converted as an annotated `let` converts (`coerce_to_expected`: member → union, literal → literal / refined type, numeric widening). | **Shipped.** Gate: `return_assign_convert`. Before, `return Circle(…)` in a function returning `Shape` was "return type mismatch" (a block's last value already converted), `return { ok: false }` from a `Json` function failed, and `shape = square` / `ratio = count` on a `mut Shape` / `mut Float64` were "cannot reassign". Found while planning D191 §4, which adds encoding into `Json` to the same conversion. |
| D191 | Conversions: `as`, `as?`, `from` clauses | **Shipped.** Freeze [`specs/2026-10-17-conversions.md`](specs/2026-10-17-conversions.md). At most one conversion per pair of types, generated or declared by the target. §1 `x as T` converts; its result is `T` when the conversion cannot fail (member → union, refined → base, widening, encodable → `Json`) and `Trial<T, Array<Issue>>` when it can (base → refined, `String` → literal union, checked narrowing, `Json` → any readable type, top-level arrays and maps included). Text is parsed, not converted: `T.read` / `T.readForm` stay. §2 `x as? T` is exactly `(x as T)?`; on an infallible conversion it is an error. §3 `from(source: A) -> Self` or `-> Trial<Self, E>` clauses in a type body declare conversions into it, replace a generated one (a `from(json: Json)` clause is the type's reader), and are exported like methods. §4 an encodable value converts implicitly where a `Json` is expected; decoding is never implicit. §5 a route clause parameter of a readable type binds the body by method and `Content-Type` (query for GET, JSON, form; else 415); `T` answers a failed read with a 400, `Trial<T, Array<Issue>>` lets the clause handle it. §6 `T.from` and `T.fromJson` are removed in favour of `as`. | **§1–§3 shipped** (gates `conversions_as`, `conversions_as_try`, `conversions_from`, `conversions_from_json`, `fail/conversion_missing`, `fail/conversion_text`, `fail/conversion_try_infallible`, `fail/conversion_from_shape`); §4 shipped (gates `json_implicit`, `match_expected_type`, `fail/json_implicit_decode`); §5 shipped (gates `route_body_params`, `fail/route_body_two`; `std/http` gains `bodyKind`, `unsupportedMediaType`, `issuesResponse`; a type that a form cannot hold answers a form or query with `T is read from JSON only`); §6 shipped (gate `fail/conversion_old_names`; generated readers call `T.__from` / `T.__fromJson`). §4 as built: a `match` now takes its expected type like an `if` (arms of different union members into an expected union failed before), and a multi-clause arm for a smaller union than the domain remaps its value (it aliased the domain's layout and emitted invalid C for `Line` / `Json` clauses). §3 as built: a `from(json: Json)` clause's issues are moved under the reading field's path (`std/json` `underPath`), so custom readers compose like generated ones. To give checked narrowing a `Trial<Byte, …>`, generic product fields now accept every integer type. From a 2026-10-17 discussion with Joshua ("`input as Order` perhaps … From/To traits from rust"; "would not `json as? Order` simply be a kind of sugar over `(json as Order)?`"). Rust's split is the model: `From` / `TryFrom` convert values, serde parses text. Swift's `as?` (yielding an optional) was set aside because Thoxa's `?` already means "or return". Picks up D182's parked custom decoding (§3), top-level array reads (§1), and route body binding (§5). |

## 2026-10-17 — C interop

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D190 | Every `extern fn` gets its own C prototype | cgen declares each extern under a private name bound to its symbol with an asm label (`int32_t thoxa_c_uname_<hash>(Thoxa_SystemName*) __asm__("uname");`), typed as the call passes its arguments: `const char *` for `String`, `void *` for `Bytes`, `T *` for `Address<T>`, the scalar types as they are. Calls use the private name. The name carries a hash of the C signature, so one symbol declared twice with different types (`open` with and without a mode) gets two prototypes. The system headers stay included but are no longer what declares an extern. | **Shipped.** Gate: `extern_prototypes` (`uname` with a typed struct pointer, `puts` and `fflush` from `stdio.h`, which the generated C does not include); every existing extern program and the libtcc session steps compile through the same prototypes. Before, an extern was called by its bare symbol and relied on a fixed header set to declare it, so `puts` or `uname` failed as implicit declarations under GCC 14, and an `Address<T>` argument failed against a header's own struct pointer type (`struct utsname *`). Chosen over dropping the headers and declaring the real names (GCC and Clang warn on redeclared builtins such as `memcpy`, and a header still included anywhere would conflict). Limit: a variadic C function is called through a fixed prototype, so a `Float64` in its variadic part is not guaranteed to arrive (x86-64 passes such a callee a vector-register count the fixed prototype does not set); integers, pointers, `String`, and `Bytes` are fine. |

## 2026-10-17 — thoxa.dev and the manual

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D189 | thoxa.dev renders the handbook as its manual; its samples are gated programs | `/manual` renders `docs/handbook/` (`introduction`, `language`, `std`, `session`, `cli`, `editor`, in that order) as one page, per request, with a Markdown renderer and a code highlighter written in Thoxa (`workloads/landing/app/markdown.thx`, `highlight.thx`). Sections are numbered; a link to another chapter becomes an anchor, any other relative link a GitHub URL. The handbook is written for that page: a reference-manual structure, no decision ids in the prose (they stay in `capabilities.md` and here), and a Markdown subset (`docs/handbook/README.md`). The home page states the design rationale, the features, the status, and how to build, and shows the programs in `workloads/landing/samples`; `cargo make test` runs each one that has a `.out` (under LeakSanitizer) and typechecks the rest. | **Shipped.** The site hold in CURRENT was lifted for this at Joshua's request (2026-10-17): "no marketing speak … speak like a programmer, to programmers". The image copies `docs/handbook` and `samples/` next to `web/`, so the deployed manual is the handbook of the image's commit. Same day: `containers/` became `workloads/`, and the repository moved to `github.com/moonlight-pm/thoxa` with thoxa.dev as its homepage. Writing the samples and checking the manual found D183–D188. |

## 2026-10-17 — Fixes found writing the manual and the site

Each was found by code written for thoxa.dev or the manual, and each has a
conformance gate that fails on the compiler before the fix.

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D188 | A session line that starts a statement is never a command | A top-level session line whose first word is `if`, `for`, `while`, `loop`, `match`, `return`, `break`, or `continue`, or that opens a block continuing on later lines, stays Thoxa. Only a top-level line can be a command; inside a block, call `shell.run([…])`. | **Shipped.** Gates: `session_blocks`, `command::tests::desugar_keeps_multiline_statement_blocks`. Before, `for value in xs {` alone did not parse, so the command guess ran a program named `for` and left the closing `}` to fail (`expected expression, found RBrace`) — in scripts, piped input, and the interactive REPL alike; `if ok { … }` on one line ran `if` as a command. Multi-line declarations were already kept. Found writing the home page's shell sample. **Superseded by D227.** |
| D187 | `Float64` text: shortest round-trip, written out between 1e-7 and 1e21 | The fewest significant digits that read back to the same bits. When the decimal exponent is in -7..20 the number is written in full, and an integral value keeps `.0` (`100.0`, `0.0000001`); further out it takes an exponent as `%g` writes it (`1e+21`, `1e-08`). Used by interpolation, tag-literal text, and JSON `encode`. Amends **D140**. | **Shipped.** Gate: `float_format`; `json` now re-encodes `1e2` as `100.0`. Before, `%.Ng` at the shortest precision switched to exponent form as soon as the exponent reached the precision, so `100.0` printed `1e+02`, against D140's "integral values keep .0". Found while the manual was being checked. |
| D186 | `let` may bind a Void value | `let x = f()` where `f` returns `Void`, `let done = loop { … break … }`, and `let w = while … { }` bind the name to the unit place instead of copying into a local. | **Shipped.** Gate: `void_bindings`. Before, cgen declared no C local for a Void binding but still assigned to it, so `cc` failed (`v_found undeclared`). Found while the manual was being checked. |
| D185 | Reading a refined value's base takes its own reference | `unwrap_refined` (used by interpolation, `==`, `let s: String = name`, an append, any place a refined value meets its base type) `Dup`s a heap base, as every other field load does. | **Shipped.** Gate: `refined_heap_values`. Before, the read aliased the refined box's `String` without a reference: appending it to an array, module-level or local, left the array holding freed memory once the box was dropped (ASan heap-use-after-free; `T.from` then a match panicked `drop on zero refcount` under LSan). Found when the home page's sample server read its notes back as `["GET"]`. |
| D184 | An array literal takes its element type from the expected type | With an expected `Array<T>` (an annotation, a parameter, a return), each element is lowered expecting `T` and converted as an annotated `let` converts: a union member widens to the union, an integer to `Float64`, a literal or refined value is checked, and a nested `[]` takes the element type. Without an expected type the elements must share one type. | **Shipped.** Gate: `array_literal_expect`. Closes two CURRENT gaps: `[Leaf(…), Branch(…)]` where `Array<Tree>` is expected was "heterogeneous array", and `[[], [1]]` could not type its empty element. Found writing the home page's first sample (`let shapes: Array<Shape> = [Circle(…), Rect(…)]`). |
| D183 | `and` / `or` short-circuit | `a and b` evaluates `b` only when `a` is true; `a or b` only when `a` is false. Lowered as `if a { b } else { false }` / `if a { true } else { b }`, so the right operand gets `if`'s scopes and drops. Both operands are Bool (`operator And expects Bool operands` otherwise). | **Shipped.** Gates: `short_circuit`, `fail/logical_operands`. Before, both operands were evaluated into places and joined with C `&&`, so `index < bytes.count and bytes.get(index) == 0` panicked out of range; the oracle SPEC always said short-circuit. Found writing the site's Markdown renderer. `==` on products still evaluates every field (D175). |

## 2026-10-16 — Typed codecs

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D182 | Typed codecs: everything encodes, reads report every issue, forms read like JSON | Freeze [`specs/2026-10-16-typed-codecs.md`](specs/2026-10-16-typed-codecs.md). §1 `toJson` / `encode` on every encodable type (scalars, `Array`, `Map<String, T>`, `May`, `Json`, refined, literal unions, products, unions of products tagged by a `"type"` key). §2 `T.read` / `T.fromJson` return `Trial<T, Array<Issue>>` with every issue (amends D149's fail-fast); `T.from` keeps one `Issue`. §3 `T.readForm(text)` reads urlencoded forms and query strings into the same types; empty means absent except for `String`. §4 `{…}` / `[…]` literals where a `Json` is expected, values via `toJson`, `...` spreads. §5 reads of unions, `Map` / `Json` fields, constructor defaults. §6 a type's own `toJson` wins. | **Shipped** (all six). `T.from` / `T.fromJson` were later replaced by `x as T` (**D191** §6), whose checked conversions all return `Array<Issue>`. §1 and §6 (gates `json_encode`, `json_custom_encode`, `fail/json_encode_*`); §2 (gates `json_read_all`, `json_read`, `where_refine`: the `std/json` read helpers report `Array<Issue>`, `asFloat` takes integers, `messages` / `issueObject` / `errorsObject` added, the `*Field` helpers removed; also fixed: a generic type in a nested `let` annotation was never instantiated, gate `nested_annotations`); §3 (gates `form_read`, `fail/form_read_field`: helpers in `std/http`, which now imports `std/json` for `Issue`; numbers parse as JSON numbers; a nested product's names are `field.sub`); §4 (gates `json_literal`, `fail/json_literal_spread`, `fail/json_literal_key`: `{}` is an empty object where a `Json` is expected; a replaced key keeps its first position, as `Map.set` does; spreads take products, unions of products, and `Map<String, T>`, not `Json` values, so an object is known at compile time); §5 (gate `json_read_more`: a union's reader picks the member by `"type"` and calls the member's reader, so the `"type"` key is ignored there; an explicit `null` is `Not` even with a default; a type with its own `toJson` gets no generated reader; in a form, `Bool` and `Array` take no default). Joshua, 2026-10-16: collect every issue, for JSON as well as forms; empty is absent. No traits (as D149). Slices in section order, each with its gate. A type's own `toJson` counts only when exported (methods are private otherwise). |

## 2026-10-15 — Review follow-ups

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D161 | Int64 shifts are defined for every count | `a << n` and `a >> n` are runtime-checked (`thoxa_int_shl` / `thoxa_int_shr`). A count of 64 or more shifts every bit out: `<<` gives 0, `>>` gives 0 or -1 by sign. A negative count panics (`negative shift count`). `<<` shifts the unsigned bits, so it wraps like `*` (D157). `>>` stays arithmetic. | **Shipped.** Gates: `int_shifts`, `fail/negative_shift`. Matches Go. Masking the count (x86 / Java) was rejected: `1 << 64` would be 1. |
| D162 | A line that starts with `-x` starts a new expression | The lexer marks the first token on each line. A `-` there that touches its operand (`-1`, `-x`) is a prefix minus, so the expression above ends; `- x` with a space still subtracts across the line break. Match patterns accept negative integer literals (`-1 =>`). | **Shipped.** Gate: `leading_minus`. Before, a block tail `-1` after an `if` parsed as `if … - 1` (D158 surfaced it in `std/ansi`). The rule is Swift's, limited to line starts. Thoxa is otherwise newline-insensitive. |
| D163 | A mutating method needs a writable receiver | A method **mutates** when its body stores through its receiver (a field assignment, or a collection / method write on a field, D107). Calling one on an immutable binding (`let`, a parameter, a `for` or match bind), on a field from outside its type, or on a temporary is a type error, as `append` / `set` already were. On a `mut` binding the call writes back: a method's field local stores through the receiver, and `items[i].method()` stores the element. Types with `fn drop()` are shared boxes (D155) and are exempt. | **Shipped.** Gates: `method_receiver_writeback`, `fail/mutating_method_let`, `fail/mutating_method_param`, `fail/mutating_method_field`. Before, the write went to a copy and was lost (`left.bump()` inside a method, `items[i].bump()`), or changed an immutable `let`. Decided once every body is lowered, so call order does not matter. No `self`: methods cannot call siblings, so mutation is never transitive. |
| D164 | `return` is Never | `return` (with or without a value) has type Never, like a bare `loop` (D128), so an `if` / `match` arm that returns fits beside any other arm, including a Void one. A `match` whose arms all diverge is Never. | **Shipped.** Gate: `return_diverges`. Before, `return x` was typed as the function's return type, so `match m { Not { return Has(…) } Has(_) { Void } }` failed with "match arms have different types". |
| D181 | Removing a Map key keeps insertion order | `remove` used to move the map's last entry into the removed slot, so after removing `"a"` from `a, b, c` iteration gave `c, b`, against the documented insertion order (D134). The runtime now marks the slot removed and its index entry a tombstone; the live slots are packed, in order, once removed ones pass half the slots, when the slots run out on an insert, and before a read by position (`for k, v`, `key_at`, `.keys()`, display). Lookups never see a removed slot. A key set again after its removal goes to the end, as the Map freeze says. | **Shipped.** Gates: `map_remove_order` (600 mixed sets and removes against an Array model, a COW snapshot, user keys; also under ASan and LSan by hand), `map_churn`; `map_user_keys` output now in insertion order. Removal stays amortized O(1): 100,000 inserts and removes run in about 20 ms. Packing changes no entry, so it is safe on a map that is shared. |
| D180 | A collection write reaches a `mut` binding or is an error | `append` / `dropLast` / `insertAt` / `removeAt` on an Array, `set` / `remove` on a Map, and `append` / `appendBytes` on a Buffer used to run on a copy when the receiver was not a plain binding, so the change was lost without a word: `board.names.append("ada")` on a `mut board`, `grid[0].append(5)`, `counts().set(…)`. Now an element of a `mut` Array writes back (`grid[0].append(5)` loads the element, changes it, and stores it, as `items[i].method()` does under D163). A field from outside its type is an error (`cannot mutate immutable field names`), as `board.names = …` and `board.label = …` already were. A temporary is an error (`cannot mutate a temporary value`). A module's own value (`module.items`) is written as before. | **Shipped.** Gates: `collection_element_writes`, `fail/field_collection_write`, `fail/temporary_collection_write`. Nothing in `std/`, the examples, or the sites relied on the lost writes. Inside a type's methods its fields are bindings and write back (D107), unchanged. |
| D179 | A user type can be a Map key | `Map<Point, V>` works for a constructor-param product or a union whose contents hash by value: integers, Bool, String, Bytes, refined and literal-union fields, and nested such products and unions. Keys hash and compare by value (D175's equality, generated in C per key type: a hash, an equality, and a `ThoxaMapKeyOps` table with the element retain / release), never by RC pointer. The runtime's third key kind stores each new key as a refcounted box holding a copy, which map copies share as they share String keys; a lookup passes the address of the caller's value and copies nothing. `for k, v`, `.keys()`, `m[k]`, `.get` / `.has` / `.set` / `.remove`, and literal keys `{Point(x: 1, y: 2): v}` all take user keys. Refused where the map is made, with the reason: a key holding Float64 (no value equality to hash), Array, Map, Node, a type with `drop`, or a C-layout type. | **Shipped.** Gates: `map_user_keys`, `map_user_keys_more` (both also under ASan and LSan by hand), `fail/map_key_float`, `fail/map_key_resource`; checked across REPL lines. Steps 2 and 3 of the trajectory the Map freeze locked (structural `==` was D175); no Eq/Hash traits. A literal key `{Name(…): v}` is a call; a bare identifier is still its own name as a String. Not yet: Array fields in keys (arrays have `==` since D178, but no hash). |
| D178 | `==` on arrays compares count and elements | Two arrays are equal when they have the same count and their elements are equal in order, each by its own type (D175 / D177 rules, nested arrays included). The loop stops at the first difference and does not run when the counts differ. Elements are loaded the way `for x in xs` loads them (one shared helper). So `Array` fields and arms compare too. | **Shipped.** Gates: `array_eq` (also under ASan and LSan by hand); `fail/product_eq_map_field` and `fail/union_eq_map_arm` replace the two Array-field rejections, since Map is now the type without `==`. Not yet: `==` on Map (it would compare entries regardless of insertion order), ordering on arrays. |
| D177 | `==` on unions compares the arm and its payload | Two values of one union are equal when they are the same arm and that arm's payloads are equal (D175's rules, recursively). A literal arm, or one with nothing stored (`Not`, `Pass(Void)`), is equal when the arms are. So `May` / `Trial` values compare: `Has(1) == Has(1)`, `Not() == Not()`, `parsed("one") == parsed("one")`. A union and one of its members compare after the member joins the union (`shape == Circle(radius: 1)`). A union field in a product compares the same way. Every arm's payload must have `==`, checked when the comparison compiles (`Json` is refused: its arrays have none). | **Shipped.** Gates: `union_eq`, `fail/union_eq_array_arm` (replaces `fail/product_eq_union`). Also fixes D175 for a `Void` field (`Pass<Void>` has no storage for it): it is equal and is not read. A bare `Pass(1)` on one side still types alone (as `Trial<Int64, Int64>`), so compare it with an annotated value. |
| D176 | A string may sit inside an interpolation hole | Inside a hole `{…}` an unescaped `"` starts a string literal of its own, which may have holes of its own: `"{greet("ada")}"`. The outer string ends at a `"` in its own text only. The lexer keeps the nested literal as source, and the parser lexes the hole again, as before. Escaped quotes in a hole (`"{indexOf(\"hello\", \"ll\")}"`, the older way) still work. A hole missing its `}` would now take the quotes after it as strings of its own, so it is reported at the `{` (`unclosed interpolation`) instead of where the file ends. The session scanners (REPL continuation, rc and `-c` splitting, command desugar) track nested strings too, so a `#` or a bracket in a nested string is not a comment or an open form. | **Shipped.** Gates: `string_nested_quotes`, `fail/unclosed_hole`, `repl::tests::needs_more_reads_strings_inside_holes`, `repl::tests::split_forms_nested_string_with_hash_stays_one_form`. Swift, Kotlin and JavaScript template literals allow the same. A literal brace is still `{{` / `}}`, in a nested string too. The Tree-sitter grammar still colours a nested string as two strings. |
| D175 | `==` on products compares their fields | `==` / `!=` on two values of one constructor-param product is true when every field is equal, compared by the field's type: integers, `Bool`, `Float64` (IEEE, so a NaN field is never equal), `String` (bytes), `Bytes` (bytes), a refined field by its base value, a literal-union field by its tag, and a nested product field the same way (a type cannot contain itself, so this ends). Fields are read as borrows, with no RC traffic. Also new: `Bytes == Bytes` (same as `.equals`). Refused with a message instead of "comparison operands must have the same type": different types (`cannot compare Point and Size`), ordering (`Point only supports == and !=`), unions, types with `drop` (resources), C-layout types, and a field with no `==` (`field items is Array<Int64>, which has no ==`); Array / Map / Node values say `cannot compare Array<Int64> values`. | **Shipped.** Gates: `product_eq`, `fail/product_eq_array_field`, `fail/product_eq_union`, `fail/product_eq_ordering`, `fail/product_eq_types`. Step 1 of the user-key trajectory the Map freeze locked (structural `==`, then a private field hash, then user types as Map keys; no Eq/Hash traits). Compares by value, never RC pointer identity. Every field is evaluated (no short circuit). Unions since **D177**, arrays since **D178**. Not yet: `==` on Map. |
| D174 | Unannotated methods infer their return type | A method without `-> T` gets its return type from its exits, by D129's rules, instead of Void (its value used to be dropped: `fn area() { width * height }` returned nothing). `drop` stays Void. Free functions and methods are inferred together: callees first, and a body that reaches one not inferred yet waits for the next round. Bodies left waiting only on each other form recursion, which is an error naming each one on a cycle (`Countdown.steps`), so callers of a recursive function are not blamed. Also fixes D129 §4 for bare constructors: after `x?`, a tail `Pass(value)` / `Has(value)` gives the success type (`Trial<Int64, String>`, `May<String>`), where it used to infer `Trial<Void, String>` and fail. | **Shipped.** Gates: `return_infer_methods`, `return_infer` (the `?` then `Pass` / `Has` cases), `fail/method_return_recursive`, `lsp::analysis` `return_types_include_inferred_method` (hover). Still Void when unannotated: multi-clause methods (each clause annotates or the method is Void) and methods of generic types (their signatures are made at monomorphization, after inference). Methods still cannot call sibling methods (no `self`). |
| D173 | What `main` returns is the exit status, 0 to 255 | Generated `main` passes `main`'s `Int64` through `thoxa_exit_status`. 0 to 255 is the exit status. Anything else panics with `main returned 256; an exit status is 0 to 255` after the tasks drain, so the process ends by SIGABRT (134 from `thoxa run`). Session scripts end the same way, since their synthetic `main` returns `Shell.status`. | **Shipped.** Gates: `fail/main_exit_status`, `driver::main_return_is_the_exit_status` (0, 255, 256, -1). Settles review finding 23. Before, `(int)` and the kernel's 8-bit status turned 256 into 0, a success, and -1 into 255. The same rule as D167 narrowing: a value that does not fit panics. `main`'s type stays `Int64`, not `Byte`, so existing programs need no change. |
| D172 | The sites send security headers and cache validators | Each site's `app/routes.thx` `finish(request, response)` runs on every response. All of them: `Content-Security-Policy` (`default-src 'none'`, plus what the pages load: styles and images from the site; fonts from `fonts.gstatic.com` with the Google Fonts stylesheet on thoxa.dev and sola.computer, from the site on oath.wicket.cloud; `base-uri` / `form-action` / `frame-ancestors` `'none'`), `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`, `Strict-Transport-Security: max-age=31536000`. A 200 goes through `http.cached`, so it carries an ETag and a matching `If-None-Match` gets a 304: images and fonts are `public, max-age=86400`, pages and CSS `no-cache` (checked on every load, so a deploy shows at once). Other statuses are `no-cache` with no ETag. `std/http.notModified()` no longer sends `Content-Length: 0` (RFC 9110 §8.6). | **Shipped**, live since the 2026-10-16 redeploy from `e0c0bcf` (review decision R11). Settles review finding 10. Policy stays in the app (D145 lock); the three copies differ only in the CSP, and merging them is finding 38. Checked by running each site and loading every page in headless Chrome: no CSP violation, every asset 200, a repeated image request 304. Hashing a 550 KB image for its ETag takes about 5 ms per request. No `includeSubDomains` / `preload` on HSTS. Gate for the 304 change: `http_helpers` (`fresh-length`). |
| D171 | The HTTP host answers pipelined requests | `std/http/server.serveOne` keeps the bytes it read past one request and starts the next request with them, so requests a client sends without waiting for replies are answered in order (RFC 9112 §9.3). A request without `Content-Length` has an empty body (RFC 9112 §6.3); before, everything after its headers became its body, which swallowed the next request. Empty lines before a request line are skipped (§2.2). `std/net` `receiveRequest(deadline:, buffered:)` starts from bytes already read and returns without a recv when they hold a whole header block. `readRequest` and `readBody` keep their signatures; `readRequest` still drops what follows its request, since only `serveOne` carries it. | **Shipped.** Gate: `http_pipeline` (two GETs in one send, a POST body then a GET, CRLF after a body, HEAD then two GETs, headers and a body split across sends, a bad second request). Settles the rest of review finding 9. Responses go out one at a time, in request order; nothing in a connection runs concurrently. What is read past a body is at most one 4 KiB receive. The sites run it since their redeploy from `e0c0bcf` on 2026-10-16. |
| D170 | A program that is PID 1 ends on SIGTERM and SIGINT | Generated batch `main` calls `thoxa_program_start()` before module init. When the process is PID 1 (a container's entrypoint) and SIGTERM / SIGINT still have the default action, it installs a handler that `_exit`s with 128 + signal, so `podman stop` ends the program at once (status 143) instead of waiting out its timeout and sending SIGKILL. The kernel drops a default-action signal sent to a PID namespace's init, even from the host. Anywhere else nothing changes, since the default action already ends the process; an ignored signal or another handler is left alone. There is no drain: a request in flight is cut, as a default-action SIGTERM would cut it. | **Shipped.** Gate: `driver::program_as_pid_one_ends_on_sigterm` (runs a `pause()` program as PID 1 in a user + PID namespace with `unshare`; skipped where that is not allowed). Settles review finding 57; the sites run it since their redeploy from `e0c0bcf` on 2026-10-16 (R11). Go's rule: its runtime exits on SIGTERM as PID 1; Node's does not. Chosen over `tini` in the images (a package, and an `apt-get` step in the final stage) and over a graceful drain in `std/http/server` (sample-driven, later). As PID 1 a program still does not reap orphans; the sites start no children. The same change made the site images build their own C (review finding 8). |
| D169 | Session module state is kept, not re-run | In the REPL a module-level value lives as long as the session. Every module global of the session program (user `let` / `mut` and `std/` state) is a host-owned slot that outlives the compiled images; each initializer runs once, and a slot counts as ready only when its initializer finished. A `let` / `mut` line runs its initializer at that line; one that panics is reported and not defined. Assignments to a `mut` persist across lines and across new declarations. The session's declarations and the runtime compile into one in-memory base image per declaration set; each line's step image links against it (`tcc_add_symbol`) and reaches module state through slot addresses baked into its C. A base image that ran a step stays loaded for the rest of the session, so a value's drop function and code pointers stay valid. | **Shipped.** Gates: `repl::session_let_runs_its_initializer_once`, `repl::session_mut_keeps_its_value_across_lines_and_declarations`, `repl::session_let_that_panics_is_not_defined`, `tcc::step_image_links_against_a_base_image`. Settles review decision R6; the freeze said declarations are "lifted once". Before, every line re-ran every `let` (a `File.create` repeated each line) and lost `mut` changes. A step is ~6 ms as before. Each `let` / `mut` line keeps one base image loaded (~0.4 MB). Module initializers also resolve default arguments in their defining module now (`io.openFile(path, flags)` at module level failed with ``undefined name `oMode644` ``; gate `module_resources`). **Superseded by D227.** |
| D168 | A resource is built only by its own module, and can live in module state | Calling the constructor of a type with `fn drop()` outside the module that defines it is a compile error, so `Db(handle:)`, `File(fd:)`, and `Socket(fd:)` are private to `std/sqlite`, `std/io`, and `std/net`. Module-level `let` / `mut` take an optional annotation (`mut connection: May<Db> = Not()`). Module-level inits number their temps across modules (a constructor in one module's init shared a C local with `std/sqlite`'s and was typed `Sql`). `drop`-type values in a `Map` / `Array` are shared boxes: reads retain the box, and a placeholder is a NULL box (a `Map.get` miss built a zero `Socket` whose `drop` would close fd 0). A `drop`-type value passed where `Address<T>` is expected passes its box pointer. `net.ownByTask(socket)` hands the runtime the box, so a panicked task's socket reads closed (`fd` = -1) through every copy (amends **D166**). `std/http/server.peerOf(request) -> May<Socket>` returns the connection's own Socket from a registry `serveOne` keeps; SSE handlers use it instead of rebuilding one from `peerFd`. | **Shipped.** Gates: `module_resources`, `http_sse_peer`, `task_panic`, `fail/resource_constructor`. Settles review decision R3. `examples/http` keeps one `Db` in module state and its SSE board keeps `Socket` subscribers. A `for` over a `Map` of Sockets used to skip the retain, so a subscriber outlived by the host's copy was a use-after-free. `Request.peerFd` stays as the registry key. |
| D167 | Implicit integer narrowing is checked | Where D21 converts at a boundary (an argument, a return, a field / element store, `let x: T`), a conversion to a type that cannot hold every source value checks the value: `Int64` / `Int32` → `UInt64` / `UInt32` / `Byte` / `Int32`, `UInt64` → `Int64` and narrower. A value that does not fit panics (`300 does not fit in Byte`, `thoxa_narrow_signed` / `thoxa_narrow_unsigned`). A literal is checked at compile time: one that cannot fit is an error with a hint. `T(truncating: x)` for `Int64` / `Int32` / `UInt64` / `UInt32` / `Byte` keeps the low bits (two's complement), unchecked. Widening is unchanged; `Int64` → `Float64` is not checked. | **Shipped.** Gates: `int_narrowing`, `fail/narrowing_runtime`, `fail/narrowing_unsigned`, `fail/narrowing_literal`. Settles review decision R9. Swift's rule: `UInt8(300)` traps, `UInt8(truncatingIfNeeded: 300)` is 44. Before, a `Byte` parameter given 300 silently got 44. Arithmetic stays in `Int64` (D21), so a `UInt64` above `Int64` max panics in `+` or `<`. |
| D166 | A panic in a task ends that task, not the process | A panic inside a `-->` task prints its message and `thoxa: task ended by panic`, then the scheduler drops the task and runs the others; `main` goes on. The task's frames are abandoned where they stand (no unwinding), so what they held leaks, by design. The runtime frees the packed arguments and the stack, and closes the sockets the task owns: `net.ownByTask(socket)` (`thoxa_task_own_socket`, since **D168** given the Socket box so it also sets `fd` to -1) marks one, and ownership ends at `close` / `drop` (`thoxa_close`) or when the task returns. `net.taskOwnedSockets()` counts them. `std/http/server.serveOne` owns its peer, so a handler that panics drops its connection and the server keeps serving; the 1024-connection cap (D165) counts owned sockets. A panic in `main` (outside any task) still aborts. | **Shipped.** Gate: `task_panic`. Settles review decision R4. Under LeakSanitizer the dead task's stack stays mapped as a root region, so the leak gate does not report the by-design leak. A stack overflow in a task (guard page) still ends the process. A module-level structure the task was changing when it panicked is left as it was. |
| D165 | The HTTP host has deadlines, a connection cap, and strict framing | Runtime fd waits take a deadline (`thoxa_recv_until` / `thoxa_send_until`, ETIMEDOUT). `std/net`: `nowMs()`; `receive` / `receiveRequest` / `sendAll` take `deadline: Int64 = 0` (a `nowMs()` time; 0 = none). `std/http/server`: headers within 10 s of accept, 30 s idle between kept-alive requests, 30 s for a body and for a response; at 1024 open connections a new one is closed at accept. `readBody` refuses `Transfer-Encoding`, two `Content-Length`s, or one that is not a number. `std/http.parseRequest` requires a visible-ASCII method, request target, and header names; `writeResponse` drops CR / LF from header text. | **Shipped.** Gate: `http_hardening`. The limits are module constants, not configuration (sample-driven, D97). No chunked bodies. Pipelined requests are answered since **D171**. The sites run it since their redeploy from `878d863` on 2026-10-15. Since **D166** the cap counts task-owned sockets, so a connection whose task panicked no longer holds a slot. |

## 2026-10-14 — Correctness pass (project review)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D157 | Int64 overflow and division | Arithmetic wraps (two's complement): every `cc` build passes `-fwrapv`, so overflow is defined instead of C undefined behaviour. `/` and `%` by zero panic (`division by zero`); `INT64_MIN / -1` wraps, `% -1` is 0. `a..=b` stops after `b`, so `..= INT64_MAX` ends. | **Shipped.** Gate: `int_semantics`. Trap-on-overflow (checked arithmetic) is not adopted; reopen as its own D-id if wanted. Shifts → **D161**. |
| D158 | `if` without `else` has no value | An `if` with no `else` is a Void statement. Using it where a value is expected (a typed function tail, `let x: T = if …`) is a type error, not a zero / NULL on the false path. | **Shipped.** Replaces the old rule that gave it the then-branch type. Surfaced `std/ansi` `hexNibble`, whose trailing `-1` parsed as `if … - 1` (a line starting with `-` continues the previous expression — not changed here). |
| D159 | `\xHH` in String literals is ASCII | `\x00`–`\x7F` only; higher bytes are a lex error (Strings are UTF-8; one high byte is not a character — write the character or build `Bytes`). | **Shipped.** Gate: `string_escapes`. Before, `\xff` silently became two UTF-8 bytes. |
| D160 | A crash inside a session line does not end the shell | Session steps run through runtime `thoxa_session_call`: a panic returns status 134, a fault (SIGSEGV / SIGBUS / SIGFPE / SIGILL) returns 128 + signal, and the shell keeps running; memory held by the abandoned step leaks. Spawned children start with default SIGPIPE / SIGINT / SIGQUIT, and the parent ignores SIGINT / SIGQUIT while it waits (the system(3) rule). Batch programs still abort on panic. | **Shipped.** Gate: `repl::panic_in_a_line_returns_status_instead_of_ending_the_shell`. Ctrl-C during a pure-Thoxa loop (no child) still ends the shell: interrupting at an arbitrary point could leave malloc locked. **Superseded by D227.** |

## 2026-09-04 — Session line editor

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D156 | Interactive session is a rustyline **emacs** editor | TTY `thoxa` uses rustyline (emacs mode): Tab complete, history, GNU-readline-style keys. Ctrl-C discards the line; Ctrl-D on empty exits. Piped / non-TTY stays cooked `read_line`. History: `THOXA_HISTFILE` → `$XDG_STATE_HOME/thoxa/history` → `~/.local/state/thoxa/history`. Tab is host-side (no libtcc): Shell methods + PATH + files/`cd` dirs + `std/` + scraped decl names. Not typed member complete; not a bash `complete` dialect. | **Shipped.** Gate: `line_edit::`. Unparks the parked “crude line editor” residual only. **Superseded by D227.** |

## 2026-09-03 — Resource `drop` on last RC

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D155 | A product may define `fn drop()`; last RC runs it | Constructor-param products with `export fn drop()` (no formals, `Void`) are heap boxes (header + payload). Generic `thoxa_drop` calls the dtor at refcount 0. Copies share the box. Explicit `close` must leave the handle inert. Not on unions, C-layout, or multi-clause. `File` / `Socket` / `Db` ship `drop()` as close. Freeze [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md). | **Shipped.** Gate: `file_drop`. Does not unpark Perceus or a cycle collector. Amends class E of D153 (still no language `Drop` word / linear types). |

## 2026-09-02 — Heap params are owned copies

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D154 | Heap arguments are moved into the callee | `needs_drop` params are in `drop_places` (dropped at fn end / early return). Caller forgets the Dup'd arg temp and NULLs it after the call. Method `_self` stays borrowed (D107 pointer). Extern / builtin args stay caller-owned. `-->` pack still Dups; thunk does not drop pack slots after the call (callee already dropped). Amends **D63** (heap params now drop on `return` / `?`). Gate: `buffer` LSan-clean, value semantics `grown(c)` leaves caller count unchanged. | **Shipped.** D153 campaign slice. |

## 2026-09-02 — Simple-RC correctness is CURRENT

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D153 | Memory safety of **simple RC** is the living priority until `lsan-skip` is empty | Campaign freeze [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md). Empty the LSan skip list, then a bounded ASan pass on non-fiber programs. **Not** Perceus (D8 unchanged). **Not** language `Drop` / cycle collector in this wave. First slice: overwrite-drop of live heap places (loop temps). Second: Buffer/Array/Map COW vs borrowed params (`buffer` panics under LSan). Session/libtcc stays unsanitized. ASan stays optional as the default gate (ucontext fibers). | **Locked.** Extends D151/D152; names D69 HTTP over-dup as class C. Does not change the value model (D4) or make ASan default. Gate done 2026-09-03: `lsan-skip` empty; 88 non-fiber ASan-clean; `cargo make test-asan` optional. Named-later 2026-09-03: `thoxa_alloc` panics on size wrap; `Socket.close` clears `fd`. Resource `drop` later as **D155**. Perceus not unparked. |

## 2026-08-23 — Simple RC balance (match / init)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D152 | Close three LSan-proven extra-retain holes | (1) Value-match bare bind: `assign` of heap `Use` already Dups — do not Dup again. (2) Union/Node match result place is `note_drop_place` so inner `match` Trials (`readFile`) are dropped. (3) Module heap/User globals get `__attribute__((used))` so `-O1` DSE cannot drop unused stores (`sqlite.null`). | **Shipped.** Gate: `match_bind`, `logging_sink`, `file_io`/`file_seek`/`file_type`/`file_write`, `strings_multiline` now LSan-clean. Does not close loop temps / HTTP over-dup / Buffer COW. |

## 2026-08-23 — LeakSanitizer conformance gate

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D151 | Heap leak gate is GCC/Clang LeakSanitizer on batch `cc` | `cargo make test` links each conformance program with `-fsanitize=leak` (`THOXA_SANITIZE=leak`). Product `thoxa run` stays unsanitized (`target/thoxa/bin/`). Sanitized bins: `target/thoxa/bin-leak/` (or `bin-address` for `THOXA_SANITIZE=address`). Session/libtcc steps ignore the sanitizer (cannot link liblsan). Tests not yet RC-clean live in `tests/conformance/lsan-skip` — stdout still gated; a skip that becomes LSan-clean **fails** until removed. Valgrind is not in the tree (not a host dep). ASan is optional, not the default gate (ucontext fibers). | **Shipped.** Does not unpark Perceus. Module-init heap temps are dropped (D122 globals kept). **D152** closed match-bind extra Dup, union-match result drop, unused-global DSE. **D153** gate done 2026-09-03 (`lsan-skip` empty + bounded ASan). |

## 2026-08-23 — Type-body route clauses

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D150 | Product HTTP routes are type-body clauses | `GET "/…"` / `POST` / … on a **nullary** type desugar to free-fn handlers + **`route(req: Request) -> Response`**. String bodies wrap `ok(…, "text/plain")`; `-> Response` pass-through; miss `notFound()`; `Int64`/`String` captures; optional `Request` formal. Compiler **names** userland `Request` / `Response` / `ok` / `notFound` / `badRequest` — no lang-items. | **Shipped.** Freeze [`specs/2026-08-23-route-clauses.md`](specs/2026-08-23-route-clauses.md). Gate: `route_clauses`. Punted: JSON/form bind (since **D191** §5: typed body parameters), fallthrough, router fields, file-based routing. Generated code names `std/http` items by the module's own names, else mangled (**D193**). Dogfood: Crema `type Routes`. |

## 2026-08-23 — Refinements + `Type.read`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D149 | Validation is types as inhabitation | **`where`** refinements are real types (`Qty` is not `Int64`). Literal inhabitation at compile time; `T.from` / `T.read` at runtime → `Trial<T, Issue>`. `Issue` in **`std/json`**. Tree `decode`/`encode` stay (**D141**). Closed slugs are literal unions <: String. Qty→base implicit; reverse is `from`. Compiler-attached methods (no traits). Fail-fast (reads report every issue since **D182** §2). | **Shipped.** Freeze [`specs/2026-08-23-type-read.md`](specs/2026-08-23-type-read.md). Gates: `json_read`, `where_refine`, `sums_literal`. Amends D141 (`Issue` + typed `read`). `T.from` replaced by `x as T` (**D191** §6). Punted: collect-all, `May<Int64 where …>`, cross-field `where`. Dogfood: Crema `CreateOrder`. |

## 2026-08-22 — `-->` nowait

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D148 | Product nowait is token `--> name(args)` | Begin a **named free Thoxa function**; do not wait. Expression type **`Void`**; callee return dropped. Token only (not a word). One OS thread; stackful fibers; Net parks; `main` / session step **drain until idle**. No Future / `async` / `run`. No `--> { }`. No `-->` of a pointer, method, extern, or type. | **Shipped.** Freeze [`specs/2026-08-22-start-tasks.md`](specs/2026-08-22-start-tasks.md). Gates: `start_tasks`, `start_nested`, `start_net`, `start_keepalive`, `http_host`. Amends D132 (named-fn `-->` is product; pointer `-->` still out). Dogfood: `std/http/server` `--> serveOne`. |

## 2026-08-22 — Builtin Node + tag literals

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D147 | Product HTML trees are builtin `Node` + tag literals | Ambient **`Node`** (`Element \| Text \| Fragment`, same `ThoxaHtml*` tree). Tags `<div>` / `<>` (no space after `<`); comparison is `a < b`. PascalCase is eager `Type(…).view(children:)`. `{e}` children: String / Int64 / Float64 / Bool / Node / `Array<Node>`; `{for x in xs { child }}` collects `Array<Node>`. Attrs stringify the same scalars; bare `disabled` is boolean. HTML `{...Array<Attr>}` appends; component `{...product}` copies ctor fields. `.html() -> String` (escaped; void tags `br hr img input meta link`). No import. Keep `.html()` — do not fold into generic `toString`. | **Shipped.** Freeze [`specs/2026-08-22-jsx-node.md`](specs/2026-08-22-jsx-node.md). Gate: `jsx`. Amends D140 (JSX stringify includes Float64). Field storage stays **D135**. `htmlResponse` / `htmlFragment` stay **D137** / **D145**. Punted: interpolation of `Node`, client VDOM, Tree-sitter `<>` polish, dedicated `.html()` hover. |

## 2026-08-19 — SSE / socket takeover

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D146 | Handler can own the peer for SSE | **`Request.peerFd`** (Int64, default 0) is host-filled by `std/http/server`. **`streamed()`** is status `0`; `serveOne` skips `writeResponse`, drains until close, then closes. **`beginEventStream` / `sendEvent`** write `text/event-stream`. Not chunked encoding, not WebSockets, not `fn` fields on Response (D48). | **Shipped.** Sample-driven (Crema Phase **E5**). Gate: `http_helpers` (`streamed=0`); live path is example-only (`examples/http` `/events`). Since **D168** the handler gets the Socket from `server.peerOf(request)`; `peerFd` is the key. |

## 2026-08-19 — HTML fragments + cache validators

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D145 | Fragment HTML + content-hash ETag | **`std/http`:** `htmlFragment(root)` is `root.html()` as `text/html` (no doctype). `etagOf(body)` is a strong quoted 8-digit hex FNV-1a-32. `ifNoneMatch` / `etagFresh` (`*` or listed token). `notModified` is 304 empty. `withCache` appends `ETag` + `Cache-Control`. `cached(req, res, cacheControl)` 304s when `If-None-Match` matches. Not weak ETags, not `Last-Modified`, not `Vary`. | **Shipped.** Sample-driven (D97/D136): Crema Phase **E2** fragment GETs. Gate: `http_helpers`. Cache *policy* (`private, no-cache` vs `max-age`) stays at the call site. |

## 2026-08-18 — `sql"…"` tagged interpolator

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D144 | `sql"…"` is a tagged interpolator | Ident **immediately** followed by a string (`sql"…"`; no space, like JSX `<tag`) is a tag. **v0 tag is `sql` only**; other tags error. Holes: String → quoted literal; Int64/Float64 → digits; Bool → `0`/`1`; `Sql` fragment → spliced raw (`ident` / `null` / `lit`). Result type is **`Sql`**. `Db.exec` / `Db.query` take `Sql`, not String. Not general JS tagged templates (no rest-args / `tag(parts, …values)`). | **Shipped.** Amends **D143** call surface. Gate: `sqlite`. `sql "…"` (space) is not a tag. Nested quotes inside `{…}` still can't live in the same lexer string (bind a local). |

## 2026-08-18 — Embedded SQLite

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D143 | Product SQLite is amalgamation + interpolated literals | **Amalgamation** in `vendor/sqlite` (3.53.4) compiled into `libthoxa_rt.a` (`SQLITE_THREADSAFE=0`, no load_extension). Thin RT `thoxa_sql_*` (Int64 handles; no stored `Address`). **`std/sqlite`:** `open` / `Db.exec` / `Db.query` / `close`; rows are `Row(cells: Map<String, SqlValue>)` with `SqlNull \| SqlInt \| SqlFloat \| SqlText`. **Injection (amended D144):** write `sql"… {value} …"`; holes are embedded as SQL. `lit` / `ident` remain as fragment helpers. Not `?` + a bind list. No named `ffi` root. | **Shipped.** Sample-driven (D97). Gate: `sqlite`. **Call surface → D144.** No blobs / prepared binds / transactions API. **No product `thoxa migrate`** — apps own schema. Crema persist is example-only (`examples/http` `/tmp/crema.db`). |

## 2026-08-18 — `Set-Cookie` builder

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D142 | Product `Set-Cookie` is a `Cookie` value | **`std/Http`:** `SameSite = "Strict" \| "Lax" \| "None"`; `Cookie(name, value, path="/", domain="", maxAge=Not(), httpOnly=false, secure=false, sameSite="Lax")` with `.header()`; `withCookie(res, cookie)` appends `Set-Cookie`; `clearCookie(name, path="/", domain="")` is `Max-Age=0` + empty value. No `Expires`, no cookie-octet escaping. Read path stays `cookie(req, name)` / `parseCookies`. | **Shipped.** Sample-driven (D97/D136): Crema remember/forget were literal `Set-Cookie` strings. **Amends D137** — wire format is std; cookie *policy* (name `customer`, Max-Age 30d, HttpOnly) stays in `examples/http`. Gate: `http_helpers`. |

## 2026-08-16 — `std/json`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D141 | Product JSON codec is a dynamic `Json` tree | `std/json`: `Json = JNull \| JBool \| JInt \| JFloat \| JStr \| JArr \| JObj`. **`decode(s) -> Trial<Json, String>`**; **`encode(v) -> String`**. Integers without `.` / exponent are `JInt` (out-of-range Int64 fails). Every other JSON number is `JFloat` via `strtod` (non-finite fails). Arrays / objects recurse through `Array<Json>` / `Map<String, Json>`. No pretty-print, no comments / trailing commas / NaN tokens, no HTTP helper in this module. | **Shipped.** Sample-driven (D97); charter [`ideas/2026-08-15-json-codec.md`](ideas/2026-08-15-json-codec.md). Face is encode/decode (codec pair), not JS parse/stringify. Enables union-member fields that are `Float64` / `Array` / `Map` / User (was a stale scalar-only check). Array/Map of **sums** now RC the live arm. `Map<K,V>` seeds `May<V>` so `.get` works for User values. RT: `thoxa_f64_from_string` / `thoxa_f64_is_finite`. Gate: `json`. Crema dogfood: `POST /api/orders`. **Typed `T.read` / `Issue` → D149.** |

## 2026-08-15 — Float64

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D140 | Product `Float64` | IEEE-754 binary64 (`double`). Literals `1.5`, `1e2`, `1.5e-1`, `1f64`, `3.14f64`. A `.` starts a fraction only when the next char is a digit, so `1..5` stays a range. No `Float32` (`f32` suffix is an error). Arith `+ - * /` and compares; no `%`; no bitwise. Int widths coerce to `Float64` at assign / call / field / return / mixed binop; never `Float64`→int implicit. Lives in ctor fields, `Array` elems, Map **values**, C-layout ABI scalars, and `extern` as `double`. Interp / JSX stringify is shortest round-trip (integral values print with `.0`). | **Shipped.** Amends **D28** (new scalar width) and **D48** (ctor fields). Not a Map key. No float literal types or match arms. Gate: `floats`. JSON codec followed as **D141**. |

## 2026-08-15 — HTTP host modules under `std/http/`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D139 | HTTP hosts live under `std/http/` | Protocol stays **`std/http.thx`** (syscall-free). Host modules are **`std/http/server`** and **`std/http/static`**. File-beside-folder: `"std/http"` is still the protocol file; `"std/http/server"` is the nested file. No directory-import (`index.thx` / `main.thx` as barrel). No re-exports from `http`. | **Shipped.** Amends D126/D136 import paths (was `std/http-server`, `std/http-static`). Nested specifiers already resolve (`format_thx` appends `.thx`). C id `std/http/server` → `std_http_server`. Hyphenated stems remain legal (D138). Gates: `http_host`, `router`. |

## 2026-08-15 — kebab-case module files

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D138 | Module files are kebab-case scopes | A `.thx` file is a **module** (a compilation unit / scope), not a type. Non-entry library stems are **kebab-case**: `[a-z][a-z0-9]*(-[a-z0-9]+)*` (`http.thx`, `http-server.thx`). Types remain PascalCase; values and namespace bindings remain camelCase. Directory entry is **`main.thx`** (amends **D68**). Import specifiers match the file (`"std/http"`, `"std/http/server"`, `"./math-utils"`). Namespace import is still not a value (**D33**). No user-facing `type Module`. | **Shipped.** Drops the oracle/loader “file describes a type / import is an instance” story (never surfaced). Amends **D35**: library module ids are path-qualified and sanitized (`std/http/server` → `std_http_server`; `/` and `-` → `_`). Do not use `__` (MIR monomorphs). Amends **D68**: entry file is `main.thx`. Duplicate ids after sanitize are an error. Lone-file entries (conformance `hello.thx`) keep any stem. Gates: `modules`, `namespaces`, `stdlib`. File-based routing stays parked: [`ideas/2026-08-15-file-based-routing.md`](ideas/2026-08-15-file-based-routing.md). |

## 2026-08-15 — HTTP leftover helpers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D137 | Public Str parse + remaining Http wrappers | **`std/Str`:** `parseUInt` → `May<Int64>`; `splitOnce` → `May<Split>` (`before` / `after`; empty sep or no match → `Not`). **`std/Http`:** `queryParam(req, key)`, `formField(body, key)`, `cookie(req, name)` (empty value → `Not`), `htmlResponse(root: Node)` (doctype + `.html()` + `text/html; charset=utf-8`). | **Shipped.** Finishes D136 optional list. Crema uses the wrappers; cookie policy (`customer` / Max-Age) stays in the example. **Set-Cookie builder → D142.** Gates: `stdlib`, `http_helpers`. |

## 2026-08-15 — HTTP host / wire into std

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D136 | Product `Request.query` + HTTP helpers + `std/HttpServer` | **`Request.query: String`** (no leading `?`); `parseRequest` keeps it and still strips path. Body stays empty at parse. **`std/Http`** (still syscall-free) gains `headerGet`, `contentLength`, `urlDecode`, `parseQuery` / `parseForm` / `parseCookies` / `queryMap`, `seeOther`, `withHeader`, `wantsKeepAlive`, `finalize`. **`std/HttpServer`** (Net syscalls): `bindListen`, `readBody`, `readRequest`, `writeResponse(peer, …)`, `serveOne`, `serve(port, handle)`. HEAD is rewritten to GET for the handle; `finalize` still sees the real method. | **Shipped.** Amends D69; extends D126. Crema-dogfooded; example `framework/` removed. Not promoted: `Incoming` header packing, `X-Thoxa-Query`. Headers stay `Array<Header>` (D134). Gates: `http_parse`, `http_helpers`, `http_host`. Leftover wrappers → **D137**. **Import path amended by D139:** `std/http/server`. |

## 2026-08-15 — Nested constructor fields

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D135 | Node + constructor-param User as ctor fields | `type Wrap(n: Node)` and `type Card(d: Drink)` are legal. User includes sums (`Drink.roast: Roast`). Stored inline (same RC field walk as today). Recursive / mutual User fields are an **error** (infinite C struct); `Array` / `Map` of the same type is fine. C-layout body-field types and function types stay out (D24 / D132). | **Shipped.** Amends D48. Gate: `nested_fields`. Dogfood: Crema `Roast` + `DrinkCard(d: Drink)`. |

## 2026-08-14 — Map and value collections

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D133 | Value collections (Swift / Buffer COW) | `Array` (and `Map`) writes go through a **`mut` place**: unique → in-place; shared → copy, update that place only. `let` aliases do not observe later writes. Mutating methods return **`Void`** (no `xs = xs.append(x)`). Temporaries allowed. | **Shipped.** Amends D54. Not Rust moves. **Bytes** splice stays D55. Fixed `Array<T,N>` unchanged. Since **D180** an element of a `mut` Array is a place too (`grid[0].append(x)`), and a write on a field from outside its type or on a temporary is an error, not a lost change. Freeze: [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md). Gates: `arrays`. |
| D134 | Builtin `Map<K, V>` | Heap RC `ThoxaMap*`. Keys v0: String / Int64 / Bool. Values: same as Array elems + nested Map (User values yes). Empty `Map()`. Read `m[k]` / `.get` → `May<V>`; `.has(k) -> Bool`; `.count`. Write `.set` / `.remove` / `m[k] =` write-back. Literals `{ k: v }` (ident key = string name; `{}` still empty block). `for k, v in m` insertion order; `.keys()` / `.values()` snapshots. Ctor fields yes (D48/D69 grow). | **Shipped.** Not `std/`. **User keys** shipped as **D179** (structural `==` D175 + a generated hash) — **not** Eq/Hash traits; not pointer identity. `==` on Array since **D178**; none on Map. `remove` keeps insertion order since **D181**. No Http header migration. Gates: `maps`, `maps_user`. |

## 2026-08-13 — Function pointers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D132 | Function pointers + bound methods (no closures) | Function type is `(a: T) -> R` (not oracle `Fn`). Values: free fn name / `module.fn`; unbound `Type.method` (type qualifier, extra `self` arg); bound `instance.method` (receiver + code; call ≡ `instance.method(args)`). Unboxed `{receiver, code}` pair; bound copy dups the instance. **Live in** args, returns, locals, **module `let`/`mut`**. **Not** ctor/body fields, arrays, or generic product fields (**D48 unchanged** — type error, not a lint). Call through a pointer requires all args (no defaults). No `==`, no interp, no JSX embed, no `-->` of a pointer, no nested/lexical fns, no multi-clause/`extern` as values. | **Shipped.** Freeze: [`specs/2026-08-13-function-pointers.md`](specs/2026-08-13-function-pointers.md). Gate: `fn_pointers`. Full FCF + ORC stay parked. Traits are a separate surface. Dogfood: Crema `serve(port, handle)`. |

## 2026-08-11 — Multi-clause functions

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D131 | Multi-clause functions as type-driven match sugar | Same name in one scope (module free fn **or** type method) → **one** function; formals stay `name: Type`; **any** types may discriminate; **source order** = match order; sugar only (one runtime entry). Bare **singleton literal types** + member-set `\|` in general type positions (formals included). Equal arity; **no defaults** on multi-clause v0; non-adjacent clauses OK. Domain = specific join of clause domains (no forced widen). Unreachable / duplicate domain / export mismatch → **errors**. Returns: **one** merged return via match/D129-style join; per-clause `-> T` is local expect on that arm. If any clause `export`, all must be. | **Not** C++ overload rank; **not** pattern formals (later optional). Cross-arity, composition order, `extern` multi-clause, fn values — out of v0. **D16** unchanged (std factories separate). Freeze: [`specs/2026-08-11-multi-clause-functions.md`](specs/2026-08-11-multi-clause-functions.md). Gate: `multi_clause`. Sample sketch: `scratch/sample.thx`. |

## 2026-08-10 — Match arm sugar

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D130 | Tag-only bare ctors + `=>` arm bodies | (1) Bare constructor pattern (`Pass`, `Fail`, `Has`, user tags, …) matches **by tag only** — no payload destructure (same as all-`_` binds; heap payload stays on the scrutinee). Payload binds still use `Name(x)` / `Name(_)`. (2) Arm body may be **`=> expr`** (one expression; sugar for a single-expr block) or **`{ … }`** (multi-stmt; newlines). Guards work on both: `Pat if g => e` / `Pat if g { … }`. No commas between arms. **`->` stays type-level**; `=>` is match-arm only. | Supersedes D66 “no bare Fail/Pass without parens (deferred)”. Gate: `match_arm_sugar`. Dogfood: `examples/http` `main` (`Pass => 0` / block `Fail`). |

## 2026-08-10 — Return-type inference

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D129 | Body-inferred return types | Unannotated non-`main` free-function return types are **inferred** from exit types when possible; explicit `-> T` is authoritative and checked. Missing `->` is **no longer** default `Void` for free fns. Unannotated `main` still **`Int64`**. | Residual `?`: structural collapse of success type-args to `Void`, keep residual extract; sum with value exits. Bare `Pass`/`Fail` alone cannot pick a union → annotate. Self/mutual recursion unannotated → annotate (no fixed-point v0). No special-casing the name `Trial`. **LSP hover** shows inferred/`->` returns from last successful lower (local + remote). Freeze: [`specs/2026-08-10-return-type-inference.md`](specs/2026-08-10-return-type-inference.md). Gate: `return_infer`. Dogfood: `examples/http` `run()`. Unannotated methods infer too since **D174**, which also lets `x?` then `Pass(value)` refine the success type. |

## 2026-08-10 — Diverging loop (no break)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D128 | `loop` without `break` diverges | A bare `loop { … }` that contains no `break` targeting that loop is typed as bottom (**Never**): it unifies with any expected type, so it may be the last expression of a function returning `Trial` / `Int64` / … without a dummy tail (`Pass(Void)`, `0`, …). Nested loops' `break`s do not count for the outer. `while` / `for` still yield `Void` (they can end). | Forced by `examples/http` `run()`. Gate: `loop_diverge`. |

## 2026-08-10 — Combined namespace + selective import

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D127 | Combined import clause | `import ns, { a, b as c } from "…"` binds a camelCase **namespace** and **selective** names from the same module in one statement | Order is namespace first, then brace list (not `{…}, ns`). Same binding/export rules as two separate imports. Extends D32/D33. Gate: `import_combined`. Dogfood: `examples/http` Logging/Net. |

## 2026-08-10 — std Io / Net / Http (sample-driven)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D126 | Product `std/Io` + `std/Net` + `std/Http` + `std/HttpStatic` | Promote the mature sample/conformance I/O and HTTP surface into product **`std/`**. **`std/Io`:** `File`, `openFile`/`readFile`/`writeFile`/`appendFile`, OpenMode, seek. **`std/Net`:** TCP `Socket` + `receiveRequest`/`sendAll`. **`std/Http`:** parse/write Request/Response + status helpers. **`std/HttpStatic`:** static-file `Router` (GET/HEAD). Fail payloads remain **String**. Linux product track. | **Shipped:** modules under `std/`; conformance gates retargeted (removed `tests/conformance/{iolib,httplib}/`). Dogfood: `examples/http` Crema-lite. Extends D56/D69/D70/D97/D109+. **Grown by D136** (`Request.query`, helpers, `std/HttpServer`). **Import path amended by D139:** `std/http/static`. Named `ffi` root still deferred. |

## 2026-08-10 — std/Logging + remove ambient println

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D125 | `std/Logging` + no ambient print | Product logging is **`std/Logging`** (not `Log` — reserve **`log`** for logger *instances* later). v0 = **default logger**: five levels (Trace…Error), default **Info** + **stderr** + full format; emit void (no `?`); generic emit named **`emit`** (not `log`); **`toFile` / path `setSink` → `Trial`**; emit write failures best-effort. Multi named loggers / `initLog` **later**. Ambient **`println`/`print` removed**. **App import face:** `import logging, { info, warn, … } from "std/logging"` (**D127** combined; two-line dual import still valid). Golden/data: `toStdout` + `formatPlain` + level fn. Import required. | **Shipped:** `std/Logging.thx` + RT `thoxa_write_line_fd`; session Shell inject loads Logging (+ deps); gates `logging_levels`, `logging_sink` + full conformance dogfood; `examples/http` uses combined import. Freeze: [`specs/2026-08-10-std-log-and-println-removal.md`](specs/2026-08-10-std-log-and-println-removal.md). |

## 2026-08-09 — Structured diagnostics (foundation)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D124 | Structured diagnostics | One **diagnostic IR** (code, severity, message, span, labels, help, expected/actual) drives **pretty `thoxa check`**, **`thoxa check --json`**, and **LSP** (`code` + `relatedInformation`). Editor/human clarity is the ship criterion; agent-only surfaces deferred | **Shipped:** `crates/compiler/src/diagnostic.rs` + enriched `Error`; codes THX001–THX010 (classifier + explicit emit; **THX010** unused-import **warning**); pretty CLI; JSON report with `compiler.version`; LSP code/related/help-in-message + warning severity. Exit/`ok` ignore warnings alone. Freeze: [`specs/2026-08-09-structured-diagnostics.md`](specs/2026-08-09-structured-diagnostics.md). Parked agent ideas: [`ideas/2026-08-09-agent-dx-from-zerolang.md`](ideas/2026-08-09-agent-dx-from-zerolang.md). |

## 2026-08-09 — Fallible import (no ambient prelude)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D123 | May/Trial not ambient | Programs **must** `import { … } from "std/fallible"`; compiler no longer injects Fallible into every module | Refines D118 ambient cutover. Session Shell inject still loads `std/Fallible` into the package (Shell imports Has/Not/May). Types remain ordinary generic unions. Gates: `mays`, `trials*`, `bytes`, `http_*`, `stdlib`. |

## 2026-08-09 — Examples / argv / module-global RC

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D121 | Program argv | `fn main(arguments: Array<String>)` receives process argv; `thoxa run path args…` forwards trailing tokens | Bare `fn main()` still zero-arg. `arguments[0]` is the binary path. Forced by `examples/kilo`. |
| D122 | Module-global end-of-fn drop | **Never** schedule module `mut`/`let` heap places (`g_*`) for function-exit drops | Assign into a global still drops the **old** value and dups the RHS; the global itself outlives the function. Bug: registering globals in `note_drop_place` double-freed `textRows` after `insertChar` (kilo empty-buffer crash). Locals unchanged. |

## 2026-08-08 — Union match guards

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D120 | Union-match guards | `Ctor if bool` / `Ctor(x) if bool` on tagged/literal union arms; binds visible to the guard | Not on catch-all (`else` / bare whole-sum bind). Guarded arms do **not** exhaust a tag (same tag may repeat). Guard fail falls through to later arms. Applies to user unions and May/Trial. Conformance: `sums_match_guard`. Extends D111 + D118. |

## 2026-08-08 — Union identity (nominal atoms · transparent aliases)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D119 | Union identity | **Nominal atoms** (`type Foo` / products always unique by decl) + **transparent union aliases** (`type Outcome = Done \| Quit` ≡ member set `Done \| Quit`) | Equality by **member set** (order irrelevant). Inference yields the set; does **not** search for a matching alias name. True type stays the atom; assignability into a union is membership. Optional opaque/newtype wrapper later. Extends D118. **Shipped:** member-set eq + alias remap + expect on `if` + unannotated if lub (`sums_alias`, `sums_if_lub`). Freeze §2.3: [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md). |

## 2026-08-07 — Sum types / fallibility (target freeze)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D118 | Sum types + general `?` | TS-leaning unions; arm marks `!`/`?`; May/Trial as std generics | **Shipped:** unions + marks + generics + May/Trial as `std/Fallible` unions; cross-module residual rewrap. **Import → D123** (no ambient inject). Gates: `sums_*`, `mays`, `trials*`, `http_*`. **Identity → D119.** Freeze: [`specs/2026-08-07-sum-types-and-fallible.md`](specs/2026-08-07-sum-types-and-fallible.md). |

## 2026-08-07 — Fixed Array locals

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D117 | First-class fixed `Array<T,N>` locals | Locals with `Array<T,N>` annotation + array lit of length N; element get/set; ABI scalar elems only | Extends field-only **D27**. **Whole-array assign/bind** via element-wise copy in cgen (C arrays not first-class). Lit elems coerce to `T`. Gates: `fixed_array_local`, `fixed_array_assign`. |

## 2026-08-07 — Default-arg defining-module scope

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D116 | Default-arg name scope | Free names in default exprs resolve in the **defining** module (not the call site) | Defaults still **lower** at the call site (D98/D106 codegen), but `name_to_*` maps swap to the definer for those exprs. Applies to free fns, methods, and ctor field defaults. Conformance: `defaults_mod`. |

## 2026-08-07 — File seek/position

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D115 | File seek / position | `File.seek(offset, whence = seekSet)` and `File.position()` → `Trial<Int64, String>`; export `seekSet`/`seekCur`/`seekEnd` | Was `iolib/Io.thx`; product home **`std/Io`** (**D126**). Closed file → Fail. Conformance: `file_seek`. |

## 2026-08-07 — Nested Trial as payload

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D114 | Nested Trial as payload | Allow **`Trial<…>`** as Trial ok/err and May inner types | Completes D113. cgen interleaves May/Trial monomorph typedefs by nest depth (`May<Trial>` and `Trial<May>` both work). Address still not a payload. Conformance: `trials_nested_trial`. |

## 2026-08-07 — Nested Trial/May payloads

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D113 | Nested Array/May payloads | Allow **dynamic `Array<T>`** and **`May<U>`** as Trial ok/err and May inner types | Nested **Trial → D114**. Lexer `>>` in type args is split into two `>` closers. cgen monomorph typedefs by nest depth. `Not()` outside `-> May` defaults to `May<Int64>` (so `Pass(Not())` works when ok is that monomorph). Conformance: `trials_nested`. |

## 2026-08-07 — File write/create

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D112 | File write/create | Free `writeFile` / `appendFile`; `File.write`; open flags `oCreat`/`oTrunc`/`oAppend` + `sysOpenMode` (3-arg open) | Was iolib; product **`std/Io`** (**D126**). `openFile` optional `mode` (default 0644). Int32 fd → `File(fd: Int64)` (**D21**). Conformance: `file_write`. |

## 2026-08-06 — Language: match guards/binds/or, File, write-back, defaults

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D111 | Value-match guards | `pattern if bool_expr { … }` on literal/or arms; cond = pattern match **and** guard | Not on catch-all (`else`/bare bind). Not on Trial/May. Same lit may repeat when guards differ. Conformance: `match_guard`. Extends D87. |
| D110 | Value-match bare bind | Final catch-all arm may be a bare name `n { … }` binding the scrutinee (instead of `else`) | `_` as bare pattern is discard catch-all (same as `else`). Not for Trial/May. Conformance: `match_bind`. Extends D87. |
| D109 | File type (product Io) | `export type File(fd: Int64)` + `openFile` / `readAll` / `close` / `isOpen`; `readFile` reuses File | Product **`std/Io`** (**D126**; was conformance iolib). `fd` Int64; close write-backs `fd = -1` (**D107**). Conformance: `file_type`. **Write → D112**. |
| D108 | Value-match or-patterns | `0 \| 1 \| 2 { … }` of string/int/bool literals; required `else` still | Lowers to `||` of equality tests. Not for Trial/May arms. Duplicate leaf lits rejected across arms. Conformance: `match_or`. Extends D87. |
| D107 | Method mut-field write-back | In methods, bare assign to a ctor-param field (or C-layout `mut` field) updates the **receiver** | C ABI: User method receivers are pointers (`Type *`). Name receivers pass the binding place (not a `needs_drop` temp). Heap FieldAssign drops old field + dups new. Unlocks File-type style `fd = …` in methods. Conformance: `method_field_writeback`. |
| D106 | Defaults on methods and ctors | Same rules as free-function defaults (D98): `name: T = expr`; omit at call; lowered at **call site** | Constructor params and methods. Ordering: no required formal after a defaulted one. **`!` / extern defaults allowed** (call-site fill; C still receives all args). **Name scope → D116**. Gates: `param_defaults`, `param_bang_defaults`. |

## 2026-08-05 — Session libtcc step backend (target)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D105 | REPL / session-step C backend | **In-process libtcc only** (`TCC_OUTPUT_MEMORY`); no compiler spawn, no step `.so` | Vendored TinyCC under `vendor/tinycc` (Linux x86_64). Batch + `libthoxa_rt` stay system `cc`. Legacy `cc`+dlopen escape **removed**. Decl-stable cache: re-lower `Shell.step`, cached base image (freeze §3.5; in memory, linked by symbol since **D169**). Freeze: `docs/specs/2026-08-05-session-libtcc.md` (removed by D227; at `eefdac4`). **Superseded by D227.** |

## 2026-08-05 — Session pipes, redirs, pure-step, std Shell

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D101 | Session OS pipelines | Command sugar `a \| b` → `shell = shell.pipeline(blob)`; RT `thoxa_pipeline` (pipe/fork/execvp/wait); status = last stage | Blob: NUL-terminated argv words per stage, empty field (double-NUL) between stages. Helpers: `sessionEncodeArgv` + `sessionPipelineBytes`. Pure int BitOr (`1 \| 2`) stays Thoxa. Conformance: `session_pipe`. **Superseded by D227.** |
| D102 | Session file redirections | Command sugar `>` / `>>` / `<` → `shell = shell.runIo(argv, in, out, append)`; RT `thoxa_spawn_wait_io` | Empty path strings inherit. Redir ops as separate words. Pure int comparison (`1 > 0`) stays Thoxa. No `2>`; no redir on pipeline stages yet. Conformance: `session_redir`. **Superseded by D227.** |
| D103 | REPL pure step only | Remove host fast paths for `status` / `cd` / `\|`; all executable chunks use session step path | In-process `chdir` still updates host cwd. Unit: `repl::pure_step_*`. Step **backend** target → **D105**. **Superseded by D227.** |
| D104 | Session Shell in product std | Default `Shell` + helpers live in `std/Shell.thx`; session inject prefers on-disk std, compile-time `include_str` fallback | Replaces compiler-only `session_prelude.thx`. Still auto-injected for session scripts (not required `import`). Shell imports Fallible; inject loads `std/Fallible` into the package (**D123**). **Superseded by D227.** |

## 2026-07-25 — Session `path()` hook

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D100 | Session PATH hook | Optional `fn path(parent: Array<String>) -> Array<String>` in rc/decls; host runs after rc and on decl change via **session step image** (stdout capture), not a child binary | `parent` = current process PATH split on `:`. Return components **replace** process PATH (join `:`); empty `[]` clears PATH. No merge unless body appends `parent`. Host captures pre-hook PATH as `THOXA_TOOLCHAIN_PATH` / `capture_toolchain_path` so `cc`/`ar` still resolve when session PATH is empty/minimal. Unit: `repl::tests`. Backend is libtcc (D105). **Superseded by D227.** |

## 2026-07-25 — Parameter defaults + std color hex

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D98 | Parameter defaults (M3 subset) | `name: T = expr`; omit at call (named or trailing positional); defaults lowered at **call site** | Originally non-`!` only; **now includes `!` and `extern`** (expand before C call). Non-default formals may not follow a defaulted one. Methods/ctors → **D106**; free-name scope → **D116**. Gates: `param_defaults`, `param_bang_defaults`. |
| D99 | Hex color → SGR | `std/Ansi.parseRgba` / `fgHex` / `bgHex`; primary `#RRGGBB`, also `#RRGGBBAA` (optional `#`); alpha optional, ignored for SGR (`a=255` if omitted) | Invalid hex → black SGR. `defaultPrompt(signature: String = "#3B9DC2")` uses `fgHex` for user@host. |

## 2026-07-24 — Session / REPL (Thoxa is the session language)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D88 | Session product shape | Bare `thoxa` / `thoxa repl` = session REPL; `thoxa path` runs program (`main`/`main.thx`) or **session script** (no `main`); subcommands unchanged | Plan: `docs/specs/2026-07-24-thoxa-session-repl.md` (removed by D227; at `eefdac4`) **Superseded by D227.** |
| D89 | Session value | `type Shell(status: Int64)` + methods (`echo`/`pwd`/`cd`/`run`/`runIo`/`pipeline`/…) | Real Thoxa; status field (not bash `$?` core). Product file: `std/Shell.thx` (**D104**). **Superseded by D227.** |
| D90 | Session code placement | Decls stay module-level; top-level stmts → `Shell.step` body; synthetic `main` returns `step().status` | `Item::SessionStmt`; `session::prepare` **Superseded by D227.** |
| D91 | In-function import/type | Not required; host lifts / top-level decls only | Deferred general nested modules |
| D92 | Command sugar | Thoxa-first; bare word/string sequences desugar to `shell = shell.*`; **pipes/redirs before Thoxa-first** so `|`/`>` are not kept as BitOr/Gt | `command.rs`; quotes from thoxash v3. Command-shaped parse trees include path Div, **flag Sub/Unary** (`ls -al`, `--all`); pure literal arith (`1 - 2`), pure int BitOr (`1 \| 2`), pure int Gt (`1 > 0`) stay Thoxa. Pipes → **D101**; redirs → **D102**. **Superseded by D227.** |
| D93 | REPL execution | Host loop: **decls accumulate**; host-held status threaded into `Shell.step`; entry remains `thoxa_session_step(ThoxaShellState*)` | Step backend → **D105** libtcc only. Was v1 compile+exec child; was cc+dlopen. Fast paths removed → **D103**. **Superseded by D227.** |
| D94 | Script execution | Splice + synthetic `main` + normal executable | No dlopen **Superseded by D227.** |
| D95 | extras/shell | Dogfood prototype of Shell API; product face is bare `thoxa` | `cargo make shell` → bare `thoxa` **Superseded by D227.** |
| D96 | Session rc + prompt | Interactive REPL loads `$HOME/.config/thoxa/shell.thx` (missing = no-op). Decls accumulate; stmts run once. Optional `fn prompt() -> String` — host rebuilds a prompt **session step image** when decls change and **calls** it every primary prompt (stdout captured). Host sets `THOXA_STATUS`. Default `§ `; continuation `> ` fixed. No startup banner. Overrides: `THOXA_SHELL_RC`, `THOXA_NORC` | Real Thoxa (not a second grammar). Piped REPL / session scripts do not load rc. Optional `fn path` → **D100**. Default was `$ ` until 2026-09-04. Step backend is libtcc (D105), not `.so`/dlopen. Form split and desugar nesting skip `#` line comments (an apostrophe in `## not Thoxa's` does not glue later decls). **Superseded by D227.** |
| D97 | Product `std/` root | Import `"std/Module"` resolves under product std tree (`THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to `thoxa` binary). Modules: `Shell`, `Str`, `Path`, `Ansi`, `Env`, `Git`, `Prompt` (`defaultPrompt`). `ffi` still deferred. **Growth is sample-driven** — not a standalone “expand std” todo | Partial unlock of D30 named roots for session prompts + shared helpers. Conformance: `stdlib`. Session Shell → **D104**. **Superseded by D227** for its session modules (`Shell`, `Prompt`); the `std/` root resolution stays. |

## 2026-07-21 — Value match (string / int / bool + else)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D87 | Value `match` patterns | String / Int64 / Bool **literal** arms + required final `else` | SPEC §7.4 subset. Lowers to `if scrut == lit` cascade (`string_eq` for String). Duplicate lits rejected; arms after `else` rejected. Trial/May still Pass/Fail or Has/Not only (exactly two arms). No bare binding pattern / or-patterns / guards yet. Conformance: `match_value`. Dogfood: thoxash `evalLine`. |

## 2026-07-21 — thoxash spawn helper

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D86 | External process spawn (dogfood) | RT `thoxa_spawn_wait(path, argv_blob, argv_len)` — fork/execv/waitpid | Array not FFI-marshalable; argv is consecutive NUL-terminated strings in `Bytes`. Child exec fail → exit 127. Parent returns exit status / 128+sig / -1 on fork fail. Used by `extras/shell` only for now; not a language surface. **Superseded by D227.** |

## 2026-07-20 — May / Has / Not + Bytes.find

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D85 | `May<T>` | Surface `Has(x)` / `Not()`; match / `?` | **Refined by D118 + D123:** `std/Fallible` union `May<T> = Has<T!> \| Not?` (import required; not a builtin layout). Nested Array/May → D113. No May↔Trial `?` mixing. |
| D46′ | `Bytes.find` | `find(needle[, from]) -> May<Int64>` | **Supersedes D46** (`-1` sentinel). Runtime still returns -1; MIR wraps to `Has(i)` / `Not`. Caller needs `May` in scope (import `std/Fallible`). Empty needle → `Has(from)` (in range); missing → `Not`. Conformance: `mays`, `bytes`. |

## 2026-07-20 — Optional empty type body

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D84 | Type body braces | `{}` optional when a type has no body fields and no methods | `type Header(name: String, value: String)` and bare `type Marker` are valid; empty `{}` still accepted. Body-field (R7) and method-bearing types still need `{ … }`. |

## 2026-07-20 — LSP: library modules need no main

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D83 | Check `require_main` | Buffer defines `fn main` → require it; else library check (`LowerOptions.require_main = false`) | Fixes false “program requires `fn main`” on library modules (was iolib/httplib; now `std/` libs + open buffers). Build/run still require main via `lower_package`. |

## 2026-07-20 — post-R9 PR9: Io OpenMode + errno polish

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D80 | errno / strerror without `ffi` root | `thoxa_errno()` in `thoxa_rt` + `extern fn … from "c" "strerror"`; Fail via `fileErr` / **`netErr`** (`"{what}: {strerror(errno)}"`) | No named `import { errno, strerror } from "ffi"` (D30 still deferred). Path included in open/seek/read tags. Net socket Fail tags: `http_net_err`. Conformance expects English ENOENT / EBADF text. |
| D81 | OpenMode (native) | Was Int32 lets only | **Refined:** nullary union `OpenMode = Rdonly \| Wronly \| Rdwr` + `openModeBits` in **`std/Io`** (**D126**); Int32 `oRdonly`/`oWronly`/`oRdwr` for bitwise OR with creat/trunc/append. Gate: `file_openmode`. |
| D82 | Extern String return | C `char*` → `thoxa_string_from_cstr(…)` in cgen | Completes D20 marshal for String *returns* (args already used `thoxa_string_cstr`). |

## 2026-07-20 — Editor PR7: cross-module + open-buffer overlay

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D78 | Cross-module hover/def | Lazy `lsp/crossmodule::resolve_remote` at hover/definition time | Selective imports (incl. `as`), namespace name (`module <spec>` + leading `##`), namespace member (`ns.x`); **typed members on values from namespace + `?`** (`server.reuseAddr` → remote `Socket` method). Jump URI = defining file; open buffer wins over disk (path-matched). Named roots (`std/`) covered for go-to-def. Failures → local fallback (never crash). |
| D79 | Open-buffer package check | `check_source_with_overlays` re-parses any package module whose path is open | Entry buffer always overlaid; other open docs replace disk AST for `lower_package`. Parse fail on overlay → keep disk AST. Untitled/non-path still cannot check imports. |

## 2026-07-20 — Editor PR6: navigation handlers

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D77 | LSP navigation (single-file) | `hover` / `definition`≡`declaration` / `references` / `documentSymbol` / `prepareRename`+`rename` via `lsp/index` + resolver | Hover: source-line signature + contiguous `##` docs; **call-arg formals** (`listen(16)` → `backlog: Int64`); `?` unwraps Trial/May for typed members. Rename: binding-id keyed; rejects builtins; PascalCase types / camelCase values. Cross-module jump: **D78**. |

## 2026-07-20 — Editor PR5: shared lexical resolver

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D76 | Lexical resolver | Standalone `resolver::resolve_module` / `resolve_source` → `Resolution` (bindings + references) | Type-independent scopes; member refs **pending** until type-driven link. Kinds: fn/extern/type/field/method/param/let/mut/forvar/pattern/import/builtin. Single-file: imports bind locally (jump-to-import). `lsp/index::build_index` for navigation. **MIR lower still uses its own env** this PR (conformance unchanged); full types-by-binding lower migration deferred. Position API: UTF-8 spans + `span_covers` / LSP 0-based line+UTF-16 char. |

## 2026-07-20 — Editor PR3: extras/ (grammar + Zed)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D71 | DX glue root | **`extras/`** with `tree-sitter-thoxa` + `zed` | Non-product personal DX; not mixed into `crates/`. Room for future `extras/vscode` etc. |
| D75 | Thin grammar | Lexer-level tree-sitter only (keywords, casing, literals, comments, operators); no full CST | Churn isolation. Semantic tokens from `thoxa lsp` refine function/variable. Committed `src/parser.c`; Zed pins `[grammars.thoxa].rev` to a commit that contains it (`file://` repo path). Highlighting islands (not a CST): JSX tags; glued `sql"…"` with SQL injection over `sql_text` (`injections.scm`). |

## 2026-07-20 — Editor PR2: `thoxa lsp`

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D72 | LSP CLI | `thoxa lsp` — Language Server Protocol over **stdio** (Content-Length JSON-RPC); not interactive | Phase 1 caps: `textDocumentSync: Full` (1), `semanticTokensProvider` (full + legend). `publishDiagnostics` on didOpen/didChange; clear on didClose. Handlers catch panics/errors → stderr + empty/error result; loop stays up. Also `thoxa check <file\|dir>` for disk DX (exit 1 on diags). Navigation deferred (phase 2). |
| D74 | JSON deps | `serde` + `serde_json` on `thoxa` compiler crate | Required for LSP payloads; zero-dep JSON rejected (E8). |

## 2026-07-20 — Editor PR1: check without cgen

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D73 | Check API | `lsp::check_source(filename, text) -> Vec<CheckDiagnostic>` — lex → parse → MIR lower only (no cgen/`cc`) | First diagnostic only (compiler `Error`); `Vec` ready for multi later. Spans: UTF-8 byte offsets → LSP `Range` via `span::{byte_to_position,span_to_range}` with **UTF-16** character units. **Imports:** no `import` → single-file lower; imports + path → `load_package` + open-module overlay; open-buffer deps via **D79** `check_source_with_overlays`. **Main:** buffer with `fn main` → `require_main`; library modules without main typecheck freely (**D83**). |

## 2026-07-20 — Native editor / LSP plan

Plan: [`docs/specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md). Phase 1 complete (D71–D75). Phase 2 complete (PR5–PR7, D76–D79).

| Planned | Choice | Status |
|---------|--------|--------|
| DX glue root | `extras/{tree-sitter-thoxa,zed}` | **D71** |
| Thin grammar | lexer-level only | **D75** |
| LSP server | `crates/compiler/src/lsp` + CLI `thoxa lsp` | **D72** |
| No Python | Oracle editor/LSP is reference only | locked |
| Phase 1 | diagnostics + semantic tokens + thin grammar + Zed | **complete** |
| Check engine | parse + MIR lower (no cgen) | **D73** |
| Lexical resolver | `resolver` + `lsp/index` | **D76** |
| Phase 2 nav | hover/def/refs/outline/rename | **D77** |
| Cross-module | open-buffer overlay | **D78–D79** |
| JSON | `serde` + `serde_json` on compiler crate | **D74** |

## 2026-07-20 — Directory entry `main.thx` (PR8)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D68 | Directory entry | `thoxa run\|build\|emit-c <dir>` resolves `<dir>/main.thx`; project root for `~/` is that directory (entry file’s parent — same as D31) | **Amended by D138:** file is `main.thx` (was `Main.thx`). Missing `main.thx` → clear error. File path mode unchanged. Default binary stem for a dir is the directory name (not `main`). Conformance harness also discovers top-level package dirs with `main.thx` + sibling `<name>.out`. Conformance: `pkgmain`. Oracle `export fn main` contract not enforced yet (entry still any `fn main`). |

## 2026-07-20 — Bounded multi-request static server (PR7)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D67 | Bounded server conformance | Single-process loop: listen on ephemeral `bind(0)`; for each of N fixed paths, client `connect(localPort)` then server `accept` → `receive` → `parseRequest` → `Router.handle` → `writeResponse` → `send` → close peer+client; then close listen socket and exit | No threads; no infinite accept loop (CI-safe). N≥2 with ≥1×200 and ≥1×404. Deterministic stdout: `"{path} {status-line}"` + `server-ok`. Integrates Net + Http + Router over `tests/conformance/io_data/web/`. Conformance: `http_server`. Closes post-R9 HTTP critical path; oracle infinite `Main` accept loop remains product-only. |

## 2026-07-20 — Router type (PR6)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D70 | Router type | `export type Router(root: String) { export fn handle(req: Request) -> Response }` | Replaces free `handle(root, path) -> Bytes` (D61). Segments + `..`/`.` reject + index.html + Content-Type. Absolute `/` path. Product home **`std/HttpStatic`** (**D126**; was httplib). Conformance: `router`. MIME subset `.html`/`.css`/`.txt`/octet-stream. |

## 2026-07-20 — Request / Response / Header (PR5)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D69 | HTTP protocol types | `Header(name,value)`, `Request(method,path,version,headers,body)`, `Response(status,reason,headers,body)`; `parseRequest(raw) -> Trial<Request,String>`; `writeResponse(res) -> Bytes`; `ok`/`notFound`/`badRequest` → `Response` | Constructor-param fields may be `Array<T>` (heap RC via existing User field walk); `user.arrField[i]` loads dynamic Array then indexes (FixedArray peel unchanged). Headers: simple `Name: value` until blank; body always empty at parse. `Bytes.find` → `May<Int64>` (**D85**). **`summarizeRequest` is intentionally an adapter over `parseRequest`** (requires `\r\n\r\n`; path is query-stripped) — not the pre-PR5 request-line-only scan. Empty method/path/version tokens not rejected (oracle parity; tighten later if server forces). Phase-A RC may over-dup on `Trial<Request>` / `Array<Header>` (LSan noise; no UAF; not a conformance gate). Conformance: `http_parse`. **Amended by D136:** `Request.query` field; helpers + `std/HttpServer`. |

## 2026-07-19 — Post-R9 HTTP plan (PR1)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D63 | Early `return` | `return` / `return expr` as expression (like `break`); type-checked against enclosing `fn`/`method` return type | Bare `return` only for `Void`; MIR `Stmt::Return { value: Option<Place> }`; drop live `needs_drop` locals before early return / `?` Fail (same set as end-of-fn `insert_function_drops`; heap params included as of **D154**); exits function from inside loops. **No ASI:** optional value is parsed whenever the next token can start an expression (newline does not end a bare `return`); write `return` then `}` / non-expr, or put the following statement after a non-expr-start token — intentional `return expr` may span lines. |

## 2026-07-20 — Array&lt;User&gt; elements (PR3)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D65 | `Array<User>` storage | `ThoxaArray { len, cap, flags, elem_size, data[] }`; User elems **inline** (`elem_size = sizeof(C struct)`) | Int64/String keep `elem_size=8` + HEAP_ELEMS; User uses cgen `ThoxaArrayElemFn` field RC (dup/drop helpers). Constructor-param User only (Int64/Bool/String/Bytes fields). Conformance: `array_user` |

## 2026-07-20 — User+heap field RC (PR2)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D64 | User+heap RC | `needs_drop` (not `is_heap`) for Trial Pass/Fail/`?`/match extract; method field prolog Dups heap fields; match binds use fresh places | cgen `emit_rc` already field-walks User; MIR must insert balanced Dup/Drop. Unblocks `Trial<User+String>` / File-shaped APIs. Conformance: `user_rc` |

## 2026-07-20 — Match discard binding

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D66 | Match discard `_` | `Pass(_)` / `Fail(_)` — pattern `_` does not introduce a binding | Standalone `_` lexes as ordinary `Ident("_")` (usable as a normal name in `let`/params/etc.); **only match patterns special-case it** (`bind: None`). A `_` in the arm body is ordinary name lookup (error if unbound; resolves an outer `let _` if present — discard does not shadow). Payload still extracted + RC-owned for end-of-fn drop; **bare tag without parens → D130** (was deferred grammar churn); `_foo` still rejected (not an ident start) |

## 2026-07-19 — R9+ file I/O + Router

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D56 | File surface | Free `readFile(path) -> Trial<Bytes, String>` | Subset of oracle `Io.thx`; whole-file via lseek size; **OpenMode constants shipped (D81)**; **File type → D109**; **write/create → D112** |
| D58 | File errors | Was simple `Fail("open"\|"read"\|"seek")` | **Superseded by D80** — path + strerror via RT errno + libc extern |
| D59 | User zero-init | Constructor-param User with String/Bytes fields zero as NULL heap | Match out-places; **field-RC hardened in D64** |
| D60 | String equality | `==` / `!=` via `thoxa_string_eq` (length + bytes) | Ordering compares not supported |
| D61 | Static Router | `handle(root, path) -> Bytes` HTTP response | Segments + `..` reject + index.html + Content-Type; **superseded by D70** (`Router` type + `Response`) |
| D62 | for-in RC | Drop previous loop-var each iter; last value dropped at fn end | Fixes double-drop of last heap element (was drop-per-iter + fn-end drop) |

## 2026-07-19 — R9+ dynamic Array

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D53 | Dynamic `Array<T>` | Heap RC `ThoxaArray*` (slots as `int64_t`; heap elems bitcast) | Distinct from fixed field `Array<T,N>` (D27); elems: Int64, Bool, String, Bytes; **User via D65** |
| D54 | Array mutation model | Functional `append`/`dropLast` return new arrays; index set mutates shared storage | **Amended by D133** (Swift/Buffer write-back; `let` aliases do not see writes). Historical oracle-list note no longer product. |
| D55 | Array / Bytes splice surface | Literals, empty `[]` + type ann, `.count`, `[i]`, append, dropLast, **`insertAt`/`removeAt`**, `for x in arr`, display; Bytes **`insertAt`/`removeAt`** + **`equals`** | Functional copy (source unchanged); insert `0 ≤ at ≤ count`, remove `0 ≤ at < count` (else panic); byte masked 0–255. Gates: `arrays`, `array_user`, `splice`. |

## 2026-07-19 — R9+ Buffer

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D50 | Growable Buffer | Heap RC `ThoxaBuffer*` with COW mutate | Geometric cap 0→8→double; `append`/`appendBytes` return unique ptr (MIR write-back on name recv); not FFI-marshalable |
| D51 | Buffer surface | `Buffer()` / `Buffer(cap)`, `.count`/`.capacity`, append, appendBytes, freeze→Bytes | `freeze` copies live prefix (consume-on-freeze deferred); `reserve` deferred |
| D52 | Buffer vs Bytes | Buffer builds; freeze → fixed Bytes for FFI/send | Matches D17; no Buffer at extern boundary |

## 2026-07-19 — R9+ control flow + String fields

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D47 | Loops (subset) | `while`, `loop`, `for x in lo..hi` / `lo..=hi`, `break`, `continue` | Loops yield `Void`; **`for x in arr` shipped with D55**; `continue` in range-for hits C for-step |
| D48 | Constructor-param field types | Int64, Bool, **String**, **Bytes** | C-layout body fields remain ABI-only. **Amended:** dynamic `Array` (D69), `Map` (D134), **Node + nested User / user sums (D135)**, **Float64 (D140)**. |
| D49 | RC on user values | `needs_drop(User)` always; cgen walks heap fields | Dup after `StructLit` and heap field loads; method `_self` is a param (not auto-dropped) |

## 2026-07-19 — R9+ Trial monomorphs + wire I/O

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D40 | Trial representation | Was `Ty::Trial` monomorph C structs | **Refined by D118 + D123:** `std/Fallible` `Trial<P,F> = Pass<P!> \| Fail<F?>` (tagged user union monomorphs; import required) |
| D41 | Trial payloads | Void, ints, Bool, String, Bytes, User | **Array + May → D113**; **nested Trial → D114**; Address still deferred |
| D42 | Pass/Fail typing | Prefer enclosing `-> Trial<…>` annotation | Unannotated Pass/Fail(Int) still infers `Trial<Int64,Int64>` when that mono exists |
| D43 | `?` across monomorphs | Same residual payload; rewrap into outer Trial/May | Cross-module Fail monomorphs rebuild by arm name (D118) |
| D44 | Trial RC | Dup live heap side on copy; drop live side on drop | Sum-aware `emit_rc`; `assign_no_drop` for sum-payload temps |
| D45 | Wire conformance | One-shot listen/connect/accept/recv/send | Ephemeral port via `getsockname`; no infinite accept loop |
| D46 | `Bytes.find` | Was Int64 / `-1` if missing | **Superseded by D46′ / D85** — now `May<Int64>` |

## 2026-07-19 — R9 HTTP subset

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D36 | R9 scope | Subset, not full oracle server | Multi-module Net + Http; offline parse/response; socket lifecycle (create→reuse→bind→listen→close). No accept loop, file router, Buffer, dynamic Array |
| D18′ | `Bytes(n)` | Ship zero-filled fixed buffer | Forced by sockaddr / setsockopt packing; locks D18 “when forced” |
| D37 | `Bytes.slice(start, end)` | Half-open copy → new Bytes | Enough for request-line parse without `find` / Array; OOB panics |
| D38 | Socket errors (R9) | Was `Trial<Int64,Int64>` codes | **Superseded by D40–D45** — String / User Trial + wire I/O |
| D39 | Net surface | Private `extern` + `export type Socket` / `createSocket` | Linux sockaddr_in pack by hand; port `0` = ephemeral for CI |

### Notes

- Oracle `examples/http` remains the north star; R9 proves modules + FFI + Bytes integration without inventing stdlib roots (`ffi` / `std/`).
- errno/`strerror`: **shipped for Io and Net (D80)** without named `ffi` root.
- Constructor-param types were Int64/Bool only at R9 → `Socket(fd: Int64)`; **String/Bytes fields shipped later (D48–D49)**.

## 2026-07-19 — R8 modules / multi-file

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D30 | Import origins (R8) | `./…` and `~/…` only at ship; **D97** adds `std/…` | `ffi` still deferred |
| D31 | Resolution | No `..` segments; `~/` = package root | Package root is the nearest ancestor `main.thx` when present, else the entry file’s directory. Matches oracle “never climb”. Nested-file `thoxa check` / LSP use the same root so `app/Pages.thx` can `~/framework/…`. |
| D32 | Export surface | `export` on top-level `fn` / `type` / `let` / `extern`; `mut` never exported | Selective + namespace imports |
| D33 | Namespace import | `import ns from "…"` camelCase binding; members via `ns.x` | Not a first-class value. Combined form → **D127** |
| D34 | Module-level values | Static C globals + `thoxa_module_init()` before main | Dep-order init; private mut state |
| D35 | Name mangling | Non-entry: `{Module}_{name}` types/fns; globals `g_{Module}_{name}` | Entry keeps unprefixed names; `main` special |

## 2026-07-19 — R7 C-layout struct / packing

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D23 | Struct layout default | Declaration-order, natural C alignment (markerless) | Matches oracle LF-B; no `repr(C)` |
| D24 | Body-field types | `type T { [mut\|let] f: Ty }` with ABI scalars / `Array<T,N>` | Separate from R3 constructor-param types |
| D25 | Construction | `T()` zero-inits C-layout types | No field args; R3 still uses named constructor args |
| D26 | `Address<T>` | Extern params only; implicit `&` on mut User places | Explicit `address()`, storing Address deferred |
| D27 | Fixed arrays | Field-only `Array<T, N>`; element get/set via `[i]` | Locals → **D117**. Whole-array assign → D117 element copy; no slice |
| D28 | New scalar widths | UInt32, Byte (=UInt8); bitwise on integers in Int64 then truncate on field store | Hex `0x…` literals. **Amended by D140:** `Float64`. |
| D29 | Struct value-copy / `sizeof` | Deferred | Oracle LF-B §9; kilo re-reads via tcgetattr |

### Notes

- Field write is a typed width boundary (truncate). Bitwise ops promote to Int64 for the op.
- Passing a non-`mut` binding where `Address<T>` is required is a compile error.
- C codegen emits real C structs (`uint32_t`, `uint8_t f_cc[4]`, …) so padding matches the platform ABI (Regs = 16 bytes on x86-64).

## 2026-07-19 — R6 extern libc (subset)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D19 | Extern libraries (R6) | `"c"` only | Headers (`unistd.h`, `stdlib.h`, `string.h`) supply prototypes |
| D20 | Extern types (R6) | Int32, Int64, UInt64, String, Bytes, Void | R7 adds UInt32, Byte, `Address<T>` |
| D21 | Int width at boundary | Coerce Int64↔Int32/UInt64 at args; widen ints for `==`/`<` etc. | Literals stay Int64; R7 extends to UInt32/Byte. Narrowing is checked since **D167**. |
| D22 | C symbol rename | `from "c" "symbol"` | When Thoxa name ≠ C name |

---

## 2026-07-19 — Bytes construction sugar (R5 follow-up)

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D14 | String → Bytes surface (now) | Keep `Bytes.fromString(s)` | Matches `apocrypha/old/ThoxaPython` SPEC §9.1; R5 ship shape |
| D15 | `"…" as Bytes` | **Rejected** (not deferred) | Encode/copy is not a cast. Named `fromString` is the product surface forever for String→UTF-8 Bytes. Do not reintroduce as sugar under M4 either. |
| D16 | Overloaded `Bytes.from(…)` | Reject (for now) | No ad-hoc overloading story; prefer explicit names if more factories appear. **D131** is the deliberate multi-clause language feature — does **not** by itself reopen std `from` overloads; revisit factories only in a separate decision after D131 ships. |
| D17 | Multi-byte / dynamic build | Prefer `Buffer` path later | `append` / `freeze` → `Bytes`; not `from([1,2,3])` as the main story |
| D18 | Extra factories | Only when forced | e.g. `Bytes(n)` zero buffer for FFI (R6 pressure); maybe `fromArray` when arrays exist |

### Analysis (why)

**`mut b = "abc" as Bytes` — rejected**

- String→Bytes is **encode/copy** (UTF-8 text → raw buffer), not a cast or rebrand.
- Rust-style “cast” is the wrong model; named construction (`fromString`) keeps encoding explicit.
- A general `as` conversion system (if ever) still should not pretend text↔bytes is free or total both ways; Bytes→String needs UTF-8 policy.

**`Bytes.from("abc")` with overloads (`from([1,2,3])`, …)**

- Friendly, but SPEC already names `fromString`. Renaming reopens the oracle.
- Overload sets need either ad-hoc resolution or a `From`-like trait; Thoxa has neither.
- Bare `from` hides source/encoding (UTF-8 text? raw ints? size?).
- Array form needs array literals + element rules — later rungs.
- If multiple sources appear, prefer **distinct names** (`fromString`, `fromArray`, `Bytes(n)`), same spirit as Buffer’s `append` vs `appendBytes`.

**Rule of thumb**

- Domain construction from a specific source → **named static method** (`fromString`).
- Multiple input shapes → **multiple names**, not silent overload, until overloading is a deliberate language feature.
- Do **not** use cast sugar for String→Bytes (**D15 rejected**).

Revisit D17 with Buffer / growable-buffer design; revisit D18 when more factories are forced.

---

## 2026-07-18 — Kernel kickoff

| ID | Decision | Choice | Notes |
|----|----------|--------|-------|
| D1 | Growth model | Example ladder toward HTTP | Small kernel first |
| D2 | Algebraic effects | Deferred | Revisit with async; may later interact with general `?` (D118) |
| D3 | Error model | `May`/`Trial` + force-to-deal | Not effect-`exn`. **D118 + D123:** May/Trial as `std/Fallible` unions (import required) + general `?` via type marks — freeze `2026-08-07-sum-types-and-fallible` |
| D4 | Value model | Hybrid (A) | Unboxed scalars; inline small sums; RC heap; C-layout FFI structs |
| D5 | Phase A backend | Compile-to-C | No QBE yet |
| D6 | Compiler host | Rust under `crates/compiler` | |
| D7 | Runtime | C `thoxa_rt` | Headered RC objects |
| D8 | RC ambition day one | Simple correct RC | Perceus later |
| D9 | Docs path | `docs/specs/`, `docs/decisions.md` | No `docs/superpowers`; no graph until needed |
| D10 | Build automation | `crates/make` xtask, `cargo make …` | No project `.sh` scripts |
| D11 | Build products | Single `target/` tree: `rust/`, `c/`, `thoxa/` | No separate `targets/`; Cargo uses `target/rust` via `.cargo/config.toml` |
| D12 | Language oracle | `apocrypha/old/ThoxaPython` SPEC + conformance | Reopen syntax only deliberately |
| D13 | Old QBE tree | Reference only | Do not revive megapass codegen |
```