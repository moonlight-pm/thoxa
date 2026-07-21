#!/usr/bin/env python3
"""Exercise the Thoxa language server's navigation from the command line.

Prints the hover and go-to-definition the editor would show at one cursor
position, without an editor. It drives the real `Server` request handlers — the
same code `thoxa lsp` runs — so cross-module hover/definition resolve exactly as
they would in Zed: imported names, `namespace.member`, and prelude types all
reach into the defining file.

Usage:
    python scripts/lsp_probe.py FILE.thx LINE COL [OPEN.thx ...]

LINE and COL are 1-based, as your editor's status bar shows them. Imported
modules are read from disk automatically; only pass extra OPEN.thx files when you
want to test *unsaved* edits in them (they load as open buffers that win over
disk).

Examples:
    # hover `createSocket` in `net.createSocket()` -> its signature + doc,
    # and the definition -> examples/http/Net.thx (read from disk):
    python scripts/lsp_probe.py examples/http/Main.thx 28 22

    # a std import needs nothing else (std/ resolves on its own):
    printf 'import { sqrt } from "std/Math"\\nlet a = sqrt(2.0)\\n' > /tmp/m.thx
    python scripts/lsp_probe.py /tmp/m.thx 2 9
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from thoxa.lsp import convert                     # noqa: E402
from thoxa.lsp.server import Server               # noqa: E402


class _NullConn:
    def send(self, msg): pass
    def read(self): return None


def _uri(path):
    return "file://" + os.path.abspath(path)


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip())
        return 2
    target, line1, col1 = argv[0], int(argv[1]), int(argv[2])

    srv = Server(_NullConn())
    for path in [target, *argv[3:]]:
        with open(path, encoding="utf-8") as f:
            srv.docs[_uri(path)] = f.read()

    pos = {"textDocument": {"uri": _uri(target)},
           "position": {"line": line1 - 1, "character": col1 - 1}}

    print(f"# {target}:{line1}:{col1}\n")

    hover = srv.hover(pos)
    print("── hover ─────────────────────────────")
    print(hover["contents"]["value"] if hover else "(nothing)")

    defn = srv.definition(pos)
    print("\n── definition ────────────────────────")
    if defn:
        r = defn["range"]["start"]
        print(f"{convert.uri_to_path(defn['uri'])}:{r['line'] + 1}:{r['character'] + 1}")
    else:
        print("(nothing)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
