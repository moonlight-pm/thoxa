import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run


def run_out(src):
    buf = []
    code = run(src, "<test>", out=buf.append)
    return "".join(buf), code


def test_arithmetic_and_interpolation():
    out, _ = run_out('fn main() -> Int64 { println("{2 + 3 * 4}") 0 }')
    assert out == "14\n"


def test_truncating_int_division():
    out, _ = run_out('fn main() -> Int64 { println("{7 / 2} {-7 / 2}") 0 }')
    assert out == "3 -3\n"


def test_main_exit_code():
    _, code = run_out("fn main() -> Int64 { 42 }")
    assert code == 42


def test_immutable_reassignment_is_error():
    with pytest.raises(ThoxaError):
        run_out("fn main() -> Int64 { let x = 1  x = 2  0 }")


def test_mutable_field_and_method():
    src = """
    type Box(n: Int64) {
        mut n = n
        export fn add(k: Int64) -> Void { n = n + k  Void }
        export fn get() -> Int64 { n }
    }
    fn main() -> Int64 {
        mut b = Box(n: 10)
        b.add(k: 5)
        println("{b.get()}")
        0
    }
    """
    out, _ = run_out(src)
    assert out == "15\n"


def test_private_member_access_is_error():
    src = """
    type Secret(x: Int64) { export fn ok() -> Int64 { x } }
    fn main() -> Int64 { let s = Secret(x: 1)  println("{s.x}")  0 }
    """
    with pytest.raises(ThoxaError):
        run_out(src)


def test_export_ctor_param_is_public():
    # `export` on a ctor param publishes the field under the same name — no body
    # re-export needed. Keyword call site preferred for self-documentation.
    src = """
    type Header(export name: String, export value: String) {}
    fn main() -> Int64 {
        let h = Header(name: "Host", value: "x")
        println("{h.name}={h.value}")
        0
    }
    """
    out, _ = run_out(src)
    assert out == "Host=x\n"


def test_export_ctor_param_and_body_export_coexist():
    # Body export let remains for renames / derived fields.
    src = """
    type Match(export r: Int64, c: Int64) {
        export let col = c
    }
    fn main() -> Int64 {
        let m = Match(r: 3, c: 7)
        println("{m.r}:{m.col}")
        0
    }
    """
    out, _ = run_out(src)
    assert out == "3:7\n"


def test_unexported_ctor_param_stays_private_alongside_export():
    src = """
    type Pair(export a: Int64, b: Int64) {}
    fn main() -> Int64 {
        let p = Pair(a: 1, b: 2)
        println("{p.b}")
        0
    }
    """
    with pytest.raises(ThoxaError):
        run_out(src)


def test_fallible_ctor_unwraps_on_success():
    # A `?` in a field initializer makes the constructor Trial-returning; on
    # success it yields Pass(instance), so `Widget(n: 5)?` unwraps to the instance.
    src = """
    fn validate(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    type Widget(n: Int64) {
        let checked = validate(n)?
        export fn value() -> Int64 { n }
    }
    fn build() -> Trial<Widget, String> {
        let w = Widget(n: 5)?
        Pass(w)
    }
    fn main() -> Int64 {
        match build() {
            Pass(w) { println("{w.value()}")  0 }
            Fail(e) { println("fail {e}")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "5\n"
    assert code == 0


def test_fallible_ctor_short_circuits_on_failure():
    # A failing field initializer makes construction yield Fail(e): the ctor
    # result is a Trial that pattern-matches as Fail, and the half-built
    # instance never escapes.
    src = """
    fn validate(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    type Widget(n: Int64) {
        let checked = validate(n)?
        export fn value() -> Int64 { n }
    }
    fn main() -> Int64 {
        match Widget(n: -1) {
            Pass(w) { println("ok {w.value()}")  0 }
            Fail(e) { println("caught {e}")  7 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "caught neg\n"
    assert code == 7


def test_infallible_ctor_is_not_a_trial():
    # No `?` in the ctor body -> the constructor returns a bare instance, not a
    # Trial. Applying `?` to it is an error, guarding against over-wrapping.
    src = """
    type Plain(n: Int64) { export fn get() -> Int64 { n } }
    fn main() -> Int64 { let p = Plain(n: 3)?  println("{p.get()}")  0 }
    """
    with pytest.raises(ThoxaError):
        run_out(src)


def test_function_infers_trial_from_question_mark():
    # No return annotation + a `?` in the body => the function auto-threads Trial:
    # the success tail is wrapped in Pass, so callers can `?`/match it.
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    fn doubled(n: Int64) {
        let x = check(n)?
        x * 2
    }
    fn main() -> Int64 {
        match doubled(n: 5) {
            Pass(v) { println("{v}")  0 }
            Fail(e) { println("f{e}")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "10\n"
    assert code == 0


def test_method_infers_trial_from_question_mark():
    # A method with a `?` and no annotation auto-threads Trial too.
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    type Doubler(n: Int64) {
        export fn run() {
            let x = check(n)?
            x * 2
        }
    }
    fn main() -> Int64 {
        match Doubler(n: 5).run() {
            Pass(v) { println("{v}")  0 }
            Fail(e) { println("f{e}")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "10\n"
    assert code == 0


def test_inferred_trial_function_short_circuits():
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    fn doubled(n: Int64) {
        let x = check(n)?
        x * 2
    }
    fn main() -> Int64 {
        match doubled(n: -1) {
            Pass(v) { println("ok {v}")  0 }
            Fail(e) { println("caught {e}")  7 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "caught neg\n"
    assert code == 7


def test_function_returning_trial_tail_needs_no_wrap():
    # accept-style: no `?`, no annotation, but the tail is already a Trial. The
    # function returns it as-is (no double-wrap), so `passthru(5)` matches as Pass.
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    fn passthru(n: Int64) { check(n) }
    fn main() -> Int64 {
        match passthru(n: 5) {
            Pass(v) { println("{v}")  0 }
            Fail(e) { println("f{e}")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "5\n"
    assert code == 0


def test_inferred_trial_tail_is_not_double_wrapped():
    # A function with a `?` whose tail is itself a Trial must not be wrapped twice.
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    fn chained(n: Int64) {
        let x = check(n)?
        check(n: x + 1)
    }
    fn main() -> Int64 {
        match chained(n: 5) {
            Pass(v) { println("{v}")  0 }
            Fail(e) { println("f{e}")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "6\n"
    assert code == 0


def test_question_mark_with_nontrial_return_is_not_wrapped():
    # `?` in a function pinned to a non-Trial return type (like main -> Int64) keeps
    # the raw value on success; it does not become Pass(...).
    src = """
    fn check(n: Int64) -> Trial<Int64, String> {
        if n < 0 { Fail("neg") } else { Pass(n) }
    }
    fn run(n: Int64) -> Int64 {
        let x = check(n)?
        x * 2
    }
    fn main() -> Int64 { println("{run(n: 5)}")  0 }
    """
    out, code = run_out(src)
    assert out == "10\n"
    assert code == 0


def test_function_infers_trial_from_fail_construction():
    # No `?` and no annotation: a `Fail(...)` in the body makes the function
    # Trial-returning (Option 2 inference), so the bare success branch is
    # auto-wrapped in Pass and the Fail branch passes through.
    src = """
    fn receive(n: Int64) {
        if n == 0 { Fail("empty") } else { n }
    }
    fn main() -> Int64 {
        match receive(n: 5) {
            Pass(v) { println("ok {v}") }
            Fail(e) { println("no {e}") }
        }
        match receive(n: 0) {
            Pass(v) { println("ok {v}") }
            Fail(e) { println("no {e}") }
        }
        0
    }
    """
    out, code = run_out(src)
    assert out == "ok 5\nno empty\n"
    assert code == 0


def test_inferred_trial_wraps_bare_branch_beside_pass():
    # The mirror case: `Pass` in one branch and a bare value in the other. The
    # bare branch is threaded to Pass too, so both arms yield a Trial.
    src = """
    fn choose(n: Int64) {
        if n < 0 { Pass(0) } else { n }
    }
    fn main() -> Int64 {
        match choose(n: 7) {
            Pass(v) { println("{v}")  0 }
            Fail(e) { println("f")  1 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "7\n"
    assert code == 0


def test_function_infers_may_from_not_construction():
    # A `Not()` in the body makes the function May-returning; the bare success
    # branch is auto-wrapped in Has (not Pass), matching the family it saw.
    src = """
    fn lookup(n: Int64) {
        if n == 0 { Not() } else { n }
    }
    fn main() -> Int64 {
        match lookup(n: 9) {
            Has(v) { println("has {v}") }
            Not { println("none") }
        }
        match lookup(n: 0) {
            Has(v) { println("has {v}") }
            Not { println("none") }
        }
        0
    }
    """
    out, code = run_out(src)
    assert out == "has 9\nnone\n"
    assert code == 0


def test_fail_with_no_argument_defaults_to_void():
    # `Fail()` constructs a Fail whose error payload defaults to Void.
    src = """
    fn attempt(ok: Bool) -> Trial<Int64, Void> {
        if ok { Pass(1) } else { Fail() }
    }
    fn main() -> Int64 {
        match attempt(ok: false) {
            Pass(v) { println("ok {v}")  1 }
            Fail(e) { println("failed")  0 }
        }
    }
    """
    out, code = run_out(src)
    assert out == "failed\n"
    assert code == 0



def test_println_and_print_over_libc():
    # print/println are now Thoxa prelude fns over libc `write` (fd 1). Verify the
    # output, that println's argument is optional (bare newline), and that any
    # value interpolates. run_out bridges fd 1 back to the capture buffer.
    out, code = run_out(
        'fn main() -> Int64 {\n'
        '  print("a")\n'
        '  print("b\\n")\n'
        '  println("c")\n'
        '  println()\n'
        '  println("{1 + 2}")\n'
        '  0\n'
        '}\n')
    assert out == "ab\nc\n\n3\n"
    assert code == 0


def test_match_pattern_alone_does_not_trigger_threading():
    # A function with no annotation that only *matches* on Pass/Fail (these are
    # ConstructorPatterns, not constructions) must return its raw arm value, not
    # a wrapped Trial — the detection keys on TypeValue construction sites only.
    src = """
    fn describe(t: Trial<Int64, String>) {
        match t {
            Pass(v) { v }
            Fail(e) { 0 }
        }
    }
    fn main() -> Int64 {
        println("{describe(t: Pass(5))}")
        0
    }
    """
    out, code = run_out(src)
    assert out == "5\n"
    assert code == 0


def test_multiline_string_dedent():
    src = 'fn main() -> Int64 { let s = "\n    a\n    b\n"  print(s)  0 }'
    out, _ = run_out(src)
    assert out == "a\nb\n"


def test_index_out_of_bounds_traps():
    with pytest.raises(ThoxaError):
        run_out("fn main() -> Int64 { let xs = [1, 2]  println(\"{xs[5]}\")  0 }")


def test_condition_must_be_bool():
    with pytest.raises(ThoxaError):
        run_out("fn main() -> Int64 { if 1 { } 0 }")
