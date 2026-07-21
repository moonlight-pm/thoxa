import pytest

from thoxa import ast
from thoxa.errors import ThoxaError
from thoxa.parser import parse


def test_parses_function():
    m = parse("fn add(a: Int64, b: Int64) -> Int64 { a + b }")
    fn = m.body[0]
    assert isinstance(fn, ast.FunctionDef)
    assert fn.name == "add"
    assert [p.name for p in fn.params] == ["a", "b"]


def test_generic_constructor_call_vs_comparison():
    # Generic call: Stack<Int64>( ... )
    m = parse("mut s = Stack<Int64>(capacity: 2)")
    call = m.body[0].value
    assert isinstance(call, ast.Call)
    assert [t.name for t in call.type_args] == ["Int64"]
    # Comparison: a < b > c  is two comparisons, not generics
    m2 = parse("let z = a < b > c")
    assert isinstance(m2.body[0].value, ast.BinaryOp)


def test_type_def_with_members():
    m = parse("type P(x: Int64) { mut y = x  export fn get() -> Int64 { y } }")
    td = m.body[0]
    assert isinstance(td, ast.TypeDef)
    assert td.ctor_params[0].name == "x"
    fields = [mem for mem in td.members if isinstance(mem, ast.Field)]
    methods = [mem for mem in td.members if isinstance(mem, ast.FunctionDef)]
    assert fields[0].name == "y" and fields[0].mutable
    assert methods[0].name == "get" and methods[0].exported


def test_union_alias():
    m = parse("type May<T> = Has<T> | Not")
    ta = m.body[0]
    assert isinstance(ta, ast.TypeAlias)
    assert [v.name for v in ta.variants] == ["Has", "Not"]


def test_match_with_else():
    m = parse("let r = match n { 0 { 1 } else { 2 } }")
    mt = m.body[0].value
    assert isinstance(mt, ast.Match)
    assert len(mt.arms) == 1
    assert mt.else_body is not None


def test_positional_marker_and_default():
    m = parse("fn f(a!: Int64, b: Int64 = 3) -> Int64 { a }")
    params = m.body[0].params
    assert params[0].positional
    assert params[1].default is not None


def test_export_on_ctor_params():
    m = parse("type H(export name: String, value: String) {}")
    td = m.body[0]
    assert isinstance(td, ast.TypeDef)
    assert td.ctor_params[0].exported and td.ctor_params[0].name == "name"
    assert not td.ctor_params[1].exported and td.ctor_params[1].name == "value"


def test_export_on_function_param_is_error():
    with pytest.raises(ThoxaError) as ei:
        parse("fn f(export x: Int64) -> Int64 { x }")
    assert "constructor parameters" in str(ei.value).lower()


def test_error_on_uppercase_value_binding():
    with pytest.raises(ThoxaError):
        parse("let X = 1")
