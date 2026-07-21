"""Run every spec/conformance/*.thx through the real interpreter.

Each program's captured stdout must equal its sibling `.out` (if present), and its
exit code must equal `.exit` (default 0). This is the executable spec: a feature is
"done" only when its conformance test is green.
"""

from __future__ import annotations

import pathlib

import pytest

from thoxa.errors import ThoxaError
from thoxa.interp import run

CONFORMANCE = pathlib.Path(__file__).resolve().parent.parent / "spec" / "conformance"
CASES = sorted(CONFORMANCE.glob("*.thx"))


@pytest.mark.parametrize("path", CASES, ids=[p.name for p in CASES])
def test_conformance(path: pathlib.Path) -> None:
    text = path.read_text(encoding="utf-8")
    buf: list[str] = []
    try:
        code = run(text, str(path), out=buf.append)
    except ThoxaError as exc:  # pragma: no cover - failure path
        pytest.fail(f"{path.name} raised a Thoxa error:\n{exc.render()}")

    output = "".join(buf)
    out_file = path.with_suffix(".out")
    if out_file.exists():
        assert output == out_file.read_text(encoding="utf-8")

    exit_file = path.with_suffix(".exit")
    expected_exit = int(exit_file.read_text().strip()) if exit_file.exists() else 0
    actual_exit = code if isinstance(code, int) and not isinstance(code, bool) else 0
    assert actual_exit == expected_exit


def test_conformance_suite_is_nonempty() -> None:
    assert CASES, "no conformance programs found"
