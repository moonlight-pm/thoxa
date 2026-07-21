# extras/shell — thoxash

A **Thoxa-written shell**, living in the monorepo as double dogfood:

1. **Use Thoxa** to build a real interactive program (REPL, libc via `extern`, Io patterns).
2. **Drive language gaps** from real needs (process spawn, argv, line editing, …) instead of abstract rungs.

Not on the compile-to-C product path. Not a bash/zsh script runner — dialect and builtins grow from *using* it.

## v2 (now)

Word-split, builtins, and **external commands** (PATH + `fork`/`execv`/`wait`):

| Input | Behavior |
|-------|----------|
| `echo …` | print args joined by spaces (no args → blank line) |
| `pwd` | print cwd (`getcwd`) |
| `cd [path]` | change dir (`chdir`); no arg → `$HOME` |
| `help` | one-line usage |
| `exit` / `quit` | leave |
| other word | resolve via PATH (or absolute/relative if `/` present); run |
| missing | `thoxash: …: command not found` |
| Ctrl-D / EOF | leave |

Whitespace split only (space/tab). No quotes, pipes, redirections, jobs, or history yet.

`$ ` prompts only when stdin is a TTY; piped input is silent on the prompt (cleaner smoke tests).

Spawn uses RT helper `thoxa_spawn_wait` (Array is not FFI-marshalable yet — argv is a NUL-separated `Bytes` blob).

```bash
# from repo root
cargo make shell
# or
cargo make run extras/shell
cargo run -p thoxa -- run extras/shell

# non-interactive smoke
printf 'echo hi\nuname -s\npwd\nexit\n' | cargo make shell
```

Entry is `Main.thx` (directory entry, D68).

## Why a REPL first

A shell is basically:

```text
loop {
  prompt
  read line
  parse → eval → print status
}
```

v0 was **eval = echo**. v1 hung real builtins off `evalLine` / `splitWords`. v2 runs externals the same way.

## Likely next rungs (not scheduled)

Rough order — only when dogfooding wants them:

1. **Quotes / escapes** in word split.
2. **Pipes / redirections**.
3. **Exit status** (`$?` / `status`) after spawn.
4. **Line editing / history** — may stay crude (`read` only) for a long time.
5. **Config** — optional `~/…` startup file written in Thoxa *or* shell dialect later.

Compatibility with bash/zsh **scripts** is explicitly out of scope. Familiarity of *interactive* habits (prompt, enter runs a line, Ctrl-D) is enough.

## Layout

```text
extras/shell/
  Main.thx     # entry: REPL loop + readLine + eval
  README.md    # this file
```

Keep it small until a second module earns its keep (e.g. `Line.thx`, `Exec.thx`).
