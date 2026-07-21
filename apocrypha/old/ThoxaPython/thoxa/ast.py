"""Thoxa AST — the stable typed-AST seam between frontend and evaluator.

Every node carries a ``span`` for diagnostics. Nodes are plain dataclasses; the
interpreter (and, later, the type checker and lowering passes) walk them.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional, Union

from .errors import Span


# ---------------------------------------------------------------- types
@dataclass
class TypeRef:
    """A type expression, e.g. `Int64`, `Array<T>`, `May<String>`, `Fn(Int64)->Int64`."""

    name: str
    args: list["TypeRef"] = field(default_factory=list)
    ret: Optional["TypeRef"] = None  # for Fn(...) -> ret
    span: Optional[Span] = None
    int_value: Optional[int] = None  # a numeric type argument, e.g. the 32 in Array<Byte, 32>

    def __str__(self) -> str:
        if self.name == "Fn":
            a = ", ".join(str(x) for x in self.args)
            return f"Fn({a}) -> {self.ret}"
        if self.int_value is not None:
            return str(self.int_value)
        if self.args:
            return f"{self.name}<{', '.join(str(a) for a in self.args)}>"
        return self.name


# ---------------------------------------------------------------- params / members
@dataclass
class Param:
    name: str
    type_ann: Optional[TypeRef]
    default: Optional["Node"]
    positional: bool
    variadic: bool
    span: Span
    # Only meaningful on type constructor params: publishes the field for
    # external access (`export type R(export method: String)`). Always False
    # for function / extern parameters (parser rejects `export` there).
    exported: bool = False


@dataclass
class Field:
    name: str
    mutable: bool
    type_ann: Optional[TypeRef]
    default: Optional["Node"]
    exported: bool
    span: Span


# ---------------------------------------------------------------- top-level / statements
@dataclass
class Module:
    body: list["Node"]
    span: Span


@dataclass
class LetBinding:
    name: str
    mutable: bool
    type_ann: Optional[TypeRef]
    value: "Node"
    exported: bool
    span: Span


@dataclass
class Assign:
    target: "Node"      # Name | FieldAccess | Index
    op: str             # '=', '+=', '-=', '*=', '/='
    value: "Node"
    span: Span


@dataclass
class FunctionDef:
    name: str
    type_params: list[str]
    params: list[Param]
    return_type: Optional[TypeRef]
    body: "Block"
    exported: bool
    span: Span


@dataclass
class ExternFn:
    """A binding to a C function, e.g. `extern fn getpid() -> Int32 from "c"`.

    Has no body — the implementation lives in `library`. `symbol` is the C name
    when it differs from the (camelCase) Thoxa `name`; None means use `name`.
    """

    name: str
    params: list[Param]
    return_type: Optional[TypeRef]
    library: str
    symbol: Optional[str]
    exported: bool
    span: Span


@dataclass
class TypeDef:
    name: str
    type_params: list[str]
    ctor_params: list[Param]
    members: list[Union[Field, FunctionDef]]
    exported: bool
    span: Span
    # A valued nominal type (`type ReadOnly 0`) carries an intrinsic scalar,
    # readable via `x as Int32`. `None` for ordinary struct/unit types.
    value: Optional["Node"] = None


@dataclass
class TypeAlias:
    name: str
    type_params: list[str]
    variants: list[TypeRef]
    exported: bool
    span: Span


@dataclass
class Return:
    value: Optional["Node"]
    span: Span


@dataclass
class Break:
    span: Span


@dataclass
class Continue:
    span: Span


@dataclass
class Import:
    # Selective form: a list of (imported, local) pairs — `local` equals
    # `imported` unless renamed with `as`. `None` marks a namespace import.
    names: Optional[list[tuple[str, str]]]
    namespace: Optional[str]   # namespace import: the written name (must equal the module's canonical name)
    alias: Optional[str]       # namespace import: the `as` binding, or None
    module: str
    span: Span


# ---------------------------------------------------------------- expressions
@dataclass
class Block:
    stmts: list["Node"]
    span: Span


@dataclass
class If:
    cond: "Node"
    then: Block
    otherwise: Optional["Node"]   # Block | If | None
    span: Span


@dataclass
class Loop:
    body: Block
    span: Span


@dataclass
class While:
    cond: "Node"
    body: Block
    span: Span


@dataclass
class For:
    var: str
    iterable: "Node"
    body: Block
    span: Span


@dataclass
class Match:
    scrutinee: "Node"
    arms: list["MatchArm"]
    else_body: Optional[Block]
    span: Span


@dataclass
class MatchArm:
    pattern: "Pattern"
    body: Block
    span: Span


@dataclass
class BinaryOp:
    op: str
    left: "Node"
    right: "Node"
    span: Span


@dataclass
class Logical:
    op: str   # 'and' | 'or'
    left: "Node"
    right: "Node"
    span: Span


@dataclass
class UnaryOp:
    op: str   # 'not' | '-'
    operand: "Node"
    span: Span


@dataclass
class RangeExpr:
    start: "Node"
    end: "Node"
    inclusive: bool
    span: Span


@dataclass
class Call:
    callee: "Node"
    args: list["Arg"]
    type_args: list[TypeRef]
    span: Span


@dataclass
class Arg:
    label: Optional[str]
    value: "Node"
    spread: bool
    span: Span


@dataclass
class FieldAccess:
    obj: "Node"
    name: str
    span: Span


@dataclass
class Index:
    obj: "Node"
    index: "Node"
    span: Span


@dataclass
class Try:
    operand: "Node"
    span: Span


@dataclass
class Cast:
    operand: "Node"
    type_ref: TypeRef
    span: Span


@dataclass
class CanCheck:
    operand: "Node"
    type_ref: TypeRef
    span: Span


@dataclass
class IsCheck:
    operand: "Node"
    type_ref: TypeRef
    span: Span


@dataclass
class ArrayLit:
    elements: list["Node"]
    span: Span


@dataclass
class IntLit:
    value: int
    suffix: Optional[str]
    span: Span


@dataclass
class FloatLit:
    value: float
    suffix: Optional[str]
    span: Span


@dataclass
class BoolLit:
    value: bool
    span: Span


@dataclass
class CharLit:
    value: str
    span: Span


@dataclass
class ByteLit:
    value: int  # 0..255 — a Byte written in character form, e.g. b'q'
    span: Span


@dataclass
class StringLit:
    parts: list[tuple]   # ("str", text) | ("expr", Node)
    span: Span


@dataclass
class VoidLit:
    span: Span


@dataclass
class Name:
    ident: str
    span: Span


@dataclass
class TypeValue:
    """An UPPER-case name in expression position: a type used as a value / constructor."""

    name: str
    span: Span


# ---------------------------------------------------------------- patterns
@dataclass
class LiteralPattern:
    value: "Node"
    span: Span


@dataclass
class ConstructorPattern:
    name: str
    subpatterns: list["Pattern"]
    span: Span


@dataclass
class BindingPattern:
    name: str
    span: Span


Pattern = Union[LiteralPattern, ConstructorPattern, BindingPattern]
Node = object  # structural; nodes are the dataclasses above
