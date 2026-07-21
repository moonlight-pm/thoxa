"""Cross-module hover & go-to-definition (design 2026-07-13).

`std/Math` and `std/Prelude` resolve via the absolute `std/` root regardless of
the importer's path, so most cases need no temp files.
"""
import os

from thoxa import ast
from thoxa.lsp import convert, index


# ---------------------------------------------------------------- Task 1
def test_path_to_uri_roundtrips():
    p = "/home/joshua/Workspace/Thoxa/thoxa/std/Math.thx"
    assert convert.path_to_uri(p) == "file://" + p
    assert convert.uri_to_path(convert.path_to_uri(p)) == p


def test_path_to_uri_passes_through_non_paths():
    # the `<input>` sentinel (untitled buffers / tests) has no real location
    assert convert.path_to_uri("<input>") == "<input>"
    assert convert.path_to_uri("relative/x.thx") == "relative/x.thx"


def test_path_to_uri_quotes_spaces():
    p = "/tmp/a b/c.thx"
    uri = convert.path_to_uri(p)
    assert " " not in uri
    assert convert.uri_to_path(uri) == p


def test_build_index_stores_module_ast():
    idx = index.build_index("fn f() -> Int64 { 0 }\n", "m.thx")
    assert isinstance(idx.module, ast.Module)


def test_build_index_module_none_on_unparseable():
    idx = index.build_index("fn (( broken", "m.thx")
    assert idx.module is None


# ---------------------------------------------------------------- Task 2
_HERE = "/home/joshua/Workspace/Thoxa/probe.thx"


def _remote_at(src, line0, char0, path=_HERE, docs=None):
    from thoxa.lsp import crossmodule
    idx = index.build_index(src, path)
    ref = idx.reference_at(line0, char0)
    binding = idx.resolution.binding_of(ref)
    if binding is None:
        binding = idx.binding_at(line0, char0)
    return crossmodule.resolve_remote(idx, ref, binding, docs or {}, path)


def test_remote_selective_import():
    src = 'import { sqrt } from "std/Math"\nlet a = sqrt(2.0)\n'
    r = _remote_at(src, 1, 8)                    # the use of `sqrt`
    assert r is not None
    assert r.signature == "extern fn sqrt(x!: Float64) -> Float64"
    assert r.doc and "numeric routines" in r.doc
    assert r.path.endswith("thoxa/std/Math.thx")
    assert r.def_span is not None


def test_remote_aliased_import_shows_original_name():
    src = 'import { sqrt as root } from "std/Math"\nlet b = root(2.0)\n'
    r = _remote_at(src, 1, 8)                    # the use of `root`
    assert r is not None
    assert r.signature == "extern fn sqrt(x!: Float64) -> Float64"


def test_remote_none_for_local_name():
    src = 'fn f() -> Int64 { 0 }\nlet a = f()\n'
    assert _remote_at(src, 1, 8) is None         # `f` is local, not cross-module


def test_remote_none_for_unresolvable_import():
    src = 'import { nope } from "std/DoesNotExist"\nlet a = nope()\n'
    assert _remote_at(src, 1, 8) is None          # missing module → graceful None


# ---------------------------------------------------------------- Task 3
def test_remote_namespace_member():
    src = 'import math from "std/Math"\nlet c = math.sqrt(2.0)\n'
    r = _remote_at(src, 1, 13)                   # the `sqrt` in `math.sqrt`
    assert r is not None
    assert r.signature == "extern fn sqrt(x!: Float64) -> Float64"
    assert r.path.endswith("thoxa/std/Math.thx")
    assert r.def_span is not None


def test_remote_namespace_name_summary():
    src = 'import math from "std/Math"\nlet c = math.sqrt(2.0)\n'
    r = _remote_at(src, 1, 8)                     # the `math` namespace name itself
    assert r is not None
    assert r.signature == "module std/Math"
    assert r.doc and "numeric routines" in r.doc
    assert r.def_span is not None                # go-to-def jumps to the module top


def test_remote_member_on_non_namespace_is_none():
    # a member access whose receiver is NOT a namespace import → no remote
    src = 'fn f(s!: String) -> Int64 { s.count }\n'
    assert _remote_at(src, 0, 30) is None         # `.count` on a String value


# ---------------------------------------------------------------- Task 4
def test_remote_prelude_type_hover():
    src = 'fn f() -> May<Int64> { Not }\n'
    r = _remote_at(src, 0, 10)                    # `May` in the return type
    assert r is not None
    assert r.signature.startswith("type May")
    assert r.path.endswith("thoxa/std/Prelude.thx")
    assert r.def_span is not None


def test_remote_prelude_constructor_hover():
    src = 'fn f() -> May<Int64> { Not }\n'
    r = _remote_at(src, 0, 23)                    # the `Not` constructor value
    assert r is not None and r.signature.startswith("type Not")


def test_remote_println_resolves_to_prelude():
    # `println` is now a real prelude function, so hover resolves it into
    # std/Prelude and renders its signature (it was an unresolvable builtin before).
    src = 'fn f() -> Void {\n  println("hi")\n}\n'
    r = _remote_at(src, 1, 2)
    assert r is not None
    assert "fn println(text!: String" in r.signature and "-> Void" in r.signature          # println has no Thoxa source


# ---------------------------------------------------------------- Task 5 (handlers)
from thoxa.lsp import server as S


class _FakeConn:
    def send(self, msg): pass
    def read(self): return None


def _server(uri, text, extra=None):
    srv = S.Server(_FakeConn())
    srv.docs[uri] = text
    for u, t in (extra or {}).items():
        srv.docs[u] = t
    return srv


def _pos(uri, line0, char0):
    return {"textDocument": {"uri": uri}, "position": {"line": line0, "character": char0}}


def test_hover_shows_remote_signature_and_doc():
    uri = "file:///m.thx"
    text = 'import { sqrt } from "std/Math"\nlet a = sqrt(2.0)\n'
    h = _server(uri, text).hover(_pos(uri, 1, 8))          # the use of `sqrt`
    assert h is not None
    val = h["contents"]["value"]
    assert "extern fn sqrt(x!: Float64) -> Float64" in val
    assert "numeric routines" in val
    assert h["range"]["start"] == {"line": 1, "character": 8}   # local use span


def test_definition_jumps_into_defining_module():
    uri = "file:///m.thx"
    text = 'import { sqrt } from "std/Math"\nlet a = sqrt(2.0)\n'
    loc = _server(uri, text).definition(_pos(uri, 1, 8))
    assert loc is not None
    assert loc["uri"].endswith("thoxa/std/Math.thx")
    lines = open(convert.uri_to_path(loc["uri"])).read().splitlines()
    decl0 = next(i for i, ln in enumerate(lines) if "fn sqrt" in ln)
    r = loc["range"]
    assert r["start"]["line"] == decl0
    assert lines[decl0][r["start"]["character"]:r["end"]["character"]] == "sqrt"


def test_hover_namespace_member_and_name():
    uri = "file:///m.thx"
    text = 'import math from "std/Math"\nlet c = math.sqrt(2.0)\n'
    srv = _server(uri, text)
    member = srv.hover(_pos(uri, 1, 13))                   # math.sqrt
    assert member and "extern fn sqrt" in member["contents"]["value"]
    name = srv.hover(_pos(uri, 1, 8))                      # the `math` namespace name
    assert name and "module std/Math" in name["contents"]["value"]


def test_hover_prelude_type():
    uri = "file:///m.thx"
    h = _server(uri, "fn f() -> May<Int64> { Not }\n").hover(_pos(uri, 0, 10))
    assert h and "type May" in h["contents"]["value"]


def test_hover_local_unchanged_and_println_resolves():
    uri = "file:///m.thx"
    text = "fn greet() -> Int64 { 0 }\nfn main() -> Int64 { greet() }\n"
    h = _server(uri, text).hover(_pos(uri, 1, 21))         # `greet` use — local
    assert h and "fn greet() -> Int64" in h["contents"]["value"]
    txt2 = 'fn f() -> Void {\n  println("hi")\n}\n'
    h2 = _server(uri, txt2).hover(_pos(uri, 1, 2))         # `println` — prelude fn
    assert h2 and "fn println(text!: String" in h2["contents"]["value"]   # println: no source


def test_hover_open_buffer_wins_over_disk(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export fn helper(x!: Int64) -> Int64 { x }\n")
    main = tmp_path / "Main.thx"
    main_text = 'import { helper } from "./Lib"\nlet a = helper(1)\n'
    main.write_text(main_text)
    main_uri = "file://" + str(main)
    lib_uri = "file://" + str(lib)
    edited = "export fn helper(x!: Int64, y!: Int64) -> Int64 { x }\n"
    srv = _server(main_uri, main_text, extra={lib_uri: edited})
    h = srv.hover(_pos(main_uri, 1, 8))                    # `helper` use
    assert h is not None
    assert "y!: Int64" in h["contents"]["value"]           # reflects the unsaved buffer


# ---------------------------------------------------------------- Task 5
def test_typed_member_resolves_into_defining_module(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text(
        "export type Widget(id: Int64) {\n"
        "  export fn spin() -> Int64 { id }\n"
        "}\n"
        "export fn make() -> Trial<Widget, String> {\n  Pass(Widget(id: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = (
        'import lib from "./Lib"\n'
        "fn go() -> Int64 {\n"
        "  let w = lib.make()?\n"
        "  w.spin()\n"
        "}\n")
    main.write_text(main_text)
    path = str(main)
    from thoxa.lsp import crossmodule
    idx = index.build_index(main_text, path)
    ref = idx.reference_at(3, 4)                # `spin` in `w.spin()` (line 4, col 5)
    r = crossmodule.resolve_remote(idx, ref, idx.resolution.binding_of(ref), {}, path)
    assert r is not None
    assert r.signature == "fn spin() -> Int64"
    assert r.path.endswith("Lib.thx")
    assert r.def_span is not None


# ---------------------------------------------------------------- Task 6
def test_hover_on_import_site_namespace_name(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("## A little library.\nexport fn f() -> Int64 { 0 }\n")
    main = tmp_path / "Main.thx"
    main_text = 'import lib from "./Lib"\nfn g() -> Int64 { lib.f() }\n'
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 0, 7))            # `lib` on the import line (0-based char 7)
    assert h is not None
    assert "module ./Lib" in h["contents"]["value"]


def test_field_hover_shows_inferred_type(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n  export fn f() -> Int64 { fd }\n}\n"
                   "export fn open() -> Trial<Sock, String> {\n  Pass(Sock(fd: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import net from "./Lib"\n'
                 "type Server(x = 1) {\n  let sock = net.open()?\n}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 2, 6))            # `sock` field (0-based char 6)
    assert h is not None
    assert "sock: Sock" in h["contents"]["value"]


# ---------------------------------------------------------------- Task 7
def test_hover_fallible_ctor_shows_trial_return(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {}\n"
                   "export fn open() -> Trial<Sock, String> {\n  Pass(Sock(fd: 1))\n}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import net from "./Lib"\n'
                 'type Server(host = "127.0.0.1", port = 8000) {\n'
                 "  let sock = net.open()?\n}\n"
                 "fn run() -> Int64 {\n  let s = Server()?\n  0\n}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 5, 10))           # `Server` in `Server()?` (0-based char 10)
    assert h is not None
    val = h["contents"]["value"]
    assert ('type Server(host: String = "127.0.0.1", port: Int64 = 8000) '
            '-> Trial<Server, String>') in val


def test_ctor_param_default_hover_shows_inferred_type(tmp_path):
    # An unannotated ctor param with a default takes the default's inferred type,
    # both when hovering the param name itself and in the type's own signature.
    main = tmp_path / "Main.thx"
    main_text = 'type Server(host = "127.0.0.1", port = 8000) {}\n'
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    hhost = srv.hover(_pos(uri, 0, 12))       # `host` param name
    assert hhost is not None and "host: String" in hhost["contents"]["value"]
    hport = srv.hover(_pos(uri, 0, 32))       # `port` param name
    assert hport is not None and "port: Int64" in hport["contents"]["value"]


# ---------------------------------------------------------------- Task 8
def test_ctor_param_field_hover_shows_type_not_enclosing_doc(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n"
                   "  export fn ping() -> Int64 { fd }\n"
                   "}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import lib from "./Lib"\n'
                 "## Doc for the Conn type.\n"
                 "type Conn(sock!: Sock) {\n"
                 "  export fn go() -> Int64 {\n"
                 "    sock.ping()\n"
                 "  }\n"
                 "}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    decl = srv.hover(_pos(uri, 2, 10))            # `sock` in `type Conn(sock!: Sock)`
    assert decl is not None
    assert "sock: Sock" in decl["contents"]["value"]
    assert "Doc for the Conn type" not in decl["contents"]["value"]   # not the type's doc
    use = srv.hover(_pos(uri, 4, 4))              # `sock` in the body use
    assert use is not None and "sock: Sock" in use["contents"]["value"]


def test_typed_member_on_ctor_param_field_resolves(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n"
                   "  ## Reply to a ping.\n"
                   "  export fn ping() -> Int64 { fd }\n"
                   "}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import lib from "./Lib"\n'
                 "type Conn(sock!: Sock) {\n"
                 "  export fn go() -> Int64 {\n"
                 "    sock.ping()\n"
                 "  }\n"
                 "}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 3, 9))                # `ping` in `sock.ping()`
    assert h is not None
    val = h["contents"]["value"]
    assert "fn ping() -> Int64" in val            # resolved into Lib, the type's module
    assert "Reply to a ping" in val


def test_hover_void_literal_describes_unit_type():
    uri = "file:///m.thx"
    text = "fn f() -> Void {\n  Void\n}\n"
    h = _server(uri, text).hover(_pos(uri, 1, 2))     # the bare `Void` value
    assert h is not None
    val = h["contents"]["value"]
    assert "Void" in val and "unit type" in val
    assert h["range"]["start"] == {"line": 1, "character": 2}   # highlights the 4 chars
    assert h["range"]["end"] == {"line": 1, "character": 6}


def test_inferred_fallible_method_return_threaded(tmp_path):
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n"
                   "  export fn grab() -> Trial<Sock, String> { Pass(Sock(fd: 1)) }\n"
                   "}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import lib from "./Lib"\n'
                 "type Conn(sock!: Sock) {\n"
                 "  export fn dup() {\n"           # unannotated, fallible via `?`
                 "    Conn(sock.grab()?)\n"
                 "  }\n"
                 "}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 2, 12))               # the `dup` method name
    assert h is not None
    # E (String) resolves cross-module from `sock.grab()`, so the full Trial shows.
    assert "fn dup() -> Trial<Conn, String>" in h["contents"]["value"]



def test_inferred_void_method_return_via_cross_module_call(tmp_path):
    # An unannotated method whose tail is a cross-module `Void`-returning call
    # (`sock.shut()`) infers `-> Void`: the return resolves through the
    # namespace-imported type's member table during the quiet pre-pass.
    lib = tmp_path / "Lib.thx"
    lib.write_text("export type Sock(fd: Int64) {\n"
                   "  export fn shut() -> Void { Void }\n"
                   "}\n")
    main = tmp_path / "Main.thx"
    main_text = ('import lib from "./Lib"\n'
                 "type Conn(sock!: Sock) {\n"
                 "  export fn close() {\n"
                 "    sock.shut()\n"
                 "  }\n"
                 "}\n")
    main.write_text(main_text)
    uri = "file://" + str(main)
    srv = _server(uri, main_text)
    h = srv.hover(_pos(uri, 2, 12))               # the `close` method name
    assert h is not None
    assert "fn close() -> Void" in h["contents"]["value"]


def test_inferred_plain_method_return_threaded():
    src = ("type Item(id: Int64) {}\n"
           "type Box(it: Item) {\n"
           "  export fn peek() { it }\n"          # unannotated, non-fallible
           "}\n")
    idx = index.build_index(src)
    b = idx.binding_at(2, 12)                      # `peek`
    ty = idx.fn_returns.get(b.id)
    assert index.render_signature(b, ty) == "fn peek() -> Item"
