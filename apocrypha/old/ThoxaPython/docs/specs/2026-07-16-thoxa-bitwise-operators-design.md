# Thoxa Bitwise Operators — Design (LF-A)

- **Date:** 2026-07-16
- **Status:** Approved for implementation (identity calls locked with Joshua)
- **Context:** Language feature LF-A of the Kilo port (see `2026-07-16-kilo-port-roadmap.md`),
  fork #2. Chosen over arithmetic substitutes: *add real operators now.*

## 1. Motivation

A terminal editor is built on bit operations. The port needs them for:

- `CTRL_KEY(k) = k & 0x1f` — every Ctrl command.
- `termios` flag editing in raw mode (LF-B): `lflag = lflag & ~(ECHO | ICANON | ISIG | IEXTEN)`, etc.
- File-open flag combining `O_RDWR | O_CREAT | O_TRUNC` for save — a case `examples/http/Io.thx`
  already deferred *"once byte/bit ops land."*
- Future combinable valued-flag enums (the `OpenMode`-style bitset).

## 2. Operators & spelling

C-style symbols. Six operators:

| Operator | Name | Form |
| --- | --- | --- |
| `&` | bitwise and | binary |
| `\|` | bitwise or | binary |
| `^` | bitwise xor | binary |
| `~` | bitwise not | unary prefix |
| `<<` | left shift | binary |
| `>>` | right shift | binary |

Two of these overload existing syntax; both are resolved **by grammar position**, never by
guessing — exactly the mechanism already used for `<`/`>` as generic brackets:

- **`|`** is currently the union-type separator (`type Shape = Circle | Square`). Union `|` appears
  *only* on a `type`-alias right-hand side; bitwise `|` appears *only* in expression context. Thoxa
  has no inline union in a type annotation, so the two productions never overlap.
- **`>>`** overlaps the close of nested generics (`Array<Array<Int64>>`). Shifts appear *only* in
  expression context; the type-argument parser re-splits a `>>` token into two `>` when it is closing
  a generic list. `> >` written with whitespace already lexes as two `>` and is unaffected.

## 3. Precedence

Bitwise binds **tighter than comparison** and **looser than arithmetic** — this is exactly Python's
ordering, and it fixes C's classic footgun. Updated SPEC §7.5 table (lowest → highest binding):

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
unary: not  -  ~        (prefix)
postfix: ?  as  can  is   f(args)  a[i]  a.b
```

Consequences (all intuitive):

- `x & mask == 0`  ⇒  `(x & mask) == 0`   — the reading people expect (C gets this wrong).
- `a | b == c`     ⇒  `(a | b) == c`.
- `1 << 2 + 3`     ⇒  `1 << (2 + 3)` = 32 — arithmetic binds tighter than shift (matches C & Python).
- `a & b | c`      ⇒  `(a & b) | c`        — `&` tighter than `^` tighter than `|`.
- `0 .. n & mask`  ⇒  `0 .. (n & mask)`    — range endpoints are full bitwise expressions.
- `~` joins the existing unary-prefix tier alongside `not` and unary `-`.

## 4. Operand types & runtime semantics

- **Operands:** the integer types only — `Int8/16/32/64`, `UInt8/16/32/64`, `Byte`. Not `Bool`
  (use `and`/`or`/`not`), not `Float`/`Char`/`String`. `~` takes one integer.
- **Runtime (pre-[M4], dynamically typed):** evaluated as Python integer bit operations. Widths are
  recorded but not enforced yet, so no runtime width mask is applied *inside* an expression. This is
  correct for every masking/clearing/setting use the editor makes:
  - `a & b`, `a | b`, `a ^ b` — two's-complement bitwise ops.
  - `~a` — bitwise complement. In infinite precision `~a == -a-1`; combined with `&` for bit-clearing
    it yields the correct low bits (`0b1111 & ~0b0100 == 0b1011`, `lflag & ~mask` clears `mask`).
  - `a << n` — left shift; unbounded until it crosses a typed boundary.
  - `a >> n` — arithmetic right shift (sign-preserving; logical for the non-negative values the editor
    uses).
  - **Width truncation happens at typed boundaries:** a struct-field write (LF-B), FFI marshalling,
    or an explicit `as UIntN` masks to the target width. Full width-aware masking *inside*
    expressions is deferred to the static-typing milestone [M4].
- **Literals:** hex `0x1f`, binary `0b1010`, `_` separators, and width suffixes (`255u8`) already
  exist (SPEC §1.5) — no lexer change for literals, masks are already writable.

## 5. Implementation surface (hooks)

- **`thoxa/tokens.py`** — add `T.AMP` `&`, `T.CARET` `^`, `T.TILDE` `~`, `T.SHL` `<<`, `T.SHR` `>>`
  (`T.PIPE` `|` already exists; add `<< >>` to SPEC §1.6 too).
- **`thoxa/lexer.py`** (`tokenize`) — scan the new operators. Maximal-munch on `<`/`>`: on `<`, a
  following `=` → `LE`, a following `<` → `SHL`, else `LT`; symmetric for `>`. `&`/`^`/`~` are single
  chars (no `&&`/`||` — logical ops are the keywords `and`/`or`).
- **`thoxa/ast.py`** — extend the binary-op node's operator set with `& | ^ << >>`; extend the
  unary-op node with `~`.
- **`thoxa/parser.py`** — insert four left-assoc levels **between `p_range` and `p_add`**:
  `p_bitor` (`|`) → `p_bitxor` (`^`) → `p_bitand` (`&`) → `p_shift` (`<< >>`); rewire `p_range` to call
  `p_bitor`, and `p_shift` to call `p_add`. Add `~` to `p_unary`. In `try_type_args` / `parse_type`,
  when a generic close expects `>` but sees a `SHR` token, consume it as one `>` and leave a synthetic
  `>` for the enclosing list.
- **`thoxa/interp.py`** — evaluate the new binary/unary ops via Python int operators.
- **`thoxa/checker.py`** — accept `& | ^ << >>` / unary `~` on integer operands; result keeps the
  operand's integer type (defaulting to `Int64`); reject `Bool`/`Float`/`Char`/`String` operands.
- **`editor/` (tree-sitter/LSP)** — the grammar is deliberately lexical; add the new operator tokens
  if it enumerates them. Semantic tokens/diagnostics are reused from the frontend, so they follow for
  free. Low priority; note for a later grammar `rev` bump.

## 6. Interaction: the statement-boundary sharp edge

Thoxa's grammar is newline-insensitive, so a statement that *starts* with a binary operator is
absorbed into the previous line. Adding `& | ^ << >>` extends this hazard: `x`⏎`& mask` parses as
`x & mask`. Unary `~` at line-start is **safe** (it is a prefix operator; there is no binary `~`).
Workaround unchanged: bind with `let`/`mut`, or keep the expression on one line (bit expressions
are naturally single-line). A real statement-boundary fix stays a **separate candidate feature**, not
folded into LF-A.

## 7. Conformance

`spec/conformance/bitwise.thx` (+ `.out`) and `tests/test_bitwise.py` must cover:

- Masks: `0xf0 & 0x0f == 0`, `0xf0 | 0x0f == 0xff`, `0b1010 ^ 0b0110 == 0b1100`.
- Clear via `~`: `0b1111 & ~0b0100 == 0b1011`.
- Shifts: `1 << 4 == 16`, `240 >> 4 == 15`, `1 << 2 + 3 == 32` (precedence).
- Comparison grouping: `8 & 8 == 8` evaluates as `(8 & 8) == 8` → `true`.
- `\|` disambiguation: a `type Shape = Circle | Square` union **and** an expression `a | b` both
  parse in the same file.
- `>>` disambiguation: `let m: Array<Array<Int64>> = [[1]]` parses (nested generic close) alongside
  `let s = word >> 8` (shift).
- Hex/binary literals in operands.

## 8. Out of scope / deferred

- Compound bit-assign (`&=`, `|=`, `^=`, `<<=`, `>>=`) — later add if wanted.
- Width-aware runtime masking inside expressions → static-typing milestone [M4].
- Combinable valued-flag enums (now *unblocked* by this feature, but their own small design).
- Statement-boundary fix (§6).
