# extras/shell — thoxash

A **Thoxa-written shell**, living in the monorepo as double dogfood:

1. **Use Thoxa** to build a real interactive program (REPL, libc via `extern`, Io patterns).
2. **Drive language gaps** from real needs (process spawn, argv, line editing, …) instead of abstract rungs.

Not on the compile-to-C product path. Not a bash/zsh script runner.

**Direction:** Thoxa is the session language. **Product REPL is bare `thoxa`** (D88). This package remains a pure-Thoxa dogfood of `type Shell`.  
Plan: [`docs/specs/2026-07-24-thoxa-session-repl.md`](../../docs/specs/2026-07-24-thoxa-session-repl.md).

```bash
cargo run -p thoxa --              # product session REPL
cargo make shell                   # same
cargo make run extras/shell        # this dogfood binary
```

## v4 (now) — `Shell` + `status`

Session value:

```thoxa
type Shell(status: Int64) {
    echo(args) -> Shell
    pwd() -> Shell
    cd(args) -> Shell
    run(argv) -> Shell
    showStatus() -> Shell   ## builtin name: status
    help() -> Shell
}
```

Methods return a **new** `Shell` with updated `status`. The REPL holds `mut shell` and threads it through `evalLine`. Process exit code = final `shell.status`.

| Input | Behavior | Typical status |
|-------|----------|----------------|
| `echo …` | print args (spaces); quotes ok | 0 |
| `pwd` | print cwd | 0 / 1 |
| `cd [path]` | chdir; no arg → `$HOME` | 0 / 1 |
| `status` | print `shell.status` (unchanged) | same |
| `help` | usage | 0 |
| `exit` / `quit` | leave (exit code = status) | — |
| other word | PATH / path with `/` via `run` | child / 127 / 1 |
| unclosed quote | error; line not run | 2 |
| Ctrl-D / EOF | leave | final status |

### Word split (unchanged from v3)

| Form | Behavior |
|------|----------|
| unquoted space/tab | separates words |
| `"…"` | keep spaces; `\` escapes next byte |
| `'…'` | literal until closing `'` |
| unquoted `\` | escapes next byte |
| adjacent pieces | join into one word |
| `""` / `''` | empty word |

### Target desugar table (host not present yet)

When the session host exists, command skin becomes real Thoxa on `Shell`:

| Prompt | Toward |
|--------|--------|
| `echo a b` | `shell = shell.echo(["a", "b"])` |
| `cd /tmp` | `shell = shell.cd(["/tmp"])` |
| `uname -a` | `shell = shell.run(["uname", "-a"])` |
| `status` | `shell = shell.showStatus()` |
| `let x = 1` | as-is Thoxa (later) |

No shell `if`/`for`/`fn`. Programming = Thoxa.

`$ ` prompts only on a TTY. Spawn uses RT `thoxa_spawn_wait` (argv = NUL-separated `Bytes`).

```bash
# from repo root
cargo make shell

# smoke status
printf 'echo hi\nstatus\nfalse\nstatus\nexit\n' | cargo make shell
# → hi / 0 / 1 ; process exit 1
```

Entry is `Main.thx` (D68).

## Layout

```text
extras/shell/
  Main.thx     # Shell + REPL loop
  README.md
```

## Next (plan PRs)

1. **PR1** — compiler: no-`main` session scripts (`Shell.step` wrap).
2. **PR2+** — command desugar, bare `thoxa` REPL, pipes, …

Keep this file small until a second module earns its keep.
