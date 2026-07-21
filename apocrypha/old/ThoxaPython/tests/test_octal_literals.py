"""Octal integer literals `0o…` (Stage 5).

A third integer base beside 0x / 0b, added for Unix file modes (0o644). An octal
literal is an ordinary integer literal — the base does not survive lexing — so it
behaves as an int everywhere and carries the usual sized suffixes and `_` separators.
Routing all three based-integer forms through one helper also hardens a pre-existing
latent crash: an empty digit run (`0x`, `0o`, `0b`) is now a clean lexer error.
"""

import pytest

from thoxa.checker import check
from thoxa.errors import ThoxaError
from thoxa.interp import run
from thoxa.lexer import tokenize
from thoxa.tokens import T


def only(src):
    toks = [t for t in tokenize(src) if t.kind is not T.EOF]
    assert len(toks) == 1, toks
    return toks[0]


def run_out(src):
    buf = []
    code = run(src, "<test>", out=buf.append)
    return "".join(buf), code


def _prog(body):
    return "fn main() -> Int64 {\n    " + body + "\n    0\n}\n"


# -- lexing: value ----------------------------------------------------------

def test_octal_basic_values():
    # An INT token's value is (int, suffix|None).
    assert only("0o0").value == (0, None)
    assert only("0o7").value == (7, None)
    assert only("0o10").value == (8, None)
    assert only("0o644").value == (420, None)
    assert only("0o777").value == (511, None)


def test_octal_uppercase_prefix():
    # 0O… is accepted just like 0X / 0B.
    assert only("0O644").value == (420, None)


def test_octal_underscore_separators():
    assert only("0o1_000").value == (512, None)
    assert only("0o7_7_7").value == (511, None)


def test_octal_sized_suffix():
    assert only("0o17u8").value == (15, "u8")
    assert only("0o377u8").value == (255, "u8")


def test_octal_emits_single_int_token():
    assert [t.kind for t in tokenize("0o644") if t.kind is not T.EOF] == [T.INT]


# -- lexing: greedy stop + errors -------------------------------------------

def test_octal_stops_at_non_octal_digit():
    # `8`/`9` are not octal digits, so `0o18` lexes as 0o1 then a separate decimal 8 —
    # the same greedy behavior as 0x1G. Two INT tokens.
    toks = [t for t in tokenize("0o18") if t.kind is not T.EOF]
    assert [t.kind for t in toks] == [T.INT, T.INT]
    assert toks[0].value == (1, None) and toks[1].value == (8, None)


def test_octal_no_digits_is_clean_error():
    # `0o` with no octal digit following (incl. a lone 8/9 or only `_`) is a located
    # ThoxaError, never an uncaught ValueError.
    for src in ["0o", "0O", "0o8", "0o9", "0o_"]:
        with pytest.raises(ThoxaError):
            tokenize(src)


def test_hex_and_binary_empty_now_clean_error():
    # The shared _radix_int hardening: previously `0x`/`0b` with no digits raised an
    # uncaught ValueError; now they are clean ThoxaError diagnostics.
    for src in ["0x", "0X", "0b", "0B", "0x_", "0b_"]:
        with pytest.raises(ThoxaError):
            tokenize(src)


# -- checking / interpreting ------------------------------------------------

def test_checker_accepts_octal():
    assert check(_prog("let mode = 0o644  let f = 0o1 | 0o100 | 0o1000")) == []


def test_interp_octal_equals_decimal():
    out, code = run_out(_prog('println("{0o644} {0o644 == 420}")'))
    assert out == "420 true\n" and code == 0


def test_interp_octal_flag_or():
    out, _ = run_out(_prog('println("{0o1 | 0o100 | 0o1000}")'))
    assert out == "577\n"
