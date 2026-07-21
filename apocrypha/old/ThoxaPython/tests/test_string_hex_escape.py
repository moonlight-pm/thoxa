"""String/Char `\\xNN` escapes — the Stage 2 language increment.

Every VT100 control sequence begins with ESC (0x1b), which no `"…"` literal could
express before Stage 2. `\\xNN` (exactly two hex digits) is now accepted in `'…'`
and `"…"`, restricted to ASCII (0x00–0x7f): a `Char` is a Unicode scalar and a
`String` is UTF-8, so a lone byte >= 0x80 is ambiguous. Byte literals `b'…'` keep
the full 0x00–0xff range (a byte is not a scalar).
"""

import pytest

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


# -- lexing: Char ------------------------------------------------------------

def test_char_hex_escape_lexes_to_scalar():
    assert only("'\\x1b'").value == "\x1b"
    assert only("'\\x41'").value == "A"
    assert only("'\\x00'").value == "\x00"
    assert only("'\\x7f'").value == "\x7f"
    assert only("'\\x7F'").value == "\x7f"  # hex digits are case-insensitive


# -- lexing: String ----------------------------------------------------------

def test_string_hex_escape_lexes_to_literal_part():
    tok = only('"\\x1b[2J"')
    assert tok.kind is T.STRING
    assert tok.value == [("str", "\x1b[2J")]


def test_string_hex_escape_mixes_with_other_escapes():
    assert only('"\\x1b\\r\\n"').value == [("str", "\x1b\r\n")]


# -- the ASCII restriction (the Char/String difference from byte literals) ---

def test_string_hex_escape_rejects_non_ascii():
    with pytest.raises(ThoxaError) as e:
        tokenize('"\\x80"')
    assert "ASCII" in str(e.value)


def test_char_hex_escape_rejects_non_ascii():
    with pytest.raises(ThoxaError) as e:
        tokenize("'\\xff'")
    assert "ASCII" in str(e.value)


def test_string_hex_escape_requires_two_hex_digits():
    with pytest.raises(ThoxaError):
        tokenize('"\\x1"')    # one hex digit, then the closing quote
    with pytest.raises(ThoxaError):
        tokenize('"\\xzz"')   # non-hex digits


def test_byte_literal_hex_escape_keeps_full_range():
    # the ASCII restriction is Char/String-only; a byte is not a scalar
    assert only("b'\\xff'").value == 255
    assert only("b'\\x80'").value == 128


# -- interpreting ------------------------------------------------------------

def test_interp_string_hex_escape_is_ascii_byte():
    # asserted through Bytes so no raw control byte reaches the captured output
    out, code = run_out(_prog(
        'let b = Bytes.fromString("\\x1b[2J")\n'
        '    println("{b.count} {b.get(0)} {b.get(3)}")'
    ))
    assert out == "4 27 74\n" and code == 0


def test_interp_char_hex_escape_equals_bare_char():
    out, _ = run_out(_prog('println("{\'\\x41\' == \'A\'}")'))
    assert out == "true\n"
