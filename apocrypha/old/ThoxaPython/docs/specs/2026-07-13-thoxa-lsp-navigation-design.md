# Thoxa LSP navigation — design (v1)

**Date:** 2026-07-13
**Status:** approved, implementing
**Goal tier:** Personal DX, churn-friendly. Extends the existing Zed LSP (`thoxa/lsp/`, which
already ships diagnostics + semantic tokens) with IDE navigation.

## Decisions

1. **Features:** hover (signature + `##` doc-comment), go-to-definition, find-all-references,
   document symbols (outline), and rename. `textDocument/declaration` is **aliased to definition**
   (Thoxa has no header/impl split). Deferred: completion, signature help.
2. **Scope:** **single-file (v1).** Navigation resolves within the open buffer; an imported name's
   definition is its own `import` statement. Jumping into `std/` or sibling modules, and
   workspace-wide references, are deferred.
3. **Architecture — extract a shared lexical resolver.** A new `thoxa/resolver.py` owns name binding
   as a standalone pass. **Both the checker and the LSP consume it.** This makes name resolution a
   single source of truth — no second, drifting scope implementation — which is the whole LSP-first
   principle ("reuse the frontend so it never drifts") applied one layer deeper.
4. **Members (`.field` / `.method`) are gated on a *provable object type*.** When the checker can
   prove the concrete type of the receiver, the member occurrence gets the full set (def, refs,
   hover, rename). When it cannot, the occurrence is **inert** — no popup, no navigation, not
   renamable. There is therefore never same-named-member ambiguity: we only ever act on a member
   whose owning type is known, so rename is provably safe.
5. **Doc comments** are captured by a **source pre-scan** of the contiguous `##` lines immediately
   above a binding's definition — no lexer/parser/AST changes (the lexer still discards comments).
6. **Zero-dep, hand-rolled**, consistent with the rest of the project. The checker refactor is
   **behavior-preserving**: the existing 214-test suite is the gate and diagnostics stay identical.

## Why this shape

**The key insight:** *which* definition a name refers to is purely **lexical / structural** — it is
determined by the name and its position under the scope rules, **independent of types**. A type is a
separate *attribute* of a binding, not part of deciding which binding a use denotes. (Member access
is the one exception — it resolves through the receiver's type — and Decision 4 handles it head-on.)

Today the checker does resolution and inference in one entangled walk: it threads `env` dicts of
`name → type` and, at each `Name`, looks the type up and throws the resolution away. It never records
*which definition* a use refers to, nor a definition's site. Navigation needs exactly that.

Because resolution is type-independent, it can be lifted out into a standalone pass that produces a
persistent, position-queryable binding table. Inference then layers back on top, keyed by **binding
id**. That is the clean seam that makes "both consume" real (one scope implementation) instead of two
implementations that drift apart. It also means the checker's *real inferred types* become available
to hover for free, since they are now keyed by the same ids the LSP queries.

## Components

### A. `thoxa/resolver.py` — the shared lexical resolver *(new)*

Runs on a parsed `ast.Module` and produces a `Resolution`:

```
Binding(id, name, kind, def_span, node, exported)
    kind ∈ {fn, extern, type, union, field, method, param, let, mut,
            forvar, pattern, import, namespace, typeparam, builtin}
    node = the defining AST node (for signature/outline rendering)
    def_span = None for builtins and ffi-imports (no in-file definition)

Reference(span, binding_id | None, ctx)
    ctx ∈ {value, type, call, member, label, pattern}
    binding_id = None → unresolved / external, or a pending member (see B)

Resolution
    bindings:   dict[int, Binding]
    references: list[Reference]
    + query helpers: ref_at(line, col), refs_to(binding_id), module_definitions()
```

Walks the module with a **lexical scope stack** (module → function → block → `for` → match-arm) and
resolves, without needing any type information: value names (`Name`/`TypeValue`), type-annotation
names (`TypeRef.name`), params, `let`/`mut` (module + block), `for` vars, match/constructor pattern
binds, type-params, import bindings (selective + namespace), and named-argument labels → the param
they name (best-effort, for a same-file callee).

It **reuses the checker's module-level collection** (`_collect` / `_module_exports` / the `Loader`)
for imports, namespaces, and export gating rather than re-implementing them — that shared helper is
the concrete "both consume" seam. Those pieces are factored so the resolver and the checker call the
same code.

Member and namespace-member references are emitted as **pending** (`binding_id=None, ctx=member`);
they are filled in by the checker (Component B) only when the receiver's type is provable.

### B. `thoxa/checker.py` — consumes the resolver *(refactor)*

The checker's per-name `env` of *types* becomes a `types: dict[binding_id → type]` map:

- `_infer_Name` → `resolution.ref_at(span)` → its `Binding` → `types.get(binding.id, UNKNOWN)`.
- Definitions that produce types (`let`/param/`for`/pattern) write `types[binding.id] = ty`.
- Module-level type computation (`_FnInfo`, `type_members`, union variants) stays in the checker but
  is keyed to resolver bindings.
- **Member / namespace access is type-driven**, so the checker (which already computes `objty`)
  resolves it: when the receiver type is a provable concrete type with the member, it links the
  pending `Reference.binding_id` back into the `Resolution`; otherwise it leaves it inert.

The refactor is **behavior-preserving** — diagnostics stay byte-identical, gated by the full suite,
and rolled out as its own verified phase. The known checker gap (entry/module-file *type-method
bodies* are not type-inferred — `_check_stmt` early-returns on `TypeDef`) is **preserved**, not
closed, to keep the refactor safe: the resolver still walks method bodies for navigation, so def/refs
work inside them, but their locals simply have no inferred type (hover falls back to the annotation or
is omitted). Closing that gap is separate, deferred work.

### C. `thoxa/lsp/index.py` — the document index *(new)*

`build_index(text, path) → DocumentIndex` runs parse → resolver → checker(types) **once** and wraps
the `Resolution` + `types` behind position queries used by every handler. On an unparseable buffer it
returns an empty/partial index so navigation degrades gracefully (as diagnostics already do).

### D. `thoxa/lsp/server.py` — handlers + capabilities *(extend)*

New request handlers, each wrapped in the existing catch-log-and-return-safe idiom (never crash the
loop): `textDocument/hover`, `textDocument/definition`, `textDocument/declaration` (→ definition),
`textDocument/references`, `textDocument/documentSymbol`, `textDocument/prepareRename`,
`textDocument/rename`. `initialize` advertises `hoverProvider`, `definitionProvider`,
`declarationProvider`, `referencesProvider`, `documentSymbolProvider`, and
`renameProvider: { prepareProvider: true }`.

### E. `thoxa/lsp/convert.py` — LSP structure mappings *(extend)*

Adds: a position→span lookup (0-based LSP position covered by a 1-based single-line span iff
`span.line-1 == pos.line and span.col-1 <= pos.char < span.col-1 + span.length`); `Location`;
a hierarchical `DocumentSymbol`; `Hover` (`MarkupContent`, markdown); `TextEdit` / `WorkspaceEdit`;
and a `binding.kind → SymbolKind` map. A `doc_above(text, def_line)` pure helper returns the
contiguous `##` block directly above a definition.

## Feature behavior

- **Go to definition / declaration:** `ref_at(pos).binding.def_span` → `Location`. An imported name
  resolves to its `import` statement (single-file). Builtins / ffi-imports (no `def_span`) → no
  result. Both LSP methods share one handler.
- **Find references:** from `ref_at(pos).binding`, return the definition plus `refs_to(binding)` as
  `Location[]`; honor `includeDeclaration`.
- **Hover:** the binding under the cursor → `MarkupContent` markdown = a fenced ```` ```thoxa ````
  signature rendered from the binding's AST node, followed by its `##` doc block. Un-annotated `let`
  fills its inferred type from `types[binding.id]` (omitted if `UNKNOWN`). A member with a provable
  type shows its owning member's signature + doc; an unprovable member → `null`.
  Signature per kind: `fn name(params) -> ret` (fn/method/extern); `type Name` (+ ctor params);
  `type Name = A | B` (union); `field name: T`; `param name: T`; `let`/`mut name: T`;
  `import { name } from "…"`; `namespace n = "…"`.
- **Document symbols:** module-level bindings as a hierarchical `DocumentSymbol[]`; a `TypeDef` nests
  its fields and methods. `selectionRange` = the name's `def_span`; `range` = the whole definition's
  span; `kind` via the `SymbolKind` map.
- **Rename:** `prepareRename` accepts only a renameable binding defined **in this file** (rejects
  keywords, literals, builtins, names imported from elsewhere, and unprovable members), returning its
  range + placeholder. `rename` validates the new name against Thoxa's casing rule for that kind
  (camelCase for values/params/fns/fields; PascalCase for types) — rejecting with a clear message
  rather than producing an invalid program — then emits a `WorkspaceEdit` editing the `def_span` plus
  every `refs_to(binding)`. Shadowing is handled correctly because rename keys off **binding id**,
  not name. Member rename works only where the type is provable (Decision 4).

## Data flow

```
edit .thx buffer
   └─► thoxa lsp (stdio)
         └─► build_index(text, path)  =  parse ─► resolver ─► checker(types)   [once per change]
               ├─ Resolution (bindings + references, position-queryable)
               └─ types: binding_id → inferred type
         hover / definition / references / documentSymbol / rename
               = pure position lookups over the DocumentIndex
```

The checker still publishes diagnostics on the same parse; navigation reuses the same single analysis.

## Error handling / edge cases

- **Position→binding:** prefer the narrowest reference when spans overlap. Out-of-range or between
  tokens → no binding → the graceful-empty path below.
- **Unparseable / mid-typing buffer:** index is empty/partial; hover → `null`, definition → `null`,
  references → `[]`, documentSymbol → `[]`, prepareRename → an error, rename → `null`. Consistent with
  how diagnostics already degrade.
- **Behavior-preserving refactor:** the 214-test suite gates Component B; diagnostics stay identical,
  and the type-method-body gap is intentionally preserved (see B).
- **Rename safety:** casing enforcement + the provable-type gate for members, plus binding-id keying
  for shadowing, together make a rename either correct or refused — never silently wrong.

## Testing

- **Resolver unit tests** over `.thx` fixtures: shadowing, param/block/`for`/pattern binds, imports
  (selective + namespace), named-arg labels, type-params — assert bindings and each reference→binding
  link.
- **Checker regression:** the full existing suite stays green through Component B (behavior-preserving;
  diagnostics unchanged; method-body gap preserved).
- **LSP handler tests:** drive each request over fixtures (position in → expected
  `Location`/`Hover`/`DocumentSymbol`/`WorkspaceEdit`), mirroring the existing handshake/diagnostic
  tests. Include rename casing-rejection and the provable-vs-unprovable member cases.
- **`convert` / `doc_above` unit tests:** position→span lookup, `SymbolKind` mapping, `WorkspaceEdit`
  shape, and contiguous-`##` capture (with `#`-vs-`##` and blank-line boundaries).

## Build sequencing (thin vertical slices)

1. **Resolver + tests** — standalone `thoxa/resolver.py`; no checker/LSP change yet. Fully verifiable.
2. **Checker consumes the resolver** — behavior-preserving refactor; full suite stays green. (The
   risk phase; gated by the existing tests.)
3. **`index.py` + `convert.py` additions + tests** — the query layer, no handlers yet.
4. **Handlers, in value order** — definition/references first (pure resolver), then documentSymbol,
   then hover (types + doc), then rename (+ prepareRename); each with tests and capability
   advertisement.
5. **Manual Zed smoke test** — the side-loaded extension already exists; confirm the new features in
   the editor.

## Deferred (v1.1+)

Cross-file navigation (jump into `std/` and sibling modules; workspace-wide references) — needs
loader-backed multi-file indexing and span mapping. Completion + signature help (need partial-parse
tolerance the frontend lacks). Closing the type-method-body type-checking gap. Retaining `##` as lexer
trivia (only if the source pre-scan proves insufficient). Type-definition / implementation /
call-hierarchy providers. Publishing to the Zed registry.
