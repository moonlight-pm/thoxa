"""Stage 7 of the kilo port: the `Syntax` module (examples/kilo/Syntax.thx) — the pure
classifier. Tested deterministically with an import-probe (the Stage-6 searchFrom seam):
selectSyntax by extension, plainRow, and highlightRow's per-class classification + the
cross-row comment fold. The live color draw is covered by the pty smoke (test_kilo_keys)."""
from __future__ import annotations

import pathlib

from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def test_module_type_checks_clean():
    path = KILO / "Syntax.thx"
    assert check(path.read_text(encoding="utf-8"), str(path)) == []


def _select(name: str) -> str:
    src = (
        'import { selectSyntax } from "./Syntax"\n'
        "fn main() -> Int64 {\n"
        f'  match selectSyntax("{name}") {{ Has(s) {{ println("yes") }} Not {{ println("no") }} }}\n'
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_select_syntax_by_extension():
    assert _select("main.c") == "yes"
    assert _select("kilo.h") == "yes"
    assert _select("a.cpp") == "yes"
    assert _select("notes.txt") == "no"
    assert _select("Makefile") == "no"
    assert _select("") == "no"


def _classify(line: str, prev_open: str = "false") -> str:
    """Run highlightRow over `line` (with cSyntax via selectSyntax) and return the per-cell
    class letters + ' open=<bool>'. Escapes backslash/quote and doubles braces ({{ }}) so the
    line survives embedding in a Thoxa string literal (which treats { } as interpolation)."""
    esc = (
        line.replace("\\", "\\\\").replace('"', '\\"').replace("{", "{{").replace("}", "}}")
    )
    src = (
        'import { Hl, Normal, Comment, Keyword1, Keyword2, Str, Number, highlightRow, selectSyntax } from "./Syntax"\n'
        "fn letter(h!: Hl) -> String {\n"
        '  match h { Normal { "N" } Comment { "C" } Keyword1 { "1" } Keyword2 { "2" } '
        'Str { "S" } Number { "#" } else { "?" } }\n'
        "}\n"
        "fn main() -> Int64 {\n"
        '  match selectSyntax("x.c") {\n'
        "    Has(s) {\n"
        f'      let rh = highlightRow(Bytes.fromString("{esc}"), {prev_open}, s)\n'
        '      mut out = ""\n'
        "      mut i = 0\n"
        "      loop { if i >= rh.cells.count { break }  out = \"{out}{letter(rh.cells[i])}\"  i = i + 1 }\n"
        '      println("{out} open={rh.openComment}")\n'
        "    }\n"
        '    Not { println("no syntax") }\n'
        "  }\n"
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    return "".join(buf).strip()


def test_plain_row_is_all_normal():
    src = (
        'import { Hl, Normal, plainRow } from "./Syntax"\n'
        "fn main() -> Int64 {\n"
        '  let rh = plainRow(Bytes.fromString("int x = 1;"))\n'
        '  mut allN = true\n'
        "  mut i = 0\n"
        "  loop { if i >= rh.cells.count { break }  match rh.cells[i] { Normal { Void } else { allN = false } }  i = i + 1 }\n"
        '  println("{allN} {rh.cells.count} open={rh.openComment}")\n'
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    assert "".join(buf).strip() == "true 10 open=false"


def test_keyword_kw1_bounded_by_separators():
    # "if" is kw1; "(" after it is a separator, so both bytes classify Keyword1.
    # "if (x) {" is 8 chars: 11 (keyword) + 6 normal.
    assert _classify("if (x) {") == "11NNNNNN open=false"


def test_near_keyword_without_boundary_is_normal():
    # "ifx" is not the keyword "if" — the char after "if" (x) is not a separator.
    assert _classify("ifx") == "NNN open=false"


def test_kw2_type_is_keyword2():
    assert _classify("int x") == "222NN open=false"


def test_decimal_number():
    assert _classify("x = 42;") == "NNNN##N open=false"


def test_digit_inside_identifier_is_not_a_number():
    # a digit only starts a number after a separator (or another number).
    assert _classify("a1b") == "NNN open=false"


def test_string_with_escaped_quote_stays_a_string():
    # "a\"b" : the \ escapes the following ", which does NOT close the string.
    assert _classify('x = "a\\"b"') == "NNNNSSSSSS open=false"


def test_single_line_comment_runs_to_end_of_line():
    assert _classify("x // hi") == "NNCCCCC open=false"


def test_block_comment_within_a_line():
    assert _classify("a /* c */ b") == "NNCCCCCCCNN open=false"


def test_block_comment_opens_and_carries_to_next_row():
    # a line that opens /* but never closes it → openComment true; the whole "/* ..." is comment.
    assert _classify("x /* open") == "NNCCCCCCC open=true"
    # the next row, fed prevOpen=true, is all comment until it closes at "*/".
    assert _classify("still open */ y", prev_open="true") == "CCCCCCCCCCCCCNN open=false"


def test_form_feed_is_a_separator():
    # kilo's is_separator (via isspace) treats form-feed (0x0c) as a separator, so a keyword
    # sitting against one still highlights. Built with a Buffer so the raw 0x0c byte need not go
    # through a string literal. render "int" + 0x0c + "x": int is kw2 (222), then FF + x normal.
    src = (
        'import { Hl, Normal, Keyword2, highlightRow, selectSyntax } from "./Syntax"\n'
        'fn letter(h!: Hl) -> String { match h { Normal { "N" } Keyword2 { "2" } else { "?" } } }\n'
        "fn main() -> Int64 {\n"
        '  match selectSyntax("x.c") {\n'
        "    Has(s) {\n"
        "      mut b = Buffer()\n"
        '      b.appendBytes(Bytes.fromString("int"))\n'
        "      b.append(b'\\x0c')\n"
        "      b.append(b'x')\n"
        "      let rh = highlightRow(b.freeze(), false, s)\n"
        '      mut out = ""\n'
        "      mut i = 0\n"
        "      loop { if i >= rh.cells.count { break }  out = \"{out}{letter(rh.cells[i])}\"  i = i + 1 }\n"
        '      println(out)\n'
        "    }\n"
        '    Not { println("no") }\n'
        "  }\n"
        "  0\n}\n"
    )
    buf: list[str] = []
    run(src, str(KILO / "syntax_probe.thx"), out=buf.append)
    assert "".join(buf).strip() == "222NN"
