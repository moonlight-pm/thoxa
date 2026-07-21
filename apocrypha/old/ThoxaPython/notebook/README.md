# Thoxa Notebook

A local, literate, interactive notebook for experimenting with Thoxa **live** — the
maintainer-facing tool for iterating on syntax while the language is unfrozen.

```bash
uv run thoxa notebook          # opens http://127.0.0.1:8099 in your browser
uv run thoxa notebook --port 9000 --no-open
```

## What it is

- A **local kernel over the real interpreter** (`thoxa/notebook.py`). Cells run against a
  single persistent `Interpreter`, so `let` bindings, `fn`s, and `type`s **persist across cells**
  (the evcxr model). Full fidelity — it is the same engine `thoxa run` uses, never a re-implementation.
- **Zero external dependencies** — a `http.server` on `127.0.0.1` serving one self-contained HTML
  page. Because it runs *your own* code on localhost, no sandboxing is needed.
- The last expression of a cell shows its value (`=> …`); `println` output and friendly,
  caret-annotated errors render inline.

The kernel logic (`NotebookSession`) is covered by `tests/test_notebook.py`. This is the M5
milestone; a public, zero-install browser playground (compiling the interpreter to WASM) is
deferred to the eventual host-language port, per the design.
