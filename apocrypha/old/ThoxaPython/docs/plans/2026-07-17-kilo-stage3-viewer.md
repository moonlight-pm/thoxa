# Kilo Stage 3 — Read-only Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build kilo's read-only file viewer in Thoxa — open a file named on the command line, load its lines as `Row`s, and draw the visible window with vertical/horizontal scroll and tab expansion.

**Architecture:** Two language increments first (a copying `Bytes.slice`, and an optional command-line `arguments` parameter on `main`), then four editor modules: `Row` (tab-expanded render + `cxToRx` + `window`), `FileIO` (open → split into rows), a file-coordinate cursor + scroll in `State`, and text-row drawing in `Screen`, wired together in `Main`.

**Tech Stack:** Thoxa (prototype interpreter in Python under `thoxa/`); pytest; the `.thx` example modules under `examples/kilo/`.

## Global Constraints

- **Design source of truth:** `docs/specs/2026-07-17-kilo-stage3-viewer-design.md`. Section refs below (`§N`) point there.
- **Rows are byte-buffers, ASCII-first.** `chars`/`render` are `Bytes`; `cx`/`cy`/`rx`/`rowoff`/`coloff` are byte offsets (`Int64`). Screen dimensions stay `Int32` (from `Terminal.windowSize()`); widen `Int32→Int64` at use, never narrow back.
- **`Bytes.slice(start, end)` is strict:** requires `0 ≤ start ≤ end ≤ count`, else traps. Callers clamp first.
- **`Array.append` returns a new array** (`rows = rows.append(x)`); there is no in-place push.
- **Commit to `master`** directly (no feature branch). Each task ends green and gets its own commit. Commit trailer:
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6
  ```
- **Run the suite** with `python -m pytest -q` from the repo root; single tests via `python -m pytest tests/FILE.py::NAME -q`.
- **Naming:** full words, no abbreviations (the param is `arguments`, not `argv`).

## File / Task Map

| Task | Create | Modify | Test |
| --- | --- | --- | --- |
| 1 Bytes.slice | `spec/conformance/bytes_slice.thx`(+`.out`) | `thoxa/ffi.py`, `thoxa/interp.py` | `tests/test_bytes_slice.py` |
| 2 arguments | — | `thoxa/cli.py`, `thoxa/interp.py` | `tests/test_arguments.py` |
| 3 Row | `examples/kilo/Row.thx` | — | `tests/test_kilo_row.py` |
| 4 FileIO | `examples/kilo/FileIO.thx`, `examples/kilo/sample.txt` | — | `tests/test_kilo_fileio.py` |
| 5 State | — | `examples/kilo/State.thx` | `tests/test_kilo_state.py` |
| 6 Screen | — | `examples/kilo/Screen.thx` | `tests/test_kilo_screen.py` |
| 7 Main | — | `examples/kilo/Main.thx`, `tests/test_kilo_keys.py` | `tests/test_kilo_keys.py` |
| 8 Docs | — | `spec/SPEC.md`, `CHANGELOG.md`, roadmap | (suite) |

Tasks 1–4 are independent and each end green. Task 5 keeps `State`'s Stage-2 exports alive (so `Screen`/`Main` still type-check) while adding the new surface; Tasks 6–7 migrate the consumers; Task 7 removes the dead Stage-2 movement API.

---

### Task 1: `Bytes.slice(start, end) -> Bytes`

**Files:**
- Modify: `thoxa/ffi.py` (`Bytes` class), `thoxa/interp.py` (`_member` gate + `_bytes_method`)
- Create: `spec/conformance/bytes_slice.thx`, `spec/conformance/bytes_slice.out`
- Test: `tests/test_bytes_slice.py`

**Interfaces:**
- Produces: `someBytes.slice(start: Int64, end: Int64) -> Bytes` — a fresh, independent copy of `[start, end)`; traps on out-of-range bounds.

- [ ] **Step 1: Write the failing conformance program** `spec/conformance/bytes_slice.thx`

```thoxa
## Bytes.slice(start, end) — the Stage 3 language increment (fork #5). A copying,
## strict-bounds byte slice. Every value is asserted through .count/.get/.toString so
## this program's stdout carries no raw control bytes.

export fn main() -> Int64 {
    let s = Bytes.fromString("hello, world")

    ## a full-range slice equals the source
    let all = s.slice(0, s.count)
    println("all count={all.count} text={all.toString()}")

    ## a sub-range has the right length and bytes
    let sub = s.slice(7, 12)
    println("sub count={sub.count} text={sub.toString()}")

    ## an empty slice
    let empty = s.slice(3, 3)
    println("empty count={empty.count}")

    ## the slice is an independent copy: mutating a Buffer built from the source
    ## does not disturb the earlier slice
    mut b = Buffer()
    b.appendBytes(s)
    b.set(7, b'W')
    println("sub-after count={sub.count} text={sub.toString()}")

    ## splitting on a byte using slice: "ab\ncd" -> "ab", "cd"
    let data = Bytes.fromString("ab\ncd")
    let line0 = data.slice(0, 2)
    let line1 = data.slice(3, 5)
    println("split a={line0.toString()} b={line1.toString()}")
    0
}
```

- [ ] **Step 2: Write the expected output** `spec/conformance/bytes_slice.out`

```
all count=12 text=hello, world
sub count=5 text=world
empty count=0
sub-after count=5 text=world
split a=ab b=cd
```

- [ ] **Step 3: Run it — expect FAIL** (`slice` unknown)

Run: `python -m pytest "tests/test_conformance.py::test_conformance[bytes_slice.thx]" -q`
Expected: FAIL (`Bytes has no member 'slice'`).

- [ ] **Step 4: Implement `Bytes.slice` in `thoxa/ffi.py`**

Add a method to the `Bytes` class (near `get`/`set`/`to_string`). Use the class's existing internal store + `from_bytes`; confirm the exact field name (`_len`/`count` and the backing buffer) by reading the class first. Shape:

```python
    def slice(self, start: int, end: int) -> "Bytes":
        start, end = int(start), int(end)
        if not (0 <= start <= end <= self.count):
            raise IndexError((start, end))
        return Bytes.from_bytes(self.to_bytes()[start:end])
```

(If `Bytes` has no `to_bytes()` helper, slice its backing `bytearray`/`bytes` directly, e.g. `bytes(self._store[start:end])`. Return a *fresh* `Bytes`.)

- [ ] **Step 5: Wire dispatch in `thoxa/interp.py` (two sites)**

1. `_member` gate (~line 1453): add `"slice"` to the allowed-members tuple:
   `if name in ("get", "set", "toString", "slice"):`
2. `_bytes_method` (~lines 1121–1141): before the final `self.error(...)`, add:

```python
        if name == "slice":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Bytes.slice(start, end) expects two integers", span)
            try:
                return buf.slice(values[0], values[1])
            except IndexError:
                self.error(
                    f"slice [{values[0]}..{values[1]}] out of bounds "
                    f"(length {buf.count})",
                    span,
                )
```

(Use whatever the handler's receiver local is named — match `get`/`set` above it.)

- [ ] **Step 6: Run conformance + checker + lsp — expect PASS**

Run: `python -m pytest "tests/test_conformance.py::test_conformance[bytes_slice.thx]" "tests/test_checker.py" "tests/test_lsp.py" -q -k "bytes_slice or conformance"`
Expected: PASS (type-checks clean, no diagnostics, stdout matches `.out`).

- [ ] **Step 7: Write the error-path unit tests** `tests/test_bytes_slice.py`

```python
"""Bytes.slice — the Stage 3 slice primitive. Strict bounds (trap on out-of-range),
copying (source unaffected)."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<slice>", out=buf.append)
    return "".join(buf), code


def test_slice_full_and_sub():
    out, _ = _run(
        'let s = Bytes.fromString("abcdef")\n'
        'println("{s.slice(0,6).toString()} {s.slice(2,4).toString()}")'
    )
    assert out == "abcdef cd\n"


@pytest.mark.parametrize("call", [
    "s.slice(0, 7)",     # end > count
    "s.slice(4, 2)",     # start > end
    "s.slice(0 - 1, 3)", # negative start
])
def test_slice_out_of_bounds_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let s = Bytes.fromString("abcdef")\nprintln("{{{call}.count}}")')
    assert "out of bounds" in exc.value.render()


def test_slice_is_a_copy():
    # the slice keeps its bytes even after the source buffer is mutated
    out, _ = _run(
        'let s = Bytes.fromString("abcdef")\n'
        'let sl = s.slice(0, 3)\n'
        'mut b = Buffer()\n'
        'b.appendBytes(s)\n'
        'b.set(0, b\'Z\')\n'
        'println(sl.toString())'
    )
    assert out == "abc\n"
```

- [ ] **Step 8: Run the unit tests — expect PASS**

Run: `python -m pytest tests/test_bytes_slice.py -q`
Expected: PASS (all).

- [ ] **Step 9: Commit**

```bash
git add thoxa/ffi.py thoxa/interp.py spec/conformance/bytes_slice.thx spec/conformance/bytes_slice.out tests/test_bytes_slice.py
git commit -m "kilo/lang: Bytes.slice(start, end) — copying, strict-bounds byte slice (fork #5)"
```

---

### Task 2: command-line `arguments`

**Files:**
- Modify: `thoxa/cli.py` (`run` subparser + `cmd_run`), `thoxa/interp.py` (`run()` + `call_main`)
- Test: `tests/test_arguments.py`

**Interfaces:**
- Produces: `export fn main(arguments: Array<String>) -> Int64` receives `[<module-path>, <trailing tokens…>]`; a zero-parameter `main()` still runs.

- [ ] **Step 1: Write the failing test** `tests/test_arguments.py`

```python
"""Command-line `arguments` on main. `thoxa run <module> a b` gives main
arguments == [<module>, "a", "b"]; a zero-arg main still runs."""
from __future__ import annotations

import subprocess
import sys
import pathlib

REPO = pathlib.Path(__file__).resolve().parent.parent


def _run_cli(rel_module: str, *extra: str):
    return subprocess.run(
        [sys.executable, "-m", "thoxa", "run", rel_module, *extra],
        cwd=REPO, capture_output=True, text=True,
    )


def test_arguments_reach_main(tmp_path):
    mod = tmp_path / "args_probe.thx"
    mod.write_text(
        'export fn main(arguments: Array<String>) -> Int64 {\n'
        '    println("count={arguments.count}")\n'
        '    if arguments.count > 1 { println("first={arguments[1]}") }\n'
        '    0\n'
        '}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod), "hello.txt", "second")
    assert r.returncode == 0, r.stderr
    assert "count=3" in r.stdout
    assert "first=hello.txt" in r.stdout


def test_no_arguments(tmp_path):
    mod = tmp_path / "args_probe.thx"
    mod.write_text(
        'export fn main(arguments: Array<String>) -> Int64 {\n'
        '    println("count={arguments.count}")\n    0\n}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod))
    assert r.returncode == 0, r.stderr
    assert "count=1" in r.stdout  # just the module path


def test_zero_param_main_still_runs(tmp_path):
    mod = tmp_path / "noargs.thx"
    mod.write_text(
        'export fn main() -> Int64 {\n    println("ok")\n    0\n}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod))
    assert r.returncode == 0, r.stderr
    assert "ok" in r.stdout
```

- [ ] **Step 2: Run — expect FAIL** (`main` currently gets no arguments)

Run: `python -m pytest tests/test_arguments.py -q`
Expected: FAIL (`arguments.count` → main called with none / arity error).

- [ ] **Step 3: CLI — collect and thread the tokens** (`thoxa/cli.py`)

- In the `run` subparser, after `p_run.add_argument("file")`, add:
  `p_run.add_argument("arguments", nargs=argparse.REMAINDER)`  (REMAINDER, not `"*"`, so option-like tokens pass through)
- Change the dispatch `return cmd_run(args.file)` → `return cmd_run(args.file, args.arguments)`.
- Change `def cmd_run(path: str) -> int:` → `def cmd_run(path: str, arguments: list[str]) -> int:` and its `run(text, path)` call → `run(text, path, arguments=arguments)`.

- [ ] **Step 4: Runtime — forward to `main` via `apply`** (`thoxa/interp.py`)

- `run(...)` signature: add `arguments=()` to the keyword-only params; change `return interp.call_main()` → `return interp.call_main(arguments)`.
- Replace `call_main`:

```python
    def call_main(self, arguments=()):
        if self.globals.has("main"):
            main = self.globals.get("main")
            if isinstance(main, Function):
                if main.defn.params:                       # main(arguments)
                    return self.apply(main, [list(arguments)], Span(1, 1))
                return self.apply(main, [], Span(1, 1))     # nullary main
        return VOID
```

(Must be `apply`, not `call_function` — `apply`/`_bind_values` bind runtime values by position.)

- [ ] **Step 5: Run — expect PASS**

Run: `python -m pytest tests/test_arguments.py -q`
Expected: PASS (all three).

- [ ] **Step 6: Regression — the full suite still green**

Run: `python -m pytest -q`
Expected: PASS (existing `main()` examples/conformance unaffected).

- [ ] **Step 7: Commit**

```bash
git add thoxa/cli.py thoxa/interp.py tests/test_arguments.py
git commit -m "kilo/lang: optional command-line `arguments` on main (C-style, via apply)"
```

---

### Task 3: `Row` module

**Files:**
- Create: `examples/kilo/Row.thx`
- Test: `tests/test_kilo_row.py`

**Interfaces:**
- Produces: `Row(chars!: Bytes)` with `charCount()`/`renderCount()`/`cxToRx(cx!: Int64)`/`window(coloff!: Int64, width!: Int64) -> Bytes`. Import as `import { Row } from "./Row"`.

- [ ] **Step 1: Write the failing tests** `tests/test_kilo_row.py`

```python
"""Row — kilo's erow: tab-expanded render, cxToRx, and the visible window slice."""
from __future__ import annotations

import pathlib
from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = 'import { Row } from "./Row"\nfn main() -> Int64 {\n' + body + "\n0\n}\n"
    buf: list[str] = []
    run(src, str(KILO / "row_probe.thx"), out=buf.append)
    return "".join(buf)


def test_row_module_type_checks_clean():
    p = KILO / "Row.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_tab_expands_to_next_stop():
    # "a\tb": 'a' at col0, tab fills cols 1..7 (to stop 8), 'b' at col8 -> render len 9
    out = _probe(
        'let r = Row(Bytes.fromString("a\\tb"))\n'
        'println("rc={r.renderCount()} cc={r.charCount()} b8={r.window(8,1).toString()}")'
    )
    assert out == "rc=9 cc=3 b8=b\n"


def test_cx_to_rx_counts_tab_width():
    # chars "a\tb": cx=0 -> rx 0; cx=1 (at tab) -> rx 1; cx=2 (after tab) -> rx 8
    out = _probe(
        'let r = Row(Bytes.fromString("a\\tb"))\n'
        'println("{r.cxToRx(0)} {r.cxToRx(1)} {r.cxToRx(2)}")'
    )
    assert out == "0 1 8\n"


def test_window_clamps_both_ends():
    out = _probe(
        'let r = Row(Bytes.fromString("hello"))\n'
        'println("mid={r.window(1,3).toString()} past={r.window(9,4).count} tail={r.window(3,99).toString()}")'
    )
    assert out == "mid=ell past=0 tail=lo\n"
```

- [ ] **Step 2: Run — expect FAIL** (no `Row.thx`)

Run: `python -m pytest tests/test_kilo_row.py -q`
Expected: FAIL (module not found).

- [ ] **Step 3: Create `examples/kilo/Row.thx`**

```thoxa
## Row — Stage 3 of the kilo port: one text line (kilo's `erow`). `chars` is the raw
## bytes; `render` is the display expansion (tabs → spaces to the next tab stop), built
## once at construction. Both are `Bytes` — the viewer is byte-oriented (fork #5: rows
## are byte-buffers end-to-end, ASCII-first). Read-only; the rebuild-on-edit path lands
## with editing (Stage 4).

let tabStop = 8    # KILO_TAB_STOP

## Expand tabs in `chars` to spaces up to the next multiple of `tabStop`. Positional
## `chars!` so the bare `expandTabs(chars)` in the field initializer binds.
fn expandTabs(chars!: Bytes) -> Bytes {
    mut buf = Buffer()
    mut col = 0
    mut i = 0
    let n = chars.count
    loop {
        if i >= n { break }
        let b = chars.get(i)
        if b == b'\t' {
            buf.append(b' ')
            col = col + 1
            loop {
                if col % tabStop == 0 { break }
                buf.append(b' ')
                col = col + 1
            }
        } else {
            buf.append(b)
            col = col + 1
        }
        i = i + 1
    }
    buf.freeze()
}

export type Row(chars!: Bytes) {
    let render = expandTabs(chars)

    export fn charCount() -> Int64 { chars.count }
    export fn renderCount() -> Int64 { render.count }

    ## chars-index → render-column (kilo editorRowCxToRx): a tab advances to the stop.
    export fn cxToRx(cx!: Int64) -> Int64 {
        mut rx = 0
        mut i = 0
        loop {
            if i >= cx { break }
            if chars.get(i) == b'\t' {
                rx = rx + (tabStop - 1) - (rx % tabStop)
            }
            rx = rx + 1
            i = i + 1
        }
        rx
    }

    ## The visible byte window of `render`: render[coloff .. coloff+width], both bounds
    ## clamped into [0, renderCount] so the strict Bytes.slice never traps. Empty when
    ## scrolled past the line's end.
    export fn window(coloff!: Int64, width!: Int64) -> Bytes {
        let count = render.count
        let begin = if coloff > count { count } else { coloff }
        let stop = if coloff + width > count { count } else { coloff + width }
        render.slice(begin, stop)
    }
}
```

- [ ] **Step 4: Run — expect PASS**

Run: `python -m pytest tests/test_kilo_row.py -q`
Expected: PASS. If `col % tabStop == 0` mis-parses, wrap as `(col % tabStop) == 0`.

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Row.thx tests/test_kilo_row.py
git commit -m "kilo: Stage 3 Row module (tab-expanded render + cxToRx + window)"
```

---

### Task 4: `FileIO` module

**Files:**
- Create: `examples/kilo/FileIO.thx`, `examples/kilo/sample.txt`
- Test: `tests/test_kilo_fileio.py`

**Interfaces:**
- Produces: `open(path!: String) -> Trial<Array<Row>, String>`. Import as `import fileio from "./FileIO"`.

- [ ] **Step 1: Create the demo file** `examples/kilo/sample.txt`

```
Kilo, ported to Thoxa.
	an indented line (leading tab)
The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.
line four
line five
```

(The second line begins with a real tab; the third is long enough to exercise horizontal scroll.)

- [ ] **Step 2: Write the failing tests** `tests/test_kilo_fileio.py`

```python
"""FileIO — kilo's editorOpen: read a file, split into Rows on \\n (strip trailing \\r)."""
from __future__ import annotations

import pathlib
from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = 'import fileio from "./FileIO"\nfn main() -> Int64 {\n' + body + "\n0\n}\n"
    buf: list[str] = []
    run(src, str(KILO / "fileio_probe.thx"), out=buf.append)
    return "".join(buf)


def test_fileio_type_checks_clean():
    p = KILO / "FileIO.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_reads_and_splits(tmp_path):
    f = tmp_path / "t.txt"
    f.write_text("alpha\nbeta\r\ngamma", encoding="utf-8")  # CRLF on line 2, no final \n
    out = _probe(
        f'match fileio.open("{f}") {{\n'
        '  Pass(rows) { println("n={rows.count} a={rows.first().charCount()}") }\n'
        '  Fail(e) { println("FAIL {e}") }\n'
        '}'
    )
    # 3 rows; "beta\r" -> "beta" (4); first line "alpha" (5)
    assert out == "n=3 a=5\n"


def test_missing_file_fails():
    out = _probe(
        'match fileio.open("/no/such/file/here.txt") {\n'
        '  Pass(rows) { println("PASS {rows.count}") }\n'
        '  Fail(e) { println("failed") }\n'
        '}'
    )
    assert out == "failed\n"
```

- [ ] **Step 3: Run — expect FAIL**

Run: `python -m pytest tests/test_kilo_fileio.py -q`
Expected: FAIL (no `FileIO.thx`).

- [ ] **Step 4: Create `examples/kilo/FileIO.thx`**

```thoxa
## FileIO — Stage 3 of the kilo port: open a file and split it into `Row`s (kilo's
## editorOpen). Mirrors examples/http/Io.thx — private `sys`-prefixed libc externs
## behind a `Trial`-returning API. Reads the whole file in one pass, splits on `\n`,
## strips a trailing `\r`.

import { Row } from "./Row"
import { errno, strerror } from "ffi"

extern fn sysOpen(path!: String, flags!: Int32) -> Int32 from "c" "open"
extern fn sysRead(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c" "read"
extern fn sysLseek(fd!: Int32, offset!: Int64, whence!: Int32) -> Int64 from "c" "lseek"
extern fn sysClose(fd!: Int32) -> Int32 from "c" "close"

let oRdOnly = 0
let seekSet = 0
let seekEnd = 2

fn fileErr(what!: String) -> String {
    "{what}: {strerror(errno())}"
}

## Split file bytes into rows on `\n`, dropping a trailing `\r` (CRLF endings).
fn splitRows(data!: Bytes) -> Array<Row> {
    mut rows: Array<Row> = []
    let n = data.count
    mut start = 0
    mut i = 0
    loop {
        if i >= n { break }
        if data.get(i) == b'\n' {
            mut end = i
            if end > start {
                if data.get(end - 1) == b'\r' { end = end - 1 }
            }
            rows = rows.append(Row(data.slice(start, end)))
            start = i + 1
        }
        i = i + 1
    }
    ## a final line with no trailing newline
    if start < n {
        rows = rows.append(Row(data.slice(start, n)))
    }
    rows
}

## Open `path` read-only and load it as rows. Fail (with the libc error) on any
## open/seek/read error — the caller decides what a failure means (Stage 3's Main
## treats it as an empty new buffer).
export fn open(path!: String) -> Trial<Array<Row>, String> {
    let fd = sysOpen(path, oRdOnly)
    if fd < 0 {
        return Fail(fileErr("open {path}"))
    }
    let size = sysLseek(fd, 0, seekEnd)
    sysLseek(fd, 0, seekSet)          # rewind — otherwise read starts at EOF
    if size < 0 {
        sysClose(fd)
        return Fail(fileErr("seek {path}"))
    }
    if size == 0 {
        sysClose(fd)
        return Pass([])
    }
    let buf = Bytes(size)
    let got = sysRead(fd, buf, size)
    sysClose(fd)
    if got < 0 {
        return Fail(fileErr("read {path}"))
    }
    Pass(splitRows(buf))
}
```

(A single `read` fills the buffer for a regular file, mirroring `Io.read`; a short-read loop is a later hardening — note it, don't block on it for the sample.)

- [ ] **Step 5: Run — expect PASS**

Run: `python -m pytest tests/test_kilo_fileio.py -q`
Expected: PASS. (If `Bytes(size)` rejects an `Int64` from `lseek`, coerce/annotate `size` per the `Io.read` pattern.)

- [ ] **Step 6: Commit**

```bash
git add examples/kilo/FileIO.thx examples/kilo/sample.txt tests/test_kilo_fileio.py
git commit -m "kilo: Stage 3 FileIO module (open -> whole-file read -> split into Rows)"
```

---

### Task 5: `State` — file-coordinate cursor + scroll

**Files:**
- Modify: `examples/kilo/State.thx`
- Test: `tests/test_kilo_state.py`

**Interfaces:**
- Produces: `setRows(rows!: Array<Row>, name!: String)`, `numRows() -> Int64`, `row(index!: Int64) -> Row`, `rowOff()`/`colOff()`/`renderX() -> Int64`, `scroll()`, and movers `moveLeft/moveRight/moveUp/moveDown/moveHome/moveEnd/pageUp/pageDown`. `cursorX()`/`cursorY()` now return `Int64`. Keeps `setScreenSize`/`screenRows()`/`screenCols()` (Int32).
- Consumes: `Row` (`import { Row } from "./Row"`), `Row.charCount()`, `Row.cxToRx()`.

Full algorithm: design §6. **Every `row(cy)` access is guarded by `cy < numRows()`.**

- [ ] **Step 1: Write the failing tests** `tests/test_kilo_state.py`

```python
"""State — file-coordinate cursor: wrap, snap, scroll, and empty-file safety."""
from __future__ import annotations

import pathlib
from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = (
        'import state from "./State"\n'
        'import { Row } from "./Row"\n'
        'fn main() -> Int64 {\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "state_probe.thx"), out=buf.append)
    return "".join(buf)


def test_state_type_checks_clean():
    p = KILO / "State.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


LOAD2 = (
    'state.setScreenSize(24, 80)\n'
    'mut rows: Array<Row> = []\n'
    'rows = rows.append(Row(Bytes.fromString("hello")))\n'
    'rows = rows.append(Row(Bytes.fromString("hi")))\n'
    'state.setRows(rows, "t.txt")\n'
)


def test_move_end_and_snap_down():
    # cursor at end of line 0 ("hello", len 5), move down to shorter line 1 ("hi", 2):
    # cx snaps from 5 to 2.
    out = _probe(
        LOAD2 +
        'state.moveEnd()\n'
        'state.moveDown()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=2 cy=1\n"


def test_move_right_wraps_line():
    # at end of line 0, moveRight goes to (line1, col0)
    out = _probe(
        LOAD2 +
        'state.moveEnd()\n'
        'state.moveRight()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=0 cy=1\n"


def test_empty_file_moves_are_safe():
    # no rows: every mover is a guarded no-op, nothing traps
    out = _probe(
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "empty")\n'
        'state.moveEnd()\nstate.moveRight()\nstate.moveDown()\nstate.moveUp()\n'
        'println("cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "cx=0 cy=0\n"
```

- [ ] **Step 2: Run — expect FAIL** (new API absent)

Run: `python -m pytest tests/test_kilo_state.py -q`
Expected: FAIL.

- [ ] **Step 3: Rewrite `examples/kilo/State.thx`** per design §6. Key points: rename the file array to `textRows` (the screen-size fields stay `rows`/`cols`); retype `cx`/`cy`/`rx`/`rowoff`/`coloff` to `Int64`; keep `setScreenSize`/`screenRows()`/`screenCols()`; **keep the Stage-2 `moveCursor` and `Int64`-retyped `cursorX()`/`cursorY()` so Screen/Main still compile until Task 7**; add the new exports. Skeleton (fill bodies from §6):

```thoxa
import { Row } from "./Row"

mut cx = 0            # cursor byte-column into the current row's chars (Int64)
mut cy = 0            # cursor file-row (Int64), may equal numRows()
mut rx = 0            # cursor render-column (Int64)
mut rowoff = 0
mut coloff = 0
mut rows = 0          # screen height in cells (Int32) — unchanged from Stage 2
mut cols = 0          # screen width in cells (Int32)
mut textRows: Array<Row> = []
mut filename = ""

export fn setScreenSize(r!: Int32, c!: Int32) -> Void { rows = r  cols = c  Void }
export fn screenRows() -> Int32 { rows }
export fn screenCols() -> Int32 { cols }

export fn setRows(newRows!: Array<Row>, name!: String) -> Void {
    textRows = newRows
    filename = name
    Void
}
export fn numRows() -> Int64 { textRows.count }
export fn row(index!: Int64) -> Row { textRows[index] }
export fn cursorX() -> Int64 { cx }
export fn cursorY() -> Int64 { cy }
export fn renderX() -> Int64 { rx }
export fn rowOff() -> Int64 { rowoff }
export fn colOff() -> Int64 { coloff }

export fn scroll() -> Void {
    rx = if cy < textRows.count { row(cy).cxToRx(cx) } else { 0 }
    if rx < coloff { coloff = rx }
    if rx >= coloff + cols { coloff = rx - cols + 1 }
    if cy < rowoff { rowoff = cy }
    if cy >= rowoff + rows { rowoff = cy - rows + 1 }
    Void
}

## movers — see §6. Each guards row(cy) with `cy < numRows()`; moveUp/moveDown snap.
export fn moveLeft() -> Void { … }
export fn moveRight() -> Void { … }
export fn moveUp() -> Void { … }     # includes snap
export fn moveDown() -> Void { … }   # includes snap
export fn moveHome() -> Void { cx = 0  Void }
export fn moveEnd() -> Void { if cy < textRows.count { cx = row(cy).charCount() }  Void }
export fn pageUp() -> Void { … }
export fn pageDown() -> Void { … }

## Stage-2 compatibility shim — kept so Screen/Main still compile until Task 7.
export fn moveCursor(dx!: Int32, dy!: Int32) -> Void { … }   # old clamp-to-screen
```

(Copy the exact mover bodies from design §6: `moveLeft`/`moveRight`/`moveUp`/`moveDown` with the snap, `pageUp`/`pageDown` with the clamp. Note `rx >= coloff + cols` mixes Int64 `rx` with Int32 `cols` — widens implicitly; never assign an Int64 back into `rows`/`cols`.)

- [ ] **Step 4: Run — expect PASS**

Run: `python -m pytest tests/test_kilo_state.py -q`
Expected: PASS.

- [ ] **Step 5: Regression — Screen/Main still type-check** (shim intact)

Run: `python -m pytest tests/test_kilo_screen.py tests/test_kilo_keys.py -q`
Expected: PASS (Stage-2 tests still green — `moveCursor` shim keeps them valid).

- [ ] **Step 6: Commit**

```bash
git add examples/kilo/State.thx tests/test_kilo_state.py
git commit -m "kilo: Stage 3 State — file-coordinate cursor, scroll, guarded movers"
```

---

### Task 6: `Screen` — draw text rows

**Files:**
- Modify: `examples/kilo/Screen.thx`, `tests/test_kilo_screen.py`

**Interfaces:**
- Consumes: `state.scroll()`, `state.numRows()`, `state.row(i)`, `state.rowOff()`, `state.colOff()`, `state.renderX()`, `state.cursorX/Y()`, `state.screenRows/Cols()`; `Row.window(coloff, width)`.

Full algorithm: design §7.

- [ ] **Step 1: Update the Screen tests** `tests/test_kilo_screen.py`

Replace `test_cursor_position_reflects_state` (it used the removed `moveCursor`) and add row-drawing tests. Keep the empty-buffer welcome tests but note they now require `numRows() == 0` (the default). Add:

```python
def _run_with_rows(setup: str) -> str:
    src = (
        'import state from "./State"\n'
        'import screen from "./Screen"\n'
        'import { Row } from "./Row"\n'
        "fn main() -> Int64 {\n"
        f"{setup}"
        '    match screen.refresh() { Pass { Void } Fail(e) { println("FAIL {e}") } }\n'
        "    0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "screen_probe.thx"), out=buf.append)
    return "".join(buf)


def test_draws_file_rows_no_welcome():
    # 2 rows x 10 cols, one text line "abcdef" -> row0 shows the line, row1 a tilde,
    # NO welcome (numRows != 0).
    out = _run_with_rows(
        '    state.setScreenSize(2, 10)\n'
        '    mut rows: Array<Row> = []\n'
        '    rows = rows.append(Row(Bytes.fromString("abcdef")))\n'
        '    state.setRows(rows, "t")\n'
    )
    assert WELCOME not in out
    assert out.startswith("\x1b[?25l\x1b[H")
    assert "abcdef\x1b[K\r\n" in out   # row 0 = the file line
    assert "~\x1b[K" in out            # row 1 = past EOF -> tilde
    assert out.endswith("\x1b[?25h")


def test_horizontal_scroll_windows_the_row():
    # a line wider than the screen, cursor driven right past the edge -> coloff advances
    out = _run_with_rows(
        '    state.setScreenSize(1, 4)\n'
        '    mut rows: Array<Row> = []\n'
        '    rows = rows.append(Row(Bytes.fromString("0123456789")))\n'
        '    state.setRows(rows, "t")\n'
        '    mut k = 0\n'
        '    loop { if k >= 6 { break } state.moveRight() k = k + 1 }\n'
    )
    # cursor at cx=6 -> coloff makes a 4-wide window ending at col 6 -> "3456"
    assert "3456\x1b[K" in out
```

(Keep `test_frame_exact_bytes_small` / `test_welcome_falls_back...` renamed intent: the welcome now truncates via slice rather than falling back to a bare `~`. Update `test_welcome_falls_back_to_tilde_when_too_narrow`'s expectation to the truncated banner: for 10 cols the welcome row becomes `~` + `banner.slice(0,10)`? No — recompute: kilo centers only when it fits; when wider than cols it truncates from column 0. Match the new §7 code and assert the actual bytes.)

- [ ] **Step 2: Run — expect FAIL**

Run: `python -m pytest tests/test_kilo_screen.py -q`
Expected: FAIL (new draw path absent).

- [ ] **Step 3: Rewrite `Screen.refresh()`** per §7: call `state.scroll()` first; per screen row draw either the tilde/welcome (only when `numRows()==0`) or `state.row(filerow).window(state.colOff(), state.screenCols())`; welcome truncation via the if-expr slice; cursor at `\x1b[{(cy-rowOff)+1};{(rx-colOff)+1}H`.

- [ ] **Step 4: Run — expect PASS**

Run: `python -m pytest tests/test_kilo_screen.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Screen.thx tests/test_kilo_screen.py
git commit -m "kilo: Stage 3 Screen — draw scrolled text rows + welcome only when empty"
```

---

### Task 7: `Main` — load the file, drive the viewer

**Files:**
- Modify: `examples/kilo/Main.thx`, `tests/test_kilo_keys.py`

- [ ] **Step 1: Rewrite `Main.thx`** per design §8: `export fn main(arguments: Array<String>) -> Int64`; if `arguments.count > 1`, `match fileio.open(arguments[1]) { Pass(rows) { state.setRows(rows, arguments[1]) } Fail(e) { state.setRows([], arguments[1]) } }`; keep the raw-mode/refresh loop; dispatch each `Key` to the new movers with an **exhaustive** match (`Delete { Void }`, `Press(b) { if b == ctrlKey(b'q') { break } }`). Import `fileio from "./FileIO"`.

- [ ] **Step 2: Remove the dead Stage-2 shim from `State.thx`** (`moveCursor`) now that no one calls it, and re-run `python -m pytest tests/test_kilo_state.py -q` (expect PASS). If any test/probe still references it, update.

- [ ] **Step 3: Update the pty smoke** in `tests/test_kilo_keys.py`: launch `Main` with the sample file argument, `wait_for` the first line (`Kilo, ported to Thoxa.`) to render; send arrow-down enough to scroll and assert a later line appears; send a long-line + arrow-right and assert horizontal scroll; send Ctrl-Q and assert clean exit + terminal restore. (Mirror the existing pty harness in that file; pass the file path as an argument to the run command.)

- [ ] **Step 4: Run the kilo suite — expect PASS**

Run: `python -m pytest tests/test_kilo_keys.py tests/test_kilo_screen.py tests/test_kilo_state.py -q`
Expected: PASS.

- [ ] **Step 5: Manual smoke (optional but recommended)**

Run: `python -m thoxa run examples/kilo examples/kilo/sample.txt`
Expected: the file renders; arrows scroll; Ctrl-Q restores the terminal.

- [ ] **Step 6: Commit**

```bash
git add examples/kilo/Main.thx examples/kilo/State.thx tests/test_kilo_keys.py
git commit -m "kilo: Stage 3 Main — open a file argument and drive the read-only viewer"
```

---

### Task 8: Docs — SPEC, CHANGELOG, roadmap

**Files:**
- Modify: `spec/SPEC.md`, `CHANGELOG.md`, `docs/specs/2026-07-16-kilo-port-roadmap.md`

- [ ] **Step 1:** `spec/SPEC.md` — document `Bytes.slice(start, end) -> Bytes` (collections/`Bytes` section) and the `main(arguments: Array<String>)` entrypoint form.
- [ ] **Step 2:** `CHANGELOG.md` — a `### Stage 3 — Read-only viewer (Bytes.slice + arguments)` entry under `[Unreleased]`, listing green test count.
- [ ] **Step 3:** roadmap `2026-07-16-kilo-port-roadmap.md` — mark Stage 3 DONE (fork #5 resolved: bytes chosen), update the status line and the build-order block; note String slicing deferred.
- [ ] **Step 4: Full suite green**

Run: `python -m pytest -q`
Expected: PASS (whole suite).

- [ ] **Step 5: Commit**

```bash
git add spec/SPEC.md CHANGELOG.md docs/specs/2026-07-16-kilo-port-roadmap.md
git commit -m "kilo: Stage 3 done — SPEC/CHANGELOG/roadmap (fork #5 resolved: bytes)"
```

---

## Self-Review

- **Spec coverage:** §2 slice → Task 1; §3 arguments → Task 2; §4 Row → Task 3; §5 FileIO → Task 4; §6 State → Task 5; §7 Screen → Task 6; §8 Main → Task 7; §9 verification → each task's tests + Task 8; §10 deferred → untouched. All covered.
- **Type consistency:** `Row(chars!: Bytes)` with `charCount`/`renderCount`/`cxToRx`/`window`; `state.row(index!: Int64) -> Row`; `fileio.open(path!: String) -> Trial<Array<Row>, String>`; `main(arguments: Array<String>)`. Consumers match producers.
- **Known approximations (flagged, not blockers):** `rows = rows.append(...)` is O(n²) for large files (fine for the sample; a row `Buffer` is a later primitive); single `read` per file (short-read loop deferred). Both noted at their tasks.
