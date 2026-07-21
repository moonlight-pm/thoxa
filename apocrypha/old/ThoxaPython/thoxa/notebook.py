"""A local, literate Thoxa notebook — a kernel over the real interpreter.

This is the M5 deliverable, built exactly as the language-implementation research
recommended: a *local* Jupyter-style kernel (here, a localhost web page) wrapping the
one true interpreter, with bindings persisting across cells (à la evcxr). It uses only
the Python standard library — no external dependencies — and binds to 127.0.0.1 only,
so it runs your own code with full fidelity and zero sandboxing ceremony.

Run it with ``thoxa notebook``.
"""

from __future__ import annotations

import json
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from .errors import Source, ThoxaError
from .interp import (
    BreakSignal,
    ContinueSignal,
    Interpreter,
    ReturnSignal,
    VoidType,
)
from .lexer import Lexer
from .parser import Parser


class NotebookSession:
    """One persistent kernel: a single Interpreter whose globals accumulate across cells."""

    def __init__(self) -> None:
        self.interp = Interpreter()

    def run_cell(self, code: str) -> dict:
        buf: list[str] = []
        source = Source(code, "<cell>")
        self.interp._out = buf.append
        self.interp.src = source
        try:
            module = Parser(Lexer(source).tokenize(), source).parse_program()
            # FD1-CAPTURE: `print`/`println` write to real fd 1 via libc; bridge it
            # into `buf` for the cell's run. See interp._capture_stdout.
            with self.interp._capture_stdout():
                result = self.interp.run_module(module)
        except ThoxaError as exc:
            return {"output": "".join(buf), "error": exc.render()}
        except (ReturnSignal, BreakSignal, ContinueSignal):
            return {"output": "".join(buf), "error": "control flow (return/break/continue) escaped the cell"}
        except RecursionError:
            return {"output": "".join(buf), "error": "recursion limit exceeded"}
        except Exception as exc:  # pragma: no cover - defensive
            return {"output": "".join(buf), "error": f"internal error: {exc}"}
        value = None
        if result is not None and not isinstance(result, VoidType):
            value = self.interp.display(result)
        return {"output": "".join(buf), "value": value}


SEED_CELLS = [
    '# Welcome to the Thoxa notebook. Ctrl/Cmd+Enter runs a cell.\n'
    'let name = "world"\n'
    'println("Hello, {name}!")',

    '# Types unify structs and modules. Fields are private; no `self`.\n'
    'import { sqrt } from "std/Math"\n'
    'type Point(x: Float64, y: Float64) {\n'
    '    export fn distance() -> Float64 { sqrt(x * x + y * y) }\n'
    '}\n'
    'let p = Point(x: 3.0, y: 4.0)\n'
    'p.distance()',

    '# Friendly algebraic errors: May / Trial, matched with `match`.\n'
    'fn half(x!: Int64) -> Trial<Int64, String> {\n'
    '    if x % 2 == 0 { Pass(x / 2) } else { Fail("odd") }\n'
    '}\n'
    'match half(10) {\n'
    '    Pass(v) { "got {v}" }\n'
    '    Fail(e) { "no: {e}" }\n'
    '}',

    '# The last expression of a cell shows its value.\n'
    '[1, 2, 3].append(4)',
]


def _page() -> str:
    return _HTML.replace("__SEED__", json.dumps(SEED_CELLS))


def make_handler(session_holder: dict):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):  # quiet
            pass

        def _send(self, code: int, body: bytes, ctype: str) -> None:
            self.send_response(code)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path in ("/", "/index.html"):
                self._send(200, _page().encode("utf-8"), "text/html; charset=utf-8")
            else:
                self._send(404, b"not found", "text/plain")

        def do_POST(self):
            length = int(self.headers.get("Content-Length", 0))
            raw = self.rfile.read(length) if length else b"{}"
            try:
                payload = json.loads(raw or b"{}")
            except json.JSONDecodeError:
                payload = {}
            if self.path == "/run":
                result = session_holder["session"].run_cell(payload.get("code", ""))
                self._send(200, json.dumps(result).encode("utf-8"), "application/json")
            elif self.path == "/reset":
                session_holder["session"] = NotebookSession()
                self._send(200, b'{"ok":true}', "application/json")
            else:
                self._send(404, b"not found", "text/plain")

    return Handler


def serve(host: str = "127.0.0.1", port: int = 8099, open_browser: bool = True) -> int:
    session_holder = {"session": NotebookSession()}
    httpd = ThreadingHTTPServer((host, port), make_handler(session_holder))
    url = f"http://{host}:{port}/"
    print(f"Thoxa notebook running at {url}  (Ctrl-C to stop)")
    if open_browser:
        try:
            webbrowser.open(url)
        except Exception:
            pass
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopping notebook")
    finally:
        httpd.server_close()
    return 0


_HTML = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Thoxa Notebook</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin: 0; font: 15px/1.5 -apple-system, system-ui, sans-serif;
         background: #14161c; color: #e6e8ee; }
  header { padding: 14px 20px; border-bottom: 1px solid #262a35;
           display: flex; align-items: center; gap: 14px; position: sticky; top: 0;
           background: #14161c; z-index: 5; }
  header h1 { font-size: 17px; margin: 0; font-weight: 650; letter-spacing: .2px; }
  header .tag { font-size: 11px; color: #f0a35e; border: 1px solid #6a4a2a;
                padding: 1px 7px; border-radius: 999px; }
  header .spacer { flex: 1; }
  button { font: inherit; background: #232734; color: #e6e8ee; border: 1px solid #333a49;
           padding: 6px 12px; border-radius: 8px; cursor: pointer; }
  button:hover { background: #2b3040; }
  button.primary { background: #3d6adb; border-color: #3d6adb; }
  button.primary:hover { background: #4b76e6; }
  main { max-width: 900px; margin: 0 auto; padding: 22px 18px 120px; }
  .cell { border: 1px solid #262a35; border-radius: 12px; margin: 14px 0; overflow: hidden;
          background: #171a22; }
  .cell .bar { display: flex; align-items: center; gap: 8px; padding: 7px 10px;
               border-bottom: 1px solid #232734; }
  .cell .bar .n { color: #6f7891; font-size: 12px; font-variant-numeric: tabular-nums; }
  .cell .bar .spacer { flex: 1; }
  textarea { width: 100%; border: 0; outline: 0; resize: vertical; min-height: 72px;
             background: transparent; color: #e6e8ee; padding: 12px 14px;
             font: 13.5px/1.55 "SFMono-Regular", ui-monospace, Menlo, Consolas, monospace;
             tab-size: 4; }
  .out { border-top: 1px solid #232734; padding: 10px 14px; white-space: pre-wrap;
         font: 13px/1.5 ui-monospace, Menlo, Consolas, monospace; }
  .out .stdout { color: #cfd3dd; }
  .out .value { color: #7fd18b; }
  .out .value::before { content: "=> "; color: #55617a; }
  .out .error { color: #ff7b72; }
  .out:empty { display: none; }
  .hint { color: #6f7891; font-size: 12px; }
</style>
</head>
<body>
<header>
  <h1>Thoxa Notebook</h1>
  <span class="tag">pre-alpha</span>
  <span class="hint">bindings persist across cells &middot; Ctrl/&#8984;+Enter to run</span>
  <span class="spacer"></span>
  <button onclick="runAll()">Run all</button>
  <button onclick="addCell()">+ Cell</button>
  <button onclick="resetKernel()">Reset kernel</button>
</header>
<main id="cells"></main>
<script>
const seed = __SEED__;
const cellsEl = document.getElementById('cells');
let counter = 0;

function addCell(code = '') {
  counter += 1;
  const idx = counter;
  const cell = document.createElement('div');
  cell.className = 'cell';
  cell.innerHTML = `
    <div class="bar">
      <span class="n">[${idx}]</span>
      <span class="spacer"></span>
      <button onclick="runCell(this)">Run</button>
      <button onclick="this.closest('.cell').remove()">&times;</button>
    </div>
    <textarea spellcheck="false"></textarea>
    <div class="out"></div>`;
  cellsEl.appendChild(cell);
  const ta = cell.querySelector('textarea');
  ta.value = code;
  ta.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); runCell(ta); }
  });
  autosize(ta);
  ta.addEventListener('input', () => autosize(ta));
  return cell;
}

function autosize(ta) { ta.style.height = 'auto'; ta.style.height = (ta.scrollHeight + 4) + 'px'; }

async function runCell(el) {
  const cell = el.closest('.cell');
  const code = cell.querySelector('textarea').value;
  const out = cell.querySelector('.out');
  out.textContent = '…';
  try {
    const res = await fetch('/run', {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({ code })
    });
    const data = await res.json();
    render(out, data);
  } catch (err) {
    out.innerHTML = '<span class="error">notebook error: ' + String(err) + '</span>';
  }
}

function render(out, data) {
  out.innerHTML = '';
  if (data.output) {
    const s = document.createElement('div'); s.className = 'stdout'; s.textContent = data.output.replace(/\\n$/, '');
    out.appendChild(s);
  }
  if (data.error) {
    const e = document.createElement('div'); e.className = 'error'; e.textContent = data.error;
    out.appendChild(e);
  } else if (data.value !== null && data.value !== undefined) {
    const v = document.createElement('div'); v.className = 'value'; v.textContent = data.value;
    out.appendChild(v);
  }
}

async function runAll() {
  for (const cell of [...cellsEl.querySelectorAll('.cell')]) {
    await runCell(cell.querySelector('textarea'));
  }
}

async function resetKernel() {
  await fetch('/reset', { method: 'POST' });
  for (const o of cellsEl.querySelectorAll('.out')) o.innerHTML = '';
}

seed.forEach(c => addCell(c));
</script>
</body>
</html>"""
