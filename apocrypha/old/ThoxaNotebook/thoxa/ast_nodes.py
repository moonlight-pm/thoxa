"""AST node definitions using dataclasses."""

from dataclasses import dataclass, field
from typing import Any, List, Optional, Union
from enum import Enum, auto


class NodeType(Enum):
    """AST node types for visitor pattern."""
    PROGRAM = auto()
    BLOCK = auto()
    LET = auto()
    FUNCTION = auto()
    TYPE_DEF = auto()
    IF = auto()
    LOOP = auto()
    WHILE = auto()
    FOR = auto()
    MATCH = auto()
    RETURN = auto()
    BREAK = auto()
    CONTINUE = auto()
    BINARY_OP = auto()
    UNARY_OP = auto()
    CALL = auto()
    INDEX = auto()
    MEMBER = auto()
    LITERAL = auto()
    IDENTIFIER = auto()
    ARRAY = auto()
    ASSIGNMENT = auto()


@dataclass
class ASTNode:
    """Base class for all AST nodes."""
    
    def accept(self, visitor):
        """Accept a visitor for the visitor pattern."""
        method_name = f"visit_{self.node_type.name.lower()}"
        method = getattr(visitor, method_name, visitor.generic_visit)
        return method(self)


# Expressions

@dataclass
class Literal(ASTNode):
    """Literal value (int, float, string, bool, etc.)."""
    value: Any
    type_hint: Optional[str] = None
    node_type: NodeType = field(default=NodeType.LITERAL, init=False)


@dataclass 
class Identifier(ASTNode):
    """Variable or function identifier."""
    name: str
    node_type: NodeType = field(default=NodeType.IDENTIFIER, init=False)


@dataclass
class BinaryOp(ASTNode):
    """Binary operation (e.g., a + b)."""
    left: ASTNode
    op: str
    right: ASTNode
    node_type: NodeType = field(default=NodeType.BINARY_OP, init=False)


@dataclass
class UnaryOp(ASTNode):
    """Unary operation (e.g., -x, not b)."""
    op: str
    operand: ASTNode
    node_type: NodeType = field(default=NodeType.UNARY_OP, init=False)


@dataclass
class Call(ASTNode):
    """Function call."""
    func: Union[str, ASTNode]
    args: List[ASTNode] = field(default_factory=list)
    kwargs: dict = field(default_factory=dict)
    node_type: NodeType = field(default=NodeType.CALL, init=False)


@dataclass
class Index(ASTNode):
    """Array indexing (e.g., arr[0])."""
    obj: ASTNode
    index: ASTNode
    node_type: NodeType = field(default=NodeType.INDEX, init=False)


@dataclass
class Member(ASTNode):
    """Member access (e.g., obj.field)."""
    obj: ASTNode
    member: str
    node_type: NodeType = field(default=NodeType.MEMBER, init=False)


@dataclass
class Array(ASTNode):
    """Array literal [1, 2, 3]."""
    elements: List[ASTNode]
    node_type: NodeType = field(default=NodeType.ARRAY, init=False)


# Statements

@dataclass
class Block(ASTNode):
    """Block of statements."""
    statements: List[ASTNode]
    node_type: NodeType = field(default=NodeType.BLOCK, init=False)


@dataclass
class Let(ASTNode):
    """Variable declaration."""
    name: str
    value: ASTNode
    mutable: bool = False
    type_annotation: Optional[str] = None
    node_type: NodeType = field(default=NodeType.LET, init=False)


@dataclass
class Assignment(ASTNode):
    """Variable assignment."""
    target: ASTNode  # Can be Identifier, Index, or Member
    value: ASTNode
    node_type: NodeType = field(default=NodeType.ASSIGNMENT, init=False)


@dataclass
class Function(ASTNode):
    """Function definition."""
    name: str
    params: List[tuple[str, Optional[str], Optional[ASTNode]]]  # (name, type, default)
    body: Block
    return_type: Optional[str] = None
    node_type: NodeType = field(default=NodeType.FUNCTION, init=False)


@dataclass
class TypeDef(ASTNode):
    """Type definition."""
    name: str
    constructor_params: List[tuple[str, str, Optional[ASTNode]]]  # (name, type, default)
    body: Block
    node_type: NodeType = field(default=NodeType.TYPE_DEF, init=False)


@dataclass
class If(ASTNode):
    """If statement/expression."""
    condition: ASTNode
    then_branch: ASTNode
    else_branch: Optional[ASTNode] = None
    node_type: NodeType = field(default=NodeType.IF, init=False)


@dataclass
class Loop(ASTNode):
    """Infinite loop."""
    body: Block
    node_type: NodeType = field(default=NodeType.LOOP, init=False)


@dataclass
class While(ASTNode):
    """While loop."""
    condition: ASTNode
    body: Block
    node_type: NodeType = field(default=NodeType.WHILE, init=False)


@dataclass
class For(ASTNode):
    """For-in loop."""
    var: str
    iterable: ASTNode
    body: Block
    node_type: NodeType = field(default=NodeType.FOR, init=False)


@dataclass
class Match(ASTNode):
    """Match expression."""
    expr: ASTNode
    arms: List[tuple[ASTNode, ASTNode]]  # (pattern, body)
    else_clause: Optional[ASTNode] = None
    node_type: NodeType = field(default=NodeType.MATCH, init=False)


@dataclass
class Return(ASTNode):
    """Return statement."""
    value: Optional[ASTNode] = None
    node_type: NodeType = field(default=NodeType.RETURN, init=False)


@dataclass
class Break(ASTNode):
    """Break statement."""
    node_type: NodeType = field(default=NodeType.BREAK, init=False)


@dataclass
class Continue(ASTNode):
    """Continue statement."""
    node_type: NodeType = field(default=NodeType.CONTINUE, init=False)


@dataclass
class Program(ASTNode):
    """Root node of the AST."""
    statements: List[ASTNode]
    node_type: NodeType = field(default=NodeType.PROGRAM, init=False)