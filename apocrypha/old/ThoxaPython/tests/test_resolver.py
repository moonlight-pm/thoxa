# tests/test_resolver.py
from thoxa.errors import Span
from thoxa import resolver as R


def test_ref_at_prefers_narrowest_covering_span():
    res = R.Resolution()
    b = R.Binding(id=0, name="x", kind="let", def_span=Span(1, 5, 1), node=None)
    res.bindings[0] = b
    wide = R.Reference(Span(1, 1, 10), 0, "value")
    narrow = R.Reference(Span(1, 5, 1), 0, "value")
    res.references += [wide, narrow]
    assert res.ref_at(1, 5) is narrow          # 1-based line/col
    assert res.ref_at(1, 2) is wide
    assert res.ref_at(2, 1) is None


def test_refs_to_and_binding_of():
    res = R.Resolution()
    res.bindings[0] = R.Binding(0, "x", "let", Span(1, 5, 1), None)
    r = R.Reference(Span(2, 3, 1), 0, "value")
    res.references.append(r)
    assert res.binding_of(r).name == "x"
    assert res.refs_to(0) == [r]
    assert res.binding_of(R.Reference(Span(9, 9, 1), None, "value")) is None


from thoxa.parser import parse
from thoxa.errors import Source


def _resolve(src, filename="<t>"):
    source = Source(src, filename)
    return R.resolve(parse(src, filename), source)


def test_module_definitions_cover_top_level():
    src = (
        "fn add(a!: Int64, b!: Int64) -> Int64 { a }\n"
        "type Point(x: Int64, y: Int64) {}\n"
        "let base = 10\n"
    )
    res = _resolve(src)
    defs = {b.name: b.kind for b in res.module_definitions()}
    assert defs == {"add": "fn", "Point": "type", "base": "let"}


def test_function_name_span_is_precise():
    res = _resolve("fn greet() -> Int64 { 0 }\n")
    b = next(b for b in res.module_definitions() if b.name == "greet")
    # 'greet' starts at column 4 (1-based) on line 1
    assert b.def_span.line == 1 and b.def_span.col == 4 and b.def_span.length == 5


def _def_of_use(res, line, col):
    ref = res.ref_at(line, col)
    return res.binding_of(ref)


def test_local_use_resolves_to_param():
    src = "fn id(x!: Int64) -> Int64 {\n    x\n}\n"
    res = _resolve(src)
    b = _def_of_use(res, 2, 5)          # the `x` on line 2
    assert b is not None and b.kind == "param" and b.name == "x"


def test_shadowing_prefers_inner_binding():
    src = (
        "let x = 1\n"
        "fn f() -> Int64 {\n"
        "    let x = 2\n"
        "    x\n"
        "}\n"
    )
    res = _resolve(src)
    inner = _def_of_use(res, 4, 5)       # `x` on line 4 -> inner let (line 3)
    assert inner.def_span.line == 3


def test_call_resolves_callee():
    src = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    res = _resolve(src)
    b = _def_of_use(res, 2, 22)          # `g` in the call on line 2
    assert b is not None and b.name == "g" and b.kind == "fn"


def test_member_reference_is_pending():
    src = "fn f(s!: String) -> Int64 { s.count }\n"
    res = _resolve(src)
    ref = res.ref_at(1, 31)              # `count` after the dot
    assert ref is not None and ref.ctx == "member" and ref.binding_id is None


def test_return_type_reference_resolves():
    src = (
        "type Point(x: Int64) {}\n"
        "fn make() -> Point { Point(x: 1) }\n"
    )
    res = _resolve(src)
    b = _def_of_use(res, 2, 14)          # `Point` in `-> Point` on line 2
    assert b is not None and b.name == "Point" and b.kind == "type"


def test_field_default_resolves_member():
    src = (
        "type Box(n: Int64) {\n"
        "    mut count = n\n"
        "}\n"
    )
    res = _resolve(src)
    b = _def_of_use(res, 2, 17)          # `n` on the RHS of `mut count = n`
    assert b is not None and b.owner == "Box"


def test_same_name_field_and_ctor_param_unify():
    # `type Box(n: Int64) { mut n = n }` -- the ctor param `n` and the
    # same-name field `n` are ONE storage slot at runtime (construct()
    # copies the ctor arg into inst.fields["n"], then field defaults read
    # from inst.fields). The resolver must unify them into a single
    # binding rather than creating two "n" bindings on Box.
    src = (
        "type Box(n: Int64) {\n"
        "    mut n = n\n"
        "}\n"
    )
    res = _resolve(src)
    n_bindings = [b for b in res.bindings.values()
                  if b.name == "n" and b.owner == "Box"]
    assert len(n_bindings) == 1
    b = n_bindings[0]
    assert b.def_span.line == 1              # the ctor-param site `Box(n: Int64)`


def test_same_name_member_refs_cover_both_sites():
    # `mut n = n` on line 2 (4-space indent): `mut` at col 5, the field's
    # own name `n` at col 9, `=` at col 11, the RHS `n` at col 13.
    src = (
        "type Box(n: Int64) {\n"
        "    mut n = n\n"
        "}\n"
    )
    res = _resolve(src)
    n_bindings = [b for b in res.bindings.values()
                  if b.name == "n" and b.owner == "Box"]
    assert len(n_bindings) == 1
    binding_id = n_bindings[0].id

    rhs_def = _def_of_use(res, 2, 13)          # RHS `n` after `=`
    assert rhs_def is not None and rhs_def.id == binding_id

    refs = res.refs_to(binding_id)
    ref_spans = {(r.span.line, r.span.col) for r in refs}
    assert (2, 9) in ref_spans                 # the `mut n` declaration site
    assert (2, 13) in ref_spans                # the RHS `n`


def test_match_binding_is_scoped_to_arm():
    # Each arm's pattern binding is private to that arm: two arms binding the
    # same name `v` resolve to two *distinct* bindings, not one shared binding.
    src = (
        "type Box(v: Int64) {}\n"
        "type Cup(v: Int64) {}\n"
        "fn f(b!: Box) -> Int64 {\n"
        "    match b {\n"
        "        Box(v) { v }\n"
        "        Cup(v) { v }\n"
        "    }\n"
        "}\n"
    )
    res = _resolve(src)
    arm1 = _def_of_use(res, 5, 18)       # `v` in the first arm body
    arm2 = _def_of_use(res, 6, 18)       # `v` in the second arm body
    assert arm1 is not None and arm1.kind == "pattern" and arm1.name == "v"
    assert arm2 is not None and arm2.kind == "pattern" and arm2.name == "v"
    assert arm1.id != arm2.id            # scoped per arm, not a shared binding


def test_constructor_pattern_references_type():
    src = (
        "type Box(v: Int64) {}\n"
        "fn f(b!: Box) -> Int64 { match b { Box(v) { v } } }\n"
    )
    res = _resolve(src)
    ref = res.ref_at(2, 36)              # `Box` constructor in the pattern
    assert ref is not None and ref.ctx == "pattern"
    assert res.binding_of(ref).name == "Box"


def test_named_arg_label_links_to_param():
    src = (
        "fn box(width!: Int64, height!: Int64) -> Int64 { width }\n"
        "fn main() -> Int64 { box(width: 3, height: 4) }\n"
    )
    res = _resolve(src)
    ref = res.ref_at(2, 26)              # the `width` label in the call
    assert ref is not None and ref.ctx == "label"
    b = res.binding_of(ref)
    assert b is not None and b.kind == "param" and b.name == "width"


def test_top_level_let_binding_is_deduped():
    # A module-scope `let` must produce exactly ONE binding: the module-level
    # one created by _declare_top_level, reused (not duplicated) by the body
    # walk, so def/uses/refs/rename all share a single id.
    src = "let base = 10\nfn g() -> Int64 { base }\n"
    res = _resolve(src)
    # Count only in-file bindings (def_span is not None): the resolver also
    # seeds an external placeholder named "base" for the checker's global
    # value, which is unrelated to the body-walk duplicate we deduped.
    base_bindings = [b for b in res.bindings.values()
                     if b.name == "base" and b.def_span is not None]
    assert len(base_bindings) == 1
    the_binding = base_bindings[0]
    # The `base` use inside `g` resolves to that same (module-level) binding.
    used = _def_of_use(res, 2, 20)      # `base` use is at cols 19-22
    assert used is not None and used.id == the_binding.id
    # find-refs from that binding includes the use inside `g`.
    assert any(r.span.line == 2 for r in res.refs_to(the_binding.id))


def test_interpolation_identifier_resolves_to_file_position():
    src = 'fn serve(host!: String) -> Int64 {\n    println("{host}")\n    0\n}\n'
    res = _resolve(src)
    b = _def_of_use(res, 2, 15)          # the `host` inside "{host}" on line 2
    assert b is not None and b.name == "host" and b.kind == "param"


def test_multiline_interpolation_span_offsets_to_file_position():
    # A multi-line interpolation *expression* exercises both offset branches:
    # `a` sits on the fragment's first line, `b` continues on its second.
    src = 'fn f(a!: Int64, b!: Int64) -> String {\n    "{a +\nb}"\n}\n'
    res = _resolve(src)
    a = _def_of_use(res, 2, 7)           # `a` inside the string on line 2
    b = _def_of_use(res, 3, 1)           # `b` continues on line 3
    assert a is not None and a.name == "a" and a.kind == "param"
    assert b is not None and b.name == "b" and b.kind == "param"


def test_nested_interpolation_span_offsets_to_file_position():
    # A string interpolation nested inside another: the inner `host` must still
    # resolve at its true file column, not collapse to the fragment origin.
    src = (
        'fn f(x!: String) -> String { x }\n'
        'fn g(host!: String) -> String {\n'
        '    "{f("{host}")}"\n'
        '}\n'
    )
    res = _resolve(src)
    host = _def_of_use(res, 3, 11)       # the `host` inside the nested "{host}"
    assert host is not None and host.name == "host" and host.kind == "param"


def test_import_binding_def_span_is_on_the_name():
    res = _resolve('import net from "./Net"\n', "m.thx")
    b = next(b for b in res.bindings.values()
             if b.kind == "import" and b.name == "net")
    # "net" starts at column 8 (1-based) on line 1
    assert (b.def_span.line, b.def_span.col) == (1, 8)
