"""Module system — M-MOD-1: cross-file resolution, selective import, export-gating.

Programs that span files need real paths on disk, so most tests write into
pytest's `tmp_path` and run the entry file by its path (that is how the project
root and `./` resolution are derived).
"""

from __future__ import annotations

import pytest

from thoxa import ast, cli
from thoxa.checker import check
from thoxa.errors import Source, Span, ThoxaError
from thoxa.interp import run
from thoxa.lexer import Lexer
from thoxa.loader import Loader, LoaderError
from thoxa.parser import Parser


def _write(tmp_path, name, text):
    p = tmp_path / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")
    return p


def _run(path):
    buf: list[str] = []
    code = run(path.read_text(encoding="utf-8"), str(path), out=buf.append)
    return "".join(buf), code


def _parse(src):
    s = Source(src, "<test>")
    return Parser(Lexer(s).tokenize(), s).parse_program()


def _msg(exc: ThoxaError) -> str:
    return exc.diag.message


# ------------------------------------------------------------------ parser: export
def test_export_let_and_extern_parse():
    mod = _parse(
        "export let answer = 42\n"
        'export extern fn getpid() -> Int32 from "c"\n'
        "let hidden = 1\n"
        'extern fn write(fd!: Int32) -> Int64 from "c"\n'
    )
    lets = {s.name: s for s in mod.body if isinstance(s, ast.LetBinding)}
    externs = {s.name: s for s in mod.body if isinstance(s, ast.ExternFn)}
    assert lets["answer"].exported is True
    assert lets["hidden"].exported is False
    assert externs["getpid"].exported is True
    assert externs["write"].exported is False


def test_export_mut_is_rejected():
    with pytest.raises(ThoxaError) as e:
        _parse("export mut count = 0")
    assert "cannot be exported" in _msg(e.value)


# ------------------------------------------------------------------ resolution
def test_resolve_local_and_project():
    ld = Loader(project_root="/proj")
    local = ld.resolve("./sockets", "/proj/net", Span(1, 1))
    assert local.kind == "file" and local.path == "/proj/net/sockets.thx"
    proj = ld.resolve("~/config", "/proj/net", Span(1, 1))
    assert proj.kind == "file" and proj.path == "/proj/config.thx"


def test_resolve_upward_is_rejected():
    ld = Loader(project_root="/proj")
    with pytest.raises(LoaderError) as e:
        ld.resolve("../secrets", "/proj/net", Span(1, 1))
    assert "upward imports aren't allowed" in str(e.value)
    with pytest.raises(LoaderError):
        ld.resolve("./a/../b", "/proj/net", Span(1, 1))


def test_resolve_unknown_root():
    ld = Loader(project_root="/proj")
    with pytest.raises(LoaderError) as e:
        ld.resolve("weird/thing", "/proj", Span(1, 1))
    assert "unknown module root" in str(e.value)


def test_resolve_named_roots():
    ld = Loader(project_root="/proj")
    ffi = ld.resolve("ffi", "/proj", Span(1, 1))
    assert ffi.kind == "builtin" and ffi.key == "ffi"
    # `std` resolves against the bundled stdlib dir; the file need not exist yet.
    std = ld.resolve("std/Math", "/proj", Span(1, 1))
    assert std.kind == "file" and std.path.endswith("/std/Math.thx")
    assert "thoxa" in std.path


# ------------------------------------------------------------------ interpreter
def test_selective_import_happy_path(tmp_path):
    _write(tmp_path, "MathUtils.thx", "export fn add(a: Int64, b: Int64) -> Int64 { a + b }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import { add } from "./MathUtils"\n'
        'fn main() -> Int64 { println("{add(a: 2, b: 3)}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "5\n" and code == 0


def test_import_private_name_is_rejected(tmp_path):
    _write(
        tmp_path,
        "Lib.thx",
        "export fn pub() -> Int64 { 1 }\nfn priv() -> Int64 { 2 }\n",
    )
    entry = _write(tmp_path, "main.thx", 'import { priv } from "./Lib"\nfn main() -> Int64 { priv() }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'priv' is not exported by './Lib'" in _msg(e.value)


def test_import_undefined_name_is_distinct(tmp_path):
    _write(tmp_path, "Lib.thx", "export fn pub() -> Int64 { 1 }\n")
    entry = _write(tmp_path, "main.thx", 'import { ghost } from "./Lib"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'ghost' is not defined in './Lib'" in _msg(e.value)


def test_transitive_import(tmp_path):
    _write(tmp_path, "C.thx", "export fn cval() -> Int64 { 3 }\n")
    _write(tmp_path, "B.thx", 'import { cval } from "./C"\nexport fn bval() -> Int64 { cval() + 10 }\n')
    _write(tmp_path, "A.thx", 'import { bval } from "./B"\nexport fn aval() -> Int64 { bval() + 100 }\n')
    entry = _write(
        tmp_path,
        "main.thx",
        'import { aval } from "./A"\nfn main() -> Int64 { println("{aval()}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "113\n" and code == 0


def test_upward_import_rejected_in_program(tmp_path):
    entry = _write(tmp_path, "main.thx", 'import { x } from "../up"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "upward imports aren't allowed" in _msg(e.value)


def test_unknown_root_in_program(tmp_path):
    entry = _write(tmp_path, "main.thx", 'import { x } from "nope/y"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "unknown module root" in _msg(e.value)


def test_namespace_import_happy_path(tmp_path):
    _write(
        tmp_path,
        "Strings.thx",
        "export fn shout(s: String) -> String { s }\n"
        "export type Builder(seed: Int64) { export fn value() -> Int64 { seed } }\n"
        'export let greeting = "hi"\n',
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\n'
        "fn main() -> Int64 {\n"
        '  println("{strings.shout(s: "hey")}")\n'
        "  let b = strings.Builder(seed: 7)\n"
        '  println("{b.value()}")\n'
        '  println("{strings.greeting}")\n'
        "  0\n"
        "}\n",
    )
    out, code = _run(entry)
    assert out == "hey\n7\nhi\n" and code == 0


def test_fn_cycle_across_files_is_accepted_and_runs(tmp_path):
    # M-MOD-4 (§7): `fn` mutual recursion across files is legitimate — a cycle is
    # no longer a hard error. The names resolve in phase-1 registration.
    _write(
        tmp_path,
        "Evens.thx",
        'import { isOdd } from "./Odds"\n'
        "export fn isEven(n: Int64) -> Bool { if n == 0 { true } else { isOdd(n: n - 1) } }\n",
    )
    _write(
        tmp_path,
        "Odds.thx",
        'import { isEven } from "./Evens"\n'
        "export fn isOdd(n: Int64) -> Bool { if n == 0 { false } else { isEven(n: n - 1) } }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { isEven } from "./Evens"\n'
        'fn main() -> Int64 { println("{isEven(n: 8)}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "true\n" and code == 0


def test_module_top_level_is_declarations_only(tmp_path):
    _write(tmp_path, "Lib.thx", 'export fn pub() -> Int64 { 1 }\nprintln("side effect")\n')
    entry = _write(tmp_path, "main.thx", 'import { pub } from "./Lib"\nfn main() -> Int64 { pub() }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "a module's top level is declarations only" in _msg(e.value)


def test_ffi_import_still_works():
    # `ffi` routes through the resolver as a built-in root; no file needed.
    buf: list[str] = []
    code = run(
        'import { errno } from "ffi"\nfn main() -> Int64 { errno()  0 }\n',
        "<test>",
        out=buf.append,
    )
    assert code == 0


def test_exported_type_and_let_cross_files(tmp_path):
    _write(
        tmp_path,
        "Geo.thx",
        "export type Point(x: Int64, y: Int64) { export fn sum() -> Int64 { x + y } }\n"
        "export let origin = Point(x: 0, y: 0)\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { Point, origin } from "./Geo"\n'
        'fn main() -> Int64 {\n'
        '  let p = Point(x: 3, y: 4)\n'
        '  println("{p.sum()} {origin.sum()}")\n'
        "  0\n"
        "}\n",
    )
    out, code = _run(entry)
    assert out == "7 0\n" and code == 0


# ------------------------------------------------------------------ checker
def test_checker_accepts_cross_file_import(tmp_path):
    _write(tmp_path, "Lib.thx", "export fn pub() -> Int64 { 1 }\n")
    entry = _write(tmp_path, "main.thx", 'import { pub } from "./Lib"\nfn main() -> Int64 { pub() }\n')
    errors = check(entry.read_text(encoding="utf-8"), str(entry))
    assert errors == []


def test_checker_flags_non_exported_import(tmp_path):
    _write(tmp_path, "Lib.thx", "export fn pub() -> Int64 { 1 }\nfn priv() -> Int64 { 2 }\n")
    entry = _write(tmp_path, "main.thx", 'import { priv } from "./Lib"\nfn main() -> Int64 { 0 }\n')
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("'priv' is not exported by './Lib'" in m for m in msgs)


def test_checker_flags_upward_and_unknown_root(tmp_path):
    entry = _write(
        tmp_path,
        "main.thx",
        'import { a } from "../up"\nimport { b } from "weird/x"\nfn main() -> Int64 { 0 }\n',
    )
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("upward imports aren't allowed" in m for m in msgs)
    assert any("unknown module root" in m for m in msgs)


# ============================================================ M-MOD-2: aliasing
def test_selective_alias_binds_local_name(tmp_path):
    _write(tmp_path, "Lib.thx", "export fn origVal() -> Int64 { 7 }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import { origVal as val } from "./Lib"\n'
        'fn main() -> Int64 { println("{val()}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "7\n" and code == 0


def test_selective_alias_hides_the_original_name(tmp_path):
    _write(tmp_path, "Lib.thx", "export fn origVal() -> Int64 { 7 }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import { origVal as val } from "./Lib"\nfn main() -> Int64 { origVal() }\n',
    )
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'origVal' is not defined" in _msg(e.value)


def test_alias_casing_mismatch_is_rejected():
    with pytest.raises(ThoxaError) as e:
        _parse('import { foo as Bar } from "./Lib"')
    assert "can't change casing kind" in _msg(e.value)
    with pytest.raises(ThoxaError) as e:
        _parse('import { Foo as bar } from "./Lib"')
    assert "can't change casing kind" in _msg(e.value)


def test_alias_same_casing_is_allowed():
    lower = _parse('import { foo as bar } from "./Lib"').body[0]
    upper = _parse('import { Foo as Bar } from "./Lib"').body[0]
    assert lower.names == [("foo", "bar")]
    assert upper.names == [("Foo", "Bar")]


# ================================================= M-MOD-2: namespace semantics






def test_namespace_used_as_a_value_is_rejected(tmp_path):
    _write(tmp_path, "Strings.thx", "export fn shout(s: String) -> String { s }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\nfn main() -> Int64 { let x = strings\n 0 }\n',
    )
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "a module namespace cannot be used as a value" in _msg(e.value)


def test_namespace_member_gating_hides_private_member(tmp_path):
    _write(
        tmp_path,
        "Strings.thx",
        "export fn shout(s: String) -> String { s }\nfn secret() -> Int64 { 99 }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\n'
        'fn main() -> Int64 { println("{strings.secret()}")  0 }\n',
    )
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'secret' is not exported by './Strings'" in _msg(e.value)


def test_namespace_undefined_member_is_distinct(tmp_path):
    _write(tmp_path, "Strings.thx", "export fn shout(s: String) -> String { s }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\n'
        'fn main() -> Int64 { println("{strings.ghost()}")  0 }\n',
    )
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'ghost' is not defined in './Strings'" in _msg(e.value)


# ---------------------------------------------------------- M-MOD-2: checker
def test_checker_accepts_namespace_member_access(tmp_path):
    _write(
        tmp_path,
        "Strings.thx",
        "export fn shout(s: String) -> String { s }\n"
        "export type Builder(seed: Int64) { export fn value() -> Int64 { seed } }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import str from "./Strings"\n'
        "fn main() -> Int64 {\n"
        '  let s = str.shout(s: "hi")\n'
        "  let b = str.Builder(seed: 1)\n"
        "  0\n"
        "}\n",
    )
    assert check(entry.read_text(encoding="utf-8"), str(entry)) == []


def test_checker_flags_namespace_as_value(tmp_path):
    _write(tmp_path, "Strings.thx", "export fn shout(s: String) -> String { s }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\nfn main() -> Int64 { let x = strings\n 0 }\n',
    )
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("a module namespace cannot be used as a value" in m for m in msgs)


def test_checker_flags_namespace_private_member(tmp_path):
    _write(
        tmp_path,
        "Strings.thx",
        "export fn shout(s: String) -> String { s }\nfn secret() -> Int64 { 1 }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import strings from "./Strings"\n'
        'fn main() -> Int64 { println("{strings.secret()}")  0 }\n',
    )
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("'secret' is not exported by './Strings'" in m for m in msgs)





# ============================================= M-MOD-3: the closed-prelude module
# The prelude is now a real `std/prelude` module auto-imported into every file, and
# `sqrt` (and friends) are no longer ambient — they live in `std/math`.
_PRELUDE_ONLY = (
    "fn find(xs!: Array<Int64>, target!: Int64) -> May<Int64> {\n"
    "    mut i = 0\n"
    "    for x in xs {\n"
    "        if x == target { return Has(i) }\n"
    "        i = i + 1\n"
    "    }\n"
    "    Not()\n"
    "}\n"
    "fn describe(m!: May<Int64>) -> String {\n"
    "    match m {\n"
    '        Has(idx) { "found at {idx}" }\n'
    '        Not { "not found" }\n'
    "    }\n"
    "}\n"
    "fn main() -> Int64 {\n"
    "    let xs = [10, 20, 30]\n"
    "    println(describe(find(xs, 20)))\n"
    "    0\n"
    "}\n"
)


def test_prelude_only_program_checks_and_runs_with_no_imports():
    # May/Has/Not/Array/println are the closed prelude — no import needed (§4.2).
    assert check(_PRELUDE_ONLY, "<test>") == []
    buf: list[str] = []
    code = run(_PRELUDE_ONLY, "<test>", out=buf.append)
    assert "".join(buf) == "found at 1\n" and code == 0


def test_std_math_sqrt_is_importable_and_computes():
    src = (
        'import { sqrt } from "std/Math"\n'
        'fn main() -> Int64 {\n'
        '    println("{sqrt(9.0)}")\n'
        "    0\n"
        "}\n"
    )
    assert check(src, "<test>") == []
    buf: list[str] = []
    code = run(src, "<test>", out=buf.append)
    assert "".join(buf) == "3.0\n" and code == 0


def test_bare_sqrt_without_import_is_undefined_in_checker():
    src = 'fn main() -> Int64 { println("{sqrt(9.0)}")  0 }\n'
    msgs = [d.message for d in check(src, "<test>")]
    assert any("'sqrt' is not defined" in m for m in msgs)


def test_bare_sqrt_without_import_is_a_runtime_error():
    src = 'fn main() -> Int64 { println("{sqrt(9.0)}")  0 }\n'
    with pytest.raises(ThoxaError) as e:
        run(src, "<test>", out=lambda _s: None)
    assert "'sqrt' is not defined" in _msg(e.value)


def test_prelude_explicit_import_resolves():
    # Redundant with the auto-import, but the prelude is a real module you may name.
    src = (
        'import { Has, Not } from "std/Prelude"\n'
        "fn main() -> Int64 {\n"
        "    let m = Has(5)\n"
        '    println(match m { Has(v) { "{v}" } Not { "none" } })\n'
        "    0\n"
        "}\n"
    )
    assert check(src, "<test>") == []
    buf: list[str] = []
    code = run(src, "<test>", out=buf.append)
    assert "".join(buf) == "5\n" and code == 0


# ================================= M-MOD-4: circular imports & module state (§7, §9)
def test_type_cycle_across_files_is_accepted_and_runs(tmp_path):
    # `type` mutual recursion across files is allowed: each type's method uses the
    # other's exported type, resolved from phase-1 registration (§7).
    _write(
        tmp_path,
        "Ping.thx",
        'import { Pong } from "./Pong"\n'
        "export type Ping(depth: Int64) {\n"
        "  export fn play() -> Int64 { if depth == 0 { 0 } else { Pong(depth: depth - 1).play() + 1 } }\n"
        "}\n",
    )
    _write(
        tmp_path,
        "Pong.thx",
        'import { Ping } from "./Ping"\n'
        "export type Pong(depth: Int64) {\n"
        "  export fn play() -> Int64 { if depth == 0 { 0 } else { Ping(depth: depth - 1).play() + 1 } }\n"
        "}\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { Ping } from "./Ping"\n'
        'fn main() -> Int64 { println("{Ping(depth: 5).play()}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "5\n" and code == 0


def test_value_initializer_cycle_is_rejected(tmp_path):
    # Two files whose top-level `let`s reference each other's exported `let`: no
    # valid init order exists, so it is a located error naming the cycle (§7).
    _write(tmp_path, "A.thx", 'import { bVal } from "./B"\nexport let aVal = bVal + 1\n')
    _write(tmp_path, "B.thx", 'import { aVal } from "./A"\nexport let bVal = aVal + 1\n')
    entry = _write(tmp_path, "main.thx", 'import { aVal } from "./A"\nfn main() -> Int64 { aVal }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "value-initializer cycle: A -> B -> A" in _msg(e.value)


def test_namespace_value_member_in_initializer_inits_target(tmp_path):
    # A module-level `let` that reads another module's value through a *namespace*
    # must initialize that target first. Regression: a namespace import does not
    # itself force init, so this used to crash with an uncaught Undefined.
    _write(
        tmp_path,
        "Geometry.thx",
        "export type Point(x: Int64, y: Int64) { export fn sum() -> Int64 { x + y } }\n"
        "export let origin = Point(x: 1, y: 2)\n",
    )
    _write(
        tmp_path,
        "A.thx",
        'import geometry from "./Geometry"\n'
        "let g = geometry.origin\n"
        "export fn av() -> Int64 { g.sum() }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { av } from "./A"\nfn main() -> Int64 { println("{av()}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "3\n" and code == 0


def test_namespace_mediated_value_cycle_is_rejected(tmp_path):
    # A value cycle reached through namespace member access (not a selective
    # import) is caught the same way — a located error, not a crash (§7).
    _write(tmp_path, "A.thx", 'import b from "./B"\nexport let av = b.bv\n')
    _write(tmp_path, "B.thx", 'import a from "./A"\nexport let bv = a.av\n')
    entry = _write(
        tmp_path, "main.thx", 'import { av } from "./A"\nfn main() -> Int64 { av }\n'
    )
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "value-initializer cycle" in _msg(e.value)


def test_module_mut_counter_accumulates(tmp_path):
    # Module-level private `mut`: one cell, initialized once, mutated only through
    # exported functions; state accumulates across calls (§9).
    _write(
        tmp_path,
        "Counter.thx",
        "mut count = 0\n"
        "export fn bump() -> Int64 { count = count + 1  count }\n"
        "export fn current() -> Int64 { count }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { bump, current } from "./Counter"\n'
        "fn main() -> Int64 {\n"
        "  bump()\n"
        "  bump()\n"
        '  println("{current()}")\n'
        "  0\n"
        "}\n",
    )
    out, code = _run(entry)
    assert out == "2\n" and code == 0


def test_module_mut_is_not_importable(tmp_path):
    # A top-level `mut` is always private (§9): it is declared but never exported.
    _write(
        tmp_path,
        "Counter.thx",
        "mut count = 0\nexport fn bump() -> Int64 { count = count + 1  count }\n",
    )
    entry = _write(tmp_path, "main.thx", 'import { count } from "./Counter"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "'count' is not exported by './Counter'" in _msg(e.value)


def test_use_before_import_within_a_file(tmp_path):
    # Imports bind before any initializer runs, so a top-level `let` may use an
    # imported value even when the `import` sits later in the file.
    _write(tmp_path, "Nums.thx", "export let base = 10\n")
    _write(
        tmp_path,
        "Helper.thx",
        "export let derived = base + 5\n"  # uses `base` before its import below
        'import { base } from "./Nums"\n',
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { derived } from "./Helper"\n'
        'fn main() -> Int64 { println("{derived}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "15\n" and code == 0


# ============================================================ review-fix regressions
# FIX 1 — an entry re-imported by one of its dependencies must not load twice.
def test_entry_reimported_by_dependency_initializes_once(tmp_path):
    # `lib` imports the entry back via `~/e`; the entry's `let banner` initializer
    # prints a marker. Before the fix the entry registered a second time with a
    # fresh env and the marker printed twice.
    _write(tmp_path, "Lib.thx", 'import { ping } from "~/e"\nexport fn helper() -> Int64 { ping() }\n')
    entry = _write(
        tmp_path,
        "e.thx",
        'import { helper } from "./Lib"\n'
        'fn announce() -> Int64 { println("once")  0 }\n'
        "export let banner = announce()\n"
        "export fn ping() -> Int64 { 42 }\n",
    )
    out, _code = _run(entry)
    assert out.count("once\n") == 1 and out == "once\n"


def test_entry_reimported_by_dependency_shares_one_state_cell(tmp_path):
    # A private `mut counter` mutated through exported functions is one cell.
    # `peeker` imports `current` back from `~/e`; both views must read the same 2.
    _write(tmp_path, "Peeker.thx", 'import { current } from "~/e"\nexport fn peek() -> Int64 { current() }\n')
    entry = _write(
        tmp_path,
        "e.thx",
        'import { peek } from "./Peeker"\n'
        "mut counter = 0\n"
        "export fn bump() -> Int64 { counter = counter + 1  counter }\n"
        "export fn current() -> Int64 { counter }\n"
        "fn main() -> Int64 {\n"
        "  bump()\n"
        "  bump()\n"
        '  println("{current()}")\n'
        '  println("{peek()}")\n'
        "  0\n"
        "}\n",
    )
    out, code = _run(entry)
    assert out == "2\n2\n" and code == 0


# FIX 2 — a control-flow signal escaping a top-level statement is a located error.
def test_top_level_try_in_imported_module_is_a_located_error(tmp_path):
    _write(tmp_path, "Boom.thx", "export let bad = Fail(1)?\n")
    entry = _write(tmp_path, "main.thx", 'import { bad } from "./Boom"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "only valid inside a function" in _msg(e.value)


def test_top_level_try_in_entry_body_is_a_located_error():
    with pytest.raises(ThoxaError) as e:
        run("Fail(1)?\nfn main() -> Int64 { 0 }\n", "<test>", out=lambda _s: None)
    assert "only valid inside a function" in _msg(e.value)


# FIX 3 — path escapes: an absolute `rel` (from `//`) and `..` in a named root.
def test_absolute_local_rel_and_std_dotdot_are_rejected():
    ld = Loader(project_root="/proj")
    with pytest.raises(LoaderError) as e:
        ld.resolve(".//tmp/whatever", "/proj/net", Span(1, 1))
    assert "upward imports aren't allowed" in str(e.value)
    with pytest.raises(LoaderError):
        ld.resolve("std/../foo", "/proj", Span(1, 1))


def test_absolute_local_rel_in_program_is_rejected(tmp_path):
    entry = _write(tmp_path, "main.thx", 'import { x } from ".//tmp/whatever"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "upward imports aren't allowed" in _msg(e.value)


# FIX 4 — a NUL byte in a specifier is a diagnostic, not an uncaught ValueError.
def test_nul_byte_in_specifier_is_a_located_error():
    with pytest.raises(ThoxaError) as e:
        run('import { x } from "std/A\0b"\nfn main() -> Int64 { 0 }\n', "<test>", out=lambda _s: None)
    assert "cannot find module" in _msg(e.value)


# FIX 5 — `ffi/anything` is not `ffi`; a subpath is a mistake, bare `ffi` still works.
def test_ffi_subpath_is_rejected_but_bare_ffi_works():
    ld = Loader(project_root="/proj")
    with pytest.raises(LoaderError) as e:
        ld.resolve("ffi/x", "/proj", Span(1, 1))
    assert "not a package" in str(e.value)
    r = ld.resolve("ffi", "/proj", Span(1, 1))
    assert r.kind == "builtin" and r.key == "ffi"


def test_ffi_subpath_in_program_is_rejected():
    with pytest.raises(ThoxaError) as e:
        run('import { errno } from "ffi/x"\nfn main() -> Int64 { 0 }\n', "<test>", out=lambda _s: None)
    assert "not a package" in _msg(e.value)


# FIX 6 — no false "has no member" on a cross-module type with no member table.
def test_checker_conservative_on_cross_module_type_member(tmp_path):
    _write(
        tmp_path,
        "Lib.thx",
        "export type Box(n: Int64) { export fn get() -> Int64 { n } }\n"
        "export fn makeBox(n: Int64) -> Box { Box(n: n) }\n",
    )
    entry = _write(
        tmp_path,
        "main.thx",
        'import { makeBox } from "./Lib"\n'
        "fn main() -> Int64 {\n"
        "  let b = makeBox(n: 5)\n"
        '  println("{b.get()}")\n'
        "  0\n"
        "}\n",
    )
    assert check(entry.read_text(encoding="utf-8"), str(entry)) == []
    out, code = _run(entry)
    assert out == "5\n" and code == 0


# FIX 7 — the checker enforces declarations-only on an imported module.
def test_checker_flags_non_declaration_in_imported_module(tmp_path):
    _write(tmp_path, "Lib.thx", 'export fn pub() -> Int64 { 1 }\nprintln("side effect")\n')
    entry = _write(tmp_path, "main.thx", 'import { pub } from "./Lib"\nfn main() -> Int64 { pub() }\n')
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("a module's top level is declarations only" in m for m in msgs)


# FIX 8 — the checker checks parameter default expressions.
def test_checker_checks_parameter_default_expression():
    msgs = [d.message for d in check("fn f(x: Int64 = doesNotExist) -> Int64 { x }\n", "<test>")]
    assert any("'doesNotExist' is not defined" in m for m in msgs)


# FIX 9 — a legal self-import is accepted by the checker (and runs).
def test_checker_accepts_self_import(tmp_path):
    entry = _write(
        tmp_path,
        "main.thx",
        'import { ping } from "~/main"\n'
        "export fn ping() -> Int64 { 42 }\n"
        'fn main() -> Int64 { println("{ping()}")  0 }\n',
    )
    assert check(entry.read_text(encoding="utf-8"), str(entry)) == []
    out, code = _run(entry)
    assert out == "42\n" and code == 0


# ========================== PascalCase modules + free camelCase bindings (2026-07-13)
# A module file describes a type (PascalCase); a namespace import binds a singleton
# instance under a freely-chosen camelCase name. The entry (a program, not an
# importable module) is exempt from the PascalCase rule. See
# docs/specs/2026-07-13-thoxa-pascalcase-modules-and-main-entrypoint-design.md.
def test_namespace_binding_is_free_camelcase(tmp_path):
    # The binding need not match the file name — the importer names the instance.
    _write(tmp_path, "MathUtils.thx", "export fn add(a: Int64, b: Int64) -> Int64 { a + b }\n")
    entry = _write(
        tmp_path,
        "main.thx",
        'import mu from "./MathUtils"\n'
        'fn main() -> Int64 { println("{mu.add(a: 2, b: 3)}")  0 }\n',
    )
    out, code = _run(entry)
    assert out == "5\n" and code == 0


def test_pascalcase_module_required_at_runtime(tmp_path):
    _write(tmp_path, "mathUtils.thx", "export fn add(a: Int64, b: Int64) -> Int64 { a + b }\n")
    entry = _write(tmp_path, "main.thx", 'import { add } from "./mathUtils"\nfn main() -> Int64 { 0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "PascalCase" in _msg(e.value)


def test_pascalcase_module_required_checker(tmp_path):
    _write(tmp_path, "mathUtils.thx", "export fn add(a: Int64, b: Int64) -> Int64 { a + b }\n")
    entry = _write(tmp_path, "main.thx", 'import { add } from "./mathUtils"\nfn main() -> Int64 { 0 }\n')
    msgs = [d.message for d in check(entry.read_text(encoding="utf-8"), str(entry))]
    assert any("PascalCase" in m for m in msgs)


def test_namespace_binding_pascalcase_is_rejected():
    with pytest.raises(ThoxaError) as e:
        _parse('import Http from "./Http"')
    assert "instance" in _msg(e.value) and "camelCase" in _msg(e.value)


def test_namespace_as_clause_is_removed():
    # Free naming makes namespace `as` redundant; it is no longer accepted.
    with pytest.raises(ThoxaError) as e:
        _parse('import http as h from "./Http"')
    assert "'from'" in _msg(e.value)


def test_selective_as_still_works():
    mod = _parse('import { add as sum } from "./MathUtils"').body[0]
    assert mod.names == [("add", "sum")]


# ------------------------------------------------------------- Main.thx entrypoint
def test_resolve_entry_directory_finds_main(tmp_path):
    main = _write(tmp_path, "Main.thx", "export fn main() -> Int64 { 0 }\n")
    assert cli.resolve_entry(str(tmp_path)) == str(main)


def test_resolve_entry_directory_without_main_errors(tmp_path):
    with pytest.raises(cli.EntryNotFound) as e:
        cli.resolve_entry(str(tmp_path))
    assert "Main.thx" in str(e.value)


def test_resolve_entry_file_is_unchanged(tmp_path):
    f = _write(tmp_path, "hello.thx", "fn main() -> Int64 { 0 }\n")
    assert cli.resolve_entry(str(f)) == str(f)


def test_main_thx_must_export_main(tmp_path):
    # A Main.thx entry must export `main` — the runtime calls it from outside.
    entry = _write(tmp_path, "Main.thx", 'fn main() -> Int64 { println("hi")  0 }\n')
    with pytest.raises(ThoxaError) as e:
        _run(entry)
    assert "Main.thx must export" in _msg(e.value)


def test_main_thx_with_exported_main_runs(tmp_path):
    entry = _write(tmp_path, "Main.thx", 'export fn main() -> Int64 { println("ok")  0 }\n')
    out, code = _run(entry)
    assert out == "ok\n" and code == 0
