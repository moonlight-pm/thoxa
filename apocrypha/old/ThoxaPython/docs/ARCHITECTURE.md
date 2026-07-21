# Thoxa — Implementation Architecture

The revived Thoxa is a Python package with a clean, linear pipeline and a deliberate seam
between the **frontend** (which changes as syntax iterates) and the **evaluator/backends**
(which stay insulated behind a stable typed AST).

```
source → lexer → parser → AST ──▶ checker ──▶ interpreter (tree-walk)
                           │                        ▲
                           └── stable typed-AST seam ┘
                                        │
                                        └──▶ [ lower → IR → QBE / transpile-to-C ]  (deferred, post-freeze)
```

## Module responsibilities (`thoxa/`)

| Module | Responsibility | Milestone |
|--------|----------------|-----------|
| `tokens.py` | Token kinds + `Token` (kind, value, span). | M1 |
| `errors.py` | `Span`, `Diagnostic`, source-caret rendering. The friendly-errors foundation. | M1 |
| `lexer.py` | Source → tokens. Enforces `camelCase`/`PascalCase`, `#`/`##` comments, literals, interpolation splitting. | M1 |
| `ast.py` | AST node dataclasses (the stable seam). | M1 |
| `parser/` | Hand-written recursive-descent + Pratt expression parser. Disambiguates `<…>` and no-separator boundaries. | M1 |
| `interp/` | Tree-walking evaluator: environments, values, pattern matching, builtins dispatch. | M1–M3 |
| `prelude.py` | `May`/`Trial`/`Has`/`Not`/`Pass`/`Fail`, array methods — defined in Thoxa where possible. | M2 |
| `checker/` | Local/bidirectional type inference + `as`/`can`/`is`. | M4 |
| `builtins.py` | Host functions exposed to Thoxa (`println`, math, later sockets). | M1 |
| `cli.py`, `repl.py` | Command-line entry points. | M1 |

## Conformance harness

`spec/conformance/*.thx` are runnable programs; each may have a sibling `*.out` (expected exact
stdout) and `*.exit` (expected exit code, default `0`). `tests/test_conformance.py` discovers and
runs them through the real interpreter. **A language feature is "done" only when its conformance
test passes** — this is the guardrail against the "spec outran the implementation" failure that
killed the language three times.

## Testing layers

- **Unit** (`tests/test_lexer.py`, `test_parser.py`, `test_interp.py`, …): pytest over the API.
- **Conformance** (`tests/test_conformance.py`): end-to-end `.thx` programs vs expected output.
- **Examples** (`examples/`): larger demos, exercised by CI but not all pinned to exact output.

## Deferred (post-freeze)

`ir/` (typed IR), `backend_qbe/`, `backend_c/` (transpile-to-C), and the Perceus reference-counting
memory passes slot in behind the typed-AST seam without touching the frontend. A one-time host port
(OCaml front-runner) enables a lean WASM notebook. None of this is built yet.
