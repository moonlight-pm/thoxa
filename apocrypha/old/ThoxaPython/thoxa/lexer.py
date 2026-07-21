"""The Thoxa lexer.

Turns source text into a token stream. Responsibilities that are Thoxa-specific:

* enforce identifier casing (camelCase values / PascalCase types) and reject underscores;
* `#` line comments and `##` doc comments;
* numeric literals with `0x`/`0b` bases, `_` separators and sized suffixes;
* string literals with `{expr}` interpolation, `{{`/`}}` escaping and multi-line auto-dedent.

Interpolated strings become a ``STRING`` token whose value is a list of parts:
``("str", text)`` or ``("expr", source, span)``; the parser re-parses the expression sources.
"""

from __future__ import annotations

from .errors import Diagnostic, Source, Span, ThoxaError
from .tokens import KEYWORDS, T, Token

_NUM_SUFFIXES = {
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
}
_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "\\": "\\", '"': '"', "'": "'", "0": "\0"}
_HEXDIGITS = frozenset("0123456789abcdefABCDEF")


class Lexer:
    def __init__(self, source: Source) -> None:
        self.src = source
        self.s = source.text
        self.i = 0
        self.line = 1
        self.col = 1
        self.tokens: list[Token] = []

    # -- helpers ----------------------------------------------------------
    def error(self, msg: str, span: Span | None = None, notes: list[str] | None = None):
        raise ThoxaError(Diagnostic(msg, span or Span(self.line, self.col), self.src, notes or []))

    def at_end(self) -> bool:
        return self.i >= len(self.s)

    def peek(self, k: int = 0) -> str:
        j = self.i + k
        return self.s[j] if j < len(self.s) else ""

    def advance(self) -> str:
        c = self.s[self.i]
        self.i += 1
        if c == "\n":
            self.line += 1
            self.col = 1
        else:
            self.col += 1
        return c

    def take(self, pred) -> str:
        out = []
        while not self.at_end() and pred(self.peek()):
            out.append(self.advance())
        return "".join(out)

    def span_from(self, start: Span) -> Span:
        length = self.col - start.col if self.line == start.line else 1
        return Span(start.line, start.col, max(1, length))

    def add(self, kind: T, value, span: Span) -> None:
        self.tokens.append(Token(kind, value, span))

    # -- driver -----------------------------------------------------------
    def tokenize(self) -> list[Token]:
        while not self.at_end():
            c = self.peek()
            if c in " \t\r\n":
                self.advance()
                continue
            if c == "#":
                self.skip_comment()
                continue
            start = Span(self.line, self.col)
            if c.isdigit():
                self.number(start)
            elif c == '"':
                self.string(start)
            elif c == "'":
                self.char(start)
            elif c == "b" and self.peek(1) == "'":
                self.byte(start)
            elif c.isalpha():
                self.name(start)
            else:
                self.operator(start)
        self.add(T.EOF, None, Span(self.line, self.col))
        return self.tokens

    def skip_comment(self) -> None:
        # `#` and `##` are both line comments; `##` is a doc comment (captured later, M5).
        while not self.at_end() and self.peek() != "\n":
            self.advance()

    # -- numbers ----------------------------------------------------------
    def number(self, start: Span) -> None:
        if self.peek() == "0" and self.peek(1) in ("x", "X"):
            self._radix_int(start, "x", 16, "0123456789abcdefABCDEF_")
            return
        if self.peek() == "0" and self.peek(1) in ("o", "O"):
            self._radix_int(start, "o", 8, "01234567_")
            return
        if self.peek() == "0" and self.peek(1) in ("b", "B"):
            self._radix_int(start, "b", 2, "01_")
            return
        intpart = self.take(lambda c: c.isdigit() or c == "_")
        is_float = False
        text = intpart
        if self.peek() == "." and self.peek(1).isdigit():
            is_float = True
            text += self.advance()  # '.'
            text += self.take(lambda c: c.isdigit() or c == "_")
        if self.peek() in ("e", "E"):
            is_float = True
            text += self.advance()
            if self.peek() in ("+", "-"):
                text += self.advance()
            text += self.take(lambda c: c.isdigit() or c == "_")
        suffix = self._num_suffix()
        text = text.replace("_", "")
        if is_float or (suffix and suffix.startswith("f")):
            self.add(T.FLOAT, (float(text), suffix), self.span_from(start))
        else:
            self.add(T.INT, (int(text), suffix), self.span_from(start))

    def _radix_int(self, start: Span, base_char: str, base: int, digit_chars: str) -> None:
        # Shared 0x / 0o / 0b path: consume the prefix, take the base's digits (with `_`
        # separators), and emit one INT token. An empty digit run (`0x`, `0o`, `0b`, or
        # only underscores) is a clean lexer error rather than an uncaught ValueError.
        self.advance(); self.advance()  # consume the "0<base_char>" prefix
        digits = self.take(lambda c: c in digit_chars).replace("_", "")
        if not digits:
            self.error(f"0{base_char} literal has no digits", start)
        self._emit_int(int(digits, base), start)

    def _emit_int(self, value: int, start: Span) -> None:
        suffix = self._num_suffix()
        self.add(T.INT, (value, suffix), self.span_from(start))

    def _num_suffix(self) -> str | None:
        if self.peek() in "iuf" and (self.peek(1).isdigit() or self.peek() == "f"):
            mark = self.i
            run = self.take(lambda c: c.isalnum())
            if run in _NUM_SUFFIXES:
                return run
            # not a real suffix — back out (shouldn't normally happen)
            self.i = mark
        return None

    # -- char -------------------------------------------------------------
    def char(self, start: Span) -> None:
        self.advance()  # opening '
        if self.peek() == "\\":
            self.advance()
            ch = self._escape()
        else:
            ch = self.advance()
        if self.peek() != "'":
            self.error("unterminated char literal (expected closing ')", start)
        self.advance()
        self.add(T.CHAR, ch, self.span_from(start))

    # -- byte literals ----------------------------------------------------
    def byte(self, start: Span) -> None:
        # `b'c'` — a Byte (0..255) written in character form. A bare character
        # must be ASCII; `\xNN` reaches the full byte range. Value is an int.
        self.advance()  # 'b'
        self.advance()  # opening '
        c = self.peek()  # peek is EOF-safe (returns "")
        if c == "":
            self.error("unterminated byte literal (expected closing ')", start)
        if c == "'":
            self.error("empty byte literal (expected one byte)", start)
        if c == "\\":
            self.advance()
            value = self._byte_escape(start)
        else:
            code = ord(self.advance())
            if code > 0x7F:
                self.error(
                    "byte literals must be ASCII; use \\xNN for bytes >= 0x80",
                    start,
                )
            value = code
        if self.peek() != "'":
            self.error("unterminated byte literal (expected closing ')", start)
        self.advance()
        self.add(T.BYTE, value, self.span_from(start))

    def _byte_escape(self, start: Span) -> int:
        if self.at_end():
            self.error("unterminated byte literal (expected an escape)", start)
        c = self.advance()
        if c == "x":
            h1 = self.peek()   # EOF-safe; "" is not in the hex-digit set
            h2 = self.peek(1)
            if h1 not in _HEXDIGITS or h2 not in _HEXDIGITS:
                self.error("byte escape \\x requires two hex digits", start)
            self.advance()
            self.advance()
            return int(h1 + h2, 16)
        if c in _ESCAPES:
            return ord(_ESCAPES[c])
        self.error(f"unknown escape sequence '\\{c}'", start)

    # -- strings ----------------------------------------------------------
    def string(self, start: Span) -> None:
        self.advance()  # opening "
        parts: list[tuple] = []
        buf: list[str] = []
        while True:
            if self.at_end():
                self.error("unterminated string", start)
            c = self.peek()
            if c == '"':
                self.advance()
                break
            if c == "{":
                if self.peek(1) == "{":
                    self.advance(); self.advance(); buf.append("{"); continue
                self.advance()  # consume {
                if buf:
                    parts.append(("str", "".join(buf))); buf = []
                expr_src, espan = self._read_interp_expr()
                parts.append(("expr", expr_src, espan))
                continue
            if c == "}":
                if self.peek(1) == "}":
                    self.advance(); self.advance(); buf.append("}"); continue
                self.error("unmatched '}' in string (use '}}' for a literal brace)")
            if c == "\\":
                self.advance()
                buf.append(self._escape())
                continue
            buf.append(self.advance())
        if buf:
            parts.append(("str", "".join(buf)))
        parts = self._maybe_dedent(parts)
        self.add(T.STRING, parts, self.span_from(start))

    def _read_interp_expr(self) -> tuple[str, Span]:
        start = Span(self.line, self.col)
        depth = 1
        chars: list[str] = []
        while True:
            if self.at_end():
                self.error("unterminated interpolation (expected '}')", start)
            c = self.peek()
            if c == "{":
                depth += 1
                chars.append(self.advance())
            elif c == "}":
                depth -= 1
                if depth == 0:
                    self.advance()
                    break
                chars.append(self.advance())
            elif c == '"':
                chars.append(self.advance())
                while not self.at_end() and self.peek() != '"':
                    if self.peek() == "\\":
                        chars.append(self.advance())
                    chars.append(self.advance())
                if not self.at_end():
                    chars.append(self.advance())
            else:
                chars.append(self.advance())
        return "".join(chars), start

    def _escape(self) -> str:
        c = self.advance()
        if c in _ESCAPES:
            return _ESCAPES[c]
        if c == "x":
            # `\xNN` (two hex digits). In a Char/String this denotes the character
            # with codepoint 0xNN and is ASCII-restricted (0x00–0x7f): a Char is a
            # Unicode scalar and a String is UTF-8, so a lone byte >= 0x80 is
            # ambiguous. Byte literals keep the full 0x00–0xff range (`_byte_escape`).
            h1 = self.peek()   # EOF-safe; "" is not in the hex-digit set
            h2 = self.peek(1)
            if h1 not in _HEXDIGITS or h2 not in _HEXDIGITS:
                self.error("\\x escape requires two hex digits")
            self.advance()
            self.advance()
            value = int(h1 + h2, 16)
            if value > 0x7F:
                self.error("\\xNN escape in a string/char must be ASCII (0x00-0x7f)")
            return chr(value)
        self.error(f"unknown escape sequence '\\{c}'")

    def _maybe_dedent(self, parts: list[tuple]) -> list[tuple]:
        # Multi-line auto-dedent applies to purely-literal strings that start with a newline.
        if len(parts) == 1 and parts[0][0] == "str":
            text = parts[0][1]
            if "\n" in text and text.startswith("\n"):
                lines = text[1:].split("\n")
                indents = [len(ln) - len(ln.lstrip()) for ln in lines if ln.strip()]
                cut = min(indents) if indents else 0
                dedented = "\n".join(ln[cut:] if len(ln) >= cut else "" for ln in lines)
                return [("str", dedented)]
        return parts

    # -- names ------------------------------------------------------------
    def name(self, start: Span) -> None:
        chars: list[str] = []
        while not self.at_end() and (self.peek().isalnum() or self.peek() == "_"):
            if self.peek() == "_":
                self.error(
                    "identifiers may not contain underscores",
                    Span(self.line, self.col),
                    notes=["Thoxa uses camelCase for values and PascalCase for types"],
                )
            chars.append(self.advance())
        text = "".join(chars)
        span = self.span_from(start)
        if text in KEYWORDS:
            self.add(KEYWORDS[text], text, span)
        elif text[0].islower():
            self.add(T.LOWER, text, span)
        else:
            self.add(T.UPPER, text, span)

    # -- operators --------------------------------------------------------
    _THREE = {"..=": T.DOTDOTEQ, "...": T.ELLIPSIS}
    _TWO = {
        "==": T.EQEQ, "!=": T.NEQ, "<=": T.LE, ">=": T.GE, "->": T.ARROW,
        "+=": T.PLUSEQ, "-=": T.MINUSEQ, "*=": T.STAREQ, "/=": T.SLASHEQ, "..": T.DOTDOT,
        "<<": T.SHL, ">>": T.SHR,
    }
    _ONE = {
        "+": T.PLUS, "-": T.MINUS, "*": T.STAR, "/": T.SLASH, "%": T.PERCENT,
        "<": T.LT, ">": T.GT, "=": T.EQ, "?": T.QUESTION, "|": T.PIPE, "!": T.BANG,
        ".": T.DOT, "(": T.LPAREN, ")": T.RPAREN, "[": T.LBRACKET, "]": T.RBRACKET,
        "{": T.LBRACE, "}": T.RBRACE, ",": T.COMMA, ":": T.COLON,
        "&": T.AMP, "^": T.CARET, "~": T.TILDE,
    }

    def operator(self, start: Span) -> None:
        three = self.peek() + self.peek(1) + self.peek(2)
        if three in self._THREE:
            self.advance(); self.advance(); self.advance()
            self.add(self._THREE[three], three, self.span_from(start))
            return
        two = self.peek() + self.peek(1)
        if two in self._TWO:
            self.advance(); self.advance()
            self.add(self._TWO[two], two, self.span_from(start))
            return
        one = self.peek()
        if one in self._ONE:
            self.advance()
            self.add(self._ONE[one], one, self.span_from(start))
            return
        self.error(f"unexpected character {one!r}")


def tokenize(text: str, filename: str = "<input>") -> list[Token]:
    return Lexer(Source(text, filename)).tokenize()
