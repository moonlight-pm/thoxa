import pytest

from thoxa.errors import ThoxaError
from thoxa.lexer import tokenize
from thoxa.tokens import T


def kinds(src):
    return [t.kind for t in tokenize(src)]


def test_keywords_and_names():
    ks = kinds("let x = 42")
    assert ks == [T.LET, T.LOWER, T.EQ, T.INT, T.EOF]


def test_type_vs_value_casing():
    toks = tokenize("Point value")
    assert toks[0].kind is T.UPPER
    assert toks[1].kind is T.LOWER


def test_number_bases_and_suffixes():
    toks = tokenize("0xFF 0b1010 1_000 2u8 3.14 1.0f32")
    assert toks[0].value == (255, None)
    assert toks[1].value == (10, None)
    assert toks[2].value == (1000, None)
    assert toks[3].value == (2, "u8")
    assert toks[4].kind is T.FLOAT and toks[4].value[0] == 3.14
    assert toks[5].value == (1.0, "f32")


def test_string_interpolation_parts():
    tok = tokenize('"a {x} b {y + 1}"')[0]
    assert tok.kind is T.STRING
    tags = [p[0] for p in tok.value]
    assert tags == ["str", "expr", "str", "expr"]


def test_underscore_in_identifier_is_rejected():
    with pytest.raises(ThoxaError):
        tokenize("let my_value = 1")


def test_ellipsis_and_ranges():
    assert kinds("...") == [T.ELLIPSIS, T.EOF]
    assert kinds("0..10") == [T.INT, T.DOTDOT, T.INT, T.EOF]
    assert kinds("0..=10") == [T.INT, T.DOTDOTEQ, T.INT, T.EOF]
