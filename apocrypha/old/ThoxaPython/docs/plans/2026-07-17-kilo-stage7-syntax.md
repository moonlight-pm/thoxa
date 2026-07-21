# Kilo Stage 7 — Syntax Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port kilo's syntax highlighting to `examples/kilo/` — per-cell classification, ANSI color, cross-row comment state, extension-based syntax selection, and the deferred Find match highlight — adding one language primitive, `Bytes.equals`.

**Architecture:** A new pure `Syntax` module (an `Hl` sum type + a `color` match, the C `Syntax` table, `editorUpdateSyntax` as a pure `highlightRow`). `State` caches a whole-document highlight fold (recomputed on any doc/filename change, carrying the multi-line-comment-open flag from row 0). `Screen` interleaves SGR color codes as it paints. The Find match is an immutable overlay, not kilo's save/restore.

**Tech Stack:** Thoxa (the `.thx` example modules) + the Python interpreter/ffi (`thoxa/interp.py`, `thoxa/ffi.py`) for the one primitive. Tests: pytest.

## Global Constraints

- **Design doc:** `docs/specs/2026-07-17-kilo-stage7-syntax-design.md` — the source of truth. Follow its §2–§8 exactly.
- **`Bytes.equals` footprint:** interpreter + `ffi.py` only — **no checker change** (the checker types all `Bytes` members `UNKNOWN`). Mirror `Bytes.find`.
- **No new checker/parser/lexer changes.** The `.thx` modules use only existing language features (sum types, `match`, `Array.append`/`.contains`, `Bytes.slice`/`.get`/`.count`, byte literals, `if`/`loop`).
- **Thoxa has no `&&`/`||`** — nest `if`s (or use `and`/`or` where a single expression is wanted; both exist, but the existing kilo code nests, so match its style).
- **Nullary sum variants construct with a call:** `Normal()`, not bare `Normal`.
- **Cross-module `match` needs the variants exported + selectively imported** — never namespace-qualified patterns.
- **Commit after every task** to master (per `work-on-master`). Commit-message footer:
  ```
  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SDjZSyxyNtcCBXMyZggMk6
  ```
- **Run the full suite** (`.venv/bin/pytest -q`) at the end of each task; a task is done only when green. Baseline before Stage 7: **578 tests**.

---

### Task 1: The `Bytes.equals` primitive

**Files:**
- Modify: `thoxa/ffi.py` (the `Bytes` class, after `find` ~line 182)
- Modify: `thoxa/interp.py` (`_bytes_method` after the `find` branch ~line 1185; the `_member` Bytes allow-list at line 1498)
- Modify: `spec/SPEC.md` (§9.1 Bytes methods)
- Create: `spec/conformance/bytes_equals.thx`, `spec/conformance/bytes_equals.out`
- Test: `tests/test_bytes_equals.py`

**Interfaces:**
- Produces: `Bytes.equals(other: Bytes) -> Bool` (Thoxa), `Bytes.equals(self, other) -> bool` (Python). True iff same length and same bytes, length-counted (embedded-NUL safe). A non-`Bytes` argument traps with `"Bytes.equals"` in the message.

- [ ] **Step 1: Write the failing tests** — create `tests/test_bytes_equals.py`:

```python
"""Bytes.equals — the Stage 7 structural byte-comparison primitive. True iff same
length and bytes, length-counted (embedded-NUL safe), strict about its argument type."""
from __future__ import annotations

import pytest

from thoxa.errors import ThoxaError
from thoxa.ffi import Bytes
from thoxa.interp import run


def _run(body: str):
    buf: list[str] = []
    code = run(f"fn main() -> Int64 {{\n{body}\n0\n}}\n", "<equals>", out=buf.append)
    return "".join(buf), code


# -- through the interpreter (Bool surface) ----------------------------------

def test_equal_buffers_are_true():
    out, _ = _run(
        'let a = Bytes.fromString("int")\n'
        'println("{a.equals(Bytes.fromString(\\"int\\"))}")'
    )
    assert out == "true\n"


def test_different_content_same_length_is_false():
    out, _ = _run(
        'println("{Bytes.fromString(\\"int\\").equals(Bytes.fromString(\\"ins\\"))}")'
    )
    assert out == "false\n"


def test_different_length_is_false():
    out, _ = _run(
        'println("{Bytes.fromString(\\"in\\").equals(Bytes.fromString(\\"int\\"))}")'
    )
    assert out == "false\n"


def test_empty_equals_empty():
    out, _ = _run(
        'println("{Bytes.fromString(\\"\\").equals(Bytes.fromString(\\"\\"))}")'
    )
    assert out == "true\n"


# -- the Python Bytes.equals directly (edges a Thoxa string can't express) ----

def test_equals_python_api():
    assert Bytes.from_string("abc").equals(Bytes.from_string("abc")) is True
    assert Bytes.from_string("abc").equals(Bytes.from_string("abd")) is False
    assert Bytes.from_string("abc").equals(Bytes.from_string("ab")) is False


def test_equals_survives_embedded_nul():
    # length-counted, not NUL-terminated: bytes past a \x00 are still compared.
    assert Bytes.from_bytes(b"a\x00c").equals(Bytes.from_bytes(b"a\x00c")) is True
    assert Bytes.from_bytes(b"a\x00c").equals(Bytes.from_bytes(b"a\x00d")) is False


def test_equals_rejects_non_bytes_argument():
    with pytest.raises(ThoxaError) as exc:
        _run('let b = Bytes.fromString("hi").equals(3)\nprintln("{b}")')
    assert "Bytes.equals" in exc.value.render()
```

- [ ] **Step 2: Run to verify it fails**

Run: `.venv/bin/pytest tests/test_bytes_equals.py -q`
Expected: FAIL (`Bytes has no member 'equals'` / `AttributeError`).

- [ ] **Step 3: Add the `ffi.py` method** — in the `Bytes` class, immediately after `find` (before `__repr__`):

```python
    def equals(self, other: "Bytes") -> bool:
        """True iff `other` has the same length and bytes (length-counted)."""
        return self._buf.raw[: self._len] == other._buf.raw[: other._len]
```

- [ ] **Step 4: Add the interpreter branch + allow-list** — in `_bytes_method`, immediately after the `find` branch (after its `return … Instance(self.base.get("Not"))`, before the final `self.error(f"Bytes has no method '{name}'", span)`):

```python
        if name == "equals":
            if len(values) != 1 or not isinstance(values[0], Bytes):
                self.error("Bytes.equals(other) expects a Bytes buffer", span)
            return buf.equals(values[0])
```

And in `_member`, extend the `Bytes` allow-list tuple (interp.py:1498):

```python
            if name in ("get", "set", "toString", "slice", "insertAt", "removeAt", "find", "equals"):
                return BoundBuiltin(recv, name)
```

- [ ] **Step 5: Run the Python test to verify it passes**

Run: `.venv/bin/pytest tests/test_bytes_equals.py -q`
Expected: PASS (7 tests).

- [ ] **Step 6: Add the conformance program** — create `spec/conformance/bytes_equals.thx`:

```thoxa
## Bytes.equals (Stage 7): structural byte-sequence comparison — same length AND same bytes,
## length-counted. The Thoxa answer to strncmp/strcmp; what the kilo port's keyword / comment
## / extension matching is built on. (Bytes == Bytes is identity; equals is structural.)
fn show(a!: Bytes, b!: Bytes) -> String {
    if a.equals(b) { "yes" } else { "no" }
}

fn main() -> Int64 {
    println(show(Bytes.fromString("int"), Bytes.fromString("int")))    # yes
    println(show(Bytes.fromString("int"), Bytes.fromString("ins")))    # no  (same length)
    println(show(Bytes.fromString("in"),  Bytes.fromString("int")))    # no  (shorter)
    println(show(Bytes.fromString("int"), Bytes.fromString("in")))     # no  (longer)
    println(show(Bytes.fromString(""),    Bytes.fromString("")))       # yes (empty == empty)
    println(show(Bytes.fromString("//"),  Bytes.fromString("//")))     # yes (a delimiter)
    let s = Bytes.fromString("void")
    println(show(s, s))                                                # yes (same buffer)
    0
}
```

Create `spec/conformance/bytes_equals.out`:

```
yes
no
no
no
yes
yes
yes
```

- [ ] **Step 7: Update SPEC §9.1** — in `spec/SPEC.md`, in the `Bytes` sentence of §9.1, after the `.find(needle)` description (`… needle longer than the buffer → \`Not\`).`), insert:

```
`.equals(other)` returns `Bool` — `true` iff `other` has the same length and the same bytes
(structural, length-counted, embedded-NUL safe; `Bytes ==` is identity, this is content
equality).
```

- [ ] **Step 8: Run the conformance + full suite**

Run: `.venv/bin/pytest tests/test_conformance.py -q -k bytes_equals && .venv/bin/pytest -q`
Expected: PASS; total **585** (578 baseline + 7 new).

- [ ] **Step 9: Commit**

```bash
git add thoxa/ffi.py thoxa/interp.py spec/SPEC.md spec/conformance/bytes_equals.thx spec/conformance/bytes_equals.out tests/test_bytes_equals.py
git commit -m "kilo: Stage 7 — Bytes.equals (structural byte comparison, the language increment)"
```

---

### Task 2: `Syntax.thx` scaffolding — `Hl`, `color`, `Syntax`, `selectSyntax`, helpers, `plainRow`

**Files:**
- Create: `examples/kilo/Syntax.thx`
- Test: `tests/test_kilo_syntax.py`

**Interfaces:**
- Produces (all `export`ed unless noted):
  - `type Hl = Normal | Comment | Keyword1 | Keyword2 | Str | Number | Matched` (7 nullary variants, all exported).
  - `color(h!: Hl) -> Int64` — the ANSI SGR code (Normal → 39).
  - `type Syntax(keywords1!: Array<Bytes>, keywords2!: Array<Bytes>, single!: Bytes, mlStart!: Bytes, mlEnd!: Bytes, numbers!: Bool, strings!: Bool)` with reads `kw1`, `kw2`, `scs`, `mcs`, `mce`, `hlNumbers`, `hlStrings`.
  - `type RowHl(cellList!: Array<Hl>, openFlag!: Bool)` with reads `cells`, `openComment`.
  - `selectSyntax(filename!: String) -> May<Syntax>` — `Has(cSyntax())` for `.c`/`.h`/`.cpp`, else `Not()`.
  - `plainRow(render!: Bytes) -> RowHl` — every cell `Normal`, `openComment == false`.
  - Private: `cSyntax()`, `matchesAt`, `endsWith`, `isSeparator`, `separatorAt`, `isDigit`, `isNumberHl`, `punctuation`.
- Consumes: `import { Row } from "./Row"` is **not** needed yet (RowHl holds only cells); the module imports nothing this task.

- [ ] **Step 1: Write the failing tests** — create `tests/test_kilo_syntax.py`:

```python
"""Stage 7 of the kilo port: the `Syntax` module (examples/kilo/Syntax.thx) — the pure
classifier. Tested deterministically with an import-probe (the Stage-6 searchFrom seam):
selectSyntax by extension, plainRow, and highlightRow's per-class classification + the
cross-row comment fold. The live color draw is covered by the pty smoke (test_kilo_keys)."""
from __future__ import annotations

import pathlib

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def test_module_type_checks_clean():
    path = KILO / "Syntax.thx"
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


def _select(name: str) -> str:
    src = (
        'import { selectSyntax } from "./Syntax"\n'
        "fn main() -> Int64 {\n"
        f'  match selectSyntax("{name}") {{ Has(s) {{ println("yes") }} Not {{ println("no") }} }}\n'
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_select_syntax_by_extension():
    assert _select("main.c") == "yes"
    assert _select("kilo.h") == "yes"
    assert _select("a.cpp") == "yes"
    assert _select("notes.txt") == "no"
    assert _select("Makefile") == "no"
    assert _select("") == "no"


def _classify(line: str, prev_open: str = "false") -> str:
    """Run highlightRow over `line` (with cSyntax via selectSyntax) and return the per-cell
    class letters + ' open=<bool>'. Escapes backslash and quote so the line survives embedding."""
    esc = line.replace("\\", "\\\\").replace('"', '\\"')
    src = (
        'import { Hl, Normal, Comment, Keyword1, Keyword2, Str, Number, highlightRow, selectSyntax } from "./Syntax"\n'
        "fn letter(h!: Hl) -> String {\n"
        '  match h { Normal { "N" } Comment { "C" } Keyword1 { "1" } Keyword2 { "2" } '
        'Str { "S" } Number { "#" } else { "?" } }\n'
        "}\n"
        "fn main() -> Int64 {\n"
        '  match selectSyntax("x.c") {\n'
        "    Has(s) {\n"
        f'      let rh = highlightRow(Bytes.fromString("{esc}"), {prev_open}, s)\n'
        '      mut out = ""\n'
        "      mut i = 0\n"
        "      loop { if i >= rh.cells.count { break }  out = \"{out}{letter(rh.cells[i])}\"  i = i + 1 }\n"
        '      println("{out} open={rh.openComment}")\n'
        "    }\n"
        '    Not { println("no syntax") }\n'
        "  }\n"
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_plain_row_is_all_normal():
    src = (
        'import { Hl, Normal, plainRow } from "./Syntax"\n'
        "fn main() -> Int64 {\n"
        '  let rh = plainRow(Bytes.fromString("int x = 1;"))\n'
        '  mut allN = true\n'
        "  mut i = 0\n"
        "  loop { if i >= rh.cells.count { break }  match rh.cells[i] { Normal { Void } else { allN = false } }  i = i + 1 }\n"
        '  println("{allN} {rh.cells.count} open={rh.openComment}")\n'
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    assert "".join(buf).strip() == "true 10 open=false"
```

- [ ] **Step 2: Run to verify it fails**

Run: `.venv/bin/pytest tests/test_kilo_syntax.py -q`
Expected: FAIL (`Syntax.thx` does not exist).

- [ ] **Step 3: Create `examples/kilo/Syntax.thx`** with the scaffolding (Hl, color, Syntax, RowHl, cSyntax, selectSyntax, byte helpers, plainRow) — `highlightRow`/`keywordAt` are Task 3:

```thoxa
## Syntax — Stage 7 of the kilo port: classify a row's rendered bytes into highlight classes
## (kilo's editorUpdateSyntax + editorSyntaxToColor + editorSelectSyntaxHighlight). Pure — no
## terminal, no singleton — so the whole state machine is unit-tested deterministically. The
## per-cell class is a closed sum type (`Hl`); its ANSI color is a separate rendering match.

## kilo's HL_* enum. HL_COMMENT and HL_MLCOMMENT are merged (same color, nothing branches on
## the distinction). `Matched` is kilo's HL_MATCH (named to avoid Find.Match).
export type Normal {}
export type Comment {}
export type Keyword1 {}
export type Keyword2 {}
export type Str {}
export type Number {}
export type Matched {}
export type Hl = Normal | Comment | Keyword1 | Keyword2 | Str | Number | Matched

## kilo editorSyntaxToColor — the VT100 SGR code. Normal (39 = default fg) is handled by the
## emitter (Screen), which special-cases it; this returns the code for a colored class.
export fn color(h!: Hl) -> Int64 {
    match h {
        Number { 31 }
        Str { 35 }
        Comment { 36 }
        Keyword1 { 33 }
        Keyword2 { 32 }
        Matched { 34 }
        Normal { 39 }
    }
}

## A language's highlighting rules (kilo struct editorSyntax). Immutable record: ctor params
## private, projected to public reads. Two explicit keyword lists replace kilo's trailing-`|`
## keyword-2 marker; kw2 (types) → green. numbers/strings are kilo's HL_HIGHLIGHT_* flags.
export type Syntax(
    keywords1!: Array<Bytes>, keywords2!: Array<Bytes>,
    single!: Bytes, mlStart!: Bytes, mlEnd!: Bytes,
    numbers!: Bool, strings!: Bool
) {
    export let kw1 = keywords1
    export let kw2 = keywords2
    export let scs = single
    export let mcs = mlStart
    export let mce = mlEnd
    export let hlNumbers = numbers
    export let hlStrings = strings
}

## One row's highlight cells (one Hl per render byte) plus whether it ends inside an open
## /* … */ (kilo's hl_open_comment) — the only state that crosses rows.
export type RowHl(cellList!: Array<Hl>, openFlag!: Bool) {
    export let cells = cellList
    export let openComment = openFlag
}

## kilo's C_HL_keywords / C_HL_extensions as one builder.
fn cSyntax() -> Syntax {
    let kw1 = [ Bytes.fromString("switch"), Bytes.fromString("if"),
                Bytes.fromString("while"),  Bytes.fromString("for"),
                Bytes.fromString("break"),  Bytes.fromString("continue"),
                Bytes.fromString("return"), Bytes.fromString("else"),
                Bytes.fromString("struct"), Bytes.fromString("union"),
                Bytes.fromString("typedef"),Bytes.fromString("static"),
                Bytes.fromString("enum"),   Bytes.fromString("class"),
                Bytes.fromString("case") ]
    let kw2 = [ Bytes.fromString("int"),    Bytes.fromString("long"),
                Bytes.fromString("double"), Bytes.fromString("float"),
                Bytes.fromString("char"),   Bytes.fromString("unsigned"),
                Bytes.fromString("signed"), Bytes.fromString("void") ]
    Syntax(kw1, kw2, Bytes.fromString("//"), Bytes.fromString("/*"),
           Bytes.fromString("*/"), true, true)
}

## kilo editorSelectSyntaxHighlight — pick the language by the filename's extension.
export fn selectSyntax(filename!: String) -> May<Syntax> {
    let name = Bytes.fromString(filename)
    if endsWith(name, Bytes.fromString(".c"))   { return Has(cSyntax()) }
    if endsWith(name, Bytes.fromString(".h"))   { return Has(cSyntax()) }
    if endsWith(name, Bytes.fromString(".cpp")) { return Has(cSyntax()) }
    Not()
}

## Do the bytes of `hay` at index `at` equal `needle`? (kilo !strncmp.) Guarded so an
## out-of-range window is "no match", not a slice trap.
fn matchesAt(hay!: Bytes, at!: Int64, needle!: Bytes) -> Bool {
    if at + needle.count > hay.count { return false }
    hay.slice(at, at + needle.count).equals(needle)
}

fn endsWith(hay!: Bytes, suffix!: Bytes) -> Bool {
    if suffix.count > hay.count { return false }
    matchesAt(hay, hay.count - suffix.count, suffix)
}

## kilo is_separator: whitespace, NUL, or a C punctuation byte.
let punctuation: Array<Byte> = [ b',', b'.', b'(', b')', b'+', b'-', b'/', b'*',
                                 b'=', b'~', b'%', b'<', b'>', b'[', b']', b';' ]
fn isSeparator(c!: Byte) -> Bool {
    if c == b' '  { return true }
    if c == b'\t' { return true }
    if c == b'\n' { return true }
    if c == b'\r' { return true }
    if c == 0     { return true }
    punctuation.contains(c)
}

## The byte at `at` treated as a separator, where end-of-line counts as one (kilo reads
## render's trailing NUL, and is_separator('\0') is true).
fn separatorAt(render!: Bytes, at!: Int64) -> Bool {
    if at >= render.count { true } else { isSeparator(render.get(at)) }
}

fn isDigit(c!: Byte) -> Bool { if c < b'0' { false } else { c <= b'9' } }
fn isNumberHl(h!: Hl) -> Bool { match h { Number { true } else { false } } }

## No active syntax → every cell Normal, never in a comment (kilo E.syntax == NULL).
export fn plainRow(render!: Bytes) -> RowHl {
    mut cells: Array<Hl> = []
    mut i = 0
    loop { if i >= render.count { break }  cells = cells.append(Normal())  i = i + 1 }
    RowHl(cells, false)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `.venv/bin/pytest tests/test_kilo_syntax.py -q`
Expected: PASS for `test_module_type_checks_clean`, `test_select_syntax_by_extension`, `test_plain_row_is_all_normal`. (The `_classify` helper exists but no test calls it yet.)

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Syntax.thx tests/test_kilo_syntax.py
git commit -m "kilo: Stage 7 — Syntax scaffolding (Hl sum type, color, C Syntax table, selectSyntax, plainRow)"
```

---

### Task 3: `highlightRow` — the `editorUpdateSyntax` state machine

**Files:**
- Modify: `examples/kilo/Syntax.thx` (append `Keyword`, `keywordAt`, `firstMatch`, `highlightRow`)
- Test: `tests/test_kilo_syntax.py` (add the classification tests using the `_classify` helper from Task 2)

**Interfaces:**
- Produces: `highlightRow(render!: Bytes, prevOpen!: Bool, s!: Syntax) -> RowHl` — one `Hl` per render byte + the row's ending `openComment`. Invariant: `cells.count == render.count`.
- Consumes: `Hl` variants, `Syntax` reads, `matchesAt`, `separatorAt`, `isDigit`, `isNumberHl`, `Normal/Comment/Keyword1/Keyword2/Str/Number` (Task 2).

- [ ] **Step 1: Write the failing classification tests** — append to `tests/test_kilo_syntax.py`:

```python
def test_keyword_kw1_bounded_by_separators():
    # "if" is kw1; "(" after it is a separator, so both bytes classify Keyword1.
    # "if (x) {" is 8 chars: 11 (keyword) + 6 normal.
    assert _classify("if (x) {") == "11NNNNNN open=false"


def test_near_keyword_without_boundary_is_normal():
    # "ifx" is not the keyword "if" — the char after "if" (x) is not a separator.
    assert _classify("ifx") == "NNN open=false"


def test_kw2_type_is_keyword2():
    assert _classify("int x") == "222NN open=false"


def test_decimal_number():
    assert _classify("x = 42;") == "NNNN##N open=false"


def test_digit_inside_identifier_is_not_a_number():
    # a digit only starts a number after a separator (or another number).
    assert _classify("a1b") == "NNN open=false"


def test_string_with_escaped_quote_stays_a_string():
    # "a\"b" : the \ escapes the following ", which does NOT close the string.
    assert _classify('x = "a\\"b"') == "NNNNSSSSSS open=false"


def test_single_line_comment_runs_to_end_of_line():
    assert _classify("x // hi") == "NNCCCCC open=false"


def test_block_comment_within_a_line():
    assert _classify("a /* c */ b") == "NNCCCCCCCNN open=false"


def test_block_comment_opens_and_carries_to_next_row():
    # a line that opens /* but never closes it → openComment true; the whole "/* ..." is comment.
    opener = _classify("x /* open")
    assert opener == "NNCCCCCCC open=true"
    # the next row, fed prevOpen=true, is all comment until it would close.
    assert _classify("still open */ y", prev_open="true") == "CCCCCCCCCCCCCNN open=false"
```

- [ ] **Step 2: Run to verify they fail**

Run: `.venv/bin/pytest tests/test_kilo_syntax.py -q -k "keyword or number or string or comment or digit"`
Expected: FAIL (`Syntax has no member 'highlightRow'` / undefined).

- [ ] **Step 3: Append the state machine to `examples/kilo/Syntax.thx`** (after `plainRow`):

```thoxa
## A matched keyword: its byte length + whether it is a type keyword (kw2 → green).
type Keyword(length!: Int64, kw2Flag!: Bool) {
    export let len = length
    export let isKw2 = kw2Flag
}

## The byte-length of the first keyword in `list` matching at `i` with a separator after it.
fn firstMatch(render!: Bytes, i!: Int64, list!: Array<Bytes>) -> May<Int64> {
    for kw in list {
        if matchesAt(render, i, kw) {
            if separatorAt(render, i + kw.count) { return Has(kw.count) }
        }
    }
    Not()
}

## The first keyword (kw1 then kw2) whose bytes sit at `i` bounded by a separator, else Not.
fn keywordAt(render!: Bytes, i!: Int64, s!: Syntax) -> May<Keyword> {
    match firstMatch(render, i, s.kw1) { Has(w) { return Has(Keyword(w, false)) } Not { Void } }
    match firstMatch(render, i, s.kw2) { Has(w) { return Has(Keyword(w, true))  } Not { Void } }
    Not()
}

## kilo editorUpdateSyntax. `render` = the row's rendered bytes; `prevOpen` = the previous
## row's openComment (false for row 0); `s` = the active Syntax. Returns one Hl per render byte
## plus this row's openComment. Invariant: cells.count == i every iteration (each branch
## appends exactly as many cells as it advances `i`).
export fn highlightRow(render!: Bytes, prevOpen!: Bool, s!: Syntax) -> RowHl {
    let n = render.count
    mut cells: Array<Hl> = []
    mut prevSep = true
    mut inString: Int64 = 0        # 0 = not in a string; else the open quote byte
    mut inComment = prevOpen
    mut i = 0
    loop {
        if i >= n { break }
        let c = render.get(i)
        let prevHl = if cells.count > 0 { cells[cells.count - 1] } else { Normal() }

        ## 1. single-line comment: // to end of line
        if inString == 0 {
            if inComment == false {
                if matchesAt(render, i, s.scs) {
                    loop { if i >= n { break }  cells = cells.append(Comment())  i = i + 1 }
                    continue
                }
            }
        }

        ## 2. multi-line comment /* … */
        if inString == 0 {
            if inComment {
                cells = cells.append(Comment())
                if matchesAt(render, i, s.mce) {
                    mut k = 1
                    loop { if k >= s.mce.count { break }  cells = cells.append(Comment())  k = k + 1 }
                    i = i + s.mce.count
                    inComment = false
                    prevSep = true
                    continue
                }
                i = i + 1
                continue
            } else if matchesAt(render, i, s.mcs) {
                mut k = 0
                loop { if k >= s.mcs.count { break }  cells = cells.append(Comment())  k = k + 1 }
                i = i + s.mcs.count
                inComment = true
                continue
            }
        }

        ## 3. strings: track the open quote; \ escapes the next byte
        if s.hlStrings {
            if inString != 0 {
                cells = cells.append(Str())
                if c == b'\\' {
                    if i + 1 < n { cells = cells.append(Str())  i = i + 2  continue }
                }
                if c == inString { inString = 0 }
                i = i + 1
                prevSep = true
                continue
            } else if c == b'"' or c == b'\'' {
                inString = c
                cells = cells.append(Str())
                i = i + 1
                continue
            }
        }

        ## 4. numbers: a digit after a separator or another digit; a '.' continuing a number
        if s.hlNumbers {
            let startsNum = if isDigit(c) { prevSep or isNumberHl(prevHl) } else { false }
            let dotNum = if c == b'.' { isNumberHl(prevHl) } else { false }
            if startsNum or dotNum {
                cells = cells.append(Number())
                i = i + 1
                prevSep = false
                continue
            }
        }

        ## 5. keywords — only at a separator boundary on both sides
        if prevSep {
            match keywordAt(render, i, s) {
                Has(kw) {
                    mut k = 0
                    loop {
                        if k >= kw.len { break }
                        cells = cells.append(if kw.isKw2 { Keyword2() } else { Keyword1() })
                        k = k + 1
                    }
                    i = i + kw.len
                    prevSep = false
                    continue
                }
                Not { Void }
            }
        }

        ## 6. default: a normal byte
        cells = cells.append(Normal())
        prevSep = isSeparator(c)
        i = i + 1
    }
    RowHl(cells, inComment)
}
```

- [ ] **Step 4: Run the classification tests to verify they pass**

Run: `.venv/bin/pytest tests/test_kilo_syntax.py -q`
Expected: PASS (all Task 2 + Task 3 tests). If a class string mismatches, re-check the branch order against the design §3.4 (scs, ml, string, number, keyword, default) — order is load-bearing.

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Syntax.thx tests/test_kilo_syntax.py
git commit -m "kilo: Stage 7 — highlightRow (editorUpdateSyntax state machine) + cross-row comment fold"
```

---

### Task 4: `State` highlight cache + match overlay (+ `Row.renderBytes`)

**Files:**
- Modify: `examples/kilo/Row.thx` (add `renderBytes()`)
- Modify: `examples/kilo/State.thx` (imports; `activeSyntax`/`rowHls`/`matchRow`/`matchCol`/`matchLen`; `refreshHighlight`; wire `setRows`/`setFilename`/`insertChar`/`insertNewline`/`deleteChar`; `rowCells`/`overlayMatch`; `setMatch`/`clearMatch`)
- Test: `tests/test_kilo_state.py` (extend)

**Interfaces:**
- Produces (State exports): `rowCells(filerow!: Int64) -> Array<Hl>` (full-render cells, match overlaid); `setMatch(row!: Int64, col!: Int64, length!: Int64) -> Void`; `clearMatch() -> Void`.
- Produces (Row export): `renderBytes() -> Bytes` — a fresh copy of the full rendered line.
- Consumes: `Syntax` module (`highlightRow`, `plainRow`, `selectSyntax`, `Hl`, `RowHl`, `Syntax`, `Matched`); existing `Row.cxToRx`.

- [ ] **Step 1: Write the failing State tests** — append to `tests/test_kilo_state.py`. These use the existing `_probe(body)` helper (imports State + Row). Add a Syntax import to the probe body where needed by writing a local probe:

```python
def _syntax_probe(body: str) -> str:
    src = (
        'import state from "./State"\n'
        'import { Row } from "./Row"\n'
        'import { Hl, Normal, Comment, Keyword2, Number, Matched } from "./Syntax"\n'
        "fn letter(h!: Hl) -> String {\n"
        '  match h { Normal { "N" } Comment { "C" } Keyword2 { "2" } Number { "#" } '
        'Matched { "M" } else { "?" } }\n'
        "}\n"
        "fn cells(filerow!: Int64) -> String {\n"
        "  let cs = state.rowCells(filerow)\n"
        '  mut out = ""\n'
        "  mut i = 0\n"
        "  loop { if i >= cs.count { break }  out = \"{out}{letter(cs[i])}\"  i = i + 1 }\n"
        "  out\n"
        "}\n"
        "fn main() -> Int64 {\n" + body + "\n0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "state_syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_set_rows_with_c_extension_highlights():
    # "int x = 1;" under a .c name: int -> Keyword2 (222), 1 -> Number (#).
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int x = 1;")))\n'
        '  state.setRows(rs, "a.c")\n'
        '  println(cells(0))'
    )
    assert out == "222NNNNN#N"


def test_plain_text_name_is_all_normal():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int x = 1;")))\n'
        '  state.setRows(rs, "a.txt")\n'
        '  println(cells(0))'
    )
    assert out == "NNNNNNNNNN"


def test_edit_updates_the_highlight():
    # Opening a block comment on row 0 must recolor row 1 (cross-row fold, via refreshHighlight).
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("x")))\n'
        '  rs = rs.append(Row(Bytes.fromString("y")))\n'
        '  state.setRows(rs, "a.c")\n'
        '  println(cells(1))\n'                          # before: "N"
        '  state.jumpTo(1, 0)\n'                          # cursor to end of row 0
        '  state.insertChar(b\'/\')\n'
        '  state.insertChar(b\'*\')\n'                    # row 0 now "x/*" -> opens a comment
        '  println(cells(1))'                             # after: row 1 is comment -> "C"
    )
    assert out.splitlines() == ["N", "C"]


def test_set_filename_enables_highlighting():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("int a;")))\n'
        '  state.setRows(rs, "")\n'
        '  println(cells(0))\n'                           # unnamed: all normal
        '  state.setFilename("a.c")\n'
        '  println(cells(0))'                             # named .c: int -> 222
    )
    assert out.splitlines() == ["NNNNNN", "222NNN"]


def test_set_and_clear_match_overlay():
    out = _syntax_probe(
        '  mut rs: Array<Row> = []\n'
        '  rs = rs.append(Row(Bytes.fromString("hello world")))\n'
        '  state.setRows(rs, "a.txt")\n'                  # plain text: normally all Normal
        '  state.setMatch(0, 6, 5)\n'                     # match "world" at chars col 6, len 5
        '  println(cells(0))\n'                           # cols 6..11 -> Matched
        '  state.clearMatch()\n'
        '  println(cells(0))'
    )
    assert out.splitlines() == ["NNNNNNMMMMM", "NNNNNNNNNNN"]
```

- [ ] **Step 2: Run to verify they fail**

Run: `.venv/bin/pytest tests/test_kilo_state.py -q -k "highlight or match or filename or plain_text"`
Expected: FAIL (`State has no member 'rowCells'` etc.).

- [ ] **Step 3: Add `Row.renderBytes()`** — in `examples/kilo/Row.thx`, inside the `Row` type body, after `bytes()`:

```thoxa
    ## A fresh copy of the full rendered (tab-expanded) line — Syntax classifies this.
    export fn renderBytes() -> Bytes {
        render.slice(0, render.count)
    }
```

- [ ] **Step 4: Wire the highlight cache into `State`** — in `examples/kilo/State.thx`:

(a) after the existing `import { Row } from "./Row"` / `import fileio from "./FileIO"` lines, add:

```thoxa
import { Hl, RowHl, Syntax, Matched, highlightRow, plainRow, selectSyntax } from "./Syntax"
```

(b) after the existing `mut textRows: Array<Row> = []` / `mut filename = ""` fields, add:

```thoxa
mut activeSyntax: May<Syntax> = Not()   # chosen by filename (selectSyntax)
mut rowHls: Array<RowHl> = []           # one per file row, parallel to textRows
mut matchRow = -1                       # the Find match's file row (-1 = none)
mut matchCol = -1                       # its byte column into that row's chars
mut matchLen = 0                        # the query's byte length
```

(c) add the fold + accessors (place after the editing section, before the status-message section):

```thoxa
## -- syntax highlight (Stage 7): a derived cache over textRows --------------
## Recompute every row's highlight, folding the multi-line-comment-open flag from row 0. Called
## by every mutation of textRows / filename — the invariant that derived highlight never
## outlives a document change. O(total render bytes); fine for a demo's small files.
fn refreshHighlight() -> Void {
    mut hls: Array<RowHl> = []
    mut open = false
    mut i = 0
    loop {
        if i >= textRows.count { break }
        let rh = match activeSyntax {
            Has(s) { highlightRow(textRows[i].renderBytes(), open, s) }
            Not    { plainRow(textRows[i].renderBytes()) }
        }
        open = rh.openComment
        hls = hls.append(rh)
        i = i + 1
    }
    rowHls = hls
    Void
}

## The cells for a file row, with the Find match overlaid (kilo HL_MATCH). Overlay, not
## save/restore: rows are immutable, so this derives a fresh Array<Hl> for the frame.
export fn rowCells(filerow!: Int64) -> Array<Hl> {
    let base = rowHls[filerow].cells
    if filerow == matchRow { overlayMatch(base, row(filerow)) } else { base }
}

## Paint render cells [cxToRx(matchCol) .. cxToRx(matchCol+matchLen)) as Matched. The match
## column is a chars offset (Stage 6 searches chars); cxToRx maps it to render space.
fn overlayMatch(base!: Array<Hl>, r!: Row) -> Array<Hl> {
    let rxBegin = r.cxToRx(matchCol)
    let rxEnd = r.cxToRx(matchCol + matchLen)
    mut out: Array<Hl> = []
    mut k = 0
    loop {
        if k >= base.count { break }
        if k >= rxBegin {
            if k < rxEnd { out = out.append(Matched()) } else { out = out.append(base[k]) }
        } else { out = out.append(base[k]) }
        k = k + 1
    }
    out
}

## Find sets/clears the highlighted match as the cursor jumps (Stage 7).
export fn setMatch(row!: Int64, col!: Int64, length!: Int64) -> Void {
    matchRow = row  matchCol = col  matchLen = length  Void
}
export fn clearMatch() -> Void { matchRow = -1  matchCol = -1  matchLen = 0  Void }
```

(d) call `refreshHighlight()` at the mutation sites. In `setRows`, replace its body:

```thoxa
export fn setRows(newRows!: Array<Row>, name!: String) -> Void {
    textRows = newRows
    filename = name
    activeSyntax = selectSyntax(name)
    refreshHighlight()
    Void
}
```

In `setFilename`, replace its body:

```thoxa
export fn setFilename(name!: String) -> Void {
    filename = name
    activeSyntax = selectSyntax(name)
    refreshHighlight()
    Void
}
```

Add `refreshHighlight()` as the last statement (before `Void`) of `insertChar`, `insertNewline`, and `deleteChar`. For example `insertChar` ends:

```thoxa
    cx = cx + 1
    dirty = true
    refreshHighlight()
    Void
```

Apply the same one-line addition before the final `Void` of `insertNewline` and `deleteChar` (`deleteForward` calls `deleteChar`, so it is covered).

- [ ] **Step 5: Run the State tests to verify they pass**

Run: `.venv/bin/pytest tests/test_kilo_state.py -q`
Expected: PASS (existing + 5 new).

- [ ] **Step 6: Commit**

```bash
git add examples/kilo/Row.thx examples/kilo/State.thx tests/test_kilo_state.py
git commit -m "kilo: Stage 7 — State highlight cache (whole-doc fold) + match overlay + Row.renderBytes"
```

---

### Task 5: `Screen` SGR draw loop

**Files:**
- Modify: `examples/kilo/Screen.thx` (import `color`/`Normal`; add `drawRowBytes`; use it in `refresh`)
- Test: `tests/test_kilo_screen.py` (extend `_with_rows` for a filename; add color tests)

**Interfaces:**
- Consumes: `State.rowCells`, `State.row(...).window/renderCount/cxToRx`; `Syntax.color`, `Syntax.Normal`.
- Produces: colored frame bytes — a row's visible cells interleaved with `\x1b[<n>m` SGR codes, a `\x1b[39m` reset per row.

- [ ] **Step 1: Write the failing Screen tests** — in `tests/test_kilo_screen.py`, first extend `_with_rows` to take a filename (so a `.c` name enables highlighting), then add tests. Replace `_with_rows` with:

```python
def _with_rows(lines: list[str], rows: int, cols: int, moves: str = "", name: str = "t") -> str:
    setup = f"    state.setScreenSize({rows}, {cols})\n    mut rs: Array<Row> = []\n"
    for line in lines:
        setup += f'    rs = rs.append(Row(Bytes.fromString("{line}")))\n'
    setup += f'    state.setRows(rs, "{name}")\n' + moves
    return _run_screen(setup)
```

Add:

```python
def test_syntax_number_is_red():
    # a .c file: the number 42 is painted red (\x1b[31m). Tall enough to show the line.
    frame = _with_rows(["int x = 42;"], 6, 40, name="a.c")
    assert "\x1b[31m" in frame       # HL_NUMBER red
    assert "\x1b[32m" in frame       # "int" kw2 green


def test_all_normal_row_emits_no_color():
    frame = _with_rows(["plain text here"], 6, 40, name="a.txt")
    for code in ("\x1b[31m", "\x1b[32m", "\x1b[33m", "\x1b[35m", "\x1b[36m", "\x1b[34m"):
        assert code not in frame


def test_set_match_paints_blue():
    frame = _with_rows(
        ["hello world"], 6, 40,
        moves='    state.setMatch(0, 6, 5)\n', name="a.txt",
    )
    assert "\x1b[34m" in frame        # HL_MATCH blue over "world"
```

- [ ] **Step 2: Run to verify they fail**

Run: `.venv/bin/pytest tests/test_kilo_screen.py -q -k "syntax or normal_row or match"`
Expected: FAIL (no SGR color codes — refresh still draws the plain window).

- [ ] **Step 3: Add the color draw to `examples/kilo/Screen.thx`**

(a) after `import state from "./State"`, add:

```thoxa
import { color, Normal } from "./Syntax"
```

(b) add a `sgrReset` constant beside the other VT100 constants (after `let invertOff = "\x1b[m"`):

```thoxa
let sgrReset = "\x1b[39m"       # SGR default foreground — the per-row color reset
```

(c) add the helper (place before `refresh`):

```thoxa
## One visible file row, render bytes interleaved with SGR color changes (kilo editorDrawRows).
## `cells` (full render, match overlaid) is indexed from `begin` — the same clamp Row.window
## uses — so bytes and classes stay aligned. Returns Bytes (Buffer is value-typed; a callee
## can't append into refresh's frame).
fn drawRowBytes(filerow!: Int64, coloff!: Int64, cols!: Int64) -> Bytes {
    let r = state.row(filerow)
    let visible = r.window(coloff, cols)
    let cells = state.rowCells(filerow)
    let count = r.renderCount()
    let begin = if coloff > count { count } else { coloff }

    mut out = Buffer()
    mut currentColor = -1
    mut j = 0
    loop {
        if j >= visible.count { break }
        let hl = cells[begin + j]
        match hl {
            Normal {
                if currentColor != -1 {
                    out.appendBytes(Bytes.fromString(sgrReset))
                    currentColor = -1
                }
            }
            else {
                let col = color(hl)
                if col != currentColor {
                    out.appendBytes(Bytes.fromString("\x1b[{col}m"))
                    currentColor = col
                }
            }
        }
        out.append(visible.get(j))
        j = j + 1
    }
    out.appendBytes(Bytes.fromString(sgrReset))
    out.freeze()
}
```

(d) in `refresh`, replace the file-row draw line:

```thoxa
            ## the file row's visible window (render[coloff .. coloff+cols], clamped)
            buf.appendBytes(state.row(filerow).window(coloff, cols))
```

with:

```thoxa
            ## the file row's visible window, syntax-colored (kilo editorDrawRows)
            buf.appendBytes(drawRowBytes(filerow, coloff, cols))
```

- [ ] **Step 4: Run the Screen tests to verify they pass**

Run: `.venv/bin/pytest tests/test_kilo_screen.py -q`
Expected: PASS. Some existing exact-byte tests (`test_frame_exact_bytes_small`, `test_draws_file_rows_no_welcome`, `test_tab_expanded_row_is_drawn`, `test_horizontal_scroll_windows_the_row`) use a plain `"t"`/default name, so their rows are all-Normal → the only new bytes are a leading nothing + a trailing `\x1b[39m` per drawn row. **If those exact-byte assertions now include the per-row `\x1b[39m` reset, update their expected strings** to include the trailing `\x1b[39m` before each `\x1b[K` on a text row (the reset is emitted after the row's bytes, before clearLine). Re-run until green.

- [ ] **Step 5: Run the full suite**

Run: `.venv/bin/pytest -q`
Expected: PASS. Fix any other exact-frame assertions the reset shifted.

- [ ] **Step 6: Commit**

```bash
git add examples/kilo/Screen.thx tests/test_kilo_screen.py
git commit -m "kilo: Stage 7 — Screen SGR color draw loop (per-cell hl -> ANSI, per-row reset)"
```

---

### Task 6: Find match wiring + `sample.c` + pty smoke

**Files:**
- Modify: `examples/kilo/Find.thx` (call `setMatch` on a hit; `clearMatch` on every loop exit)
- Create: `examples/kilo/sample.c`
- Test: `tests/test_kilo_keys.py` (extend the pty smoke)

**Interfaces:**
- Consumes: `State.setMatch`, `State.clearMatch` (Task 4).

- [ ] **Step 1: Create `examples/kilo/sample.c`**:

```c
/* kilo sample: this block comment spans
   two lines to exercise the cross-row
   openComment fold. */
#include <stdio.h>

int main(int argc, char **argv) {
    int count = 42;                 // a decimal number and a line comment
    char *name = "world";
    for (int i = 0; i < count; i++) {
        printf("hello %s\n", name);
    }
    return 0;
}
```

- [ ] **Step 2: Wire the match into `examples/kilo/Find.thx`** — in `run()`:

(a) on the successful re-search, beside `state.jumpTo(m.col, m.row)`, add the setMatch call. The `Has(m)` arm becomes:

```thoxa
                        Has(m) {
                            lastMatch = m.row
                            state.jumpTo(m.col, m.row)
                            state.setMatch(m.row, m.col, query.count)
                        }
```

(b) clear the match on every loop exit. Replace the ESC arm's `break` path and the Enter `break`, and add a clear after the loop. Concretely: the ESC branch becomes

```thoxa
                        if b == 27 {                       # ESC -> cancel, restore the view
                            state.restoreView(savedCx, savedCy, savedColoff, savedRowoff)
                            state.clearMatch()
                            break
                        } else if b == b'\r' {             # Enter -> keep the current match
                            state.clearMatch()
                            break
                        }
```

and the two terminal-error `break`s (the `Fail(e) { break }` for `screen.refresh()` and for `readKey()`) each become `Fail(e) { state.clearMatch()  break }`. Leave the final `state.setStatusMessage("")` as is.

- [ ] **Step 3: Extend the pty smoke** — add to `tests/test_kilo_keys.py`:

```python
def test_editor_highlights_c_syntax_over_a_pty():
    # Open the C sample in a tall window so several classes are visible in one frame.
    out, _ = _drive_kilo_pty(
        [b"\x11"],                       # Ctrl-Q to quit (clean buffer quits immediately)
        winsize=(14, 80),
        wait_for=b"kilo sample",         # the first comment line's text (cyan run)
        extra_args=("examples/kilo/sample.c",),
    )
    assert "\x1b[36m" in out             # comment cyan
    assert "\x1b[31m" in out             # the number 42 red
    assert "\x1b[32m" in out             # int/char kw2 green
    assert "\x1b[35m" in out             # the "world" string magenta


def test_editor_find_match_highlights_blue_over_a_pty():
    # Ctrl-F, type "count" (a plain identifier -> normally uncolored), so the blue match
    # overlay is unambiguous; Enter keeps it.
    out, _ = _drive_kilo_pty(
        [b"\x06", b"count", b"\r"],
        winsize=(14, 80),
        wait_for=b"kilo sample",
        extra_args=("examples/kilo/sample.c",),
    )
    assert "\x1b[34m" in out             # HL_MATCH blue
```

- [ ] **Step 4: Run the pty smoke + full suite**

Run: `.venv/bin/pytest tests/test_kilo_keys.py -q && .venv/bin/pytest -q`
Expected: PASS — the two new pty tests plus every existing Stage-0..6 pty flow still green under the color draw. (If a pty test is flaky on timing, bump its `timeout`; do not weaken an assertion.)

- [ ] **Step 5: Commit**

```bash
git add examples/kilo/Find.thx examples/kilo/sample.c tests/test_kilo_keys.py
git commit -m "kilo: Stage 7 — Find HL_MATCH overlay wiring + sample.c + syntax/match pty smoke"
```

---

### Task 7: Finalize — roadmap, CHANGELOG, full green

**Files:**
- Modify: `docs/specs/2026-07-16-kilo-port-roadmap.md` (mark Stage 7 done)
- Modify: `CHANGELOG.md` (`[Unreleased]` entry)

- [ ] **Step 1: Mark the roadmap done** — in `docs/specs/2026-07-16-kilo-port-roadmap.md`, update the Status line and the build-order block so `Stage 7 Syntax highlighting` shows `✔ DONE` with its artifacts:

```
Stage 7  Syntax highlighting      → adds Bytes.equals -> Bool  ✔ DONE (per-cell Hl classify + ANSI + cross-row comment state + HL_MATCH overlay; examples/kilo/{Syntax,State,Screen,Find}.thx + sample.c; spec/conformance/bytes_equals.thx; tests/test_{bytes_equals,kilo_syntax}.py + updated test_kilo_{state,screen,keys}.py; design 2026-07-17-kilo-stage7-syntax-design.md)
```

Also update the top `**Status:**` sentence to note Stage 7 shipped and **the kilo port is complete**.

- [ ] **Step 2: Add the CHANGELOG entry** — under `## [Unreleased]` in `CHANGELOG.md`, add a bullet:

```markdown
- **Kilo Stage 7 — syntax highlighting** (final stage; completes the port). New `Bytes.equals`
  primitive (structural byte comparison; interp + ffi only). New `examples/kilo/Syntax.thx`:
  an `Hl` sum type + `color`, the C `Syntax` table, `editorUpdateSyntax` as a pure
  `highlightRow` with cross-row multi-line-comment state, and extension-based selection. State
  caches a whole-document highlight fold; Screen interleaves ANSI SGR codes; the deferred Find
  `HL_MATCH` lands as an immutable overlay. `spec/conformance/bytes_equals.thx`; `sample.c`.
```

- [ ] **Step 3: Run the full suite one final time**

Run: `.venv/bin/pytest -q`
Expected: PASS, ~**600** tests (578 baseline + 7 equals + ~10 syntax + ~5 state + 3 screen + 2 pty). Confirm the tree is clean apart from these changes: `git status`.

- [ ] **Step 4: Commit**

```bash
git add docs/specs/2026-07-16-kilo-port-roadmap.md CHANGELOG.md
git commit -m "kilo: Stage 7 done — syntax highlighting; the kilo port is complete"
```

---

## Self-Review notes (for the executor)

- **Branch order in `highlightRow` is load-bearing** (scs → ml-comment → string → number → keyword → default); do not reorder.
- **`cells.count == i` invariant**: every branch appends exactly as many cells as it advances `i`. If a class string is one letter short/long, a branch appended the wrong count.
- **`refreshHighlight()` must run after every `textRows`/`filename` mutation** — the missing-site bug is exactly what `test_edit_updates_the_highlight` guards.
- **Exact-byte Screen tests**: the per-row `\x1b[39m` reset is new output on every drawn file row; update any exact-frame expected strings rather than the code.
- **pty timing**: prefer bumping `timeout`/`winsize` over weakening a color assertion.
