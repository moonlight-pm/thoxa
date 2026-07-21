"""Command-line `arguments` on main. `thoxa run <module> a b` gives main
arguments == [<module>, "a", "b"]; a zero-arg main still runs."""
from __future__ import annotations

import subprocess
import sys
import pathlib

REPO = pathlib.Path(__file__).resolve().parent.parent


def _run_cli(rel_module: str, *extra: str):
    return subprocess.run(
        [sys.executable, "-m", "thoxa", "run", rel_module, *extra],
        cwd=REPO, capture_output=True, text=True,
    )


def test_arguments_reach_main(tmp_path):
    mod = tmp_path / "args_probe.thx"
    mod.write_text(
        'export fn main(arguments: Array<String>) -> Int64 {\n'
        '    println("count={arguments.count}")\n'
        '    if arguments.count > 1 { println("first={arguments[1]}") }\n'
        '    0\n'
        '}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod), "hello.txt", "second")
    assert r.returncode == 0, r.stderr
    assert "count=3" in r.stdout
    assert "first=hello.txt" in r.stdout


def test_no_arguments(tmp_path):
    mod = tmp_path / "args_probe.thx"
    mod.write_text(
        'export fn main(arguments: Array<String>) -> Int64 {\n'
        '    println("count={arguments.count}")\n    0\n}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod))
    assert r.returncode == 0, r.stderr
    assert "count=1" in r.stdout  # just the module path


def test_zero_param_main_still_runs(tmp_path):
    mod = tmp_path / "noargs.thx"
    mod.write_text(
        'export fn main() -> Int64 {\n    println("ok")\n    0\n}\n', encoding="utf-8",
    )
    r = _run_cli(str(mod))
    assert r.returncode == 0, r.stderr
    assert "ok" in r.stdout
