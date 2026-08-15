# examples/kilo — terminal text editor

Native port of the oracle `examples/kilo` (inspired by antirez’s kilo): raw
mode, file open/save, movement, editing, find, and C-ish syntax highlighting.

**Status:** shipped and **user-tested** (empty buffer + typing; D122 module-global
RC). Product surface is still sample-driven — modules live next to `main.thx`,
not yet under `std/`.

## Run

Needs a **real TTY** (not a plain pipe). From the **workspace root**:

```bash
# Empty buffer (new file)
cargo run -p thoxa -- run examples/kilo

# Open a file (argv after the package path)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.c   # C-ish highlight
```

## Keys

| Key | Action |
|-----|--------|
| Arrows / Home / End / PgUp / PgDn | Move |
| Printable / Tab / Enter | Insert |
| Backspace / Delete | Erase |
| **Ctrl-S** | Save (prompts for name if the buffer is unnamed) |
| **Ctrl-F** | Incremental find |
| **Ctrl-Q** | **Quit** — once if clean; **three times** if unsaved (dirty) |

## Layout

| Module | Role |
|--------|------|
| `main.thx` | Driver loop + key dispatch |
| `terminal.thx` | termios raw mode + window size (`Bytes` ABI buffers) |
| `keys.thx` | Read + decode VT100 keys |
| `row.thx` | Line buffer + tab expand |
| `file-io.thx` | Open / save |
| `state.thx` | Cursor, buffer, dirty, status (module mut) |
| `screen.thx` | Frame paint |
| `syntax.thx` | Highlight classes (recomputed per paint) |
| `find.thx` / `prompt.thx` | Search + save-as |

## Notes

- Window size / raw mode fail cleanly off a TTY (message + exit 1).
- Syntax highlight is recomputed each frame (no cache of `Array` of structs that
  themselves hold `Array` fields — not yet supported as array elements).
- Editing an empty buffer reassigns module `mut textRows`; that requires D122
  (module heap globals are not dropped at end of the assigning function).
