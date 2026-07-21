"""FileIO — kilo's editorOpen: read a file, split into Rows on \\n (strip trailing \\r)."""
from __future__ import annotations

import pathlib
from thoxa.checker import check
from thoxa.interp import run

REPO = pathlib.Path(__file__).resolve().parent.parent
KILO = REPO / "examples" / "kilo"


def _probe(body: str) -> str:
    src = 'import fileio from "./FileIO"\nfn main() -> Int64 {\n' + body + "\n0\n}\n"
    buf: list[str] = []
    run(src, str(KILO / "fileio_probe.thx"), out=buf.append)
    return "".join(buf)


def test_fileio_type_checks_clean():
    p = KILO / "FileIO.thx"
    assert check(p.read_text(encoding="utf-8"), str(p)) == []


def test_reads_and_splits(tmp_path):
    f = tmp_path / "t.txt"
    f.write_text("alpha\nbeta\r\ngamma", encoding="utf-8")  # CRLF on line 2, no final \n
    out = _probe(
        f'match fileio.open("{f}") {{\n'
        '  Pass(rows) { println("n={rows.count} a={rows.first().charCount()}") }\n'
        '  Fail(e) { println("FAIL {e}") }\n'
        '}'
    )
    # 3 rows; "beta\r" -> "beta" (4); first line "alpha" (5)
    assert out == "n=3 a=5\n"


def test_reads_sample_file():
    # the bundled demo file: 5 lines, second is the tab-indented one
    out = _probe(
        f'match fileio.open("{KILO / "sample.txt"}") {{\n'
        '  Pass(rows) { println("n={rows.count}") }\n'
        '  Fail(e) { println("FAIL {e}") }\n'
        '}'
    )
    assert out == "n=5\n"


def test_missing_file_fails():
    out = _probe(
        'match fileio.open("/no/such/file/here.txt") {\n'
        '  Pass(rows) { println("PASS {rows.count}") }\n'
        '  Fail(e) { println("failed") }\n'
        '}'
    )
    assert out == "failed\n"


# -- save (Stage 5) ---------------------------------------------------------


def test_save_round_trips_through_open(tmp_path):
    # load -> save -> the bytes on disk are the canonical serialization (each row + \n),
    # which reopens to the same rows. rowsToString appends \n after every row, so a
    # trailing-newline source round-trips exactly.
    src = tmp_path / "src.txt"
    src.write_text("alpha\nbeta\ngamma\n", encoding="utf-8")
    dst = tmp_path / "dst.txt"
    body = (
        'match fileio.open("SRC") {\n'
        '  Pass(rows) {\n'
        '    match fileio.save("DST", rows) {\n'
        '      Pass(n) { println("wrote={n}") }\n'
        '      Fail(e) { println("saveFAIL {e}") }\n'
        '    }\n'
        '  }\n'
        '  Fail(e) { println("openFAIL {e}") }\n'
        '}'
    ).replace("SRC", str(src)).replace("DST", str(dst))
    assert _probe(body) == "wrote=17\n"   # 5+1 + 4+1 + 5+1
    assert dst.read_bytes() == b"alpha\nbeta\ngamma\n"


def test_save_creates_new_file_and_truncates_old_content(tmp_path):
    # O_CREAT: dst did not exist. O_TRUNC: a longer prior content is fully replaced.
    src = tmp_path / "src.txt"
    src.write_text("hi\n", encoding="utf-8")
    dst = tmp_path / "dst.txt"
    dst.write_text("X" * 100, encoding="utf-8")   # 100 bytes of stale content
    body = (
        'match fileio.open("SRC") {\n'
        '  Pass(rows) {\n'
        '    match fileio.save("DST", rows) {\n'
        '      Pass(n) { println("ok={n}") }\n'
        '      Fail(e) { println("F {e}") }\n'
        '    }\n'
        '  }\n'
        '  Fail(e) { println("openFAIL {e}") }\n'
        '}'
    ).replace("SRC", str(src)).replace("DST", str(dst))
    assert _probe(body) == "ok=3\n"
    assert dst.read_bytes() == b"hi\n"   # truncated to exactly the new content


def test_save_empty_buffer_writes_an_empty_file(tmp_path):
    dst = tmp_path / "empty.txt"
    body = (
        'match fileio.save("DST", []) {\n'
        '  Pass(n) { println("n={n}") }\n'
        '  Fail(e) { println("F {e}") }\n'
        '}'
    ).replace("DST", str(dst))
    assert _probe(body) == "n=0\n"
    assert dst.read_bytes() == b""


def test_save_fails_on_a_directory(tmp_path):
    d = tmp_path / "adir"
    d.mkdir()
    body = (
        'match fileio.save("DST", []) {\n'
        '  Pass(n) { println("ok={n}") }\n'
        '  Fail(e) { println("failed") }\n'
        '}'
    ).replace("DST", str(d))
    assert _probe(body) == "failed\n"
