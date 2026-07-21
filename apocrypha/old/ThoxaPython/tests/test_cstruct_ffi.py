"""Tests for LF-B typed C-struct FFI: fixed-array type args, ABI structs,
Address<T> marshalling, and the conformance round-trip."""

import pytest

from thoxa import ast
from thoxa.checker import check
from thoxa.errors import Source, ThoxaError
from thoxa.interp import run
from thoxa.lexer import Lexer
from thoxa.parser import Parser


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def parse(src):
    s = Source(src, "<test>")
    return Parser(Lexer(s).tokenize(), s).parse_program()


def _messages(src):
    return [d.message for d in check(src)]


def _field_type(src, type_index=0, field_index=0):
    td = parse(src).body[type_index]
    return td.members[field_index].type_ann


def test_fixed_array_type_arg_parses():
    ann = _field_type("type Buf {\n  mut cc: Array<Byte, 32>\n}")
    assert ann.name == "Array"
    assert len(ann.args) == 2
    assert ann.args[0].name == "Byte"
    assert ann.args[1].int_value == 32


def test_ordinary_generic_type_still_parses():
    ann = _field_type("type Box {\n  mut xs: Array<Int64>\n}")
    assert ann.name == "Array"
    assert len(ann.args) == 1
    assert ann.args[0].name == "Int64"
    assert ann.args[0].int_value is None


def test_make_struct_type_matches_termios_layout():
    import ctypes
    from thoxa import ffi
    specs = [
        ("cIflag", "UInt32", None), ("cOflag", "UInt32", None),
        ("cCflag", "UInt32", None), ("cLflag", "UInt32", None),
        ("cLine", "Byte", None), ("cCc", "Byte", 32),
        ("cIspeed", "UInt32", None), ("cOspeed", "UInt32", None),
    ]
    cls = ffi.make_struct_type("Termios", specs)
    assert ctypes.sizeof(cls) == 60
    assert cls.cIflag.offset == 0
    assert cls.cLflag.offset == 12
    assert cls.cLine.offset == 16
    assert cls.cCc.offset == 17
    assert cls.cIspeed.offset == 52
    assert cls.cOspeed.offset == 56
    # cached: same specs -> identical class object
    assert ffi.make_struct_type("Termios", specs) is cls


def test_abi_struct_specs_accepts_scalar_and_array_fields():
    from thoxa import ffi
    two = parse("type Two {\n  mut a: UInt32\n  mut b: UInt32\n}").body[0]
    assert ffi.abi_struct_specs(two) == [("a", "UInt32", None), ("b", "UInt32", None)]
    buf = parse("type Buf {\n  mut cc: Array<Byte, 4>\n}").body[0]
    assert ffi.abi_struct_specs(buf) == [("cc", "Byte", 4)]


def test_abi_struct_specs_rejects_non_abi():
    from thoxa import ffi
    with_str = parse("type Bad {\n  mut name: String\n}").body[0]
    assert ffi.abi_struct_specs(with_str) is None
    with_method = parse("type M {\n  mut a: UInt32\n  fn f() -> Int64 { 0 }\n}").body[0]
    assert ffi.abi_struct_specs(with_method) is None
    with_ctor = parse("type C(fd: Int32) {\n  mut a: UInt32\n}").body[0]
    assert ffi.abi_struct_specs(with_ctor) is None
    empty = parse("type E { }").body[0]
    assert ffi.abi_struct_specs(empty) is None


def test_cstruct_value_get_set_and_addressing():
    from thoxa import ffi

    class _FakeType:  # stands in for a ThoxaType (only .name/.field_mut are used)
        name = "Two"
        field_mut = {"a": True, "b": True}

    cls = ffi.make_struct_type("Two", [("a", "UInt32", None), ("b", "UInt32", None)])
    v = ffi.CStruct(cls, _FakeType())
    assert v.get("a") == 0 and v.get("b") == 0           # zero-initialized
    v.set("a", 0xDEADBEEF)
    assert v.get("a") == 0xDEADBEEF
    assert v.has_field("a") and not v.has_field("z")
    assert v.type_name == "Two"


def test_cstruct_construct_zeroed_and_field_io():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        "fn main() -> Int64 {\n"
        '  mut t = Two()\n'
        '  println("{t.a} {t.b}")\n'
        "  t.a = 16909060\n"
        "  t.b = t.a & 0xFFFF\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "0 0\n16909060 772\n"


def test_cstruct_fixed_array_element_io():
    src = (
        "type Buf {\n  mut cc: Array<Byte, 4>\n}\n"
        "fn main() -> Int64 {\n"
        "  mut b = Buf()\n"
        '  println("{b.cc[0]} {b.cc[3]}")\n'
        "  b.cc[0] = 65\n"
        "  b.cc[3] = 200\n"
        '  println("{b.cc[0]} {b.cc[3]}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "0 0\n65 200\n"


def test_cstruct_immutable_field_rejected_at_runtime():
    src = (
        "type P {\n  let x: UInt32\n}\n"
        "fn main() -> Int64 {\n  mut p = P()\n  p.x = 1\n  0\n}\n"
    )
    with pytest.raises(ThoxaError, match="not mutable"):
        run_out(src)


def test_checker_allows_abi_struct_field_access_same_file():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        "fn main() -> Int64 {\n"
        "  mut t = Two()\n"
        "  t.a = 5\n"
        "  t.b = t.a\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert _messages(src) == []


def test_cstruct_roundtrips_through_libc():
    # C reads/writes the struct in place through its address (implicit addressing).
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memcpy(dst!: Address<Two>, src!: Address<Two>, n!: UInt64) from "c"\n'
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 {\n"
        "  mut t = Two()\n"
        "  t.a = 16909060\n"
        "  t.b = 168496141\n"
        "  mut u = Two()\n"
        "  memcpy(u, t, 8)\n"
        '  println("{u.a} {u.b}")\n'
        "  memset(t, 0, 8)\n"
        '  println("{t.a} {t.b}")\n'
        "  0\n"
        "}\n"
    )
    assert run_out(src) == "16909060 168496141\n0 0\n"


def test_checker_accepts_address_of_abi_struct():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    assert _messages(src) == []


def test_checker_rejects_address_of_non_abi_struct():
    src = (
        "type Bad {\n  let name: String\n}\n"
        'extern fn f(p!: Address<Bad>) from "c" "memset"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    msgs = _messages(src)
    assert any("Bad" in m and "ABI" in m for m in msgs), msgs


def test_checker_rejects_address_return():
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn f() -> Address<Two> from "c" "malloc"\n'
        "fn main() -> Int64 { 0 }\n"
    )
    msgs = _messages(src)
    assert any("Address" in m and ("return" in m or "pointer" in m) for m in msgs), msgs


def test_address_param_rejects_non_struct_value_at_runtime():
    # A clean FFIError, not a raw pointer deref, when a non-struct reaches the boundary.
    src = (
        "type Two {\n  mut a: UInt32\n  mut b: UInt32\n}\n"
        'extern fn memset(p!: Address<Two>, c!: Int32, n!: UInt64) from "c"\n'
        "fn main() -> Int64 {\n  memset(5, 0, 8)\n  0\n}\n"
    )
    with pytest.raises(ThoxaError, match="C struct"):
        run_out(src)
