# Bitwise Operators (LF-A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add C-style bitwise operators (`& | ^ ~ << >>`) to Thoxa — lexer, parser (with precedence and the `>>`/nested-generic disambiguation), interpreter, checker, spec, and a conformance test.

**Architecture:** Extend the existing hand-written lexer/Pratt-parser/tree-walking-interpreter pipeline. Four new left-associative precedence tiers slot into the parser's descent chain between `p_range` and `p_add`; unary `~` joins `p_unary`. The lexer's `>>` becomes a single `SHR` token, so the type-argument parser gains a backtrack-safe "split `>>` into two `>`" step to keep nested generics (`Array<Array<T>>`) parsing. Interp and checker gain integer-only branches for the new ops.

**Tech Stack:** Python 3.13, pytest. Zero runtime dependencies (stdlib only). Source in `thoxa/`, tests in `tests/`, executable spec in `spec/conformance/`.

## Global Constraints

- **Zero runtime deps** — stdlib only; add no imports beyond what's already used.
- **Design source of truth:** `docs/specs/2026-07-16-thoxa-bitwise-operators-design.md`. Operators `& | ^ ~ << >>`, C-style symbols. Precedence, low→high: `or` < `and` < `== !=` < `< <= > >=` < `.. ..=` < `|` < `^` < `&` < `<< >>` < `+ -` < `* / %` < unary(`not - ~`) < postfix. Operand types: integers only (`Int8..Int64`, `UInt8..UInt64`, `Byte`) — not `Bool`/`Float`/`Char`/`String`. Runtime = Python int ops; width-truncation happens only at typed/FFI boundaries.
- **Casing is hard-enforced:** values/functions camelCase, types PascalCase (irrelevant to operator symbols, but keep any new Thoxa identifiers compliant).
- **Statement boundaries:** the grammar is newline-insensitive; a statement must not *start* with a binary operator. Keep every bitwise expression in the conformance program on one line.
- **Commits:** commit to `master` after each task. Every commit message ends with the repo's two trailer lines (`Co-Authored-By:` and `Claude-Session:`).
- **"Done" = a `spec/conformance/` test exercises the feature and the whole `pytest` suite is green.**

---

### Task 1: Lexer — tokenize `& ^ ~ << >>`

**Files:**
- Modify: `thoxa/tokens.py` (class `T`, operators section ~line 46-79)
- Modify: `thoxa/lexer.py` (`_TWO` line 259-262, `_ONE` line 263-268)
- Create: `tests/test_bitwise.py`

**Interfaces:**
- Produces: token kinds `T.AMP` (`&`), `T.CARET` (`^`), `T.TILDE` (`~`), `T.SHL` (`<<`), `T.SHR` (`>>`). `T.PIPE` (`|`) already exists and is reused. `tokenize(src)` returns `list[Token]` (with a trailing `T.EOF`).

- [ ] **Step 1: Write the failing test**

Create `tests/test_bitwise.py`:

```python
import pytest

from thoxa import ast
from thoxa.checker import check
from thoxa.errors import ThoxaError
from thoxa.interp import run
from thoxa.lexer import tokenize
from thoxa.parser import parse
from thoxa.tokens import T


def kinds(src):
    return [t.kind for t in tokenize(src) if t.kind is not T.EOF]


def run_out(src):
    buf = []
    code = run(src, "<test>", out=buf.append)
    return "".join(buf), code


def _prog(body):
    return "fn main() -> Int64 {\n    " + body + "\n    0\n}\n"


def test_lexer_emits_and_or_xor():
    assert kinds("a & b | c ^ d") == [
        T.LOWER, T.AMP, T.LOWER, T.PIPE, T.LOWER, T.CARET, T.LOWER,
    ]


def test_lexer_shifts_are_single_tokens():
    assert kinds("x << 2 >> y") == [T.LOWER, T.SHL, T.INT, T.SHR, T.LOWER]


def test_lexer_tilde_and_munch_vs_le_ge():
    assert kinds("~a") == [T.TILDE, T.LOWER]
    # maximal munch must still prefer <= / >= before << / >>
    assert kinds("a <= b << c >= d >> e") == [
        T.LOWER, T.LE, T.LOWER, T.SHL, T.LOWER, T.GE, T.LOWER, T.SHR, T.LOWER,
    ]
```

- [ ] **Step 2: Run test to verify it fails**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q`
Expected: FAIL/ERROR — `AttributeError: AMP` (the token members don't exist yet).

- [ ] **Step 3: Add the token kinds**

In `thoxa/tokens.py`, in the `# operators & punctuation` section of `class T`, immediately after `PIPE = "|"`, add:

```python
    AMP = "&"
    CARET = "^"
    TILDE = "~"
    SHL = "<<"
    SHR = ">>"
```

- [ ] **Step 4: Scan the new operators**

In `thoxa/lexer.py`, add `<<`/`>>` to the two-char table and `&`/`^`/`~` to the one-char table:

```python
    _TWO = {
        "==": T.EQEQ, "!=": T.NEQ, "<=": T.LE, ">=": T.GE, "->": T.ARROW,
        "+=": T.PLUSEQ, "-=": T.MINUSEQ, "*=": T.STAREQ, "/=": T.SLASHEQ, "..": T.DOTDOT,
        "<<": T.SHL, ">>": T.SHR,
    }
    _ONE = {
        "+": T.PLUS, "-": T.MINUS, "*": T.STAR, "/": T.SLASH, "%": T.PERCENT,
        "<": T.LT, ">": T.GT, "=": T.EQ, "?": T.QUESTION, "|": T.PIPE, "!": T.BANG,
        ".": T.DOT, "(": T.LPAREN, ")": T.RPAREN, "[": T.LBRACKET, "]": T.RBRACKET,
        "{": T.LBRACE, "}": T.RBRACE, ",": T.COMMA, ":": T.COLON,
        "&": T.AMP, "^": T.CARET, "~": T.TILDE,
    }
```

(The `operator()` method already tries three-, then two-, then one-char matches — maximal munch — so `<<` is preferred over `<`, and `<=` over `<<` only when the second char is `=`.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add thoxa/tokens.py thoxa/lexer.py tests/test_bitwise.py
git commit -m "lexer: tokenize bitwise operators & ^ ~ << >>"
```

---

### Task 2: Parser — precedence tiers, unary `~`, and the `>>` / nested-generic split

**Files:**
- Modify: `thoxa/parser.py` (`__init__` 21-24; `parse_type` 285-307; `p_range` 341-347; `p_unary` 363-370; `p_postfix` LT branch 403-410; `try_type_args` 415-426)
- Modify: `tests/test_bitwise.py`

**Interfaces:**
- Consumes: `T.AMP/CARET/TILDE/SHL/SHR/PIPE` (Task 1); `ast.BinaryOp(op, left, right, span)` and `ast.UnaryOp(op, operand, span)` (op is the token's `.value` string, e.g. `"&"`, `"<<"`, `"~"`); `Token` (already imported in parser.py).
- Produces: expression parses in which `& | ^ << >>` build `ast.BinaryOp` and `~` builds `ast.UnaryOp`; nested generic type annotations parse; the token stream is left intact when a speculative generic-call parse backtracks.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_bitwise.py`:

```python
def test_bitand_binds_tighter_than_bitor():
    top = parse("let z = a | b & c").body[0].value
    assert isinstance(top, ast.BinaryOp) and top.op == "|"
    assert isinstance(top.right, ast.BinaryOp) and top.right.op == "&"


def test_bitwise_binds_tighter_than_comparison():
    top = parse("let z = a & b == c").body[0].value
    assert top.op == "==" and isinstance(top.left, ast.BinaryOp) and top.left.op == "&"


def test_shift_binds_looser_than_add():
    top = parse("let z = 1 << 2 + 3").body[0].value
    assert top.op == "<<" and isinstance(top.right, ast.BinaryOp) and top.right.op == "+"


def test_unary_tilde_parses():
    node = parse("let z = ~a").body[0].value
    assert isinstance(node, ast.UnaryOp) and node.op == "~"


def test_nested_generic_annotation_parses():
    m = parse("let m: Array<Array<Int64>> = [[1]]")
    assert len(m.body) == 1


def test_generic_call_still_parses():
    call = parse("mut s = Stack<Int64>(capacity: 2)").body[0].value
    assert isinstance(call, ast.Call) and [t.name for t in call.type_args] == ["Int64"]


def test_shift_survives_generic_backtrack():
    # 'Foo < Bar >> c' is not a generic call; the speculative type-args attempt
    # splits the '>>' then rejects, so the '>>' must be restored as a shift.
    top = parse("let z = Foo < Bar >> c").body[0].value
    assert top.op == "<" and isinstance(top.right, ast.BinaryOp) and top.right.op == ">>"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q`
Expected: FAIL — the parser does not yet consume `|`/`&`/`~` in expressions (parse errors), and `Array<Array<Int64>>` fails to close on the `>>` token.

- [ ] **Step 3: Add the split-undo field**

In `thoxa/parser.py`, `Parser.__init__`, add the last line:

```python
    def __init__(self, tokens: list[Token], source: Source) -> None:
        self.toks = tokens
        self.src = source
        self.pos = 0
        self._split_undo = []
```

- [ ] **Step 4: Insert the four bitwise precedence tiers and unary `~`**

In `thoxa/parser.py`, replace `p_range` and add the four new methods before `p_add`:

```python
    def p_range(self):
        left = self.p_bitor()
        if self.cur().kind in (T.DOTDOT, T.DOTDOTEQ):
            inclusive = self.cur().kind is T.DOTDOTEQ
            sp = self.advance().span
            return ast.RangeExpr(left, self.p_bitor(), inclusive, sp)
        return left

    def p_bitor(self):
        left = self.p_bitxor()
        while self.check(T.PIPE):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_bitxor(), op.span)
        return left

    def p_bitxor(self):
        left = self.p_bitand()
        while self.check(T.CARET):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_bitand(), op.span)
        return left

    def p_bitand(self):
        left = self.p_shift()
        while self.check(T.AMP):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_shift(), op.span)
        return left

    def p_shift(self):
        left = self.p_add()
        while self.cur().kind in (T.SHL, T.SHR):
            op = self.advance()
            left = ast.BinaryOp(op.kind.value, left, self.p_add(), op.span)
        return left
```

Then replace `p_unary` to add the `~` prefix:

```python
    def p_unary(self):
        if self.check(T.NOT):
            sp = self.advance().span
            return ast.UnaryOp("not", self.p_unary(), sp)
        if self.check(T.MINUS):
            sp = self.advance().span
            return ast.UnaryOp("-", self.p_unary(), sp)
        if self.check(T.TILDE):
            sp = self.advance().span
            return ast.UnaryOp("~", self.p_unary(), sp)
        return self.p_postfix()
```

- [ ] **Step 5: Add the backtrack-safe `>>` split and wire it into generic closes**

In `thoxa/parser.py`, add two helpers next to `try_type_args`:

```python
    def _expect_gt(self, msg: str):
        """Close a generic argument list, splitting a `>>` into two `>`.

        With shift operators, `Array<Array<T>>` lexes its trailing `>>` as one
        SHR token. When a generic close wants `>` but finds `>>`, consume one
        `>` and rewrite the token to the leftover `>` for the enclosing list.
        The rewrite is recorded so a speculative caller can undo it."""
        tok = self.cur()
        if tok.kind is T.GT:
            return self.advance().span
        if tok.kind is T.SHR:
            self._split_undo.append((self.pos, tok))
            self.toks[self.pos] = Token(T.GT, ">", tok.span)
            return tok.span
        return self.expect(T.GT, msg)

    def _undo_splits(self, mark: int) -> None:
        while len(self._split_undo) > mark:
            pos, tok = self._split_undo.pop()
            self.toks[pos] = tok
```

In `parse_type`, change the type-argument close (line ~306) from `self.expect(T.GT, "'>' to close type arguments")` to:

```python
            self._expect_gt("'>' to close type arguments")
```

In `try_type_args`, change the close (line ~422) from `self.expect(T.GT, "'>'")` to:

```python
            self._expect_gt("'>'")
```

In `p_postfix`, replace the `elif k is T.LT:` branch so a rejected generic-call attempt also undoes any `>>` split it performed:

```python
            elif k is T.LT:
                saved = self.pos
                mark = len(self._split_undo)
                targs = self.try_type_args()
                if targs is not None and self.check(T.LPAREN):
                    e = self.finish_call(e, targs)
                else:
                    self.pos = saved
                    self._undo_splits(mark)
                    break
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py tests/test_parser.py -q`
Expected: PASS (all of `test_bitwise.py`'s parser tests, and the existing `test_parser.py` — `test_generic_constructor_call_vs_comparison` still passes).

- [ ] **Step 7: Commit**

```bash
git add thoxa/parser.py tests/test_bitwise.py
git commit -m "parser: bitwise precedence tiers, unary ~, backtrack-safe >> generic split"
```

---

### Task 3: Interpreter — evaluate the bitwise operators

**Files:**
- Modify: `thoxa/interp.py` (`_binop` 1463-1489; `eval_UnaryOp` 832-840)
- Modify: `tests/test_bitwise.py`

**Interfaces:**
- Consumes: `ast.BinaryOp`/`ast.UnaryOp` with op strings `"&" "|" "^" "<<" ">>" "~"` (Task 2).
- Produces: integer results; a `ThoxaError` (via `self.error`) on non-integer operands or a negative shift amount.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_bitwise.py`:

```python
def test_interp_masks_and_or_xor():
    out, code = run_out(_prog('println("{0xf0 & 0x0f} {0xf0 | 0x0f} {0b1010 ^ 0b0110}")'))
    assert out == "0 255 12\n" and code == 0


def test_interp_shift_and_tilde_clear():
    out, _ = run_out(_prog('println("{1 << 4} {240 >> 4} {0b1111 & ~0b0100}")'))
    assert out == "16 15 11\n"


def test_interp_precedence_matches_spec():
    out, _ = run_out(_prog('println("{1 << 2 + 3} {8 & 8 == 8}")'))
    assert out == "32 true\n"


def test_interp_bitwise_on_bool_traps():
    with pytest.raises(ThoxaError):
        run_out(_prog("let x = true & 3"))


def test_interp_negative_shift_traps():
    with pytest.raises(ThoxaError):
        run_out(_prog("let x = 1 << -1"))
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q -k interp`
Expected: FAIL — `_binop` reaches `self.error("unknown operator '&'")` (the ops aren't handled).

- [ ] **Step 3: Handle bitwise ops in `_binop`**

In `thoxa/interp.py`, in `_binop`, insert this block immediately after the `if op in ("<", "<=", ">", ">="):` comparison block and **before** the `if not self._numeric(left) or not self._numeric(right):` arithmetic guard:

```python
        if op in ("&", "|", "^", "<<", ">>"):
            if (not isinstance(left, int) or isinstance(left, bool)
                    or not isinstance(right, int) or isinstance(right, bool)):
                self.error(f"'{op}' expects two integers", span)
            if op in ("<<", ">>") and right < 0:
                self.error(f"negative shift amount in '{op}'", span)
            if op == "&":
                return left & right
            if op == "|":
                return left | right
            if op == "^":
                return left ^ right
            if op == "<<":
                return left << right
            return left >> right
```

- [ ] **Step 4: Handle `~` in `eval_UnaryOp`**

In `thoxa/interp.py`, in `eval_UnaryOp`, insert before the final `self.error(f"unknown unary operator ...")`:

```python
        if node.op == "~":
            if isinstance(val, bool) or not isinstance(val, int):
                self.error("unary '~' expects an integer", node.span)
            return ~val
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q`
Expected: PASS (all lexer + parser + interp tests).

- [ ] **Step 6: Commit**

```bash
git add thoxa/interp.py tests/test_bitwise.py
git commit -m "interp: evaluate bitwise & | ^ ~ << >> (integer operands, guard negative shift)"
```

---

### Task 4: Checker — integer-only inference for the bitwise operators

**Files:**
- Modify: `thoxa/checker.py` (`_infer_BinaryOp` 793-815; `_infer_UnaryOp` 828-835)
- Modify: `tests/test_bitwise.py`

**Interfaces:**
- Consumes: `ast.BinaryOp`/`ast.UnaryOp`; module-level `_INT_TYPES` (set of int width names), `_INT_TAG`, `UNKNOWN`, `ty_str(...)`, `self.err(msg, span)` — all already defined in `checker.py`.
- Produces: bitwise op expressions infer to `_INT_TAG`; a diagnostic when an operand is a known non-integer type.

- [ ] **Step 1: Write the failing tests**

Append to `tests/test_bitwise.py`:

```python
def test_checker_accepts_integer_bitwise():
    diags = check("fn main() -> Int64 { let x = (1 << 4) | (0xff & 3)  0 }")
    assert not any("integer" in d.message for d in diags)


def test_checker_rejects_bitwise_on_string():
    diags = check('fn main() -> Int64 { let x = "a" & 3  0 }')
    assert any("integer" in d.message for d in diags)


def test_checker_rejects_tilde_on_bool():
    diags = check("fn main() -> Int64 { let x = ~true  0 }")
    assert any("integer" in d.message for d in diags)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py -q -k checker`
Expected: FAIL — the checker currently routes `&`/`~` through the numeric path, so `"a" & 3` and `~true` are not flagged with an "integer" message.

- [ ] **Step 3: Add the bitwise branch to `_infer_BinaryOp`**

In `thoxa/checker.py`, in `_infer_BinaryOp`, insert immediately after the `if n.op in ("<", "<=", ">", ">="):` block and **before** the `# arithmetic` / string-plus block:

```python
        if n.op in ("&", "|", "^", "<<", ">>"):
            for ty, sub in ((lt, n.left), (rt, n.right)):
                if ty is not UNKNOWN and ty not in _INT_TYPES:
                    self.err(f"'{n.op}' expects integer operands, found {ty_str(ty)}", sub.span)
            return _INT_TAG
```

- [ ] **Step 4: Add the `~` branch to `_infer_UnaryOp`**

In `thoxa/checker.py`, in `_infer_UnaryOp`, insert after the `if n.op == "not":` block and before the `self._require_numeric(ty, n.operand, "-")` line:

```python
        if n.op == "~":
            if ty is not UNKNOWN and ty not in _INT_TYPES:
                self.err(f"'~' expects an integer, found {ty_str(ty)}", n.span)
            return ty if ty in _INT_TYPES else _INT_TAG
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `.venv/bin/python -m pytest tests/test_bitwise.py tests/test_checker.py -q`
Expected: PASS (new checker tests plus the existing `test_checker.py`, including `test_conformance_programs_type_check_clean`).

- [ ] **Step 6: Commit**

```bash
git add thoxa/checker.py tests/test_bitwise.py
git commit -m "checker: integer-only inference for bitwise & | ^ ~ << >>"
```

---

### Task 5: Conformance program + SPEC.md

**Files:**
- Create: `spec/conformance/bitwise.thx`
- Create: `spec/conformance/bitwise.out`
- Modify: `spec/SPEC.md` (§1.6 Operators ~63-77; §7.5 Operators & precedence ~289-302)

**Interfaces:**
- Consumes: everything from Tasks 1-4.
- Produces: an auto-discovered conformance case (`tests/test_conformance.py` finds every `spec/conformance/*.thx`) and updated spec text. This is the feature's "done" gate.

- [ ] **Step 1: Write the conformance program (the failing test)**

Create `spec/conformance/bitwise.thx`:

```
## Bitwise operators: masks, shifts, complement, precedence (tighter than comparison),
## flag-combining, and a nested generic (>> re-split).
fn main() -> Int64 {
    println("{0xf0 & 0x0f} {0xf0 | 0x0f} {0b1010 ^ 0b0110}")
    println("{1 << 4} {240 >> 4} {0b1111 & ~0b0100}")
    println("{1 << 2 + 3} {8 & 8 == 8}")
    let flags = 0x01 | 0x40 | 0x200
    println("{flags}")
    let m: Array<Array<Int64>> = [[1, 2], [3]]
    println("{m[0][1]} {m[1][0]}")
    0
}
```

Create `spec/conformance/bitwise.out` (exact expected stdout):

```
0 255 12
16 15 11
32 true
577
2 3
```

- [ ] **Step 2: Run the conformance test to verify it passes end-to-end**

Run: `.venv/bin/python -m pytest tests/test_conformance.py -q -k bitwise`
Expected: PASS. (If it fails, the `.out` must match byte-for-byte — recompute: `0xf0 & 0x0f`=0, `0xf0 | 0x0f`=255, `0b1010 ^ 0b0110`=12; `1<<4`=16, `240>>4`=15, `0b1111 & ~0b0100`=11; `1 << 2 + 3`=`1<<5`=32, `8 & 8 == 8`→`(8&8)==8`→true; `0x01 | 0x40 | 0x200`=577; `m[0][1]`=2, `m[1][0]`=3.)

- [ ] **Step 3: Update SPEC §1.6 (operators & punctuation)**

In `spec/SPEC.md`, in the `### 1.6 Operators & punctuation` code block, add a bitwise line after the `and or not` logical line, and extend the generic-bracket disambiguation note:

```
&  |  ^  ~  << >>             bitwise: and, or, xor, not (unary), shift left/right
```

and change the existing `<  >  … disambiguated by the parser` note to also read:

```
<  >                          also generic argument brackets (disambiguated by the parser)
<< >>                         shift in expressions; a `>>` closing nested generics is re-split
```

Also note next to the `|` line that `|` doubles as bitwise-or in expression position (union-only inside a `type` alias).

- [ ] **Step 4: Update SPEC §7.5 (precedence)**

In `spec/SPEC.md`, replace the `### 7.5` precedence code block (lowest → highest) with:

```
or
and
== !=
< <= > >=
.. ..=
|
^
&
<< >>
+ -
* / %
unary: not  -  ~          (prefix)
postfix: ?  as  can  is   f(args)  a[i]  a.b
```

and, below the block, append to the prose: "Bitwise operators (`& | ^ ~ << >>`) take integer operands only and bind tighter than comparison — `x & mask == 0` is `(x & mask) == 0`."

- [ ] **Step 5: Run the full suite**

Run: `.venv/bin/python -m pytest -q`
Expected: PASS — the entire suite green, including the new `bitwise` conformance case and `test_checker.py::test_conformance_programs_type_check_clean` (which now also checks `bitwise.thx`).

- [ ] **Step 6: Commit**

```bash
git add spec/conformance/bitwise.thx spec/conformance/bitwise.out spec/SPEC.md
git commit -m "spec: bitwise operators conformance test + SPEC operator/precedence tables"
```

---

## Self-Review

**Spec coverage** (against `docs/specs/2026-07-16-thoxa-bitwise-operators-design.md`):
- §2 operators & spelling → Task 1 (tokens), Task 2 (parse). ✓
- §2 `|` union vs bit-or disambiguation → Task 2 (union stays in `type_decl`; `p_bitor` consumes `PIPE` only in expressions) + `test_generic_call_still_parses`/existing union tests. ✓
- §2 `>>` nested-generic re-split → Task 2 (`_expect_gt` + backtrack undo) + `test_nested_generic_annotation_parses`, `test_shift_survives_generic_backtrack`, conformance `Array<Array<Int64>>`. ✓
- §3 precedence → Task 2 tiers + `test_bitand_binds_tighter_than_bitor`, `test_bitwise_binds_tighter_than_comparison`, `test_shift_binds_looser_than_add`, conformance `1 << 2 + 3`, `8 & 8 == 8`. ✓
- §4 integer operands only + runtime semantics → Task 3 (interp) + Task 4 (checker) + trap/reject tests. ✓
- §4 `~` clear via infinite-precision two's complement → `0b1111 & ~0b0100 == 11`. ✓
- §5 hooks → each named file touched. ✓
- §7 conformance contract → Task 5. ✓
- §6 statement-boundary interaction → honored by keeping conformance expressions single-line (documented in Global Constraints); no code change (correctly out of scope).

**Placeholder scan:** none — every step has concrete code and exact commands.

**Type consistency:** op strings are uniform across parser (`op.kind.value`), interp (`_binop`/`eval_UnaryOp`), and checker (`_infer_*`): `"&" "|" "^" "<<" ">>" "~"`. Helper names consistent: `_expect_gt`, `_undo_splits`, `_split_undo`, `run_out`, `kinds`, `_prog`.
