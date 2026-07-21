"""Thoxa programming language interpreter."""

__version__ = "0.1.0"

from .interpreter import Interpreter
from .parser import parse
from .ast_nodes import *


def interpret(ast):
    """Interpret a parsed AST."""
    interpreter = Interpreter()
    return interpreter.interpret(ast)


__all__ = ["Interpreter", "parse", "interpret"]