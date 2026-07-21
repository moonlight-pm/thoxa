from thoxa import ast
from thoxa.checker import check
from thoxa.interp import run
from thoxa.parser import parse


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


PROGRAM = """
type ReadOnly 0
type WriteOnly 1
type ReadWrite 2
type OpenMode = ReadOnly | WriteOnly | ReadWrite

fn flags(mode!: OpenMode) -> Int32 { mode as Int32 }

fn main() -> Int64 {
    println("{flags(ReadOnly())} {flags(WriteOnly())} {flags(ReadWrite())}")
    0
}
"""


def test_parses_valued_type_decl():
    td = parse("type ReadOnly 0").body[0]
    assert isinstance(td, ast.TypeDef)
    assert td.name == "ReadOnly"
    assert td.ctor_params == []
    assert td.members == []
    assert isinstance(td.value, ast.IntLit)
    assert td.value.value == 0


def test_plain_type_has_no_value():
    assert parse("type ReadOnly {}").body[0].value is None


def test_valued_type_casts_to_its_declared_int():
    assert run_out(PROGRAM) == "0 1 2\n"


def test_valued_type_program_checks_clean():
    assert check(PROGRAM) == []


# A valued type is still a nominal type: it can be matched by variant and the
# checker still enforces exhaustiveness over the union (the Design-A guarantee).
MATCH_PROGRAM = """
type ReadOnly 0
type WriteOnly 1
type ReadWrite 2
type OpenMode = ReadOnly | WriteOnly | ReadWrite

fn label(mode!: OpenMode) -> String {
    match mode {
        ReadOnly  { "r" }
        WriteOnly { "w" }
        ReadWrite { "rw" }
    }
}

fn main() -> Int64 {
    println("{label(ReadOnly())} {label(WriteOnly())} {label(ReadWrite())}")
    0
}
"""


def test_valued_type_is_still_matchable_nominally():
    assert run_out(MATCH_PROGRAM) == "r w rw\n"
    assert check(MATCH_PROGRAM) == []


# A valued type's scalar is not limited to integers: string/char/float/bool
# literals work too, each read back out with the matching cast.
def test_parses_string_valued_type():
    td = parse('type Slash "/"').body[0]
    assert isinstance(td, ast.TypeDef)
    assert td.value is not None


def test_string_valued_type_casts_to_string():
    src = 'type Slash "/"\nfn main() -> Int64 { println(Slash() as String)  0 }'
    assert run_out(src) == "/\n"


def test_char_valued_type_casts_to_char():
    src = "type Q 'x'\nfn main() -> Int64 { println(Q() as Char)  0 }"
    assert run_out(src) == "x\n"


def test_float_valued_type_casts_to_float():
    src = "type Pi 3.14\nfn main() -> Int64 { println(Pi() as Float64)  0 }"
    assert run_out(src) == "3.14\n"


def test_bool_valued_type_casts_to_bool():
    src = "type Yes true\nfn main() -> Int64 { println(Yes() as Bool)  0 }"
    assert run_out(src) == "true\n"


def test_negative_int_valued_type():
    src = "type NegOne -1\nfn main() -> Int64 { println(NegOne() as Int64)  0 }"
    assert run_out(src) == "-1\n"


def test_nonexhaustive_match_on_valued_union_still_flagged():
    src = MATCH_PROGRAM.replace('        ReadWrite { "rw" }\n', "")
    assert any("non-exhaustive" in d.message for d in check(src))
