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


def test_checker_accepts_integer_bitwise():
    diags = check("fn main() -> Int64 { let x = (1 << 4) | (0xff & 3)  0 }")
    assert not any("integer" in d.message for d in diags)


def test_checker_rejects_bitwise_on_string():
    diags = check('fn main() -> Int64 { let x = "a" & 3  0 }')
    assert any("integer" in d.message for d in diags)


def test_checker_rejects_tilde_on_bool():
    diags = check("fn main() -> Int64 { let x = ~true  0 }")
    assert any("integer" in d.message for d in diags)


def test_checker_unknown_operand_does_not_false_positive():
    # An unresolved (UNKNOWN-typed) operand must NOT trigger the integer
    # diagnostic — the checker stays permissive pre-static-typing. There may
    # be an "undefined name" diagnostic, but never an "integer" one.
    diags = check("fn main() -> Int64 { let x = undefinedName & 3  0 }")
    assert not any("integer" in d.message for d in diags)
