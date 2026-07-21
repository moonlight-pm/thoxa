"""Tests for the libc FFI: extern fn, the Bytes buffer, and the ffi import."""

import os
import socket as socket_mod
import threading
import time

import pytest

from thoxa import ast
from thoxa.checker import check
from thoxa.errors import Source, ThoxaError
from thoxa.interp import run
from thoxa.lexer import Lexer
from thoxa.parser import Parser


def run_out(src):
    buf = []
    run(src, "<test>", out=buf.append)
    return "".join(buf)


def parse(src):
    s = Source(src, "<test>")
    return Parser(Lexer(s).tokenize(), s).parse_program()


# --------------------------------------------------------------------- parsing
def test_parses_extern_forms():
    mod = parse(
        'extern fn getpid() -> Int32 from "c"\n'
        'extern fn inetPton(af!: Int32, src!: Bytes, dst!: Bytes) -> Int32 from "c" "inet_pton"\n'
        'extern fn freeMem(ptr!: Int64) from "c"\n'
    )
    externs = [s for s in mod.body if isinstance(s, ast.ExternFn)]
    assert [e.name for e in externs] == ["getpid", "inetPton", "freeMem"]
    assert externs[0].library == "c" and externs[0].symbol is None
    assert externs[1].symbol == "inet_pton"
    assert all(p.positional for p in externs[1].params)
    assert externs[2].return_type is None  # omitted return => Void


def test_parses_ffi_import():
    mod = parse('import { errno, strerror } from "ffi"')
    imp = mod.body[0]
    assert isinstance(imp, ast.Import)
    assert imp.names == [("errno", "errno"), ("strerror", "strerror")] and imp.module == "ffi"


# --------------------------------------------------------------------- checker
def _messages(src):
    return [d.message for d in check(src)]


def test_check_clean_extern_program():
    src = (
        'import { errno, strerror } from "ffi"\n'
        'extern fn getpid() -> Int32 from "c"\n'
        'extern fn write(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c"\n'
        'fn main() -> Int64 { let n = write(1, Bytes.fromString("hi"), 2)  errno()  0 }\n'
    )
    assert check(src) == []


@pytest.mark.parametrize(
    "src, needle",
    [
        ('extern fn f(x!: Bool) -> Int32 from "c"', "Bool' cannot cross"),
        ('extern fn f(x!: Int32) -> Bool from "c"', "Bool' cannot cross"),
        ('extern fn f(x!: Char) -> Int32 from "c"', "Char' cannot cross"),
        ('extern fn f(x: Int32) -> Int32 from "c"', "must be positional"),
        ('extern fn f(x!: Int32) -> Int32 from "rust"', "only \"c\" is available"),
        ('extern fn f(x!: Int) -> Int32 from "c"', "Int' cannot cross"),
        ('extern fn f(x!: Int32) -> String from "c"', "String' cannot cross"),
        ('import { nope } from "ffi"', "has no export"),
        ('import { errno } from "os"', "unknown module root"),
        ('import ffi from "ffi"', "must name its members"),
        # Int/Float are rejected even nested inside a generic type argument.
        ('fn f() -> May<Int> { Not() }', "Int' is not a valid"),
        ('fn f(x!: Trial<Int64, Float>) -> Int64 { 0 }', "Float' is not a valid"),
        # extern params cannot carry a default or a variadic marker.
        ('extern fn f(x!: Int32 = 0) -> Int32 from "c"', "cannot have a default"),
        ('extern fn h(...z!: Int32) -> Int32 from "c"', "cannot be variadic"),
    ],
)
def test_check_rejects(src, needle):
    assert any(needle in m for m in _messages(src)), _messages(src)


def test_errno_undefined_without_import():
    # errno is not ambient; using it without the ffi import is an undefined name.
    assert any("errno" in m and "not defined" in m for m in _messages(
        'fn main() -> Int64 { let e = errno()  0 }'
    ))


# --------------------------------------------------------------------- runtime
def test_getpid_matches_process():
    src = 'extern fn getpid() -> Int32 from "c"\nfn main() -> Int64 { getpid() as Int64 }'
    out = []
    assert run(src, "<t>", out=out.append) == os.getpid()


def test_strlen_over_bytes_and_string():
    src = (
        'extern fn strlen(s!: Bytes) -> UInt64 from "c"\n'
        'extern fn strlenS(s!: String) -> UInt64 from "c" "strlen"\n'
        'fn main() -> Int64 {\n'
        '  let a = strlen(Bytes.fromString("hello"))\n'
        '  let b = strlenS("worldwide")\n'
        '  println("{a} {b}")\n'
        '  0\n'
        '}\n'
    )
    assert run_out(src) == "5 9\n"


def test_bytes_construction_and_indexing():
    src = (
        'fn main() -> Int64 {\n'
        '  let b = Bytes(4)\n'
        '  b.set(0, 65)\n'
        '  b.set(1, 66)\n'
        '  println("{b.count} {b.get(0)} {b.get(1)} {b.toString()}")\n'
        '  0\n'
        '}\n'
    )
    assert run_out(src) == "4 65 66 AB\n"


def test_bytes_from_string_roundtrip():
    src = 'fn main() -> Int64 { println(Bytes.fromString("héllo").toString())  0 }'
    assert run_out(src) == "héllo\n"


def test_bytes_index_out_of_range_errors():
    src = 'fn main() -> Int64 { let b = Bytes(2)  b.get(2)  0 }'
    with pytest.raises(ThoxaError, match="out of range"):
        run_out(src)


def test_memcpy_writes_into_buffer():
    # C writes into a Bytes buffer in place; the mutation is visible to Thoxa.
    src = (
        'extern fn memcpy(dst!: Bytes, src!: Bytes, n!: UInt64) from "c"\n'
        'fn main() -> Int64 {\n'
        '  let dst = Bytes(9)\n'
        '  memcpy(dst, Bytes.fromString("worldwide"), 5)\n'
        '  println(dst.toString())\n'
        '  0\n'
        '}\n'
    )
    assert run_out(src) == "world\n"


def test_errno_and_strerror_on_failure():
    src = (
        'import { errno, strerror } from "ffi"\n'
        'extern fn close(fd!: Int32) -> Int32 from "c"\n'
        'fn main() -> Int64 {\n'
        '  let r = close(-1)\n'  # EBADF
        '  println("{r} {errno() > 0} {strerror(errno()).count > 0}")\n'
        '  0\n'
        '}\n'
    )
    assert run_out(src) == "-1 true true\n"


def test_write_through_a_pipe():
    r, w = os.pipe()
    try:
        src = (
            'extern fn write(fd!: Int32, buf!: Bytes, count!: UInt64) -> Int64 from "c"\n'
            f'fn main() -> Int64 {{ write({w}, Bytes.fromString("ping"), 4)  0 }}\n'
        )
        assert run_out(src) == ""
        assert os.read(r, 16) == b"ping"
    finally:
        os.close(r)
        os.close(w)


def test_unknown_symbol_errors():
    src = 'extern fn nope() -> Int32 from "c" "no_such_symbol_xyzzy"\nfn main() -> Int64 { 0 }'
    with pytest.raises(ThoxaError, match="no such symbol"):
        run_out(src)


# --------------------------------------------------------------------- sockets
_ECHO_EXTERNS = (
    'extern fn socket(domain!: Int32, kind!: Int32, protocol!: Int32) -> Int32 from "c"\n'
    'extern fn setsockopt(fd!: Int32, level!: Int32, optname!: Int32, optval!: Bytes, optlen!: UInt32) -> Int32 from "c"\n'
    'extern fn bind(fd!: Int32, addr!: Bytes, addrlen!: UInt32) -> Int32 from "c"\n'
    'extern fn listen(fd!: Int32, backlog!: Int32) -> Int32 from "c"\n'
    'extern fn accept(fd!: Int32, addr!: Bytes, addrlen!: Bytes) -> Int32 from "c"\n'
    'extern fn recv(fd!: Int32, buf!: Bytes, len!: UInt64, flags!: Int32) -> Int64 from "c"\n'
    'extern fn send(fd!: Int32, buf!: Bytes, len!: UInt64, flags!: Int32) -> Int64 from "c"\n'
    'extern fn close(fd!: Int32) -> Int32 from "c"\n'
)

_SOCKADDR_IN = (
    'fn sockaddrIn(port!: Int32) -> Bytes {\n'
    '  let a = Bytes(16)  a.set(0, 2)  a.set(2, port / 256)  a.set(3, port % 256)  a\n'
    '}\n'
)


def _free_port():
    s = socket_mod.socket(socket_mod.AF_INET, socket_mod.SOCK_STREAM)
    s.setsockopt(socket_mod.SOL_SOCKET, socket_mod.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def _serve_in_thread(src):
    t = threading.Thread(target=lambda: run(src, "<server>"), daemon=True)
    t.start()
    return t


def _connect(port, timeout=5.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            return socket_mod.create_connection(("127.0.0.1", port), timeout=1)
        except OSError:
            time.sleep(0.05)
    raise AssertionError("server never started listening")


def test_tcp_echo_roundtrip():
    port = _free_port()
    # One-shot echo: accept a single connection, echo one chunk, exit.
    src = _ECHO_EXTERNS + _SOCKADDR_IN + (
        'fn main() -> Int64 {\n'
        '  let fd = socket(2, 1, 0)\n'
        '  let one = Bytes(4)  one.set(0, 1)  setsockopt(fd, 1, 2, one, 4)\n'
        f'  bind(fd, sockaddrIn({port}), 16)\n'
        '  listen(fd, 16)\n'
        '  let clen = Bytes(4)  clen.set(0, 16)\n'
        '  let client = accept(fd, Bytes(16), clen)\n'
        '  let buf = Bytes(4096)\n'
        '  let n = recv(client, buf, 4096, 0)\n'
        '  send(client, buf, n, 0)\n'
        '  close(client)  close(fd)  0\n'
        '}\n'
    )
    server = _serve_in_thread(src)
    conn = _connect(port)
    try:
        conn.sendall(b"hello thoxa")
        assert conn.recv(64) == b"hello thoxa"
    finally:
        conn.close()
        server.join(timeout=2)


def test_http_responds_200():
    import http.client

    port = _free_port()
    # One-shot HTTP responder: accept a single connection, answer 200 OK, exit.
    # The handler logic (building the HTTP/1.1 response) is Thoxa, not Python.
    src = _ECHO_EXTERNS + _SOCKADDR_IN + (
        'fn main() -> Int64 {\n'
        '  let fd = socket(2, 1, 0)\n'
        '  let one = Bytes(4)  one.set(0, 1)  setsockopt(fd, 1, 2, one, 4)\n'
        f'  bind(fd, sockaddrIn({port}), 16)\n'
        '  listen(fd, 16)\n'
        '  let clen = Bytes(4)  clen.set(0, 16)\n'
        '  let client = accept(fd, Bytes(16), clen)\n'
        '  let buf = Bytes(4096)\n'
        '  let n = recv(client, buf, 4096, 0)\n'
        '  let body = "Hello from Thoxa over real syscalls!\\n"\n'
        '  let resp = "HTTP/1.1 200 OK\\r\\nContent-Type: text/plain\\r\\n'
        'Content-Length: {body.count}\\r\\nConnection: close\\r\\n\\r\\n{body}"\n'
        '  let out = Bytes.fromString(resp)\n'
        '  send(client, out, out.count, 0)\n'
        '  close(client)  close(fd)  0\n'
        '}\n'
    )
    server = _serve_in_thread(src)
    # Reuse _connect for its retry-until-listening loop, then drive a real HTTP
    # GET over that socket via http.client so we assert on a parsed status line.
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    conn.sock = _connect(port)
    try:
        conn.request("GET", "/")
        resp = conn.getresponse()
        body = resp.read()
        assert resp.status == 200
        assert body == b"Hello from Thoxa over real syscalls!\n"
        assert int(resp.getheader("Content-Length")) == len(body)
    finally:
        conn.close()
        server.join(timeout=2)


def test_extern_extra_args_is_a_clean_error():
    # Too many arguments must be a located Thoxa error, not a silent drop.
    src = 'extern fn close(fd!: Int32) -> Int32 from "c"\nfn main() -> Int64 { close(1, 2)  0 }'
    with pytest.raises(ThoxaError, match="argument"):
        run_out(src)


def test_extern_too_few_args_is_a_clean_error():
    # Too few arguments must be a located Thoxa error, not a raw ctypes TypeError.
    src = 'extern fn close(fd!: Int32) -> Int32 from "c"\nfn main() -> Int64 { close()  0 }'
    with pytest.raises(ThoxaError, match="argument"):
        run_out(src)


def test_extern_wrong_arg_type_is_a_clean_error():
    # A non-String where C wants char* must error, never deref a raw address.
    src = 'extern fn strlen(s!: String) -> UInt64 from "c"\nfn main() -> Int64 { strlen(12345) as Int64 }'
    with pytest.raises(ThoxaError, match="String"):
        run_out(src)


def test_extern_arg_out_of_range_is_a_clean_error():
    # An out-of-range integer must error, not silently truncate (ctypes masks it).
    src = 'extern fn absv(x!: UInt8) -> Int32 from "c" "abs"\nfn main() -> Int64 { absv(300) as Int64 }'
    with pytest.raises(ThoxaError, match="out of range"):
        run_out(src)
