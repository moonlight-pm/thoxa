"""Token kinds and the Token record for the Thoxa lexer."""

from __future__ import annotations

import enum
from dataclasses import dataclass
from typing import Any

from .errors import Span


class T(enum.Enum):
    # literals
    INT = "INT"
    FLOAT = "FLOAT"
    STRING = "STRING"
    CHAR = "CHAR"
    BYTE = "BYTE"
    # identifiers (casing enforced at the lexer)
    LOWER = "LOWER"  # camelCase — a value / function name
    UPPER = "UPPER"  # PascalCase — a type name
    # keywords
    LET = "let"
    MUT = "mut"
    FN = "fn"
    TYPE = "type"
    EXPORT = "export"
    IMPORT = "import"
    FROM = "from"
    EXTERN = "extern"
    IF = "if"
    ELSE = "else"
    LOOP = "loop"
    WHILE = "while"
    FOR = "for"
    IN = "in"
    BREAK = "break"
    CONTINUE = "continue"
    RETURN = "return"
    MATCH = "match"
    AND = "and"
    OR = "or"
    NOT = "not"
    AS = "as"
    CAN = "can"
    IS = "is"
    TRUE = "true"
    FALSE = "false"
    # operators & punctuation
    PLUS = "+"
    MINUS = "-"
    STAR = "*"
    SLASH = "/"
    PERCENT = "%"
    EQEQ = "=="
    NEQ = "!="
    LT = "<"
    GT = ">"
    LE = "<="
    GE = ">="
    EQ = "="
    PLUSEQ = "+="
    MINUSEQ = "-="
    STAREQ = "*="
    SLASHEQ = "/="
    QUESTION = "?"
    PIPE = "|"
    AMP = "&"
    CARET = "^"
    TILDE = "~"
    SHL = "<<"
    SHR = ">>"
    ARROW = "->"
    BANG = "!"
    DOT = "."
    DOTDOT = ".."
    DOTDOTEQ = "..="
    ELLIPSIS = "..."
    LPAREN = "("
    RPAREN = ")"
    LBRACKET = "["
    RBRACKET = "]"
    LBRACE = "{"
    RBRACE = "}"
    COMMA = ","
    COLON = ":"
    EOF = "EOF"


KEYWORDS: dict[str, T] = {
    "let": T.LET, "mut": T.MUT, "fn": T.FN, "type": T.TYPE, "export": T.EXPORT,
    "import": T.IMPORT, "from": T.FROM, "extern": T.EXTERN, "if": T.IF, "else": T.ELSE, "loop": T.LOOP,
    "while": T.WHILE, "for": T.FOR, "in": T.IN, "break": T.BREAK, "continue": T.CONTINUE,
    "return": T.RETURN, "match": T.MATCH, "and": T.AND, "or": T.OR, "not": T.NOT,
    "as": T.AS, "can": T.CAN, "is": T.IS, "true": T.TRUE, "false": T.FALSE,
}


@dataclass
class Token:
    kind: T
    value: Any
    span: Span

    def __repr__(self) -> str:
        return f"Token({self.kind.name}, {self.value!r}, {self.span})"
