# Kilo Stage 4 — Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the kilo read-only viewer into an editor — type a character, split a line (Enter), join/erase (Backspace, Delete) — by adding a functional value splice (`insertAt`/`removeAt`) to `Bytes` and `Array<T>` (resolves roadmap fork #4), then building kilo's editing algorithms on it.

**Architecture:** The one language increment is a **copying, strict-bounds, functional** splice: `Bytes.insertAt/removeAt` return a fresh `Bytes`; `Array<T>.insertAt/removeAt` return a fresh array (like `.append`/`.dropLast`). Rows stay fixed-length `Bytes`; an edit builds a new `Row` (so `render` re-derives for free). Editing folds into `State` (owns `cx`/`cy`/`textRows`); `Row` gains per-line edit methods; `Main` fills the Stage-3 no-op key cases. Stops before disk save (Stage 5).

**Tech Stack:** Python 3 tree-walking interpreter (`thoxa/{ffi,interp,checker}.py`), Thoxa modules under `examples/kilo/`, pytest, `spec/conformance/*.thx` executable spec.

**Design doc:** `docs/specs/2026-07-17-kilo-stage4-editing-design.md`.

## Global Constraints

- **Commit to `master`** directly (this project's convention); one commit per task.
- **Done = conformance green.** A feature is done only when its `spec/conformance/*.thx` test passes; `tests/test_conformance.py` auto-discovers `*.thx` and compares stdout to the sibling `.out`. New conformance programs are ALSO run through `tests/test_checker.py::test_conformance_programs_type_check_clean` and `tests/test_lsp.py::test_conformance_programs_have_no_diagnostics`, so they must type-check clean with no diagnostics.
- **Zero runtime deps.** Only the stdlib + the existing `ffi.py` ctypes bridge.
- **Editing tools:** use Serena (`replace_symbol_body`, `insert_after_symbol`) for the Python files (`thoxa/*.py`); use the built-in `Edit`/`Write` for `.thx`, `.md`, `.out` (not parsed by the Python LSP).
- **No `&&`/`||` in Thoxa** — verified they do not parse. Boolean conjunction is expressed with nested `if`.
- **The checker has no method tables for `Bytes`/`Buffer`/`Array`** — it types their methods `Unknown` (permissive), and `Unknown` unifies with anything. So the splice needs **no checker change**; all runtime work is in `ffi.py` + `interp.py`. Verified: assigning an `Unknown`-typed method result to a typed module-`mut` array type-checks and runs.
- **Test runner:** `.venv/bin/pytest`. **CLI:** `.venv/bin/thoxa {check,run} <file>`.
- **Validated language idioms this plan relies on** (each run against the real interpreter): module-`mut` index-assign (`textRows[cy] = newRow`), early `return Void` guards, cross-instance *public method* calls (a cross-instance private *field* read traps), `b'\r'` = 13, `Bool` interpolates as `true`/`false`.

---

### Task 1: `Bytes.insertAt` / `Bytes.removeAt` (byte splice — op a)

**Files:**
- Modify: `thoxa/ffi.py` (add two methods to class `Bytes`, after `Bytes.slice`)
- Modify: `thoxa/interp.py` (`Interpreter._bytes_method`; the `Bytes` branch of `Interpreter._member`)
- Test: `tests/test_splice.py` (create)

**Interfaces:**
- Produces (host, `ffi.Bytes`): `insert_at(self, at: int, b: int) -> Bytes`, `remove_at(self, at: int) -> Bytes` — copying, strict bounds, raise `IndexError` on a bad index.
- Produces (Thoxa surface): `Bytes.insertAt(at, byte) -> Bytes`, `Bytes.removeAt(at) -> Bytes`.

- [ ] **Step 1: Write the failing test**

Create `tests/test_splice.py`:

```python
"""Value splice — Bytes.insertAt/removeAt and Array.insertAt/removeAt (fork #4).
Copying (receiver/source unchanged) and strict-bounds (trap on a bad index)."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<splice>", out=buf.append)
    return "".join(buf), code


def test_bytes_insert_and_remove():
    out, _ = _run(
        'let b = Bytes.fromString("helo")\n'
        "let b2 = b.insertAt(2, b'l')\n"      # h e [l] l o
        'let b3 = b.removeAt(1)\n'            # h l o
        'println("{b2.toString()} {b3.toString()} src={b.toString()}")'
    )
    assert out == "hello hlo src=helo\n"


def test_bytes_insert_at_end_appends():
    out, _ = _run(
        'let b = Bytes.fromString("ab")\n'
        "let b2 = b.insertAt(2, b'c')\n"      # at == count -> append
        'println(b2.toString())'
    )
    assert out == "abc\n"


def test_bytes_insert_masks_to_byte():
    out, _ = _run(
        'let b = Bytes.fromString("")\n'
        'let b2 = b.insertAt(0, 300)\n'       # 300 & 0xFF == 44 (",")
        'println("{b2.count} {b2.get(0)}")'
    )
    assert out == "1 44\n"


@pytest.mark.parametrize("call", [
    "b.insertAt(0 - 1, 65)",   # negative
    "b.insertAt(5, 65)",       # past insert bound (count 4)
    "b.removeAt(4)",           # == count
    "b.removeAt(0 - 1)",       # negative
])
def test_bytes_splice_out_of_range_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let b = Bytes.fromString("abcd")\nprintln("{{{call}.count}}")')
    assert "out of range" in exc.value.render()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_splice.py -q`
Expected: FAIL — `Bytes has no method 'insertAt'` (ThoxaError).

- [ ] **Step 3a: Implement the host methods in `ffi.py`**

Using Serena `insert_after_symbol` with `name_path="Bytes/slice"`, `relative_path="thoxa/ffi.py"`, insert:

```python

    def insert_at(self, at: int, b: int) -> "Bytes":
        """A fresh Bytes with byte `b` inserted at index `at` (0 <= at <= count).

        Copying, never a view — like `slice`, the result is independently owned, so
        it can never dangle. `b` is masked to a byte. Strict bounds, else IndexError.
        """
        at = int(at)
        if not (0 <= at <= self._len):
            raise IndexError(at)
        data = self._buf.raw[: self._len]
        return Bytes.from_bytes(data[:at] + bytes([int(b) & 0xFF]) + data[at:])

    def remove_at(self, at: int) -> "Bytes":
        """A fresh Bytes with the byte at index `at` removed (0 <= at < count)."""
        at = int(at)
        if not (0 <= at < self._len):
            raise IndexError(at)
        data = self._buf.raw[: self._len]
        return Bytes.from_bytes(data[:at] + data[at + 1 :])
```

- [ ] **Step 3b: Route the methods in `interp.py` `_bytes_method`**

Using Serena `replace_symbol_body` on `Interpreter/_bytes_method` (`thoxa/interp.py`), replace the final line `self.error(f"Bytes has no method '{name}'", span)` by inserting these two branches immediately before it (keep everything above unchanged):

```python
        if name == "insertAt":
            if len(values) != 2 or not all(
                isinstance(v, int) and not isinstance(v, bool) for v in values
            ):
                self.error("Bytes.insertAt(at, byte) expects two integers", span)
            try:
                return buf.insert_at(values[0], values[1])
            except IndexError:
                self.error(
                    f"Bytes.insertAt index {values[0]} out of range (count {buf.count})",
                    span,
                )
        if name == "removeAt":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Bytes.removeAt(at) expects an integer index", span)
            try:
                return buf.remove_at(values[0])
            except IndexError:
                self.error(
                    f"Bytes.removeAt index {values[0]} out of range (count {buf.count})",
                    span,
                )
        self.error(f"Bytes has no method '{name}'", span)
```

- [ ] **Step 3c: Whitelist the member names in `interp.py` `_member`**

In `Interpreter._member`, the `Bytes` branch reads `if name in ("get", "set", "toString", "slice"):`. Change it to include the two new names (use Serena `replace_content` / built-in Edit on the single line):

```python
            if name in ("get", "set", "toString", "slice", "insertAt", "removeAt"):
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/pytest tests/test_splice.py -q`
Expected: PASS (the Array tests are added in Task 2; if running the whole file now, the Bytes tests pass and Array tests are not yet present).

- [ ] **Step 5: Commit**

```bash
git add thoxa/ffi.py thoxa/interp.py tests/test_splice.py
git commit -m "kilo/lang: Bytes.insertAt/removeAt — copying value splice (fork #4)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 2: `Array<T>.insertAt` / `Array<T>.removeAt` (row splice — op d)

**Files:**
- Modify: `thoxa/interp.py` (`ARRAY_METHODS` constant; `Interpreter._array_method`)
- Test: `tests/test_splice.py` (append array cases)

**Interfaces:**
- Produces (Thoxa surface): `Array<T>.insertAt(at, item) -> Array<T>`, `Array<T>.removeAt(at) -> Array<T>` — functional (new list), strict bounds.
- Consumes: nothing from Task 1 (independent), but shares the `tests/test_splice.py` file.

- [ ] **Step 1: Write the failing test**

Append to `tests/test_splice.py`:

```python
def test_array_insert_and_remove():
    out, _ = _run(
        'let xs = [10, 20, 30]\n'
        'let ys = xs.insertAt(1, 99)\n'       # [10, 99, 20, 30]
        'let zs = xs.removeAt(0)\n'           # [20, 30]
        'println("ys={ys.count},{ys[1]} zs={zs.count},{zs[0]} src={xs.count},{xs[1]}")'
    )
    assert out == "ys=4,99 zs=2,20 src=3,20\n"


def test_array_insert_at_end_appends():
    out, _ = _run(
        'let xs = [1, 2]\n'
        'let ys = xs.insertAt(2, 3)\n'        # at == count -> append
        'println("{ys.count} {ys[2]}")'
    )
    assert out == "3 3\n"


@pytest.mark.parametrize("call", [
    "xs.insertAt(0 - 1, 9)",   # negative
    "xs.insertAt(4, 9)",       # past insert bound (count 3)
    "xs.removeAt(3)",          # == count
    "xs.removeAt(0 - 1)",      # negative
])
def test_array_splice_out_of_range_traps(call):
    with pytest.raises(ThoxaError) as exc:
        _run(f'let xs = [1, 2, 3]\nprintln("{{{call}.count}}")')
    assert "out of range" in exc.value.render()
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_splice.py::test_array_insert_and_remove -q`
Expected: FAIL — `arrays have no method 'insertAt'`.

- [ ] **Step 3a: Add the names to `ARRAY_METHODS`**

In `thoxa/interp.py` line 41, change (built-in Edit on the one line):

```python
ARRAY_METHODS = {"append", "dropLast", "first", "last", "reversed", "contains", "insertAt", "removeAt"}
```

(The `_member` array branch already dispatches any name in `ARRAY_METHODS` to a `BoundBuiltin`, so no `_member` change is needed.)

- [ ] **Step 3b: Add the branches to `_array_method`**

Using Serena `replace_symbol_body` on `Interpreter/_array_method` (`thoxa/interp.py`):

```python
    def _array_method(self, recv: list, name: str, values, span: Span):
        if name == "append":
            return recv + [values[0]]
        if name == "dropLast":
            return recv[:-1]
        if name == "first":
            return recv[0] if recv else VOID
        if name == "last":
            return recv[-1] if recv else VOID
        if name == "reversed":
            return list(reversed(recv))
        if name == "contains":
            return values[0] in recv
        if name == "insertAt":
            if len(values) != 2 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Array.insertAt(at, item) expects an integer index", span)
            at = values[0]
            if not (0 <= at <= len(recv)):
                self.error(f"Array.insertAt index {at} out of range (count {len(recv)})", span)
            return recv[:at] + [values[1]] + recv[at:]
        if name == "removeAt":
            if len(values) != 1 or not isinstance(values[0], int) or isinstance(values[0], bool):
                self.error("Array.removeAt(at) expects an integer index", span)
            at = values[0]
            if not (0 <= at < len(recv)):
                self.error(f"Array.removeAt index {at} out of range (count {len(recv)})", span)
            return recv[:at] + recv[at + 1 :]
        self.error(f"arrays have no method '{name}'", span)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/pytest tests/test_splice.py -q`
Expected: PASS (all Bytes + Array cases).

- [ ] **Step 5: Commit**

```bash
git add thoxa/interp.py tests/test_splice.py
git commit -m "kilo/lang: Array insertAt/removeAt — functional value splice (fork #4)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 3: Conformance test + SPEC (the fork-#4 "done" gate)

**Files:**
- Create: `spec/conformance/splice.thx`, `spec/conformance/splice.out`
- Modify: `spec/SPEC.md` (§9 and §9.1)

**Interfaces:**
- Consumes: `Bytes.insertAt/removeAt` (Task 1), `Array.insertAt/removeAt` (Task 2).

- [ ] **Step 1: Write the conformance program (the test)**

Create `spec/conformance/splice.thx`:

```thoxa
## insertAt / removeAt — the Stage 4 value splice (fork #4). Copying and strict-
## bounds on both Bytes (a byte in a line) and Array (a row in the file). Every value
## is asserted through .count/.get/.toString/index so stdout carries no control bytes.

export fn main() -> Int64 {
    ## Bytes: insert a byte mid-line, remove one, and at the end (append).
    let b = Bytes.fromString("helo")
    let ins = b.insertAt(2, b'l')          # h e [l] l o
    let del = b.removeAt(1)                # h l o
    let app = b.insertAt(b.count, b'!')    # at == count -> append
    println("bytes ins={ins.toString()} del={del.toString()} app={app.toString()}")

    ## Bytes splice is copying: the source is untouched by the ops above.
    println("bytes src count={b.count} text={b.toString()}")

    ## Array: insert an element mid-sequence, remove one, and at the end.
    let xs = [10, 20, 30]
    let ays = xs.insertAt(1, 99)           # [10, 99, 20, 30]
    let azs = xs.removeAt(0)               # [20, 30]
    println("array ins count={ays.count} at1={ays[1]}")
    println("array del count={azs.count} at0={azs[0]}")

    ## Array splice is copying: the source array is untouched.
    println("array src count={xs.count} at1={xs[1]}")
    0
}
```

- [ ] **Step 2: Run it and capture the output as the fixture**

Run: `.venv/bin/thoxa run spec/conformance/splice.thx`
Expected output (write it verbatim into `spec/conformance/splice.out`):

```
bytes ins=hello del=hlo app=helo!
bytes src count=4 text=helo
array ins count=4 at1=99
array del count=2 at0=20
array src count=3 at1=20
```

Create `spec/conformance/splice.out` with exactly those 5 lines (trailing newline).

- [ ] **Step 3: Update SPEC.md**

In `spec/SPEC.md`, §9 (line ~353-354), replace the "Additional array methods" sentence:

```markdown
Indexing out of bounds is a runtime error (a trap), never silent. Additional array methods
(`append`, `dropLast`, `insertAt(at, item)`, `removeAt(at)`, …) are provided by the
prelude/builtins. `insertAt`/`removeAt` are **functional** — they return a new array (a copy)
with the element inserted/removed, leaving the source unchanged; strict bounds
(`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove, else a trap).
```

In §9.1 (line ~356-360), extend the `Bytes` sentence to list the splice:

```markdown
### 9.1 `Bytes` and `Buffer`
`Bytes` is a fixed byte buffer (the FFI boundary): `Bytes.fromString(s)`, `.count`, `.get(i)`,
`.set(i, b)`, `.slice(start, end)` — a **fresh copy** of the half-open range `[start, end)`
with strict bounds (`0 ≤ start ≤ end ≤ count`, else a trap) — and `.insertAt(at, byte)` /
`.removeAt(at)`, each returning a **fresh copy** with a byte inserted/removed (strict bounds
`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove; `byte` masked to 0–255). A growable `Buffer`
builds one up (`.append`, `.appendBytes`, `.reserve`, `.freeze()`); see the FFI / growable-buffer
design docs.
```

- [ ] **Step 4: Run the conformance + checker + LSP gates**

Run: `.venv/bin/pytest tests/test_conformance.py -q -k splice tests/test_checker.py -q tests/test_lsp.py -q`
Expected: PASS — `splice.thx` output matches `splice.out`, type-checks clean, no LSP diagnostics.

- [ ] **Step 5: Commit**

```bash
git add spec/conformance/splice.thx spec/conformance/splice.out spec/SPEC.md
git commit -m "conformance+spec: splice.thx pins Bytes/Array insertAt/removeAt (fork #4)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 4: `Row.thx` — per-line editing methods

**Files:**
- Modify: `examples/kilo/Row.thx` (add methods inside `export type Row`, after `window`)
- Test: `tests/test_kilo_row.py` (append editing tests)

**Interfaces:**
- Consumes: `Bytes.insertAt`/`removeAt` (Task 1), existing `Bytes.slice`, `Buffer.appendBytes`/`freeze`.
- Produces: `Row.insertChar(at, byte) -> Row`, `deleteChar(at) -> Row`, `take(count) -> Row`, `drop(count) -> Row`, `concat(other: Row) -> Row`, `bytes() -> Bytes`, `text() -> String`.

- [ ] **Step 1: Write the failing test**

Append to `tests/test_kilo_row.py` (mirrors the existing `_probe` there; if that file has no `_probe`, use this self-contained one):

```python
def _row_probe(body: str) -> str:
    src = (
        'import { Row } from "./Row"\n'
        'fn main() -> Int64 {\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "row_probe.thx"), out=buf.append)
    return "".join(buf)


def test_row_insert_and_delete_char():
    out = _row_probe(
        'let r = Row(Bytes.fromString("helo"))\n'
        "let a = r.insertChar(2, b'l')\n"      # "hello"
        'let b = r.deleteChar(0)\n'            # "elo"
        'println("{a.text()} {b.text()} src={r.text()}")'
    )
    assert out == "hello elo src=helo\n"


def test_row_take_drop_concat():
    out = _row_probe(
        'let r = Row(Bytes.fromString("hello"))\n'
        'let head = r.take(2)\n'               # "he"
        'let tail = r.drop(2)\n'               # "llo"
        'let joined = head.concat(tail)\n'     # "hello"
        'println("{head.text()} {tail.text()} {joined.text()}")'
    )
    assert out == "he llo hello\n"


def test_row_insert_rebuilds_render():
    # inserting a tab must re-expand render (proves render is not stale from the ctor)
    out = _row_probe(
        'let r = Row(Bytes.fromString("ab"))\n'
        "let e = r.insertChar(1, b'\\t')\n"    # chars "a\tb" -> render "a"+7sp+"b" = 9
        'println("{e.renderCount()}")'
    )
    assert out == "9\n"
```

Ensure the file's imports include `from thoxa.interp import run` and the `KILO` path constant (copy the header from `tests/test_kilo_state.py` if absent).

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_kilo_row.py -q -k "insert or take or rebuild"`
Expected: FAIL — `'Row' has no member 'insertChar'` (or a check failure).

- [ ] **Step 3: Implement the Row methods**

In `examples/kilo/Row.thx`, add these methods inside `export type Row(chars!: Bytes)`, immediately after the `window` method's closing `}` (built-in Edit — `.thx` is not Serena-parsed):

```thoxa

    ## -- editing (Stage 4) -------------------------------------------------
    ## Each returns a NEW Row, so the constructor rebuilds `render` (kilo's
    ## editorUpdateRow); `chars` stays immutable.

    ## Insert `byte` at char-index `at` (0 ≤ at ≤ charCount()).
    export fn insertChar(at!: Int64, byte!: Byte) -> Row {
        Row(chars.insertAt(at, byte))
    }

    ## Delete the byte at char-index `at` (0 ≤ at < charCount()).
    export fn deleteChar(at!: Int64) -> Row {
        Row(chars.removeAt(at))
    }

    ## Prefix row chars[0, count) — the part left of a split at `count`.
    export fn take(count!: Int64) -> Row {
        Row(chars.slice(0, count))
    }

    ## Suffix row chars[count, charCount()) — the part right of a split at `count`.
    export fn drop(count!: Int64) -> Row {
        Row(chars.slice(count, chars.count))
    }

    ## Join (kilo editorRowAppendString): a new Row with `other`'s bytes appended.
    ## Reads `other.bytes()` (a public method) — a private field of another instance
    ## is not readable across instances.
    export fn concat(other!: Row) -> Row {
        mut buf = Buffer()
        buf.appendBytes(chars)
        buf.appendBytes(other.bytes())
        Row(buf.freeze())
    }

    ## A fresh copy of the raw line bytes (public read for `concat` and callers).
    export fn bytes() -> Bytes {
        chars.slice(0, chars.count)
    }

    ## The line as a String (tests + the Stage-5 status bar).
    export fn text() -> String {
        chars.toString()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/thoxa check examples/kilo/Row.thx && .venv/bin/pytest tests/test_kilo_row.py -q`
Expected: `Row.thx: ok` and PASS.

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Row.thx tests/test_kilo_row.py
git commit -m "kilo: Stage 4 Row — per-line edit methods (insert/delete/take/drop/concat)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 5: `State.thx` — editing entry points + dirty flag

**Files:**
- Modify: `examples/kilo/State.thx` (add `mut dirty`; `isDirty`; `emptyRow`; `insertChar`; `insertNewline`; `deleteChar`; `deleteForward`)
- Test: `tests/test_kilo_edit.py` (create)

**Interfaces:**
- Consumes: `Row` editing methods (Task 4), `Array.insertAt/removeAt` (Task 2), existing `numRows`/`row`/`moveRight`.
- Produces: `state.insertChar(byte)`, `state.insertNewline()`, `state.deleteChar()`, `state.deleteForward()`, `state.isDirty() -> Bool`.

- [ ] **Step 1: Write the failing test**

Create `tests/test_kilo_edit.py`:

```python
"""State editing (kilo editorInsertChar/InsertNewline/DelChar) — Stage 4.
Deterministic: drive the exported ops and assert row text, cursor, and dirty."""
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
        'fn main() -> Int64 {\n'
        'state.setScreenSize(24, 80)\n'
        'state.setRows([], "t")\n' + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "edit_probe.thx"), out=buf.append)
    return "".join(buf)


def test_state_type_checks_clean():
    p = KILO / "State.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_insert_char_into_empty_buffer():
    # cy starts at 0 == numRows() (phantom line): the first insert materializes a row
    out = _probe(
        "state.insertChar(b'h')\n"
        "state.insertChar(b'i')\n"
        'println("n={state.numRows()} r0={state.row(0).text()} '
        'cx={state.cursorX()} cy={state.cursorY()} d={state.isDirty()}")'
    )
    assert out == "n=1 r0=hi cx=2 cy=0 d=true\n"


def test_insert_newline_splits_line():
    out = _probe(
        "state.insertChar(b'a')\n"
        "state.insertChar(b'b')\n"       # row0 = "ab", cx=2
        'state.moveHome()\n'             # cx=0
        'state.moveRight()\n'            # cx=1 (between a and b)
        'state.insertNewline()\n'        # split: row0="a", row1="b", cy=1, cx=0
        'println("n={state.numRows()} r0={state.row(0).text()} r1={state.row(1).text()} '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=2 r0=a r1=b cx=0 cy=1\n"


def test_newline_at_column_zero_inserts_blank_above():
    out = _probe(
        "state.insertChar(b'x')\n"       # row0 = "x", cx=1
        'state.moveHome()\n'             # cx=0
        'state.insertNewline()\n'        # blank line above; cursor moves to old text (row1)
        'println("n={state.numRows()} r0=[{state.row(0).text()}] r1=[{state.row(1).text()}] '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=2 r0=[] r1=[x] cx=0 cy=1\n"


def test_backspace_within_line():
    out = _probe(
        "state.insertChar(b'a')\nstate.insertChar(b'b')\nstate.insertChar(b'c')\n"  # "abc", cx=3
        'state.deleteChar()\n'           # remove 'c': "ab", cx=2
        'println("r0={state.row(0).text()} cx={state.cursorX()}")'
    )
    assert out == "r0=ab cx=2\n"


def test_backspace_at_column_zero_joins_previous():
    out = _probe(
        "state.insertChar(b'a')\nstate.insertChar(b'b')\n"   # row0 "ab"
        'state.insertNewline()\n'                            # row0 "ab", row1 "", cy=1, cx=0
        "state.insertChar(b'c')\nstate.insertChar(b'd')\n"   # row1 "cd", cx=2
        'state.moveHome()\n'                                 # cx=0 on row1
        'state.deleteChar()\n'                               # join: row0 "abcd", cy=0, cx=2
        'println("n={state.numRows()} r0={state.row(0).text()} '
        'cx={state.cursorX()} cy={state.cursorY()}")'
    )
    assert out == "n=1 r0=abcd cx=2 cy=0\n"


def test_delete_forward_at_line_end_joins_next():
    out = _probe(
        "state.insertChar(b'a')\n"       # row0 "a"
        'state.insertNewline()\n'        # row0 "a", row1 "", cy=1
        "state.insertChar(b'b')\n"       # row1 "b"
        'state.moveUp()\nstate.moveEnd()\n'  # cursor at end of row0 ("a"), cx=1, cy=0
        'state.deleteForward()\n'        # forward-delete joins row1 onto row0 -> "ab"
        'println("n={state.numRows()} r0={state.row(0).text()}")'
    )
    assert out == "n=1 r0=ab\n"


def test_start_of_file_backspace_is_noop():
    out = _probe(
        'state.deleteChar()\n'           # empty buffer, cy==numRows(): no-op, no trap
        'println("n={state.numRows()} d={state.isDirty()}")'
    )
    assert out == "n=0 d=false\n"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_kilo_edit.py -q`
Expected: FAIL — `State` has no exported `insertChar` (check or member error).

- [ ] **Step 3a: Add the `dirty` field**

In `examples/kilo/State.thx`, after the line `mut filename = ""` (line 19), add:

```thoxa
mut dirty = false              # unsaved changes since load (Stage 5 quit-confirm)
```

- [ ] **Step 3b: Add the editing section**

Append at the end of `examples/kilo/State.thx` (after `pageDown`'s closing `}`):

```thoxa

## -- editing (Stage 4): mutate the file through the Row primitives -----------
## Rows are spliced functionally (textRows = textRows.insertAt/removeAt); a within-
## line change swaps the element in place (textRows[cy] = newRow). Every edit sets
## `dirty`. `cy == numRows()` is the phantom line one past EOF.

export fn isDirty() -> Bool { dirty }

## A fresh empty line.
fn emptyRow() -> Row { Row(Bytes(0)) }

## kilo editorInsertChar: insert `byte` at the cursor, materializing the phantom
## EOF row first if the cursor sits one past the last line.
export fn insertChar(byte!: Byte) -> Void {
    if cy == numRows() {
        textRows = textRows.insertAt(numRows(), emptyRow())
    }
    textRows[cy] = row(cy).insertChar(cx, byte)
    cx = cx + 1
    dirty = true
    Void
}

## kilo editorInsertNewline: Enter — split the current line at the cursor.
export fn insertNewline() -> Void {
    if cx == 0 {
        textRows = textRows.insertAt(cy, emptyRow())
    } else {
        let line = row(cy)
        textRows = textRows.insertAt(cy + 1, line.drop(cx))
        textRows[cy] = line.take(cx)
    }
    cy = cy + 1
    cx = 0
    dirty = true
    Void
}

## kilo editorDelChar: Backspace — erase left of the cursor, or join with the
## previous line at column 0. (No `&&` in Thoxa, so the start-of-file guard nests.)
export fn deleteChar() -> Void {
    if cy == numRows() { return Void }
    if cx == 0 {
        if cy == 0 { return Void }
        let prev = row(cy - 1)
        cx = prev.charCount()
        textRows[cy - 1] = prev.concat(row(cy))
        textRows = textRows.removeAt(cy)
        cy = cy - 1
    } else {
        textRows[cy] = row(cy).deleteChar(cx - 1)
        cx = cx - 1
    }
    dirty = true
    Void
}

## kilo's Delete key = move right then Backspace (forward delete / line join).
export fn deleteForward() -> Void {
    moveRight()
    deleteChar()
    Void
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/thoxa check examples/kilo/State.thx && .venv/bin/pytest tests/test_kilo_edit.py -q`
Expected: `State.thx: ok` and PASS (all 8 tests).

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/State.thx tests/test_kilo_edit.py
git commit -m "kilo: Stage 4 State — editing entry points + dirty flag

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 6: `Main.thx` — key dispatch drives the editor

**Files:**
- Modify: `examples/kilo/Main.thx` (the `Press(b)` arm and the `Delete` arm)
- Test: `tests/test_kilo_keys.py` (add an editing pty smoke)

**Interfaces:**
- Consumes: `state.insertChar/insertNewline/deleteChar/deleteForward` (Task 5); existing `ctrlKey`, `Press`, `Delete`.

- [ ] **Step 1: Write the failing test**

Append to `tests/test_kilo_keys.py` (reuses `_drive_kilo_pty` already defined there):

```python
@pytest.mark.skipif(sys.platform != "linux", reason="pty raw-mode smoke is Linux-specific")
def test_editor_types_newline_and_deletes_over_a_pty():
    # New empty buffer (no file arg): the welcome banner shows first. Type "hi",
    # Enter, "yo", then a Backspace — the typed text must appear in the painted
    # frames (proving insertChar + newline reach State and get rendered), and the
    # process exits cleanly on Ctrl-Q (proving Backspace/Delete wiring doesn't crash).
    keys = [b"h", b"i", b"\r", b"y", b"o", b"\x7f", b"\x7f", b"\x11"]
    text, code = _drive_kilo_pty(
        keys, winsize=(5, 40), wait_for=b"Kilo editor", extra_args=()
    )
    assert "hi" in text, text          # first line typed and painted
    assert "yo" in text, text          # second line after Enter
    assert "\x1b[2J" in text, text     # screen cleared on entry/exit
    assert code == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/pytest tests/test_kilo_keys.py -q -k editor_types`
Expected: FAIL — typed text does not appear (Stage 3 `Press(b)` discards it) / or the run errors.

- [ ] **Step 3: Wire the dispatch in `Main.thx`**

In `examples/kilo/Main.thx`, replace the `Press(b)` match arm (lines ~73-79) with:

```thoxa
                    Press(b) {
                        ## Ctrl-Q quits (dirty-confirm is Stage 5). Enter splits the
                        ## line; Backspace / Ctrl-H erase; Ctrl-L and ESC are no-ops;
                        ## every other byte (including Tab) is inserted.
                        if b == ctrlKey(b'q') {
                            break
                        }
                        if b == b'\r' {
                            state.insertNewline()
                        } else if b == 127 {
                            state.deleteChar()
                        } else if b == ctrlKey(b'h') {
                            state.deleteChar()
                        } else if b == ctrlKey(b'l') {
                            Void
                        } else if b == 27 {
                            Void
                        } else {
                            state.insertChar(b)
                        }
                    }
```

And replace the `Delete { Void }` arm (line ~88) with:

```thoxa
                    Delete { state.deleteForward() }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `.venv/bin/thoxa check examples/kilo/Main.thx && .venv/bin/pytest tests/test_kilo_keys.py -q`
Expected: `Main.thx: ok` and PASS (existing viewer smoke + the new editing smoke).

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Main.thx tests/test_kilo_keys.py
git commit -m "kilo: Stage 4 Main — key dispatch drives the editor

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

### Task 7: Roadmap + CHANGELOG (mark Stage 4 done)

**Files:**
- Modify: `docs/specs/2026-07-16-kilo-port-roadmap.md` (fork #4 row; module table; build order; status line)
- Modify: `CHANGELOG.md` (`[Unreleased]` entry)

**Interfaces:** none (documentation).

- [ ] **Step 1: Run the whole suite green first (the gate for "done")**

Run: `.venv/bin/pytest -q`
Expected: PASS (all tests, including `test_splice`, `test_kilo_edit`, `test_kilo_row`, `test_kilo_keys`, `test_conformance`). Record the total count.

- [ ] **Step 2: Update the roadmap**

In `docs/specs/2026-07-16-kilo-port-roadmap.md`:

a) The **Status** line (top): append that Stage 4 shipped, e.g. change the tail to:
```
… Stage 3 (Read-only viewer …) shipped 2026-07-17. **Stage 4 (Editing, resolving fork #4) shipped 2026-07-17.** Stage 5 (Save + quit-confirm) is next.
```

b) The **module table** `Edit` row — fold it into `State` (editing lives in `State`):
```
| ~~**Edit**~~ | *folded into `State` (Stage 4): editing ops own `cx`/`cy`/`textRows`, kept private* | — |
```

c) The **fork #4 row** in "Language-design forks & decisions":
```
| 4 | Array/`Buffer` splice (`insertAt`/`removeAt`) | **✔ Resolved (Stage 4):** committed to a **functional, copying, strict-bounds value splice** — `Bytes.insertAt/removeAt` (a byte in a line) and `Array<T>.insertAt/removeAt` (a row in the file), each returning a fresh value with the source untouched; in-place `Buffer`/`Array` memmove set aside for the Perceus/native pass. Rows stay fixed-length `Bytes`; an edit builds a new `Row` (render re-derived). Design `2026-07-17-kilo-stage4-editing-design.md`; `spec/conformance/splice.thx`. |
```

d) The **build order** block — mark Stage 4:
```
Stage 4  Editing                 → resolves fork #4          ✔ DONE (Bytes/Array insertAt/removeAt + examples/kilo/{Row,State,Main}.thx editing; spec/conformance/splice.thx; tests/test_{splice,kilo_edit}.py + updated pty smoke)
```

- [ ] **Step 3: Add the CHANGELOG entry**

In `CHANGELOG.md`, under `## [Unreleased]`, add a new section at the top (fill `<N>` with the total test count from Step 1):

```markdown
### Kilo Stage 4 — the editor (`insertAt`/`removeAt` value splice) (green: <N> tests)
- **`Bytes.insertAt(at, byte)` / `Bytes.removeAt(at)` and `Array<T>.insertAt(at, item)` /
  `Array<T>.removeAt(at)`.** A **functional, copying, strict-bounds** value splice: each
  returns a *fresh* value (a new `Bytes` / a new array) with the byte/element inserted or
  removed, leaving the receiver untouched; out-of-range indices trap like `slice`/indexing
  (`0 ≤ at ≤ count` insert, `0 ≤ at < count` remove; `Bytes` masks the byte to 0–255). This
  **resolves roadmap fork #4** by committing to value semantics — in-place `Buffer`/`Array`
  memmove (and static Perceus reuse of a provably-unique copy) is left to the native pass.
  The checker is unchanged (it types these methods `Unknown`, as it does `slice`).
  `spec/conformance/splice.thx`; `tests/test_splice.py`; `SPEC.md` §9 / §9.1.
- **`examples/kilo/Row.thx` gains per-line editing.** `insertChar`/`deleteChar`/`take`/`drop`/
  `concat` each return a **new `Row`** (so `render` re-derives via the constructor — kilo's
  `editorUpdateRow`), plus a public `bytes()`/`text()` read accessor (`concat` reads
  `other.bytes()` since a private field of another instance isn't readable). `tests/test_kilo_row.py`.
- **`examples/kilo/State.thx` becomes the editor core.** New `insertChar`/`insertNewline`/
  `deleteChar`/`deleteForward` run kilo's `editorInsertChar`/`InsertNewline`/`DelChar`
  algorithms — splicing `textRows` functionally, swapping a within-line edit in place,
  materializing the phantom EOF row, joining/splitting lines, and fixing the cursor — plus a
  `dirty` flag (`isDirty()`; consumed by Stage 5's quit-confirm). Editing lives here (not a
  separate `Edit` module) to keep `cx`/`cy`/`textRows` private. `tests/test_kilo_edit.py`.
- **`examples/kilo/Main.thx` dispatches edits.** The Stage-3 no-op key cases are filled:
  printables insert, Enter splits, Backspace/Ctrl-H erase or join, Delete forward-deletes;
  Ctrl-L/ESC are no-ops; Ctrl-Q still quits (quit-confirm is Stage 5). The `tests/test_kilo_keys.py`
  pty smoke now types text, Enter and Backspace and asserts the painted frames. Deferred:
  save + status/message bars (Stage 5).
```

- [ ] **Step 4: Verify the suite is still green and docs render**

Run: `.venv/bin/pytest -q`
Expected: PASS (unchanged count — docs-only change).

- [ ] **Step 5: Commit**

```bash
git add docs/specs/2026-07-16-kilo-port-roadmap.md CHANGELOG.md
git commit -m "kilo: Stage 4 done — roadmap/CHANGELOG (fork #4 resolved: value splice)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6"
```

---

## Self-Review

**Spec coverage** (design doc §-by-§): §2 splice primitives → Tasks 1, 2; §2.3 checker-unchanged → Global Constraints + Tasks 1-2; §2.4 conformance + `test_splice` → Tasks 1-3; §3 Row methods → Task 4; §4 State ops + dirty → Task 5; §5 Main dispatch → Task 6; §6 Screen unchanged → no task (verified via the pty smoke in Task 6); §7 verification (conformance, test_splice, test_kilo_edit, pty smoke, SPEC, roadmap, CHANGELOG) → Tasks 3, 5, 6, 7; §8 deferred → out of scope. All covered.

**Type consistency:** `Row.insertChar(at, byte)`, `deleteChar(at)`, `take(count)`, `drop(count)`, `concat(other)`, `bytes()`, `text()` used identically in Tasks 4, 5, and the tests. `state.insertChar(byte)`/`insertNewline()`/`deleteChar()`/`deleteForward()`/`isDirty()` consistent across Tasks 5, 6. `Bytes.insertAt/removeAt`, `Array.insertAt/removeAt` consistent across Tasks 1-5.

**Placeholder scan:** every code/edit step carries real code and an exact command with expected output; no TBD/TODO. The only symbolic dependency across tasks (`<N>` test count in Task 7) is explicitly resolved from Task 7 Step 1's run.

**Ordering:** Task dependencies flow 1→(4), 2→(5), 1&2→3, 4&2→5, 5→6, all→7. Each task is independently testable and committable.
