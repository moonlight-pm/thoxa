"""Byte literals `b'…'` — a `Byte` (0..255) written in character form.

Resolves kilo-port fork #3: no char↔int conversion, just a byte-native literal.
A byte literal is an integer literal spelled in character form, so it behaves as
an int everywhere (arithmetic, bitwise, comparison, equality, match patterns).
"""

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


# -- lexing -----------------------------------------------------------------

def test_lexer_emits_byte_token():
    assert kinds("b'A'") == [T.BYTE]


def test_byte_bare_ascii_value():
    assert only("b'A'").value == 65
    assert only("b'0'").value == 48
    assert only("b' '").value == 32
    assert only("b'~'").value == 126


def test_byte_standard_escapes():
    assert only("b'\\n'").value == 10
    assert only("b'\\r'").value == 13
    assert only("b'\\t'").value == 9
    assert only("b'\\0'").value == 0
    assert only("b'\\''").value == 39
    assert only("b'\\\\'").value == 92


def test_byte_hex_escape_full_range():
    assert only("b'\\x1b'").value == 27
    assert only("b'\\x7f'").value == 127
    assert only("b'\\xff'").value == 255
    assert only("b'\\x00'").value == 0
    assert only("b'\\xFF'").value == 255  # hex digits are case-insensitive


def test_bare_b_is_still_an_identifier():
    # A lone `b` not immediately followed by `'` is an ordinary identifier.
    assert kinds("b + 1") == [T.LOWER, T.PLUS, T.INT]
    assert kinds("bc") == [T.LOWER]
    # Only a token-start `b` abutting `'` is a byte literal; `ab'x'` is not.
    assert kinds("ab'x'") == [T.LOWER, T.CHAR]


def test_byte_non_ascii_bare_char_rejected():
    with pytest.raises(ThoxaError) as e:
        tokenize("b'é'")
    assert "ASCII" in str(e.value)


def test_byte_hex_escape_requires_two_hex_digits():
    with pytest.raises(ThoxaError):
        tokenize("b'\\x1'")   # only one hex digit before the closing quote
    with pytest.raises(ThoxaError):
        tokenize("b'\\xg0'")  # non-hex digit


def test_byte_empty_and_multichar_rejected():
    with pytest.raises(ThoxaError):
        tokenize("b''")
    with pytest.raises(ThoxaError):
        tokenize("b'ab'")


def test_byte_literal_truncated_at_eof_is_clean_error():
    # A byte literal truncated at end-of-input must raise a ThoxaError diagnostic,
    # not an uncaught IndexError (which would crash the CLI and the LSP token path).
    for src in ["b'", "b'\\", "b'\\x", "b'\\x1", "b'A", "b'\\xg"]:
        with pytest.raises(ThoxaError):
            tokenize(src)


def test_char_literal_gained_hex_escape_in_stage2():
    # Stage 2 extended \xNN to Char/String literals (ASCII-restricted); see
    # tests/test_string_hex_escape.py. A Char \xNN now lexes to that scalar,
    # while >= 0x80 is rejected (a Char is a Unicode scalar, not a byte).
    assert only("'\\x1b'").value == "\x1b"
    with pytest.raises(ThoxaError):
        tokenize("'\\x80'")


# -- parsing ----------------------------------------------------------------

def test_parses_to_bytelit():
    node = parse("let x = b'A'").body[0].value
    assert isinstance(node, ast.ByteLit) and node.value == 65


def test_byte_literal_as_match_pattern():
    node = parse(_prog("match 65 { b'A' { 1 } else { 0 } }")).body[0]
    # just assert it parses without error (structure covered by interp test)
    assert node is not None


# -- checking ---------------------------------------------------------------

def test_checker_accepts_byte_literal_as_int():
    diags = check(_prog("let x = b'q' & 0x1f  let y = x + 1"))
    assert diags == []


def test_checker_byte_literal_ordered_compare_clean():
    # ordered comparison on bytes is a numeric int compare — no diagnostic
    diags = check(_prog("let ok = b'0' <= b'5' and b'5' <= b'9'"))
    assert diags == []


# -- interpreting -----------------------------------------------------------

def test_interp_byte_value_is_its_code():
    out, code = run_out(_prog('println("{b\'A\'} {b\'\\x1b\'} {b\'\\xff\'}")'))
    assert out == "65 27 255\n" and code == 0


def test_interp_ctrl_key_mask():
    out, _ = run_out(_prog('println("{b\'q\' & 0x1f}")'))
    assert out == "17\n"


def test_interp_ordered_range_check():
    out, _ = run_out(_prog('println("{b\'0\' <= b\'5\' and b\'5\' <= b\'9\'}")'))
    assert out == "true\n"


def test_interp_byte_equals_int():
    out, _ = run_out(_prog('println("{b\'[\' == 91}")'))
    assert out == "true\n"


def test_interp_match_byte_pattern():
    prog = _prog(
        'match 65 {\n'
        '        b\'A\' { println("letterA") }\n'
        '        b\'B\' { println("letterB") }\n'
        '        else { println("other") }\n'
        '    }'
    )
    out, _ = run_out(prog)
    assert out == "letterA\n"
