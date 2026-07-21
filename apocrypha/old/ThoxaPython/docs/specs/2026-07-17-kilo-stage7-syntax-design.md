# Thoxa × Kilo — Stage 7: Syntax highlighting (+ the deferred Find match highlight)

- **Date:** 2026-07-17
- **Status:** Approved design forks; full design pending Joshua's spec review. Stage 7 of the
  kilo-port roadmap (`2026-07-16-kilo-port-roadmap.md`) — the **final** stage. Follows Stage 6
  (Find + the modal prompt).
- **Lands one language increment:**
  1. **`Bytes.equals(other: Bytes) -> Bool`** (§2) — structural byte-sequence comparison, the
     Thoxa answer to `strncmp`/`strcmp`. `Bytes == Bytes` is *identity* today (no `__eq__`),
     so keyword / `//` / `/*` / `*/` / extension matching has no primitive. Interpreter +
     `ffi.py` only — **no checker change** (mirrors `Bytes.find`'s footprint exactly).
- **Introduces two new modules** — `examples/kilo/Syntax.thx` (the classifier: an `Hl`
  highlight-class sum type, the C `Syntax` table, `is_separator`, the `editorUpdateSyntax`
  state machine as a pure `highlightRow`, extension→syntax selection, and `Hl`→ANSI color) and
  a `.c` sample so the feature is actually exercised (`thoxa-demo-is-means`).
- **Delivers the deferred Stage-6 `HL_MATCH`** search highlight — and does it *better than kilo*:
  because rows are immutable and the highlight is derived, the match is an **overlay parameter**
  to the draw, not kilo's `malloc`/`memcpy`/`memset`/restore of a mutable `hl[]` (§5.3).

## 1. Context & goal

Stage 6 made kilo navigable. Stage 7 is the last piece: **color**. kilo's
`editorUpdateSyntax` walks a row's rendered bytes and classifies each cell into a highlight
class (single-/multi-line comment, keyword-1, keyword-2, string, number, or normal), tracking
three pieces of state as it goes — `prev_sep` (was the previous byte a separator?), `in_string`
(the open quote byte, or 0), and `in_comment` (are we inside a `/* … */` that opened on an
earlier row?). `editorSyntaxToColor` maps the class to a VT100 SGR code; `editorDrawRows`
emits those codes as it paints, minimizing escapes by tracking the current color;
`editorSelectSyntaxHighlight` picks the language by the filename's extension; and **Find**
temporarily paints the matched substring `HL_MATCH` (blue), saving and restoring the row's
real highlight around the search.

Almost all of that is expressible in Thoxa today. The state machine is byte comparisons and
ranges; `is_separator`'s punctuation set is an `Array<Byte>.contains` (that method already
exists); a row's per-cell classes are an `Array<Hl>` built with `.append` (also already
exists — verified in a scratch run). The **one** thing the language cannot do is **compare two
byte sequences** — and keyword matching (`strncmp(&render[i], kw, klen)`), comment-delimiter
matching (`//`, `/*`, `*/` at a position), and extension matching (`".c"`) all need it. That
is Stage 7's language increment, the same shape of gap `Bytes.find` filled in Stage 6.

### 1.1 Design-fork decisions (Joshua)

| Fork | Options weighed | Decision |
| --- | --- | --- |
| **`Hl` representation** | sum type + color match / valued nominal types (`type X 31`) / raw `Bytes` of codes | **Sum type + a separate `color` match.** The most idiomatic (closed union, exhaustive `match`); the ANSI mapping is a *rendering* concern kept out of the type. Building `Array<Hl>` with `.append` is clean now. |
| **Byte comparison** | `Bytes.equals` method / structural `==` operator / positional `startsWith` / compose `slice`+`find` | **`Bytes.equals(other) -> Bool`.** The fundamental missing operation, as a named method — interp+ffi only, no checker change (mirrors `find`). Leaves `==`-on-heap-types a deliberate future call rather than a side effect of this stage. |

### 1.2 Calls made within those (mine, faithful to the model)

- **Highlight lives in `State` as a derived cache** (`Array<RowHl>`), recomputed by a single
  `refreshHighlight()` fold whenever the document or filename changes (§4). `Screen` stays a
  pure reader. This is a **whole-document fold** carrying the multi-line-comment-open flag from
  row 0 — which is *why* it needs no kilo-style "recurse into the next row when the flag flips":
  the fold recomputes every row's comment state from the top, so cross-row propagation is
  automatic (§3.4).
- **The match highlight is an overlay, not a save/restore** (§5.3).
- **Flags** (`HL_HIGHLIGHT_NUMBERS | HL_HIGHLIGHT_STRINGS`) → two plain `Bool` fields on the
  `Syntax` record. Combinable bitflag-enums stay deferred: the one syntax (C) always sets both,
  so there is no forcing use this stage.
- **`HL_COMMENT` and `HL_MLCOMMENT` are merged** into one `Comment` class. kilo keeps them as
  distinct enum values but they share a color (cyan) and nothing downstream branches on the
  distinction (`prev_hl` is only ever compared to `HL_NUMBER`), so one class is behavior-
  equivalent and simpler.
- **Scope:** port kilo's C syntax faithfully (its keyword set + `//`/`/*`/`*/`); ship a small
  `examples/kilo/sample.c` so highlighting is visibly exercised end-to-end.

## 2. Language increment — `Bytes.equals(other: Bytes) -> Bool`

### 2.1 What forces it

kilo classifies a keyword with `!strncmp(&row->render[i], keywords[j], klen)` and a comment
delimiter with `!strncmp(&row->render[i], scs, scs_len)` — "do the bytes at position `i` equal
this needle?" Extension selection is `!strcmp(ext, ".c")`. All three are byte-sequence
equality. Today `Bytes` defines no `__eq__`, so the interpreter's `_equal` (interp.py:1613)
compares two `Bytes` by **object identity** — two distinct buffers with identical content are
`!=`. A byte buffer you can search (`find`, Stage 6) but cannot compare is an odd gap; closing
it is the same "grow the real primitive" reasoning, not a kilo-only convenience
(`thoxa-demo-is-means`).

### 2.2 The decision

```thoxa
let a = Bytes.fromString("int")
let b = Bytes.fromString("int")
a.equals(b)                       # true  (structural, content-wise)
a.equals(Bytes.fromString("in")) # false (different length)
```

- Returns `true` iff `other` has the same length and the same bytes, **length-counted** so an
  embedded NUL is compared like any other byte (consistent with `find`/`slice`; `Bytes` is not
  NUL-terminated internally).
- A **method**, not the `==` operator. Making `==` structural for `Bytes` would raise the same
  question for `Buffer` and `String` (a broader semantic decision about heap-type equality)
  and is out of scope; `equals` is the minimal, unambiguous primitive.

### 2.3 Implementation — interpreter + `ffi.py`, no checker change

**`thoxa/ffi.py`** — one method on the `Bytes` class, beside `find`:

```python
def equals(self, other: "Bytes") -> bool:
    """True iff `other` has the same length and bytes (length-counted)."""
    return self._buf.raw[: self._len] == other._buf.raw[: other._len]
```

**`thoxa/interp.py`** — an `equals` branch in `_bytes_method` (beside the `find` branch), plus
`"equals"` in the `Bytes` member allow-list in `_member`:

```python
if name == "equals":
    if len(values) != 1 or not isinstance(values[0], Bytes):
        self.error("Bytes.equals(other) expects a Bytes buffer", span)
    return buf.equals(values[0])
# _member (interp.py:1498):
#   name in ("get","set","toString","slice","insertAt","removeAt","find","equals")
```

**No checker change.** The checker types every `Bytes` member as `UNKNOWN` (only the
interpreter validates member names), so `a.equals(b)` is `UNKNOWN` — usable directly as an
`if` condition (an `UNKNOWN` is accepted where a `Bool` is expected, exactly as
`fname.count == 0` and the other `Bytes` methods already are). Precise `Bool` typing (for
inferred hover) is a deferred nicety, consistent with `find` and the rest of the `Bytes`
surface.

### 2.4 Conformance & tests

- **`spec/conformance/bytes_equals.thx` (+ `.out`)** — the "done" gate: equal buffers → true;
  differing content, same length → false; different lengths → false; empty == empty → true;
  a single-byte pair; a `fromString` buffer vs the same string; and **embedded-NUL** equality
  (two buffers that agree past a `\x00` are equal — length-counted, not cut at the NUL).
- **`tests/test_bytes_equals.py`** — the same at the Python layer, plus the non-`Bytes`
  argument trap (`a.equals(5)` errors cleanly) and the embedded-NUL case built via `Buffer`.
- **`spec/SPEC.md` §9.1** gains `.equals(other) -> Bool` in the `Bytes` method list.

## 3. `examples/kilo/Syntax.thx` — the classifier

A new module. Pure functions and data — no terminal, no singleton — so the whole state machine
is unit-tested deterministically (the Stage-4/5/6 "extract the testable core" seam). It imports
only `Row` (for the `RowHl` cells to line up with a row's `render`).

### 3.1 The `Hl` highlight class (the chosen sum type)

```thoxa
type Normal {}    | Comment {}  | Keyword1 {}
   | Keyword2 {}  | Str {}      | Number {}  | Matched {}
type Hl = Normal | Comment | Keyword1 | Keyword2 | Str | Number | Matched
```

`Matched` (kilo's `HL_MATCH`) is named to avoid colliding with `Find.Match`. Color is a
rendering concern kept out of the type — one exhaustive match:

```thoxa
## kilo editorSyntaxToColor. Normal is handled by the emitter (§5.2), not here — it maps to
## the default-foreground reset, not a color; this returns the SGR code for a *colored* class.
export fn color(h!: Hl) -> Int64 {
    match h {
        Number { 31 }   Str { 35 }      Comment { 36 }
        Keyword1 { 33 } Keyword2 { 32 } Matched { 34 }
        Normal { 39 }                   # 39 = default fg; the emitter special-cases it
    }
}
```

### 3.2 The `Syntax` table (C, data-driven)

```thoxa
## A language's highlighting rules (kilo struct editorSyntax). Immutable record: ctor params
## private, projected to public reads. Keyword lists are Array<Bytes>; kilo's trailing-`|`
## keyword-2 marker is replaced by two explicit lists (kw2 = types → green).
export type Syntax(
    keywords1!: Array<Bytes>, keywords2!: Array<Bytes>,
    single!: Bytes, mlStart!: Bytes, mlEnd!: Bytes,
    numbers!: Bool, strings!: Bool
) {
    export let kw1 = keywords1        export let kw2 = keywords2
    export let scs = single           export let mcs = mlStart   export let mce = mlEnd
    export let hlNumbers = numbers     export let hlStrings = strings
}

## kilo's C_HL_keywords / C_HL_extensions, as one builder.
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

## kilo editorSelectSyntaxHighlight: pick the language by the filename's extension. One C
## entry today → Has(cSyntax()) iff the name ends with .c / .h / .cpp, else Not.
export fn selectSyntax(filename!: String) -> May<Syntax> {
    let name = Bytes.fromString(filename)
    if endsWith(name, Bytes.fromString(".c"))   { return Has(cSyntax()) }
    if endsWith(name, Bytes.fromString(".h"))   { return Has(cSyntax()) }
    if endsWith(name, Bytes.fromString(".cpp")) { return Has(cSyntax()) }
    Not()
}
```

### 3.3 Byte helpers (all pure, all unit-tested)

```thoxa
## Do the bytes of `hay` at index `at` equal `needle`? (kilo !strncmp.) The new Bytes.equals,
## guarded so an out-of-range window is simply "no match" rather than a slice trap.
fn matchesAt(hay!: Bytes, at!: Int64, needle!: Bytes) -> Bool {
    if at + needle.count > hay.count { return false }
    hay.slice(at, at + needle.count).equals(needle)
}

## endsWith for the extension test.
fn endsWith(hay!: Bytes, suffix!: Bytes) -> Bool {
    if suffix.count > hay.count { return false }
    matchesAt(hay, hay.count - suffix.count, suffix)
}

## kilo is_separator: whitespace, NUL, or one of the C punctuation bytes. Array<Byte>.contains.
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

## The byte at `at` in `render` treated as a separator, where end-of-line counts as one (kilo
## reads render's trailing NUL, and is_separator('\0') is true). Used for the keyword boundary.
fn separatorAt(render!: Bytes, at!: Int64) -> Bool {
    if at >= render.count { true } else { isSeparator(render.get(at)) }
}

fn isDigit(c!: Byte) -> Bool { if c < b'0' { false } else { c <= b'9' } }
fn isNumberHl(h!: Hl) -> Bool { match h { Number { true } else { false } } }
```

### 3.4 The state machine — `highlightRow` (faithful `editorUpdateSyntax`)

Operates on a row's **`render`** (the tab-expanded bytes `Screen` actually paints, so the
classes line up with what's drawn), carrying in the previous row's open-comment state and
returning this row's cells **plus** its own open-comment state for the next row:

```thoxa
## One row's highlight cells parallel to `render`, plus whether the row ends inside an open
## /* … */ (kilo's hl_open_comment) — the only state that crosses rows.
export type RowHl(cellList!: Array<Hl>, openFlag!: Bool) {
    export let cells = cellList
    export let openComment = openFlag
}

## kilo editorUpdateSyntax. `render` = the row's rendered bytes; `prevOpen` = the previous
## row's openComment (false for row 0); `s` = the active Syntax. Returns cells (one Hl per
## render byte) + this row's openComment. The invariant `cells.count == i` holds every
## iteration: each branch appends exactly as many cells as it advances `i`.
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

        ## 1. single-line comment: // to end of line (unless in a string/mlcomment)
        if inString == 0 {
            if inComment == false {
                if matchesAt(render, i, s.scs) {
                    loop { if i >= n { break }  cells = cells.append(Comment())  i = i + 1 }
                    continue                    # i == n → the outer loop then breaks
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

        ## 3. strings (kilo HL_HIGHLIGHT_STRINGS): track the open quote; \ escapes the next byte
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

        ## 4. numbers (kilo HL_HIGHLIGHT_NUMBERS): a digit after a separator or another digit;
        ## a '.' continuing a number
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

        ## 5. keywords — only at a separator boundary on both sides (kilo's prev_sep gate +
        ## is_separator(render[i+klen]))
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

## A matched keyword: its byte length + whether it is a type keyword (kw2 → green).
type Keyword(length!: Int64, kw2Flag!: Bool) {
    export let len = length      export let isKw2 = kw2Flag
}

## The first keyword (kw1 then kw2) whose bytes sit at `i` bounded by a separator, else Not.
fn keywordAt(render!: Bytes, i!: Int64, s!: Syntax) -> May<Keyword> {
    match firstMatch(render, i, s.kw1) { Has(w) { return Has(Keyword(w, false)) } Not { Void } }
    match firstMatch(render, i, s.kw2) { Has(w) { return Has(Keyword(w, true))  } Not { Void } }
    Not()
}

## The byte-length of the first keyword in `list` that matches at `i` with a separator after it.
fn firstMatch(render!: Bytes, i!: Int64, list!: Array<Bytes>) -> May<Int64> {
    for kw in list {
        if matchesAt(render, i, kw) {
            if separatorAt(render, i + kw.count) { return Has(kw.count) }
        }
    }
    Not()
}

## No active syntax → every cell Normal, never in a comment (kilo E.syntax == NULL).
export fn plainRow(render!: Bytes) -> RowHl {
    mut cells: Array<Hl> = []
    mut i = 0
    loop { if i >= render.count { break }  cells = cells.append(Normal())  i = i + 1 }
    RowHl(cells, false)
}
```

The one deviation from kilo is structural, not behavioral: kilo mutates a preallocated
`hl[i]`; we **append** to a growing `Array<Hl>`, maintaining `cells.count == i`. Verified
against a scratch build that `Array<Hl>` appends, indexes, and pattern-matches as needed.

## 4. `examples/kilo/State.thx` — the derived highlight cache

State gains the active syntax, the per-row highlight cache, and the transient search-match
span — all module-private, reached through exported functions (the established singleton
pattern). `Row` gains a public `renderBytes()` so the fold can classify the full rendered line.

```thoxa
## One selective import (functions called unqualified + the types/ctor), avoiding a
## namespace+selective double-import of the same module.
import { Hl, RowHl, Syntax, Matched, highlightRow, plainRow, selectSyntax } from "./Syntax"

mut activeSyntax: May<Syntax> = Not()   # chosen by filename (selectSyntax)
mut rowHls: Array<RowHl> = []           # one per file row, parallel to textRows
mut matchRow = -1                       # the Find match's file row (-1 = none)
mut matchCol = -1                       # its byte column into that row's chars
mut matchLen = 0                        # the query's byte length

## Recompute every row's highlight, folding the multi-line-comment-open flag from row 0. Called
## by every mutation of textRows / filename (setRows, the edit ops, setFilename) — the single
## invariant: derived highlight never outlives a document change.
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
```

Wiring: `setRows(rows, name)` and `setFilename(name)` set `filename`, then
`activeSyntax = selectSyntax(name)` and call `refreshHighlight()`. Each editing op
(`insertChar`, `insertNewline`, `deleteChar`) ends with `refreshHighlight()` — cheap for a
demo's small files, and cross-row-correct because the fold restarts from row 0 (typing `/*`
re-colors every following row until a `*/`, with no kilo-style recursion). Screen reads:

```thoxa
## The cells for a file row, with the Find match overlaid (kilo's HL_MATCH). Overlay, not
## save/restore: rows are immutable, so this derives a fresh Array<Hl> for the frame.
export fn rowCells(filerow!: Int64) -> Array<Hl> {
    let base = rowHls[filerow].cells
    if filerow == matchRow { overlayMatch(base, row(filerow)) } else { base }
}

## Paint render cells [cxToRx(matchCol) .. cxToRx(matchCol+matchLen)) as Matched. The match
## column is a *chars* offset (Stage 6 searches chars); cxToRx maps it to render space so it
## aligns with the drawn, tab-expanded bytes.
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

## Find sets/clears the highlighted match as the cursor jumps (§6).
export fn setMatch(row!: Int64, col!: Int64, length!: Int64) -> Void {
    matchRow = row  matchCol = col  matchLen = length  Void
}
export fn clearMatch() -> Void { matchRow = -1  matchCol = -1  matchLen = 0  Void }
```

## 5. `examples/kilo/Screen.thx` — the SGR draw loop

### 5.1 What changes

Today each visible file row is drawn with one `buf.appendBytes(row.window(coloff, cols))`.
Stage 7 replaces that with a per-cell walk that interleaves SGR color codes, faithful to
kilo's `editorDrawRows` color tracking. Because `Buffer` is value-typed (a callee can't append
into the caller's frame — the established Screen constraint), the row is built in a helper that
**returns `Bytes`**, like `statusBarBytes` / `messageBarBytes`:

### 5.2 The helper

```thoxa
import { color, Normal } from "./Syntax"

let sgrReset = "\x1b[39m"          # default foreground (kilo's per-row reset)

## One visible file row, render bytes interleaved with SGR color changes (kilo editorDrawRows).
## `cells` (full-render, match already overlaid) is indexed from `begin` — the same clamp
## Row.window uses — so bytes and classes stay aligned.
fn drawRowBytes(filerow!: Int64, coloff!: Int64, cols!: Int64) -> Bytes {
    let r = state.row(filerow)
    let visible = r.window(coloff, cols)          # the drawn bytes (already windowed)
    let cells = state.rowCells(filerow)           # full render, match overlaid
    let count = r.renderCount()
    let begin = if coloff > count { count } else { coloff }

    mut out = Buffer()
    mut currentColor = -1                          # -1 = "default fg", kilo's sentinel
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
    out.appendBytes(Bytes.fromString(sgrReset))    # leave fg default for the next line
    out.freeze()
}
```

`refresh()`'s row branch becomes `buf.appendBytes(drawRowBytes(filerow, coloff, cols))`.
Everything else in `refresh()` (welcome banner, `~` rows, status/message bars, cursor) is
unchanged; the `Screen` control-sequence header comment gains the SGR-color note.

### 5.3 Why the match highlight is *cleaner* than kilo here

kilo paints `HL_MATCH` by mutating the row's `hl[]` in place, having first `malloc`'d a copy of
the original and `memcpy`'d it back on ESC/Enter/next-search — a save/restore dance with an
easy-to-leak `saved_hl`. Our rows are immutable and the highlight is derived, so there is
nothing to save: `State.rowCells` overlays `Matched` onto a **fresh** `Array<Hl>` for the frame
(§4), and clearing the match (`clearMatch`) makes the next frame's derivation plain again. The
immutable model turns a stateful save/restore into a pure overlay — a small but real payoff.

## 6. `examples/kilo/Find.thx` — wiring the match highlight

Stage 6's `Find.run` already jumps the cursor to each match; Stage 7 additionally records the
match span so Screen colors it, and clears it when the search ends. Three lines change in the
existing loop:

- on a successful re-search, beside `state.jumpTo(m.col, m.row)`, add
  `state.setMatch(m.row, m.col, query.count)`;
- **ESC** and **Enter** (the two `break`s) each `state.clearMatch()` before breaking (ESC also
  keeps its existing `restoreView`);
- the terminal-error `break`s clear the match too, so a failed search leaves no stray highlight.

No new language surface — `setMatch`/`clearMatch` are plain State mutations. The match column is
the row's byte (chars) column Stage 6 already computes; §4's `overlayMatch` maps it to render
space.

## 7. `examples/kilo/Main.thx` — selection is automatic

No structural change. `state.setRows(rows, path)` already runs on load, and §4 makes it select
the syntax and refresh the highlight; the save-as `state.setFilename(name)` does the same, so
naming a new buffer `foo.c` lights it up. The startup hint is unchanged. (kilo calls
`editorSelectSyntaxHighlight` in both `editorOpen` and `editorSave`; folding it into
`setRows`/`setFilename` is the same two call sites.)

## 8. `examples/kilo/sample.c` — the exercised sample

A small C file that hits every class, so the pty smoke sees real color:

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

It exercises every class in the real editor: the top **multi-line `/* … */`** (the cross-row
`openComment` fold — rows 2–3 are colored comment because row 1 left the comment open), a
single-line `//`, keyword-1 control words (`for`, `return`), keyword-2 types (`int`, `char`),
decimal numbers (`42`, `0`), and a `"world"` string with an escape (`\n`). `sample.txt` stays
for the plain-text (no-syntax) path.

## 9. Verification

Per the roadmap discipline (**design → doc + `SPEC.md` → conformance test → TDD → green →
commit**), Stage 7 is "done" only when these are green:

1. **`spec/conformance/bytes_equals.thx` + `.out`** — the `Bytes.equals` gate (§2.4). This is
   the stage's conformance addition — the *language increment*, standalone. (The conformance
   harness runs each program **standalone**, and imports may not use `..`, so — as with Stage 6,
   where `searchFrom` was gated by `test_kilo_find.py` and only `bytes_find` got a conformance
   file — the `Syntax` module's state machine is gated deterministically in
   `test_kilo_syntax.py` (item 3) rather than a conformance file that would duplicate all ~150
   lines of `highlightRow`.)
2. **`tests/test_bytes_equals.py`** — §2.4 at the Python layer (+ the non-`Bytes` trap).
3. **`tests/test_kilo_syntax.py`** — the module type-checks; `selectSyntax` by extension
   (`.c`/`.h`/`.cpp` → `Has`, `.txt`/no-ext → `Not`); `plainRow` all-Normal; and `highlightRow`
   as a pure function over crafted lines with `cSyntax`, asserted via a compact per-cell class
   letter (`N`/`C`/`1`/`2`/`S`/`#`): a keyword bounded by separators (and a non-boundary
   near-keyword `ifx` left Normal), a kw2 type, a decimal number (and a digit inside an
   identifier left Normal), a `"string"` with a `\` escape, a `//` line comment to
   end-of-line, a `/*…*/` block **within** a line, and — the cross-row case — a line that
   **opens** a block comment (`openComment == true`) feeding the next row (`prevOpen == true`
   → `Comment` until `*/`). The keyword boundary exercises `isSeparator` indirectly.
4. **`tests/test_kilo_state.py`** (extended) — `setRows(..., "x.c")` makes `rowCells` reflect
   C highlighting; an **edit updates the highlight** (type `/*` → following rows recolor — this
   catches a missed `refreshHighlight()` call site); `setFilename("x.c")` enables it for an
   unnamed buffer; `setMatch`/`clearMatch` overlay and remove `Matched` on the right cells;
   a plain-text name leaves every cell Normal.
5. **`tests/test_kilo_screen.py`** (updated) — `drawRowBytes` emits the expected SGR for a
   colored row (a number → `\x1b[31m`, a keyword → `\x1b[33m`/`32m`), emits **no** color for an
   all-Normal row, and paints a set match `\x1b[34m` (blue).
6. **`tests/test_kilo_keys.py`** (updated pty smoke) — open `sample.c` in a real pty and assert
   the frame contains the syntax SGR codes (a red number, a cyan comment, a yellow/green
   keyword, a magenta string); a **Ctrl-F** for a term that occurs in the file shows the blue
   `\x1b[34m` match; and every Stage-0..6 flow still holds under the color draw.
7. **`spec/SPEC.md`** §9.1 updated (§2.3); **roadmap** Stage-7 line marked done with this doc +
   the conformance test; **`CHANGELOG.md`** `[Unreleased]` entry — added once green. This
   **completes the kilo port** (all 10 modules, all 5 forks resolved).

## 10. Out of scope / deferred

- **Combinable bitflag enums** — the `Syntax` flags are two plain `Bool`s; a real OR-able
  bitset enum stays deferred (no forcing use: one syntax, both flags always on). LF-A unblocked
  it; it awaits a demo that actually combines flags.
- **`HL_COMMENT` vs `HL_MLCOMMENT` as distinct classes** — merged into `Comment` (same color,
  no downstream branch). If a theme ever colored them differently, split the variant.
- **Non-printable render bytes as inverted `^X`** — kilo renders control bytes specially in the
  same routine; our rows are plain text (tabs already expanded to spaces in `render`), so this
  is skipped. Add a `Screen` branch if a binary file is ever opened.
- **Incremental re-highlight** — kilo re-highlights only the changed row and recurses forward on
  a comment-flag flip; we recompute the whole document per change (simpler, cross-row-correct,
  fine for a demo). The incremental version is a native/perf-pass concern.
- **Precise `Bool` typing of `Bytes.equals`** in the checker (for inferred hover) — stays
  `UNKNOWN` like the other `Bytes` methods.
- **Multi-language HLDB** — one C `Syntax`; more languages are just more `selectSyntax` entries,
  no new machinery.
```
