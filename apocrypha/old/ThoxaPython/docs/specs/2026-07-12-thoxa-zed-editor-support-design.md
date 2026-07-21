# Thoxa editor support for Zed — design (v1)

**Date:** 2026-07-12
**Status:** approved, implementing
**Goal tier:** Personal DX, churn-friendly (private dev extension, not the Zed registry).

## Decisions

1. **Audience:** personal — make `.thx` pleasant to edit while the language is *unfrozen*. Don't over-invest.
2. **Strategy A — LSP-first + thin grammar.** The language server (reusing the real compiler) is the
   churn-proof, high-value artifact; the Tree-sitter grammar stays deliberately thin.
3. **v1 scope:** squiggles (diagnostics) + color (semantic tokens). No hover/def/completion yet.
4. **No LSP library — hand-rolled, zero-dep.** The project is intentionally `dependencies = []`.
   The v1 LSP surface is small enough to implement directly over stdio JSON-RPC, keeping the project
   dependency-free and insulated from external LSP-library API churn. (pygls was the first instinct;
   rejected to preserve zero-dep.)

## Why this shape

Confirmed Zed facts that constrain the design:

- A Tree-sitter grammar is **required** to register a language in Zed — there is no "no-grammar" path,
  so the LSP cannot be the *sole* source of highlighting.
- `semantic_tokens` modes: `off` (tree-sitter only), `combined` (tree-sitter base **+** LSP overlay),
  `full` (LSP replaces tree-sitter). We use **`combined`**.
- Providing a language server from a Zed extension **requires a small Rust→WASM shim** implementing
  `zed::Extension::language_server_command`, which returns `Command { command, args, env }`. The command
  itself can be any local process, so the server stays in Python and reuses the existing frontend.
- Grammars can be referenced locally (`repository = "file://…"`, pinned by `rev`) and the extension can
  be side-loaded via Zed's *Install Dev Extension* — no registry, no GitHub.

**Key asymmetry:** the grammar is the churn-prone / lower-value part (breaks on syntax changes, only gives
color); the LSP is churn-proof / higher-value (always matches the real language). So we invest in the LSP
and keep the grammar thin.

## Components

### A. `tree-sitter-thoxa` (thin, lexer-level grammar) — `editor/tree-sitter-thoxa/`
Tokenizes rather than fully parses: keywords, `#`/`##` comments, string literals with `{…}` interpolation,
char/number literals (`0x`/`0b`, `_`, sized suffixes), operators, and **casing-based identifier coloring**
(`/[A-Z]…/` → `type`, `/[a-z]…/` → `variable`) — free because the lexer enforces casing. Tracks only the
lexical layer, which barely moves as grammar is redesigned. Query files: `highlights.scm`, `brackets.scm`.

### B. Python LSP — `thoxa/lsp/` + `thoxa lsp` subcommand
- Transport: **stdio**, hand-rolled Content-Length-framed JSON-RPC (`protocol.py`).
- `convert.py`: `Span` (1-based line/col + length) → LSP `Range` (0-based, single-line).
  `Diagnostic` (message/span/notes) → LSP diagnostic (notes folded into the message).
- `server.py`: on `didOpen`/`didChange` (full-document sync, debounced), run `check(text, uri)`:
  catch `ThoxaError` → one diagnostic (first parse error); else map the returned `list[Diagnostic]`
  (all type errors). Publish diagnostics.
- **Semantic tokens** (`semanticTokens/full`): driven by `tokenize()` (robust on unparseable buffers),
  emitting LSP token types keyword/type/variable/function/parameter/number/string/operator. This overlay
  *refines* the grammar's base color (function-vs-variable, params, named-arg labels, variant-vs-type).

### C. Zed extension + Rust shim — `editor/zed/`
- `extension.toml`: `[grammars.thoxa]` (local `file://` + rev), `[language_servers.thoxa]`
  (`languages = ["Thoxa"]`).
- `languages/thoxa/config.toml`: `path_suffixes = ["thx"]`, `line_comment = "#"`, brackets,
  `semantic_tokens = "combined"`.
- `src/lib.rs`: `language_server_command` → prefer `<worktree>/.venv/bin/thoxa lsp`, fall back to `thoxa`
  on `PATH`. Compiles to `wasm32-wasip1`.
- Install: Zed → *Install Dev Extension* → `editor/zed/`.

## Data flow

```
edit .thx buffer
   ├─► tree-sitter grammar ─► base highlighting     (instant, offline, survives parse errors)
   └─► thoxa lsp (stdio) ─► check(text)  ─► diagnostics (squiggles)
                          └► tokenize()  ─► semantic tokens (overlay refines color)
```
Two independent lanes: grammar never waits on the server; server never blocks typing.

## Error handling / edge cases

- **1-based → 0-based** position conversion, single-line spans (all the lexer emits); clamp col ≥ 0.
- **Parse error vs type errors:** one squiggle while unparseable, many once it parses — documented, accepted.
- **Server robustness:** any exception in a handler is caught and logged to stderr; never crash the loop.
  Semantic tokens fall back to an empty set if `tokenize()` raises.
- **Interpreter discovery** in the shim: `.venv/bin/thoxa` first, then PATH; log which was used.
- **Grammar drift:** when syntax changes, edit `grammar.js` + bump the local `rev`; color may lag until then
  (diagnostics never lag — they're the real compiler).

## Testing

- **Python (runnable here, CI):** unit tests for `convert` (Span→Range, notes folding); diagnostics over
  every `spec/conformance/*.thx` (expect zero diagnostics on the good files) plus crafted bad inputs for
  both the `ThoxaError` and multi-error paths; semantic-token relative-encoding round-trip.
- **Grammar:** `tree-sitter test` corpus + highlight snapshot (built via `nix-shell -p tree-sitter nodejs`).
- **Zed integration:** manual smoke test — Install Dev Extension, open a `.thx`, confirm color + squiggles.

## Build sequencing (thin vertical slices)

1. **LSP diagnostics** — subcommand + protocol + convert + diagnostics + tests. (Fully verifiable here.)
2. **LSP semantic tokens** — add the overlay + tests.
3. **Grammar** — grammar.js + queries + corpus; build via nix-shell.
4. **Zed extension** — toml + config + Rust shim; Joshua side-loads and smoke-tests.

## Deferred (v1.1+)

Hover (types + `##` doc-comments), go-to-definition, document outline, completion, format-on-save;
publishing to the Zed registry; a proper GitHub-hosted grammar repo.
