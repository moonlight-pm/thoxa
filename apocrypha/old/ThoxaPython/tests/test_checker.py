import pathlib

import pytest

from thoxa.checker import check
from thoxa.lsp import index as _index

CONF = pathlib.Path(__file__).resolve().parent.parent / "spec" / "conformance"
PROGRAMS = sorted(CONF.glob("*.thx"))


def _binding_named(idx, name, kind=None):
    for b in idx.resolution.bindings.values():
        if b.name == name and b.def_span is not None and (kind is None or b.kind == kind):
            return b
    raise AssertionError(f"no binding named {name!r}")


@pytest.mark.parametrize("path", PROGRAMS, ids=[p.name for p in PROGRAMS])
def test_conformance_programs_type_check_clean(path):
    """No false positives: every valid conformance program must check clean."""
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


def test_flags_undefined_name():
    assert check('fn main() -> Int64 { println("{x}")  0 }')


def test_flags_nonexhaustive_union_match():
    src = """
    type A(v: Int64) {}
    type B(v: Int64) {}
    type AB = A | B
    fn f(x!: AB) -> Int64 { match x { A(v) { v } } }
    fn main() -> Int64 { 0 }
    """
    errs = check(src)
    assert any("non-exhaustive" in d.message for d in errs)


def test_exhaustive_union_match_is_clean():
    src = """
    type A(v: Int64) {}
    type B(v: Int64) {}
    type AB = A | B
    fn f(x!: AB) -> Int64 { match x { A(v) { v } B(v) { v } } }
    fn main() -> Int64 { 0 }
    """
    assert check(src) == []


def test_flags_unknown_member():
    src = """
    type P(x: Int64) { export fn get() -> Int64 { x } }
    fn main() -> Int64 { let p = P(x: 1)  println("{p.nope}")  0 }
    """
    assert any("no member" in d.message for d in check(src))


def test_flags_private_member_access():
    src = """
    type P(x: Int64) { let secret = 1  export fn get() -> Int64 { secret } }
    fn main() -> Int64 { let p = P(x: 1)  println("{p.secret}")  0 }
    """
    assert any("private" in d.message for d in check(src))


def test_flags_string_plus():
    assert any("interpolation" in d.message for d in check('fn main() -> Int64 { let s = "a" + "b"  0 }'))


def test_flags_nonbool_condition():
    assert any("must be a Bool" in d.message for d in check("fn main() -> Int64 { if 3 { }  0 }"))


def test_checker_exposes_resolution_after_check():
    from thoxa.checker import Checker
    from thoxa.errors import Source
    from thoxa.parser import parse
    src = "fn f(x!: Int64) -> Int64 { x }\n"
    source = Source(src, "<t>")
    chk = Checker(source)
    chk.check_module(parse(src, "<t>"))
    assert chk.resolution is not None
    assert any(b.name == "x" and b.kind == "param"
               for b in chk.resolution.bindings.values())


def test_module_let_type_not_flowed_into_sibling():
    from thoxa.checker import check
    # A module-level `let` is UNKNOWN across functions (a preserved known gap).
    # Recording inferred types must NOT make its concrete type visible to a
    # sibling function, which would introduce a new "expects numbers" diagnostic.
    src = 'let n = "hello"\nfn g() -> Int64 { n * 2 }\n'
    diags = check(src, "<t>")
    assert not any("expects numbers" in d.message for d in diags)


# -- Task 1: generic payloads retained in union type-reps -----------------
def test_trial_type_retains_payload_args():
    # A param annotated Trial<Int64,String> records a 3-tuple carrying the args.
    idx = _index.build_index("fn f(x!: Trial<Int64, String>) -> Int64 {\n  0\n}\n")
    b = _binding_named(idx, "x", "param")
    assert idx.type_of(b) == ("union", "Trial", ("Int64", "String"))


def test_may_type_retains_single_payload_arg():
    idx = _index.build_index("fn f(x!: May<Int64>) -> Int64 {\n  0\n}\n")
    b = _binding_named(idx, "x", "param")
    assert idx.type_of(b) == ("union", "May", ("Int64",))


def test_match_exhaustiveness_still_fires_on_trial():
    diags = check("fn f(t!: Trial<Int64, String>) -> Int64 {\n"
                  "  match t {\n    Pass { 1 }\n  }\n}\n")
    assert any("non-exhaustive match on 'Trial'" in d.message for d in diags)


# -- Task 2: `?` unwraps the success payload ------------------------------
def test_try_unwraps_trial_payload():
    idx = _index.build_index(
        "fn f() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "fn g() -> Int64 {\n  let x = f()?\n  x\n}\n")
    assert idx.type_of(_binding_named(idx, "x", "let")) == "Int64"


def test_try_unwraps_may_payload():
    idx = _index.build_index(
        "fn h() -> May<Int64> {\n  Has(0)\n}\n"
        "fn g() -> Int64 {\n  let y = h()?\n  y\n}\n")
    assert idx.type_of(_binding_named(idx, "y", "let")) == "Int64"


def test_try_passthrough_on_ctor_call():
    idx = _index.build_index(
        "type Box(n: Int64) {\n  let bad = maybe()?\n}\n"
        "fn maybe() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "fn g() -> Int64 {\n  let b = Box(n: 1)?\n  0\n}\n")
    assert idx.type_of(_binding_named(idx, "b", "let")) == ("named", "Box")


# -- Task 3: quiet inference of unannotated fn/method returns --------------
def test_unannotated_method_return_inferred_from_body():
    # accept() has no annotation and returns Client(...); a caller then types.
    src = (
        "type Client(socket!: Int64) {\n"
        "  export fn id() -> Int64 { socket }\n"
        "}\n"
        "type Server(fd: Int64) {\n"
        "  export fn accept() {\n"
        "    Client(fd)\n"
        "  }\n"
        "}\n"
        "fn run(s!: Server) -> Int64 {\n"
        "  let c = s.accept()\n"
        "  c.id()\n"
        "}\n")
    idx = _index.build_index(src)
    assert idx.type_of(_binding_named(idx, "c", "let")) == ("named", "Client")


# -- Task 4: quiet inference of field-initializer types -------------------
def test_field_initializer_type_recorded():
    src = (
        "fn make() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "type Server(host = 1) {\n"
        "  let fd = make()?\n"
        "}\n")
    idx = _index.build_index(src)
    b = _binding_named(idx, "fd", "field")
    assert idx.type_of(b) == "Int64"


# -- Task 7: fallible ctor's inferred Trial<Self, E> return ----------------
def test_fallible_ctor_return_inferred():
    src = (
        "fn make() -> Trial<Int64, String> {\n  Pass(0)\n}\n"
        "type Server(host = 1) {\n"
        "  let fd = make()?\n"
        "}\n")
    idx = _index.build_index(src)
    assert idx.ctor_returns.get("Server") == (
        "union", "Trial", (("named", "Server"), "String"))


def test_infallible_ctor_has_no_trial_return():
    idx = _index.build_index("type Point(x = 0, y = 0) {\n}\n")
    assert "Point" not in idx.ctor_returns


def test_fallible_ctor_infers_no_return_when_error_type_undeterminable():
    # `mystery()` is undefined, so the ?-operand's error type is unknown. A Trial
    # always shows BOTH args, so with E undeterminable the ctor return is left
    # undeclared (absent from ctor_returns) — never a partial `Trial<Widget>`.
    idx = _index.build_index("type Widget(x = 1) {\n  let v = mystery()?\n}\n")
    assert "Widget" not in idx.ctor_returns



def test_fallible_if_with_fail_branch_infers_void_void_trial():
    # `if c { Fail() } else { Void }`: the `Fail()` branch is the error channel, so
    # success is `Void`; a bare `Fail()` carries the default `Void` error. The
    # method's inferred return is the full two-arg Trial<Void, Void>, never a
    # partial Trial<Void>.
    src = ("type Conn(fd: Int64) {\n"
           "  export fn recv() {\n"
           "    if fd == 0 { Fail() } else { Void }\n"
           "  }\n"
           "}\n")
    idx = _index.build_index(src)
    b = _binding_named(idx, "recv", "method")
    assert idx.fn_returns.get(b.id) == ("union", "Trial", ("Void", "Void"))
