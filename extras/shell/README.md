# extras/shell — dogfood Shell API

A **pure-Thoxa** interactive shell used as double dogfood:

1. Exercise real programs (REPL loop, libc via `extern`, Io patterns).
2. Keep the `type Shell` API shape readable without the host.

**This is not the product session face.** Product REPL is **bare `thoxa`**
(D88, D95). Prefer:

```bash
cargo run -p thoxa --              # product session REPL
cargo make shell                   # same
cargo make run extras/shell        # this dogfood binary only
```

Handbook: [`docs/handbook/session.md`](../../docs/handbook/session.md).  
Plan: [`docs/specs/2026-07-24-thoxa-session-repl.md`](../../docs/specs/2026-07-24-thoxa-session-repl.md).

## Shell shape (dogfood)

```thx
type Shell(status: Int64) {
    echo(args) -> Shell
    pwd() -> Shell
    cd(args) -> Shell
    run(argv) -> Shell
    showStatus() -> Shell   ## builtin name: status
    help() -> Shell
}
```

Methods return a **new** `Shell` with updated `status`. Process exit code =
final `shell.status`.

| Input | Behavior |
|-------|----------|
| `echo …` | print args; quotes ok |
| `pwd` / `cd [path]` | cwd helpers |
| `status` | print `shell.status` |
| `help` / `exit` / `quit` | usage / leave |
| other word | PATH lookup via `run` |
| unclosed quote | error; line not run |

Word split: unquoted space; `"…"` / `'…'`; backslash escapes; adjacent pieces
join.

### Target desugar (product host)

When using bare `thoxa`, command skin becomes real Thoxa on `Shell`:

| Prompt | Toward |
|--------|--------|
| `echo a b` | `shell = shell.echo(["a", "b"])` |
| `cd /tmp` | `shell = shell.cd(["/tmp"])` |
| `uname -a` | `shell = shell.run(["uname", "-a"])` |
| `let x = 1` | as-is Thoxa |

No shell `if`/`for`/`fn`. Programming = Thoxa.

```bash
printf 'echo hi\nstatus\nfalse\nstatus\nexit\n' | cargo make run extras/shell
```

Entry is `Main.thx` (D68). Spawn uses RT `thoxa_spawn_wait`.

## Layout

```text
extras/shell/
  Main.thx     # Shell + REPL loop
  README.md
```
