# Architecture — as-built system map

**Purpose:** What exists in the product tree today — pipeline, crates, runtime,
and build layout. Target design lives in freezes under [`specs/`](specs/).
Priority lives in root [`CURRENT.md`](../CURRENT.md). Capability
maturity lives in [`capabilities.md`](capabilities.md).

**As of:** 2026-10-19 (**D245**: `mir/order.rs` `show_value` writes a value's text, `nested` for `showNested` (a `String` through `quoted()`, a derived type structurally with its union literals quoted); a derived `show` gets a derived `showNested` (`trait_desugar::default_member`, `__showNested(self)`), and a product's fields are written with `__showNested`; interpolation of arrays and maps goes through std (`Array.show`, `Map`'s `Show` conformance in std/core), and the runtime's array and map formatters are gone; **D243**: `mir::type_as_value` names a type written as a value; `trait_desugar::unimported_type` reports a conformance member's type its module does not import (`mir::unknown_type_error`); `mir::check_generic_labels` runs the label rule before `generic::instantiate_call` infers; **D242**: `Parser::at_postfix` takes a call's `(` or an index's `[` only off a line's start; **D241**: `lsp::analysis::semantic_tokens` marks placed keywords (`placed_keyword`, tracking an accessor's braces for `field`); the Zed and grammar `highlights.scm` colour them with `#any-of?`; the site's `highlight.thx` decides them by line context (`placed`); **D240**: `trait_desugar::check_conformance` refuses a generic type's body-less `Encodable` / `Decodable` conformance (`OwnMembers::reads_json`); `validate_desugar::encodable_ty` accepts a generic application whose type writes `toJson`; the MIR's `encode` falls back to `toJson().encode()`; **D239**: `trait_desugar::implementation` copies a trait's defaults onto its members with `Method::defaults_module`, which `merge_method_clauses` / `instantiate_product_methods` take as `MethodSig::def_mod`; `clause_implementations` checks a member written in clauses against the trait; **D238**: `trait_desugar` sets `Method::scope_module` on members written outside their type's module, and `mir::method_scope` resolves them there; a union's conformance members are its methods, a generic union's in `GenericKind::Union` made per instance by `instantiate_product_methods`; `union_method_jobs` and the by-name job sets (`method_jobs_made`, `lowered_method_jobs`) lower each instance method once; **D237**: the parser reads `where S.Item: Show` after a generic type's header into `TypeDef.assoc_bounds`; `trait_desugar::checking_instances` resolves it with the type's parameters and records `Checking::derived`; the MIR's `GenericDef::derived` feeds `derived_subst` in `monomorphize_pending` (archetypes for checking instances, `assoc_of` over `AssocTables` for the rest) and `instance_meets_bounds`; **D236**: std/core conforms `Map` and `String` to `Sequence` (`MapCursor`, `Entry`, `StringCursor`) and `Array<T: Ordered>` to `Ordered`; `lower_for_array` sends a single-name `for` over a map or a string to `lower_for_cursor`; `order::lower_compare` calls an array's `compare`; `enqueue_type_app_mono` resolves nested applications by instance name; `fallible_mono` makes a missing `May` / `Trial` instance; `receiver_in_place` reads elements with `read_array_elem`; **D235**: `trait_desugar::mixed_unions` makes each `mixed T` union empty and passes `T`'s lineage's members (`TraitOutput::mixed_members`); the MIR adds members as values convert in or arms name them (`mixed_variant` into `SyntheticRegistry::mixed_added`, seeded by `seed_synthetics`; archetypes stay local); `lower_mixed_call` calls `__mixed_{M}_{member}` by address with the trait's signature (`mixed_member_sig`), and `order::lower_compare` calls `__mixed_{M}__compare`; `lower_mixed_functions` makes them in the late-job loop once nothing else is pending, again when members arrive; `==` uses D212's `__eq_{M}`; the final layouts get the added members; `Pattern::Variant::type_args` (`Box<Int64>(value)`) resolve through `mixed_arm`; `job_applies` reads late instances' signatures; **D233**: `mir::named_receiver` / `self_field` make `self.name` the field's local for writes; `receiver_in_place` passes a binding or an element of one in place for user methods and built-in members; a built-in type's member takes `self` as a `mut` parameter (`extend_desugar`, `trait_desugar::extension_function`), called with `mut` an alias of the receiver's place (`extension_receiver`); `lower_mixed_call` passes the payload in place (`value.u.vN`); `store_receiver` wraps stores back in `Stmt::StoreBack { callees, body }`, which C runs as is and `mutating_functions` (one fixpoint over methods' `_self` and members' `*self`, `ReceiverEffects`) counts as a write only when a callee writes; getters are checked against it (`GetterCheck`); cgen's `FieldAssign` drops an `Array` / `Map` field through `emit_rc`; **D234**: `Parser::expect_rangle` splits a `>>` token in place (`split_shr`, restored by `call_type_args`); **D232**: `trait_desugar::checking_instances`, after conformances are merged, gives each generic product archetypes per set of bounds through `resolve_generics` (`TypeDef.checking`: archetype names and the conformance bounds they add); the MIR carries them as `GenericDef.checking`, makes those instances first in `monomorphize_package` (so their jobs sort first), lowers each member on the instance for its bounds (`checks_member`), records a failed one by its template (`template_member`) so the real instances skip it, and checks method templates on checking instances only; `without_checking_types` drops archetypes, instances made with them, and types holding one from `Program.types` before `retain_reachable`, so their members are never rooted; **D231**: the parser reads `where S.Item == Int64` into `TypeParamBound::same`; `trait_desugar::resolve_generics` puts the type in the archetypes (`Archetypes::fixed`, type parameters as their archetypes) and on the derived `GenericParam::fixed` where written; the MIR resolves it for the checking instance and checks or binds it in `generic::complete_bindings`; **D230**: `Parser::call_type_args` reads `<…>` glued to a call's name (restoring position, `>>` state, and soft errors when it is not one); `ExprKind::Call` / `MethodCall` carry `type_args`; the MIR resolves them (`resolve_type_args`) and passes them to `generic::instantiate_call` / `instantiate_method` (`written_bindings`), or gives a generic constructor, `Map()`, or `Has` its type (`explicit_type_call`); **D229**: `is_ctor_field_ty` takes every integer type; the lexer's `TokenKind::BigInt` and the parser's `ExprKind::BigInt` carry a literal above `Int64`, lowered as a `UInt64` place `_big…` whose value `literal_int_value` reads unsigned; `validate_desugar`'s `Shape::Narrow` reads narrow integer fields through std/json's `asInt32` / `asUInt32` / `asUInt64` / `asByte`; cgen writes `INT64_MIN`; **D228** slice 10: `trait_desugar::type_bounds` resolves a declared type's methods' type parameters with `resolve_generics` (`Method::generic`); the MIR registers such a method as a `MethodTemplate` in `TypeInfo::templates` (a generic instance's after `subst_method`), and `mir/generic.rs` `instantiate_method` / `method_instance` make `{method}_{args}` methods: the signature joins the type's methods and `SyntheticRegistry::method_instances` (seeded into every context by `seed_synthetics`), the body a `MethodJob` lowered after the generic functions' instances, checking ones (`check_templates`, up front, on non-generic types and generic types' checking instances) first; slice 9 part 2: a built-in type's conformance members become `extend` functions (`extension_function`, generic over the conformance's parameters, bounds resolved where written via `resolve_generics`' `resolved`), in the conformance's module or the trait's; std's `extend` functions are the built-in's own members, with `BUILT_IN_PRIMITIVES`; built-in associated types go to the MIR as `TraitOutput::built_in_assoc` (`built_in_assoc_type`); a bare call or property in a built-in's method resolves on `self` (`extension_sibling`); extension lookups use the exported mangled names, so an entry module's are found; `lower_method` sees instances made while lowering; slice 9 part 1: `trait_desugar` renames a generic type's conformance's parameters to the type's, moves its members into the template with `Method::requires` (its bounds per parameter), and records its associated types on the template (`GenericDef::assoc`); an instance's `MethodSig::requires` holds the arguments and bounds, checked at calls (`check_requires`) and before its body is lowered (`job_applies`); `mir::assoc_type` works out an instance's associated type on first use and caches it in `SyntheticRegistry::assoc`; `monomorphize_pending` makes every instance first and resolves their methods' signatures after (`instantiate_product_methods`); `own_key_ops` decides a Map key's own `hash` / `equals` for instances too; a nested associated type through a generic type substitutes its arguments in MIR names (`Scope::params`, `unmangle`); slice 8: `trait_desugar` resolves associated types: each trait's (`AssocEntry`, bounds resolved), the nested tree a value of some bounds has (`assoc_tree`, aliases followed by `unalias`), each conformance's (named before any is checked, so `Cursor.Item` reaches another type's), substituted into the type's conformance and provided methods by `mir::subst_method` over a `TypeSubst` (a name → `Ty` map, or → `TypeExpr` in MIR names, `Scope::globalize`), and recorded on `TypeDef.assoc`; a generic function's `S.Item` is a `GenericParam` with `derived`, its archetype one of `Archetypes`; the MIR resolves `TypeDef.assoc` into `SyntheticRegistry::assoc` (bounds checked) and binds a derived parameter from its owner's argument; `lower_for_cursor` desugars `for` over an `Iterator` / `Sequence`; a generic template's names resolve in its defining module (`GenericDef.name_map`), and a provided member's type applications are collected in its trait's module; a substituted type's name starts with `ast::RESOLVED_TYPE` (`@`) and `resolve_ty` takes it as is, so a type argument (an entry type `Order`) is never re-read through another module's names (std/core's `Order`). Slice 1: `trait_desugar` is the first desugar pass: it resolves traits (local, imported, or std/core's), checks each `conform` against them, moves its methods into the type as public methods, and records `TypeDef.traits`; `TypeInfo.resource` (a `Resource` conformance) replaces finding a `drop` method by name, and `retain_reachable` roots destructors from layouts with `has_drop`; the loader adds core module `std/core` (the built-in traits) to every package; the resolver binds traits (`BindingKind::Trait`) and a conformance's methods as members of its type. **D227**: the session host, libtcc, vendored TinyCC, the command layer, and the line editor are gone; recoverable from `eefdac4`. Before it: `cargo make unicode` writes `std/normalize-data.thx` too, read by the opt-in `std/normalize` (D226); `coerce_place` turns an `Int64` / `UInt64` → `Float64` into `thoxa_f64_from_i64` / `_u64` (checked exact), and `as Float64` goes through generated `__asFloat_*` readers (D224); an unannotated `main` is a return-inference candidate, and the C `main` returns 0 for a Void one (D223); `lower_binary` lowers UInt64 operations as UInt64 places (`unsigned_operand`), with `thoxa_uint_div` / `_rem` / `_shl` / `_shr` in the runtime header (D219); a generic instance's `from` clauses are conversion `MonoMethodJob`s, registered and lowered as `__conversion_{instance}_N` free functions (D217); `cargo make unicode` also reads DerivedCoreProperties.txt and writes `casedRanges` / `caseIgnorableRanges` into `std/unicode.thx`, which `mapCase` reads for final sigma (D216); map key ops cover arrays (`thoxa_key_hash_arr_*` / `thoxa_key_eq_arr_*`) and Float64 parts (`thoxa_hash_f64_key` / `thoxa_f64_key_eq`); a `Map<Float64, V>` key travels as `thoxa_f64_key_bits` (D214); `lower_map_lit` takes the expected key / value types (D213); `==` on maps is `lower_map_eq` (borrowed `MapGet` slots), and a type met again during its own inline comparison calls a generated `__eq_{type}` (both sides `mut`, by address), lowered after the other bodies by `lower_eq_functions`; a session step needing a new one falls back to a fresh base image (D212); return inference also probes multi-clause methods (`probe_multi_clause_return` with the type's fields) and each generic instance's methods (from `subst_method` bodies of the mono jobs) (D211); `cargo make test` runs the `repl::tests` unit tests one at a time after the rest, since they capture fd 1 (and captures are serialized); the resolver links call labels and puns after every body is walked (`Resolution.puns`), and `lsp::index::rename_edits` writes a pun's label out (D210); `thoxa -c` goes through `apply_session_source` (D209); `LowerCtx.method_type` makes a bare call to a sibling method `lower_sibling_call` on `_self`, which reloads the field locals after it, and `mutating_methods` is a fixpoint over calls on `_self` (D208); a field indexes through `lower_index_value` (D207); a field passed `mut` is recorded in `LowerCtx::mut_field_args` and stored back through `_self` by `emit_call_result` (D206); every call resolves through `resolve_call_args_full` with marks: built-ins pass all-positional marks via `resolve_call_args`, or explicit ones for their options (D203); `resolve_call_args_full` first turns a bare-name argument that names a labelled parameter into a labelled one (`punned_arguments`, D202) and checks `-->` starts too. 2026-10-17: `resolve_call_args_full` checks `!` / label use against `FnSig.positional`, `MethodSig.positional`, and `FieldInfo.positional`; generated parameters are `!` (D200); a `mut` parameter is a C pointer and its MIR place is `(*name)`, never in the drop set; the caller passes `&slot` (D199); `compile_to_c` runs `mir::retain_reachable` before cgen, so a program's C holds only functions reachable from `main`, module initializers, and destructors (D197); `extend_desugar` (first desugar pass) turns std's `extend` blocks into `__extend_{Type}_{method}` functions that method calls on built-in receivers find; the loader adds core module `std/string` to every package; all-constant integer array literals lower to `RValue::IntArray` (D196); `ThoxaString` carries its scalar count; `thoxa_string_new` validates UTF-8 and repairs with U+FFFD, so every String is valid (D195 §4); String's methods are Thoxa in `std/string` over runtime byte primitives `thoxa_string_byte_*`; case tables are `std/unicode.thx` from `cargo make unicode` (D195, D196); call arguments resolve positional then named (D194); `conversion_desugar` turns `from` clauses into `__conversion_T_N` functions before the route and codec passes; the codec pass also generates a checked reader per `as` target and fills route body readers; `mir/convert.rs` lowers `as` / `as?` and encodes where a `Json` is expected; `return` and `=` lower against the expected type; route desugar names `std/http` items by local name or mangled name (D191–D193); cgen emits an asm-labelled prototype per `extern fn` and calls it by that private name (D190); `and` / `or` lower as `if`; array literals lower against the expected element type; `unwrap_refined` dups a heap base; a Void `let` binds the unit place; runtime `Float64` text found in scientific form, written in full between 1e-7 and 1e21; session desugar keeps keyword and block-opening lines as Thoxa; `cargo make test` adds the site samples step; thoxa.dev's image carries `docs/handbook` and the samples. 2026-10-16: structural `==` expanded in MIR; generated Map key ops and in-order compaction after `remove`; `main` exit status checked; the lexer follows interpolation holes. 2026-10-15: deadline-aware fd waits; checked shifts; `tests/conformance/fail/`. 2026-10-14: batch CLI on a large-stack thread; session steps run through `thoxa_session_call`; guarded task stacks; no `_Thread_local` in the runtime; 2026-09-04: session compile cache: re-lower `Shell.step` + libtcc base `.o`; **D156** session emacs line editor; **D155** resource `drop()`; **D153** gate done: empty `lsan-skip` + bounded ASan; **D152** match/init RC; **D151** LeakSanitizer gate; **D150** route clauses shipped; **D149** `where` / `Type.read` shipped; **D148** `-->` nowait shipped; **D147** JSX/`Node` shipped;
`workloads/landing` thoxa.dev live; `workloads/sola` sola.computer live;
static JPEG/font MIME;
**D146** SSE `peerFd` / `streamed()`;
**D145** `htmlFragment` / `cached`;
**D144** `sql"…"`;
**D143** `std/sqlite`;
**D142** `Cookie` / `withCookie`;
**D141** `std/json` `decode`/`encode`;
**D139** HTTP hosts under `std/http/`;
**D138** kebab-case modules + `main.thx` entry;
**D137** Str parse + Http wrappers; **D136**
`std/http/server` + `Request.query`; **D135** nested User / Node ctor fields;
D133/D134 Map; D132 fn pointers; program `run`/`build` reuse stamped binaries)

---

## One sentence

Native Thoxa is a **Rust compiler that emits C**, linked with a **C runtime**
(`thoxa_rt`). Every program has `fn main` and is built by the system `cc`.

## Pipeline

```text
.thx → lexer → parser → AST → MIR (+ simple RC) → cgen
  → cc + libthoxa_rt  (reuse bin when inputs unchanged)
```

| Stage | Location | Notes |
|-------|----------|--------|
| CLI / driver | `crates/compiler/src/main.rs`, `driver.rs` | `run` / `build` / `check` (`--json`) / `emit-c` / `runtime` / `lsp` / `version`; bare `thoxa` prints usage (**D227**). Program `run`/`build` stamp package+std+runtime+compiler and skip `cc` when fresh. `THOXA_SANITIZE=leak` / `address` (**D151**) |
| Frontend | `lexer`, `parser`, `ast`, `resolver`, `trait_desugar`, `extend_desugar`, `conversion_desugar`, `route_desugar`, `validate_desugar`, `refine` | Shared resolver used by LSP; `trait_desugar` (first pass) checks `conform` items against their traits and moves their methods into the type, with associated types substituted (**D228**); parse soft-recover + `Error.recovered_module` for check multi-diag. Tag literals desugar in the parser to `__jsx_el` / `__jsx_child` / `{...}` spreads (**D147**). The parser reads `x as T` / `x as? T` into `ExprKind::Cast` (**D191**). A type body's `from` clauses become free functions `__conversion_T_N`, exported when the clause is, before the route and codec passes (`conversion_desugar.rs`, **D191** §3). **`Type.read` / `where`** generate `__read_T` / `__fromJson_T` / `__from_T` (`validate_desugar.rs` + `refine.rs`, **D149**); user code reaches the last two only through `as`, and generated code calls them as `T.__fromJson` / `T.__from` (**D191** §6). Each `as` target in a module gets a checked reader there, `__asJson_K` / `__asBase_K` / `__asNarrow_K`, keyed by the written type (**D191** §1); a `from(json: Json)` clause becomes the type's `__fromJson_T`, its issues moved under the field path; every encodable product, union, refined, and literal type in a non-`std/` module gets `__toJson_T` / `__encode_T` once `std/json` is in the package, naming `std/json` items by their mangled names (**D182**). A generated reader reads each field into a `Trial<Field, Array<Issue>>`, gathers the issues, and builds the value only when there are none (arrays the same per element; **D182** §2). With `std/http` in the package a form-readable product also gets `__readForm_T` / `__fromForm_T` over `std/http`'s `FormPair`s, sharing the field walk and `__check_T_field` (**D182** §3). A union of readable products gets a `__fromJson_U` that picks the member by `"type"` and calls its reader; `Map<String, T>` / `Json` fields and constructor defaults are read in the same walk (**D182** §5). Synthesized AST is built with `ast_build.rs`. Type-body `GET "/…"` desugars in resolve + package lower (`route_desugar.rs`, **D150**); a body parameter reads through a `__routeBody_K` stub that the codec pass fills (**D191** §5), and generated code names `std/http` items by the module's own names when it has them, else by their mangled names (**D193**). The lexer follows interpolation holes, so a `"` in a hole starts a nested string, kept as source for the hole's own parse (**D176**) |
| Diagnostics | `diagnostic.rs`, `error.rs` | Structured IR (D124): codes/help/labels → pretty CLI, JSON, LSP |
| MIR | `mir.rs` | RC (**D155** / **D228** `Resource` product boxes; **D154** callee-owned heap params; **D153** overwrite-drop + if/match result overwrite-drop + early-return drops from the final `drop_places` set; **D152** match-bind one Dup; union/Node match result dropped); generic monomorphs (type applications gathered from every `let` annotation, nested blocks included); `std/fallible` via import (D118/D123); multi-error block lower; module globals not end-of-fn dropped (D122); **Never** for break-free `loop` (D128); **return-type inference** probe before body lower, free fns and methods in one callee-first worklist (**D129**, **D174**); tag-only bare union patterns (**D130**); structural `==` expanded inline over fields, union arms, and array elements, reading fields as borrows (**D175**, **D177**, **D178**); a user Map key passed by address (**D179**); a collection write on a field or temporary refused, on an array element written back (**D180**); route-clause bodies after desugar; `toJson` / `encode` on scalars, `Array`, `Map`, `May`, and `Json` lowered from synthesized AST with `std/json`'s names in scope (`mir/codec.rs`, **D182**); a map / array literal where a `Json` is expected lowered as a `Json` object / array, its values first, then built from them (`MapEntry::Spread` for `...`; **D182** §4); `x as T` lowered in `mir/convert.rs`: a matching `from` clause first, then the built-in table, a checked pair calling the module's `__as*_K` reader, `as?` through the `?` path (**D191**); a value with an encoding encoded where a `Json` is expected (`encode_where_json_expected`, **D191** §4); `return`, `=`, and `match` arms lowered against the expected type (**D192**); call arguments resolved by `resolve_call_args_full` for every callee, built-ins included: bare names punned to labels (`punned_arguments`, **D202**), positional then labelled (**D194**), checked against `!` marks (**D200**, **D203**), `mut` arguments passed by address with a method field stored back through `_self` after the call (**D199**, **D206**); builtin **`Ty::Node`** (`Element` / `Text` / `Fragment` / `Attr`; `html_el` / `html_text` / `html_frag` / `html_render`); **`Ty::Fn`** unboxed pair (**D132**) |
| Cgen | `cgen.rs` | Per user Map key type: a hash, an equality, and a `ThoxaMapKeyOps` table, generated from the layouts (**D179**); topo typedef order for nested union monomorphs; pure-lit scalars; no dummy `return 0` after diverging body; D132 thunks + `ThoxaFn`; **D148** start thunks + `thoxa_sched_drain` after `main`; module heap/User globals `__attribute__((used))` (**D152**); pointer locals NULL after Drop (**D153**); **D155** boxed `Resource` types + `thoxa_set_dtor` |
| Loader | `loader.rs` | `./` `~/` `std/…`; combined import maps (D127); `load_package_with_entry_ast` |
| Build script | `build.rs` | Stamps `THOXA_GIT_HASH` / dirty for `thoxa version` and the LSP's identity |
| LSP | `lsp/` | stdio server; multi-diag + D124; nav; hover shows D129 inferred return types (cached on publish); call-arg formals; identity log |

## Crates and trees

| Path | Role |
|------|------|
| `crates/compiler` | `thoxa` binary + compiler library |
| `crates/make` | xtask (`cargo make …`); `unicode.rs` generates `std/unicode.thx`, the case tables, from the Unicode Character Database |
| `runtime/` | `thoxa_rt` C sources + `include/thoxa_rt.h` (incl. `ThoxaHtml` / `THOXA_TAG_HTML`; **`ThoxaFn`** D132; `thoxa_sql_*` D143; String byte primitives `thoxa_string_byte_*` D196; `ThoxaString { len, count, data }`, always valid UTF-8) |
| `vendor/sqlite` | Vendored SQLite amalgamation (D143; compiled into `libthoxa_rt`) |
| `std/` | Product std (`Fallible`, `Logging`, `Io`/`Net`/`Http` + `http/server`/`http/static`, `Json`, `Sqlite`, Path/Env/…; `string`, the core module with String's methods, in every package, and `unicode`, its case tables, D196) |
| `tests/conformance/` | `.thx` + `.out` gate; `fail/` holds programs that must be rejected or panic (`.err` lines must appear in stderr) |
| `examples/` | Runnable demos (`http` Crema-lite, `kilo` terminal editor); not the gate |
| `workloads/` | Deployed Thoxa programs (`landing` = thoxa.dev, `sola` = sola.computer). `landing` renders `docs/handbook` at `/manual` (Markdown renderer + highlighter in Thoxa, per request) and shows `landing/samples` on `/`; those samples run in `cargo make test` (D189), the sites themselves are not the gate. Each image builds `thoxa` and emits the site's C in a digest-pinned `rust` stage, then runs it as UID 65534 from a digest-pinned `debian:bookworm-slim` |
| `extras/` | Non-product: tree-sitter, Zed extension |
| `apocrypha/` | Historical + Python oracle |

**Fallible (`std/fallible`):** `Has` / `Not` / `May` / `Pass` / `Fail` /
`Trial` are ordinary exported generic unions. Programs **import** them (no
ambient inject). They lower as `Ty::User` monomorphs.
Builtin `Ty::May` / `Ty::Trial` variants and their cgen paths are gone.

## CLI execution (as-built)

- **Programs:** `fn main` (or a directory's `main.thx`) is the entry. A
  top-level statement is a parse error (`THX005`); an entry without `main`
  reports that the program requires `fn main` (`THX006`). There is no session or
  REPL (**D227**; the libtcc session host is at `eefdac4`).
- The CLI runs on a worker thread with a 256 MiB stack, so deep but legal
  ASTs do not overflow; the parser caps expression nesting at 1000 levels.

## Build products

Everything generated lives under one tree:

| Path | Role |
|------|------|
| `target/rust/` | Cargo (compiler + xtask binaries, deps) |
| `target/c/` | C runtime objects + `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C from Thoxa sources |
| `target/thoxa/bin/` | Linked Thoxa program binaries + sidecar `*.stamp` |
| `target/thoxa/bin-leak/` | LSan-linked programs (`THOXA_SANITIZE=leak`; **D151**) |
| `target/thoxa/bin-address/` | ASan-linked programs (`THOXA_SANITIZE=address`) |

Prefer `cargo make …` over ad-hoc scripts. `cargo make clean` removes `target/c`
and `target/thoxa`; `cargo make clean --all` also removes `target/rust`.

## Import roots (as-built)

| Form | Meaning |
|------|---------|
| `./…` | Relative to current module |
| `~/…` | Relative to **entry** file’s directory |
| `std/…` | Product std tree (`THOXA_STD` → `{THOXA_ROOT}/std` → workspace `std/` → next to binary) |
| `ffi/…` | **Deferred** |

## Runtime notes

- Heap today: `String`, `Bytes`, dynamic `Array`, `Map`, `Buffer`, and
  `fn drop()` products (**D155**) — simple RC.
- `Map`: insertion-ordered slots plus an open-addressed index. Keys are i64
  bits, a String, or a user type through a generated `ThoxaMapKeyOps` (hash,
  equality, element retain / release); a stored user key is a refcounted box
  holding a copy, and a lookup passes the caller's address (**D179**).
  `remove` marks its slot removed and live slots are packed in order lazily,
  before reads by position (**D181**). Writes COW when shared (D134).
  Conformance leak gate: GCC/Clang LeakSanitizer (**D151**). Simple-RC
  correctness campaign (**D153**) gate done: empty `lsan-skip` and documented
  ASan (`cargo make test-asan`, non-fiber). `thoxa_alloc` panics if header
  plus payload wraps `size_t`. Product `fn drop()` boxes the type and runs
  on last RC (**D155**). Perceus not unparked. Freeze
  [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md)
  · [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md).
- Cooperative tasks (**D148**): `thoxa_task_start` /
  `thoxa_wait_ms` / `thoxa_wait_fd` / `thoxa_sched_drain` — one OS thread,
  stackful fibers; poll + timers. `std/net` accept/connect/recv/send go
  through `thoxa_*` and park on EAGAIN. Generated `main` drains after
  `thoxa_fn_main`. RC stays non-atomic. Task stacks: 512 KiB mmap'd with a
  PROT_NONE guard page, pooled for reuse; a SIGSEGV in the guard prints
  `stack overflow in task`. `accept` waits 50 ms on EMFILE / ENFILE; `send`
  passes `MSG_NOSIGNAL`. An fd wait may carry a deadline (`wake_at_ns` with
  `wait_fd`; the fiber wakes with `timed_out` set): `thoxa_recv_until` /
  `thoxa_send_until` fail with ETIMEDOUT at a `thoxa_now_ms` deadline
  (**D165**). A panic with a current task does not abort: `thoxa_panic`
  closes the task's owned fds (indexed by fd; released
  by `thoxa_close` or a normal return), frees the packed arguments, marks the
  fiber done, and switches to the scheduler from the panic site, so the dead
  frames are never unwound (**D166**). Ownership records the Socket box
  (`thoxa_task_own_socket`; a box passes to `Address<T>` as its pointer), so
  the panic path also writes `fd = -1` into it (**D168**). Under LSan
  (`__lsan_register_root_region` present) that stack stays mapped as a root.
- Integer `/` and `%` call `thoxa_int_div` / `thoxa_int_rem` (panic on zero),
  `<<` / `>>` call `thoxa_int_shl` / `thoxa_int_shr` (**D161**);
  fixed-array subscripts go through `thoxa_fixed_index`; an implicit integer
  narrowing (MIR `NarrowChecked`) goes through `thoxa_narrow_signed` /
  `thoxa_narrow_unsigned`, and `T(truncating: x)` is a plain C cast
  (**D167**); programs build with
  `-fwrapv` (**D157**), the site Containerfiles too. `thoxa_panic` is
  `noreturn`.
- Generated batch `main` calls `thoxa_program_start()` first: as PID 1 it
  gives SIGTERM / SIGINT a handler that `_exit`s with 128 + signal, since the
  kernel drops default-action signals sent to a namespace's init (**D170**).
  It returns `thoxa_exit_status(main())` after the tasks drain: a status
  outside 0 to 255 panics instead of being cut to its low 8 bits (**D173**).
- Extern from `"c"` (+ optional C symbol); cgen pulls headers when needed.
- Phase A: **no** product tree-walk interpreter; **no** QBE backend.

## Related freezes

| Topic | Spec |
|-------|------|
| Kernel / value model | [`specs/2026-07-18-thoxa-native-kernel-design.md`](specs/2026-07-18-thoxa-native-kernel-design.md) |
| HTTP path | [`specs/2026-07-19-post-r9-http-completion-plan.md`](specs/2026-07-19-post-r9-http-completion-plan.md) |
| Editor / LSP | [`specs/2026-07-20-native-editor-lsp-plan.md`](specs/2026-07-20-native-editor-lsp-plan.md) |
| Map / value collections | [`specs/2026-08-14-map-and-value-collections.md`](specs/2026-08-14-map-and-value-collections.md) |
| Node / tag literals | [`specs/2026-08-22-jsx-node.md`](specs/2026-08-22-jsx-node.md) (**D147**) |
| Nowait `-->` | [`specs/2026-08-22-start-tasks.md`](specs/2026-08-22-start-tasks.md) (**D148**) |
| Refinements / `Type.read` | [`specs/2026-08-23-type-read.md`](specs/2026-08-23-type-read.md) (**D149**) |
| Route clauses | [`specs/2026-08-23-route-clauses.md`](specs/2026-08-23-route-clauses.md) (**D150**) |
| Simple-RC correctness | [`specs/2026-09-02-simple-rc-correctness.md`](specs/2026-09-02-simple-rc-correctness.md) (**D153**) |
| Resource `drop` | [`specs/2026-09-03-resource-drop.md`](specs/2026-09-03-resource-drop.md) (**D155**) |
| Typed codecs | [`specs/2026-10-16-typed-codecs.md`](specs/2026-10-16-typed-codecs.md) (**D182**) |
