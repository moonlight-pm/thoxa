# Session

`thoxa` with no arguments is an interactive shell, and a `.thx` file without a
`main` is a session script. Both run Thoxa: there is no second command
language. Familiar command lines (`ls -al`, `echo hi | wc -c`) are sugar for
calls on a `Shell` value, and everything else is ordinary Thoxa compiled to C.

## Modes

| Invocation | Mode |
|------------|------|
| `thoxa`, `thoxa repl`, `thoxa -l` / `--login` | Interactive shell (a login shell passes `-l`, or argv0 `-thoxa`) |
| `thoxa -c '<chunk>'` | Runs one chunk and exits (what `ssh host cmd` runs); its `import`, `fn`, `type`, and `let` forms are declarations, and the rest runs in order |
| `thoxa file.thx`, no `fn main` | Session script, run once |
| `thoxa file.thx`, with `fn main` | A program; top-level statements are a compile error |
| `thoxa dir/` | Runs `dir/main.thx` as a program |

## Shell

Session code runs as the body of a method `Shell.step` on a live `Shell`
value. The value carries at least:

```text
type Shell(status: Int64) { … methods … }
```

`status` is the outcome of the last command (0 is success); the exit status of
a session script is its final `status`. There is no `$?`. The built-in
commands and process helpers are ordinary methods (`echo`, `pwd`, `cd`, `run`,
`runIo`, `pipeline`, …) defined in [`std/shell.thx`](../../std/shell.thx),
which is loaded automatically when a session script defines no `type Shell` of
its own.

`Shell` uses `May`, `Has`, and `Not`, so loading it also loads `std/fallible`;
session code can use those names without importing them. Programs with
`fn main` must import `std/fallible` themselves.

## Command sugar

A line that reads as a command line becomes a call on the shell:

| You type | Becomes |
|----------|---------|
| `echo a b` | `shell = shell.echo(["a", "b"])` |
| `cd /tmp` | `shell = shell.cd(…)` |
| `uname -a` | `shell = shell.run(["uname", "-a"])` |
| `ls -al`, `ls --all` | flags are command words, not subtraction |
| `echo hi \| cat` | `shell = shell.pipeline(…)` (OS pipes; the status is the last stage's) |
| `cmd > file`, `cmd >> file`, `cmd < file` | `shell = shell.runIo(…)` |
| `1 - 2`, `1 \| 2`, `1 > 0` | stay Thoxa: subtraction, bitwise or, comparison |

Words split as in a POSIX shell: double and single quotes and backslash
escapes. Command words are literal text, so `awk '{print $2}'` passes the
braces through rather than reading them as interpolation. There is no shell
`if`, `for`, or function syntax; control flow is Thoxa.

Only a top-level line can be a command. A line that starts with `if`, `for`,
`while`, `loop`, `match`, `return`, `break`, or `continue`, or that opens a
block continuing on the next lines, is Thoxa. Inside such a block, call the
shell directly:

```thx
let names = ["ada", "grace"]
for name in names {
    shell = shell.run(["echo", "hello", name])
}
```

A Thoxa value does not reach a command line by interpolation (`echo {name}`
is not a command); pass it in the argument list of `shell.run` as above.

### Pipelines and redirections

| Form | Behaviour |
|------|-----------|
| `a \| b \| c` | OS pipes; the status is the last stage's |
| `cmd > file` | standard output to `file`, truncating |
| `cmd >> file` | standard output appended to `file` |
| `cmd < file` | standard input from `file` |
| `<` with `>` or `>>` | both on one command |

Redirection operators must be separate words (surrounded by whitespace). They
run commands found on `PATH` (so `echo` is the system one), not the
`Shell.echo` builtin.
There is no `2>` and no redirection on individual pipeline stages.

## REPL behavior

- Declarations (`import`, `type`, `fn`, …) accumulate across lines. A
  declaration that does not compile is reported and not kept.
- A top-level `let` or `mut` runs its initializer once, at that line, and keeps
  its value for the rest of the session. Later lines and new declarations do
  not run it again, and assignments to a `mut` persist. If the initializer
  panics, the name is not defined. Module state in `std/` is kept the same way.

  ```thx
  § mut seen = 0
  § seen = seen + 1
  § seen = seen + 1
  § logging.info("{seen}")      ## 2
  § let log = io.openFile("/tmp/log", io.oWronly + io.oCreat)   ## opened once
  ```

- Top-level statements run as that line's `step`.
- A line that fails to compile prints diagnostics, sets a non-zero status, and
  leaves the session as it was.
- A panic or crash inside a line prints its message and sets status 134 (a
  panic) or 128 + signal (a crash); the session keeps running.
- While a command runs, Ctrl-C stops the command, not the session. Commands
  start with default signal handling, so `yes | head -n1` ends quietly.
- Edits to an imported `.thx` file (or to `std/`) take effect on the next
  line.
- The primary prompt is `§ ` unless customized; the continuation prompt for an
  unfinished block is `> `. There is no startup banner.
- Every line, including `cd`, `status`, pipelines, and redirections, runs as a
  compiled step, in process (TinyCC). There are no special host commands.

### Line editing

On a terminal the shell uses an Emacs-style line editor. Tab completes: the
first word against `Shell` methods, `PATH` commands, keywords, and declared
`fn` / `let` / `type` names; later words against files; `cd` arguments against
directories; `std/…` against standard modules. History is kept in
`$XDG_STATE_HOME/thoxa/history` (else `~/.local/state/thoxa/history`);
`THOXA_HISTFILE` overrides the path, and an empty value disables history. Input
that is not a terminal is read line by line, with no editor and no rc file.

| Key | Action |
|-----|--------|
| Tab | Complete (a second Tab lists the choices) |
| Ctrl-A / Ctrl-E | Start / end of line |
| Ctrl-B / Ctrl-F | Back / forward one character |
| Alt-B / Alt-F | Back / forward one word |
| Ctrl-K | Delete to end of line |
| Ctrl-U | Delete to start of line |
| Ctrl-W | Delete the previous word |
| Ctrl-Y | Paste the last deletion |
| Ctrl-P / Ctrl-N | Previous / next history entry |
| Ctrl-R | Search history backwards |
| Ctrl-T | Swap two characters |
| Ctrl-L | Clear the screen |
| Ctrl-_ | Undo |
| Ctrl-C | Discard the line |
| Ctrl-D | Delete a character, or exit on an empty line |

### rc file

An interactive shell on a terminal loads `~/.config/thoxa/shell.thx` at
start. A missing file is not an error. `THOXA_SHELL_RC` names another file;
`THOXA_NORC` skips it. Piped input, `thoxa -c`, and session scripts do not load
it.

`#` and `##` lines in the rc file are comments, so punctuation in them
(apostrophes, parentheses, backticks) does not join later `import` / `fn`
lines. Declarations accumulate; top-level statements run once.

### Custom prompt

Define `prompt` in the rc file (or at the prompt):

```thx
fn prompt() -> String {
    "§ "
}
```

The shell calls it before each line; `THOXA_STATUS` holds the last status
while it runs.

### Custom PATH

Define `path` to compute the command search path:

```thx
fn path(parent: Array<String>) -> Array<String> {
    parent   ## or a new list; [] clears PATH
}
```

The returned array replaces the process `PATH` (joined with `:`). The shell
still finds `cc` and `ar` for its own use when `PATH` is empty.

### Relocated installs

An install outside the build tree (such as Oath's `pkg:thoxa`) sets
`THOXA_ROOT`, `THOXA_STD`, `THOXA_TCCDIR`, and optionally `THOXA_C_INCLUDE` /
`THOXA_TCC_LIBPATHS`, so that session steps do not depend on the build
machine's paths. The interactive shell and `thoxa -c` compile with the built-in
TinyCC and need no system C compiler; `thoxa build` and `thoxa run` of programs
still need `cc`. Session steps compile in memory and write nothing to disk.
With `THOXA_REPL_KEEP_C=1` the generated C is kept under
`$XDG_CACHE_HOME/thoxa/repl` (or `~/.cache/thoxa/repl`).

## Session scripts

A file without `main` is wrapped: its declarations stay at module scope, its
statements become `Shell.step`, and a generated `main` returns the final
`status`.

## The extras/shell package

`extras/shell` is an earlier pure-Thoxa prototype of the `Shell` API, kept as a
test program. It is not the shell; use `thoxa` (or `cargo make shell` in a
checkout).
