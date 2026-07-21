# Cross-module hover & go-to-definition — design

Date: 2026-07-13
Status: approved (scope chosen by Joshua)
Follows: `docs/plans/2026-07-13-thoxa-lsp-navigation.md` (single-file nav)

## Problem

Today's LSP navigation is single-file. Hovering a name that was **imported** from
another module shows nothing useful, and go-to-definition jumps to the local
`import` line rather than the real definition. A use of `sqrt` from
`import { sqrt } from "std/Math"` resolves to a binding of kind `import` whose
node is the *import statement*, so `render_signature` falls through to the bare
name and `def_span` points at the import. The signature and its `##` doc live in
a file the LSP never opens.

## Scope (approved)

1. **Selective imports**, incl. `as` aliases: `import { sqrt } from "std/Math"`,
   `import { sqrt as root } from "std/Math"` — hover/def on the local name.
2. **Namespace imports**: `import math from "std/Math"` — hover/def on `math.sqrt`
   (the member) and on `math` (the namespace name → a module summary).
3. **Prelude types**: hovering `Has`/`Not`/`May`/`Pass`/`Fail`/`Trial` resolves
   into `std/Prelude`. `print`/`println` are Python builtins with no Thoxa source,
   so they stay bare (unchanged).
4. **Both hover and go-to-definition** (cmd-click jumps into the defining file).

Out of scope: cross-module find-all-references and rename (single-file only, as
today); the `~/` project-root import form when the LSP has no known project root.

## Approach: lazy, hover-time remote resolution

Only the single hovered module is loaded, and only when hover/def lands on an
external name. Everything degrades to today's behaviour on any failure.

### New module `thoxa/lsp/crossmodule.py`

```python
@dataclass
class Remote:
    signature: str
    doc: Optional[str]
    path: str          # absolute path of the defining module
    def_span: Span     # name span in that module (1-based)

def resolve_remote(binding, ref, module, docs, current_path) -> Optional[Remote]:
    ...
```

`resolve_remote` decides which *origin* (spec + imported name) the cursor points
at, loads that module, resolves it, finds the exported binding, and renders it.
It never raises — any problem returns `None`.

**Origin selection:**

- `binding.kind == "import"`, selective form (`node.names is not None`): recover
  the imported name by matching `local == binding.name` in `node.names`; spec =
  `node.module`.
- `binding.kind == "import"`, namespace form (`node.names is None`): the cursor is
  on the namespace name itself → module summary (imported name = `None`).
- `ref.ctx == "member"` and `binding is None` (a `ns.member`): walk `module` to
  the `FieldAccess` whose `resolution.member_refs[id(fa)] is ref`; if its
  receiver `Name` resolves to a namespace-import binding, spec = that import's
  `node.module`, imported name = `fa.name`.
- `binding.kind == "builtin"`: attempt resolution against `std/Prelude`; if the
  prelude exports a binding of that name with a real `def_span`, use it. This
  covers the prelude *types* without hardcoding a list; `print`/`println` simply
  aren't in `Prelude.thx`, so they return `None`.

**Loading the target:** resolve the spec with the existing `loader.Loader`
(`importer_dir = dirname(current_path)`; `project_root = None`, so `~/` imports
degrade). Prefer an **open buffer** for the target path (from the server's `docs`
map) over disk, so hover reflects unsaved edits. Parse + `resolve()` the target —
**no checker**, because declared signatures (`fn`/`extern`/`type`/`union`/`field`)
need no inference.

**Finding the binding:** the exported binding whose `name == imported_name`,
`exported`, and `def_span is not None`. For the namespace-name case, synthesise a
`module <spec>` summary with the target's leading `##` doc and a `def_span` at the
file top.

**Rendering:** reuse `index.render_signature(binding)` and
`convert.doc_above(remote_text, def_span.line)` — pointed at the other file.

### Handler integration (`thoxa/lsp/server.py`)

- `DocumentIndex` gains a `module` field (the parsed AST, already produced in
  `build_index`) so the `ns.member` receiver walk has an AST to traverse.
- `hover`: compute the local `ref`/`binding` as today, then try `resolve_remote`
  **before** the `binding is None`/`builtin` early-returns; on success render the
  remote signature+doc at the local cursor span; otherwise fall back to today's
  local rendering.
- `definition`: compute `ref`/`binding`, try `resolve_remote`; on success return a
  `Location` in the remote file (`convert.path_to_uri(remote.path)` +
  `remote.def_span`); otherwise today's `binding_at` local jump.
- `convert.path_to_uri(path)` is added (inverse of `uri_to_path`).

`references` and `rename` are unchanged (single-file).

## Failure isolation

`resolve_remote` is internally defensive (broad try/except → `None`) and both
handlers keep their existing try/except. A missing file, parse error,
unresolvable spec, or `~/` import with no project root all fall back to today's
behaviour. No new exception may reach the JSON-RPC layer.

## Testing

New `tests/test_lsp_crossmodule.py`. `std/Math` and `std/Prelude` resolve via the
absolute `std/` root regardless of the importer's path, so most cases need no
temp files; `./` local imports use `tmp_path`.

- selective import → hover shows `extern fn sqrt(x!: Float64) -> Float64` + its
  doc; go-to-def returns a `std/Math.thx` location on the `sqrt` name.
- aliased import (`as root`) → hover on `root` shows `sqrt`'s signature.
- namespace member (`math.sqrt`) → hover + def resolve into `std/Math`.
- namespace name (`math`) → hover shows a `module std/Math` summary.
- prelude type (`May`, `Trial`) → hover shows the type def; def jumps into
  `std/Prelude`.
- `./` local import via `tmp_path` → exercises `importer_dir` resolution and the
  open-buffer-over-disk preference.
- degradation: unresolvable import and `println` → hover returns `None`, no crash.

## Task breakdown

1. `convert.path_to_uri` + `DocumentIndex.module` (store parsed AST); tests.
2. `crossmodule.resolve_remote` — selective + aliased imports (the core); unit
   tests against `std/Math`.
3. Extend `resolve_remote` — namespace member + namespace name; tests.
4. Extend `resolve_remote` — prelude-type builtins; tests.
5. Wire `hover` + `definition` to try remote first; open-buffer preference;
   handler-level tests incl. degradation.
6. Final review + full suite; update memory/goal ledger.
```
