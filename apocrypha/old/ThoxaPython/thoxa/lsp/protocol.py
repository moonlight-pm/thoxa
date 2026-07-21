"""A tiny, dependency-free JSON-RPC-over-stdio transport for the LSP.

Just enough of the Language Server Protocol base protocol to frame messages:
``Content-Length``-delimited JSON bodies. No third-party LSP library — the v1
surface is small and staying zero-dependency matches the rest of the project.
"""
from __future__ import annotations

import json


def write_message(stream, message: dict) -> None:
    """Write one LSP message (headers + JSON body) to a binary stream."""
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    stream.write(b"Content-Length: %d\r\n\r\n" % len(body))
    stream.write(body)
    flush = getattr(stream, "flush", None)
    if flush is not None:
        flush()


def read_message(stream) -> dict | None:
    """Read one LSP message from a binary stream, or None at end-of-stream."""
    headers: dict[bytes, bytes] = {}
    while True:
        line = stream.readline()
        if not line:
            return None
        line = line.rstrip(b"\r\n")
        if line == b"":
            break
        key, _, value = line.partition(b":")
        headers[key.strip().lower()] = value.strip()

    length = int(headers.get(b"content-length", b"0"))
    if length <= 0:
        return {}
    body = b""
    while len(body) < length:
        chunk = stream.read(length - len(body))
        if not chunk:
            return None
        body += chunk
    return json.loads(body.decode("utf-8"))


class Connection:
    """Reads and writes LSP messages over a pair of binary streams."""

    def __init__(self, rstream, wstream) -> None:
        self._r = rstream
        self._w = wstream

    def read(self) -> dict | None:
        return read_message(self._r)

    def send(self, message: dict) -> None:
        write_message(self._w, message)
