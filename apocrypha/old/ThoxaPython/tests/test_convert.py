from thoxa.interp import run


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def test_numeric_casts():
    assert run_out('fn main() -> Int64 { println("{7 as Float64} {7.9 as Int64}")  0 }') == "7.0 7\n"


def test_to_string_cast():
    assert run_out('fn main() -> Int64 { println(42 as String)  0 }') == "42\n"


def test_is_guards():
    src = 'fn main() -> Int64 { println("{5 is Int64} {5 is Float64} {5.0 is Float64} {true is Bool}")  0 }'
    assert run_out(src) == "true false true true\n"


def test_can_reports_definability():
    src = 'fn main() -> Int64 { println("{3 can Float64} {3 can Bool}")  0 }'
    assert run_out(src) == "true false\n"


def test_union_is_membership():
    src = """
    type A(v: Int64) {}
    type B(v: Int64) {}
    type AB = A | B
    fn main() -> Int64 {
        let a = A(v: 1)
        println("{a is A} {a is AB} {a is B}")
        0
    }
    """
    assert run_out(src) == "true true false\n"
