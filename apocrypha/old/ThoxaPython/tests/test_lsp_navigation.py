import pytest

from thoxa.lsp import index as I
from thoxa.lsp import server as S
from thoxa.lsp import protocol


def test_build_index_binding_at_finds_definition():
    src = "fn add(a!: Int64, b!: Int64) -> Int64 { a }\n"
    idx = I.build_index(src)
    b = idx.binding_at(0, 4)                 # 0-based: the `a` param name? no -> the fn name 'add'
    assert b is not None and b.name == "add" and b.kind == "fn"


def test_build_index_is_empty_on_unparseable():
    idx = I.build_index('fn main() { "unterminated ')
    assert idx.binding_at(0, 3) is None       # no crash, empty index


def test_render_signature_function():
    src = "fn serve(port!: Int32) -> Int64 { 0 }\n"
    idx = I.build_index(src)
    b = idx.binding_at(0, 3)                   # 'serve'
    assert I.render_signature(b) == "fn serve(port!: Int32) -> Int64"


def test_render_signature_type_and_field():
    src = "type Point(x: Int64, y: Int64) {}\n"
    idx = I.build_index(src)
    t = idx.binding_at(0, 5)                   # 'Point'
    assert I.render_signature(t) == "type Point(x: Int64, y: Int64)"


class _FakeConn:
    def __init__(self): self.sent = []
    def send(self, msg): self.sent.append(msg)
    def read(self): return None


def _server_with(uri, text):
    srv = S.Server(_FakeConn())
    srv.docs[uri] = text
    return srv


def _pos(uri, line0, char0):
    return {"textDocument": {"uri": uri}, "position": {"line": line0, "character": char0}}


def test_definition_jumps_to_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    srv = _server_with(uri, text)
    loc = srv.definition(_pos(uri, 1, 21))       # `g` in the call (0-based)
    assert loc["uri"] == uri
    assert loc["range"]["start"] == {"line": 0, "character": 3}   # 'g' at col 4 (1-based)


def test_definition_returns_none_off_symbol():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn g() -> Int64 { 0 }\n")
    assert srv.definition(_pos(uri, 0, 0)) is None   # the `fn` keyword


def test_references_lists_all_uses_and_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 3)                       # on the declaration of `g`
    params["context"] = {"includeDeclaration": True}
    locs = srv.references(params)
    starts = sorted((l["range"]["start"]["line"], l["range"]["start"]["character"]) for l in locs)
    # declaration at (0,3) plus two calls on line 1
    assert (0, 3) in starts
    assert sum(1 for s in starts if s[0] == 1) == 2


def test_references_can_exclude_declaration():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 3)
    params["context"] = {"includeDeclaration": False}
    locs = srv.references(params)
    assert all(l["range"]["start"]["line"] != 0 for l in locs)


def test_document_symbol_outline_with_nested_members():
    uri = "file:///m.thx"
    text = (
        "fn top() -> Int64 { 0 }\n"
        "type Point(x: Int64, y: Int64) {\n"
        "    fn mag() -> Int64 { 0 }\n"
        "}\n"
    )
    srv = _server_with(uri, text)
    syms = srv.document_symbol({"textDocument": {"uri": uri}})
    names = {s["name"]: s for s in syms}
    assert set(names) == {"top", "Point"}
    child_names = {c["name"] for c in names["Point"].get("children", [])}
    assert {"x", "y", "mag"} <= child_names


def test_hover_shows_signature_and_doc():
    uri = "file:///m.thx"
    text = (
        "## Starts the echo server.\n"
        "## Threads failures as Trial.\n"
        "fn serve(port!: Int32) -> Int64 { 0 }\n"
        "fn main() -> Int64 { serve(port: 8080) }\n"
    )
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 3, 21))              # `serve` in the call
    val = h["contents"]["value"]
    assert "```thoxa" in val
    assert "fn serve(port!: Int32) -> Int64" in val
    assert "Starts the echo server." in val
    assert "Threads failures as Trial." in val


def test_hover_none_off_symbol():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f() -> Int64 { 0 }\n")
    assert srv.hover(_pos(uri, 0, 0)) is None    # `fn` keyword


def test_hover_on_definition_name_shows_signature():
    uri = "file:///m.thx"
    text = (
        "## Starts the echo server.\n"
        "fn serve(port!: Int32) -> Int64 { 0 }\n"
    )
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 1, 3))                # `serve` in its own declaration
    val = h["contents"]["value"]
    assert "fn serve(port!: Int32) -> Int64" in val
    assert "Starts the echo server." in val


def test_prepare_rename_on_local_returns_range():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f(x!: Int64) -> Int64 { x }\n")
    r = srv.prepare_rename(_pos(uri, 0, 27))     # the `x` use
    assert r == {"start": {"line": 0, "character": 27}, "end": {"line": 0, "character": 28}}


def test_prepare_rename_refuses_builtin():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f() -> Int64 { println(\"hi\")  0 }\n")
    assert srv.prepare_rename(_pos(uri, 0, 18)) is None   # `println`


def test_rename_edits_declaration_and_uses():
    uri = "file:///m.thx"
    text = "fn g() -> Int64 { 0 }\nfn main() -> Int64 { g() g() }\n"
    srv = _server_with(uri, text)
    params = _pos(uri, 1, 21)                     # a `g` call
    params["newName"] = "h"
    we = srv.rename(params)
    edits = we["changes"][uri]
    assert len(edits) == 3                        # decl + two calls
    assert all(e["newText"] == "h" for e in edits)


def test_rename_rejects_wrong_case():
    uri = "file:///m.thx"
    srv = _server_with(uri, "type Point(x: Int64) {}\nfn main() -> Int64 { 0 }\n")
    params = _pos(uri, 0, 5)                       # `Point`
    params["newName"] = "point"                    # not PascalCase
    assert srv.rename(params) is None


def test_prepare_rename_on_definition_name_returns_range():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn serve(port!: Int32) -> Int64 { 0 }\n")
    r = srv.prepare_rename(_pos(uri, 0, 3))       # cursor on the `serve` definition name
    assert r == {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 8}}


def test_checker_records_inferred_let_type():
    from thoxa.checker import Checker, ty_str
    from thoxa.errors import Source
    from thoxa.parser import parse
    src = "fn f() -> Int64 {\n    let n = 41\n    n\n}\n"
    source = Source(src, "<t>")
    chk = Checker(source)
    chk.check_module(parse(src, "<t>"))
    # find the `let n` binding and confirm its inferred type was captured
    b = next(b for b in chk.resolution.bindings.values()
             if b.name == "n" and b.kind == "let")
    assert ty_str(chk.types[b.id]) == "Int64"


def test_checker_links_member_ref_on_provable_type():
    from thoxa.checker import Checker
    from thoxa.errors import Source
    from thoxa.parser import parse
    src = ("type Point(x: Int64, y: Int64) {\n"
           "    fn mag() -> Int64 { 0 }\n"
           "}\n"
           "fn f(p!: Point) -> Int64 { p.mag() }\n")
    source = Source(src, "<t>")
    chk = Checker(source)
    chk.check_module(parse(src, "<t>"))
    # the pending `mag` member ref (ctx 'member', on line 4) is now LINKED to the method binding
    member_refs = [r for r in chk.resolution.references
                   if r.ctx == "member" and r.span.line == 4]
    assert member_refs, "no member reference recorded on line 4"
    linked = [r for r in member_refs if r.binding_id is not None]
    assert linked, "mag member ref was not linked by the checker"
    b = chk.resolution.binding_of(linked[0])
    assert b is not None and b.kind == "method" and b.name == "mag"


def test_member_inert_when_type_unprovable():
    uri = "file:///m.thx"
    text = "fn f(p!: Any) -> Int64 { p.mystery() }\n"
    srv = _server_with(uri, text)
    assert srv.definition(_pos(uri, 0, 27)) is None


def test_member_resolves_on_provable_type():
    uri = "file:///m.thx"
    text = ("type Point(x: Int64, y: Int64) {\n"
            "    fn mag() -> Int64 { 0 }\n"
            "}\n"
            "fn f(p!: Point) -> Int64 { p.mag() }\n")
    srv = _server_with(uri, text)
    loc = srv.definition(_pos(uri, 3, 29))
    assert loc is not None
    assert loc["range"]["start"]["line"] == 1


def test_hover_shows_inferred_let_type():
    uri = "file:///m.thx"
    text = "fn f() -> Int64 {\n    let n = 41\n    n\n}\n"
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 2, 4))               # the `n` use on line 3
    assert "let n: Int64" in h["contents"]["value"]


def test_rename_member_updates_declaration_and_uses():
    uri = "file:///m.thx"
    text = (
        "type Point(x: Int64, y: Int64) {\n"
        "    fn mag() -> Int64 { 0 }\n"
        "}\n"
        "fn f(p!: Point) -> Int64 { p.mag() }\n"
    )
    srv = _server_with(uri, text)
    params = _pos(uri, 3, 29)                     # `mag` use
    params["newName"] = "magnitude"
    we = srv.rename(params)
    edits = we["changes"][uri]
    assert len(edits) == 2                        # method decl + the call use
    assert all(e["newText"] == "magnitude" for e in edits)


def test_rename_member_refused_when_unprovable():
    uri = "file:///m.thx"
    srv = _server_with(uri, "fn f(p!: Any) -> Int64 { p.mystery() }\n")
    params = _pos(uri, 0, 27)
    params["newName"] = "clarity"
    assert srv.rename(params) is None


def test_initialize_advertises_navigation_capabilities():
    srv = S.Server(_FakeConn())
    caps = srv.initialize({})["capabilities"]
    for key in ("hoverProvider", "definitionProvider", "declarationProvider",
                "referencesProvider", "documentSymbolProvider"):
        assert caps.get(key) is True
    assert caps.get("renameProvider") == {"prepareProvider": True}


def test_rename_does_not_corrupt_on_string_interpolation():
    uri = "file:///m.thx"
    text = ('fn serve(host!: String) -> Int64 {\n'
            '    println("{host}")\n'
            '    0\n'
            '}\n')
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 9)          # the `host` param declaration (0-based col 9)
    params["newName"] = "hostname"
    we = srv.rename(params)
    if we is not None:
        lines = text.splitlines()
        for e in we["changes"][uri]:
            l0 = e["range"]["start"]["line"]
            c0 = e["range"]["start"]["character"]
            c1 = e["range"]["end"]["character"]
            assert lines[l0][c0:c1] == "host", (
                f"rename would corrupt: edit targets {lines[l0][c0:c1]!r}, not 'host'")


def test_references_excludes_bogus_interpolation_span():
    uri = "file:///m.thx"
    text = ('fn serve(host!: String) -> Int64 {\n'
            '    println("{host}")\n'
            '    0\n'
            '}\n')
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 9)
    params["context"] = {"includeDeclaration": True}
    locs = srv.references(params)
    lines = text.splitlines()
    for loc in locs:
        l0 = loc["range"]["start"]["line"]
        c0 = loc["range"]["start"]["character"]
        c1 = loc["range"]["end"]["character"]
        assert lines[l0][c0:c1] == "host", (
            f"references returned a bogus span targeting {lines[l0][c0:c1]!r}")


def test_rename_updates_interpolation_occurrence():
    uri = "file:///m.thx"
    text = 'fn serve(host!: String) -> Int64 {\n    println("{host}")\n    0\n}\n'
    srv = _server_with(uri, text)
    params = _pos(uri, 0, 9)             # the `host` param declaration
    params["newName"] = "hostname"
    we = srv.rename(params)
    edits = we["changes"][uri]
    # now 2 edits: the param decl (line 0) AND the interpolation use (line 1)
    lines = text.splitlines()
    targets = sorted((e["range"]["start"]["line"], e["range"]["start"]["character"]) for e in edits)
    assert (1, 14) in targets            # 0-based: line 1, char 14 == the `host` in "{host}"
    for e in edits:
        l0 = e["range"]["start"]["line"]; c0 = e["range"]["start"]["character"]; c1 = e["range"]["end"]["character"]
        assert lines[l0][c0:c1] == "host"
    assert len(edits) == 2


def test_render_signature_valued_type():
    src = "type ReadOnly 0\n"
    idx = I.build_index(src)
    t = idx.binding_at(0, 5)                   # 'ReadOnly'
    assert I.render_signature(t) == "type ReadOnly 0"


def test_hover_valued_type_shows_value():
    uri = "file:///m.thx"
    text = "## An open(2) access mode.\ntype ReadOnly 0\n"
    srv = _server_with(uri, text)
    h = srv.hover(_pos(uri, 1, 5))             # 'ReadOnly' in its declaration
    assert h is not None
    val = h["contents"]["value"]
    assert "type ReadOnly 0" in val
    assert "An open(2) access mode." in val
