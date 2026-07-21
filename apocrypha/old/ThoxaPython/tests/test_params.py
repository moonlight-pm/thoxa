import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run

GREET = """
fn greet(name: String, greeting: String = "Hi", loud: Bool = false) -> String {
    if loud { "{greeting} {name}!" } else { "{greeting} {name}" }
}
"""


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def expect_error(body):
    with pytest.raises(ThoxaError):
        run_out(GREET + "fn main() -> Int64 { " + body + "  0 }")


def test_defaults_and_order_preserving_skip():
    out = run_out(GREET + 'fn main() -> Int64 { println(greet(name: "A", loud: true))  0 }')
    assert out == "Hi A!\n"


def test_named_out_of_order_is_error():
    expect_error('println(greet(greeting: "Hey", name: "A"))')


def test_unknown_named_argument_is_error():
    expect_error('println(greet(name: "A", color: "red"))')


def test_missing_required_argument_is_error():
    expect_error("println(greet(greeting: \"x\"))")


def test_positional_param_rejects_label():
    src = """
    fn only(x!: Int64) -> Int64 { x }
    fn main() -> Int64 { println("{only(x: 3)}")  0 }
    """
    with pytest.raises(ThoxaError):
        run_out(src)


def test_positional_after_named_is_error():
    expect_error('println(greet(name: "A", "B"))')


def test_variadic_collects_trailing():
    src = """
    fn sumAll(first!: Int64, ...rest: Int64) -> Int64 {
        mut t = first
        for r in rest { t = t + r }
        t
    }
    fn main() -> Int64 { println("{sumAll(1, 2, 3, 4)}")  0 }
    """
    assert run_out(src) == "10\n"


# -- bare-name argument punning ------------------------------------------------
# An unlabeled bare identifier is promoted to the named argument of the same name
# iff it matches one of the callee's keyword parameters; otherwise it stays
# positional. Only bare names pun — literals and compound expressions never do.


def test_bare_name_puns_matching_keyword():
    src = GREET + """
fn main() -> Int64 {
    let name = "A"
    let greeting = "Hey"
    println(greet(name, greeting))
    0
}
"""
    assert run_out(src) == "Hey A\n"


def test_bare_name_puns_alongside_explicit_label():
    src = GREET + """
fn main() -> Int64 {
    let name = "A"
    println(greet(name, greeting: "Yo"))
    0
}
"""
    assert run_out(src) == "Yo A\n"


def test_bare_name_not_matching_keyword_stays_positional():
    src = """
fn tag(value!: Int64, kind: String = "n") -> String { "{kind}{value}" }
fn main() -> Int64 {
    let n = 7
    println(tag(n))
    0
}
"""
    assert run_out(src) == "n7\n"


def test_bare_name_matching_keyword_cannot_fill_positional():
    # `count` matches the keyword parameter, so it is punned and can never fill
    # the positional `x!` — which is then reported missing (the sharp edge).
    src = """
fn f(x!: Int64, count: Int64 = 0) -> Int64 { x + count }
fn main() -> Int64 {
    let count = 5
    println("{f(count)}")
    0
}
"""
    with pytest.raises(ThoxaError):
        run_out(src)


def test_punning_on_method_call():
    src = """
type Counter(start: Int64) {
    export fn plus(amount: Int64) -> Int64 { start + amount }
}
fn main() -> Int64 {
    let c = Counter(start: 10)
    let amount = 5
    println("{c.plus(amount)}")
    0
}
"""
    assert run_out(src) == "15\n"


def test_punning_on_constructor():
    src = """
type Point(x: Int64, y: Int64) {
    export fn sum() -> Int64 { x + y }
}
fn main() -> Int64 {
    let x = 3
    let y = 4
    let p = Point(x, y)
    println("{p.sum()}")
    0
}
"""
    assert run_out(src) == "7\n"
