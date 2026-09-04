# CLI

The product binary is **`thoxa`** (`cargo run -p thoxa -- …`, or via
`cargo make`).

## Invocations

| Command | Behavior |
|---------|----------|
| `thoxa` (no args) | Session **REPL** (interactive; loads rc on TTY) |
| `thoxa -l` / `thoxa --login` | Same as bare REPL (login-shell argv) |
| `thoxa -c '<chunk>'` | One-shot session chunk (POSIX `sh -c`) |
| `thoxa repl` | Same as bare REPL |
| `thoxa path.thx` | Program if `fn main` exists; else **session script** |
| `thoxa dir/` | Runs `dir/main.thx` if present |
| `thoxa run path [args…]` | Compile and run; trailing args go to `fn main(arguments: Array<String>)` (D121). Reuses `target/thoxa/bin/*` when inputs are unchanged. |
| `thoxa build path -o out` | Compile to a binary (same reuse; prints `up to date` when skipped) |
| `thoxa check path` | Typecheck only (no C emit); multi-diagnostic when possible; **pretty** codes/caret/help |
| `thoxa check path --json` | Same check; machine-readable report (stable fields + compiler identity) |
| `thoxa emit-c path` | Emit C under `target/thoxa/c/` |
| `thoxa lsp` | Language server (stdio) |
| `thoxa version` | Build identity: package + git hash (+`.dirty`) + binary path |

Also accepted: `thoxa --version`, `thoxa -V`. Flag order for check is flexible:
`thoxa check --json path` works too.

Use `thoxa version` to confirm which binary is on `PATH` (what Zed launches after
`cargo make zed`). After rebuilding, **restart the language server** so LSP
picks up the new process.

### `thoxa check` diagnostics

Default output is human-oriented (D124):

```text
error[THX001]: undefined name `x`
 --> main.thx:1:22
   |
 1 | fn main() -> Int64 { x }
   |                      ^
  = help: declare the name, import it, or fix the spelling
```

Stable codes when recognized include `THX001` (undefined / unknown name),
`THX002` (type mismatch), `THX003` (unknown type), `THX004` (Fallible import),
`THX005` (top-level statement with `fn main`), `THX010` (unused import —
**warning**), and a few others. Not every message is catalogued yet; free-form
errors still print without a code.

**Exit status:** `0` when there are no **errors** (warnings alone still succeed).
`--json` sets `"ok": true` the same way.

`--json` prints a single JSON object: `ok`, `compiler.version` / `compiler.path`,
and `diagnostics[]` with `code`, `severity`, `message`, `path`, 1-based
`line`/`column`, byte spans, `help`, `expected`/`actual`, and `related`.

Prefer **`cargo make`** helpers from a workspace checkout:

```bash
cargo make test              # conformance suite (LeakSanitizer; D151)
cargo make run path.thx
cargo make run examples/http # Crema-lite HTTP example (port 9091; experiment)
# kilo editor (real TTY; Ctrl-Q quit, 3× if dirty)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt
cargo run -p thoxa -- run examples/kilo   # empty buffer
cargo make shell             # bare thoxa REPL
cargo make shell --login     # clean login-like env
cargo make emit-c path.thx
cargo make runtime           # libthoxa_rt.a + compile_commands.json
cargo make zed               # sync Zed extras + LSP binary
```

## Build products

| Path | Role |
|------|------|
| `target/rust/` | Cargo outputs |
| `target/c/` | Runtime objects + `libthoxa_rt.a` |
| `target/thoxa/c/` | Generated C |
| `target/thoxa/bin/` | Linked program binaries + `*.stamp` (freshness fingerprint) |
| `target/thoxa/bin-leak/` | LSan-linked programs (`THOXA_SANITIZE=leak`) |
| `target/thoxa/bin-address/` | ASan-linked programs (`THOXA_SANITIZE=address`) |

## Environment (selected)

| Variable | Role |
|----------|------|
| `THOXA_STD` | Override product `std/` root |
| `THOXA_ROOT` | Workspace/product root for std resolution |
| `THOXA_SHELL_RC` | Override session rc path |
| `THOXA_NORC` | Skip session rc when set |
| `THOXA_STATUS` | Set by REPL host for prompt helpers |
| `THOXA_FORCE_BUILD` | Set to `1` to ignore the program-binary stamp and rebuild |
| `THOXA_SANITIZE` | Batch `cc` only: `leak` (LeakSanitizer) or `address` (ASan). Product default is unset. Session/libtcc ignores this. `cargo make test` always uses `leak`. `cargo make test-asan` uses `address` on non-fiber conformance (`asan-skip` lists `-->` programs). |

Details for session: [session.md](session.md).
