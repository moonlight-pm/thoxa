# Session (REPL and scripts)

**Thoxa is the session language.** Interactive use and no-`main` scripts share
one model: a live **`Shell`** value, optional command sugar, compile-to-C, and
execution against session state.

Product face: **bare `thoxa`** (not `extras/shell`).

## Modes

| Invocation | Mode |
|------------|------|
| `thoxa` / `thoxa repl` / `thoxa -l` | Long-lived REPL (login shells use `-l` / argv0 `-thoxa`) |
| `thoxa -c '<chunk>'` | One-shot session (what `ssh host cmd` runs) |
| `thoxa file.thx` without `fn main` | Session script (one-shot) |
| `thoxa file.thx` with `fn main` | Ordinary **program** (top-level statements illegal) |
| `thoxa dir/` | `main.thx` program entry |

Session scripts use top-level statements as the body of `Shell.step`. Programs
must keep the file top level to declarations only — put work inside `main`.

## Shell

Session code runs as the body of `Shell.step` (conceptually). The session value
carries at least:

```text
type Shell(status: Int64) { … methods … }
```

Builtins and process helpers are ordinary methods (echo, pwd, cd, run, runIo,
pipeline, …). Default definition lives in product **`std/shell.thx`** and is
auto-injected when a session script has no user `type Shell`.
**`status`** is the last outcome (0 = success). Process exit of a one-shot
script is final `status`. There is no required bash-style `$?` core.

Shell uses `May` / `Has` / `Not` from **`std/fallible`**. When the product Shell
is injected, Fallible is loaded into the package so those names resolve for the
session entry (you do not need a separate Fallible import solely for Shell).
Ordinary programs with `fn main` still **must** import Fallible themselves when
they use May/Trial.

## Command sugar

Familiar command lines desugar to Thoxa on `Shell`:

| You type | Toward |
|----------|--------|
| `echo a b` | `shell = shell.echo(["a", "b"])` |
| `cd /tmp` | `shell = shell.cd(…)` |
| `uname -a` | `shell = shell.run(["uname", "-a"])` |
| `ls -al` / `--all` | Flags desugar as command words (not Thoxa subtraction) |
| `echo hi \| cat` | `shell = shell.pipeline(…)` (OS pipes; last-stage status) |
| `cmd > f` / `>>` / `<` | `shell = shell.runIo(…)` (PATH command; whitespace around ops) |
| `1 - 2` / `1 \| 2` / `1 > 0` | Stays pure Thoxa arithmetic / BitOr / comparison |

Quotes follow the same word-split rules as the dogfood shell (double/single
quotes, backslash escapes). Command words are literal text: `awk '{print $2}'`
passes the braces through (they are not Thoxa interpolation). There is **no**
shell `if`/`for`/`fn` dialect — programming is Thoxa.

## REPL behavior

- Declarations (`import`, `type`, `fn`, …) **accumulate** across chunks.
- A top-level `let` / `mut` runs its initializer **once**, at that line, and
  keeps its value for the rest of the session (**D169**). Later lines and new
  declarations do not run it again, and assignments to a `mut` persist. If the
  initializer panics, the binding is not defined. Module state in `std/` is
  kept the same way.

```thx
§ mut seen = 0
§ seen = seen + 1
§ seen = seen + 1
§ logging.info("{seen}")      ## 2
§ let log = io.openFile("/tmp/log", io.oWronly + io.oCreat)   ## opened once
```
- Top-level statements run as this chunk’s `step` body.
- Failed compiles print diagnostics and keep prior session state (status ≠ 0).
  A declaration that does not compile is reported and **not** kept.
- A panic or crash inside a line prints its message and returns status 134
  (panic) or 128 + signal (crash); the session keeps running (**D160**).
- While a command runs, Ctrl-C stops the command, not the session.
  Commands start with default signal handling (`yes | head -n1` ends quietly).
- Editing an imported `.thx` file (or `std/`) is picked up on the next line.
- Continuation prompt `> ` for unfinished blocks; primary prompt defaults to
  `§ ` unless customized. No startup banner.
- Interactive TTY uses an **emacs** line editor (GNU readline-style keys).
  Tab completes: first word → Shell methods + PATH commands + keywords +
  accumulated `fn`/`let`/`type` names; later words → files; `cd` → dirs;
  `std/…` → product std modules. Ctrl-C cancels the line (session stays).
  Ctrl-D on an empty line exits. History:
  `$XDG_STATE_HOME/thoxa/history` (else `~/.local/state/thoxa/history`).
  Override or disable with `THOXA_HISTFILE` (empty = no history). Piped /
  non-TTY input is still cooked `read_line` (no editor, no rc).
- Executable lines (including `cd`, `status`, `|`, redirections) run as compiled
  session steps (in-process libtcc) — no separate host command shortcuts.

### Line editor keys (emacs)

| Key | Action |
|-----|--------|
| Tab | Complete (list on a second Tab if ambiguous) |
| Ctrl-A / Ctrl-E | Start / end of line |
| Ctrl-B / Ctrl-F | Back / forward char |
| Alt-B / Alt-F | Back / forward word |
| Ctrl-K | Kill to end of line |
| Ctrl-U | Kill to start of line |
| Ctrl-W | Kill previous word |
| Ctrl-Y | Yank |
| Ctrl-P / Ctrl-N | Previous / next history |
| Ctrl-R | Reverse history search |
| Ctrl-T | Transpose chars |
| Ctrl-L | Clear screen |
| Ctrl-_ | Undo |
| Ctrl-C | Discard line |
| Ctrl-D | Delete char, or EOF if empty |

### rc file

Interactive TTY REPL loads:

```text
~/.config/thoxa/shell.thx
```

Missing file = no-op. Overrides: `THOXA_SHELL_RC`, `THOXA_NORC`.  
Piped / non-interactive REPL and session scripts do **not** load rc.

Line comments (`#` / `##`) are comments, not strings: punctuation in them
(apostrophes, parentheses, backticks) does not glue later `import` / `fn`
lines. Declarations accumulate; top-level statements run once.

### Custom prompt

Optional in rc / accumulated decls:

```thx
fn prompt() -> String {
    "§ "
}
```

The host rebuilds and calls this via the session step path (stdout captured).
`THOXA_STATUS` is set for prompt code.

### Custom PATH

Optional:

```thx
fn path(parent: Array<String>) -> Array<String> {
    parent   ## or replace entirely; [] clears PATH
}
```

Return value **replaces** process PATH (joined with `:`). Host keeps toolchain
resolution for `cc` / `ar` when PATH is emptied.

### Relocated install (Oath `pkg:thoxa`)

A prefix-dir install sets `THOXA_ROOT`, `THOXA_STD`, `THOXA_TCCDIR`, and
optional `THOXA_C_INCLUDE` / `THOXA_TCC_LIBPATHS` so session steps do not
need the build machine's Nix paths. REPL / `thoxa -c` use libtcc (no
guest `cc`). `thoxa build` / `thoxa run` of programs still need `cc`.
Session steps compile in memory and write nothing to disk. With
`THOXA_REPL_KEEP_C=1` the generated C goes under `$XDG_CACHE_HOME/thoxa/repl`
(or `~/.cache/thoxa/repl`) — not the store.

### Pipelines and redirections

| Sugar | Behavior |
|-------|----------|
| `a \| b \| c` | `Shell.pipeline` — OS pipes; status = last stage |
| `cmd > file` | Truncate/write stdout via `Shell.runIo` |
| `cmd >> file` | Append stdout |
| `cmd < file` | stdin from file |
| Combined `<` and `>` / `>>` | Supported on one command |

Redir ops must be separate words (whitespace). Uses PATH commands (including
`echo`), not the Thoxa `Shell.echo` builtin. No stderr redir (`2>`) yet; pipe
stages do not yet carry per-stage redirs.

## Session scripts

A file without `main` is wrapped: declarations stay at module scope; statements
become `Shell.step`; synthetic `main` returns `step().status`.

## Dogfood package

`extras/shell` is a pure-Thoxa prototype of the Shell API for ladder dogfood.
It is **not** the product session face. Prefer:

```bash
cargo make shell
# or
cargo run -p thoxa --
```
