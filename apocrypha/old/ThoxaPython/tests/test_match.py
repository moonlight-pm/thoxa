import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def test_has_not_construction_and_match():
    src = """
    fn f(b!: Bool) -> May<Int64> { if b { Has(7) } else { Not() } }
    fn main() -> Int64 {
        println(match f(true) { Has(x) { "{x}" } Not { "none" } })
        println(match f(false) { Has(x) { "{x}" } Not { "none" } })
        0
    }
    """
    assert run_out(src) == "7\nnone\n"


def test_question_propagates_fail():
    src = """
    fn check(x!: Int64) -> Trial<Int64, String> {
        if x > 0 { Pass(x) } else { Fail("neg") }
    }
    fn chain(x!: Int64) -> Trial<Int64, String> { let a = check(x)?  Pass(a + 1) }
    fn main() -> Int64 {
        println(match chain(5) { Pass(v) { "{v}" } Fail(e) { e } })
        println(match chain(-1) { Pass(v) { "{v}" } Fail(e) { e } })
        0
    }
    """
    assert run_out(src) == "6\nneg\n"


def test_literal_and_binding_patterns():
    src = """
    fn classify(n!: Int64) -> String {
        match n { 0 { "zero" } 1 { "one" } other { "many: {other}" } }
    }
    fn main() -> Int64 {
        println(classify(0))
        println(classify(1))
        println(classify(9))
        0
    }
    """
    assert run_out(src) == "zero\none\nmany: 9\n"


def test_nonexhaustive_without_else_traps():
    with pytest.raises(ThoxaError):
        run_out('fn main() -> Int64 { let r = match 5 { 0 { "z" } }  println(r)  0 }')


def test_prelude_constructors_take_positional_payloads():
    # Has / Pass / Fail carry their payload positionally — no label.
    src = """
    fn main() -> Int64 {
        println(match Has(7) { Has(x) { "{x}" } Not { "none" } })
        println(match Pass(3) { Pass(v) { "p{v}" } Fail(e) { "f{e}" } })
        println(match Fail("bad") { Pass(v) { "p{v}" } Fail(e) { "f{e}" } })
        0
    }
    """
    assert run_out(src) == "7\np3\nfbad\n"


def test_prelude_constructors_reject_labels():
    # A label on the (now positional) payload is an error.
    for ctor in ("Pass(value: 1)", "Fail(error: 1)", "Has(value: 1)"):
        with pytest.raises(ThoxaError):
            run_out("fn main() -> Int64 { let r = " + ctor + "  0 }")
