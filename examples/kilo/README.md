# examples/kilo — terminal text editor

Native port of the oracle `examples/kilo` (inspired by antirez’s kilo): raw
mode, file open/save, movement, editing, find, and C-ish syntax highlighting.

## Run

Needs a **real TTY** (not a plain pipe). From the workspace root:

```bash
cargo make run examples/kilo -- examples/kilo/sample.txt
# or
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.c
```

| Key | Action |
|-----|--------|
| Arrows / Home / End / PgUp / PgDn | Move |
| Printable / Tab / Enter | Insert |
| Backspace / Delete | Erase |
| **Ctrl-S** | Save (prompts for name if unnamed) |
| **Ctrl-F** | Incremental find |
| **Ctrl-Q** | Quit (3× if dirty) |

## Layout

| Module | Role |
|--------|------|
| `Main.thx` | Driver loop + key dispatch |
| `Terminal.thx` | termios raw mode + window size |
| `Keys.thx` | Read + decode VT100 keys |
| `Row.thx` | Line buffer + tab expand |
| `FileIO.thx` | Open / save |
| `State.thx` | Cursor, buffer, dirty, status |
| `Screen.thx` | Frame paint |
| `Syntax.thx` | Highlight classes |
| `Find.thx` / `Prompt.thx` | Search + save-as |
