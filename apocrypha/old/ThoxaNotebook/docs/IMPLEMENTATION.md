# Thoxa Python Implementation Guide

## Project Structure

```
thoxa/
├── __init__.py
├── __main__.py           # Entry point
├── lexer.py              # Tokenization
├── parser.py             # AST generation
├── ast.py                # AST node definitions
├── types.py              # Type system
├── value.py              # Value representation
├── interpreter.py        # Evaluation engine
├── environment.py        # Variable scoping
├── builtins.py           # Built-in functions
├── type_checker.py       # Static type checking
├── type_inference.py     # Type inference
├── patterns.py           # Pattern matching
├── modules.py            # Module system
├── errors.py             # Error handling
└── repl.py              # Interactive REPL

tests/
├── test_lexer.py
├── test_parser.py
├── test_interpreter.py
├── test_types.py
├── test_patterns.py
└── integration/
    └── test_programs.py

examples/
├── hello.thx
├── fibonacci.thx
├── shapes.thx
└── error_handling.thx
```

## Core Components

### Lexer Implementation

```python
# lexer.py
from dataclasses import dataclass
from enum import Enum, auto
from typing import Iterator, Optional

class TokenType(Enum):
    # Literals
    INTEGER = auto()
    FLOAT = auto()
    STRING = auto()
    CHAR = auto()
    BOOL = auto()
    
    # Keywords
    LET = auto()
    MUT = auto()
    FN = auto()
    TYPE = auto()
    IF = auto()
    ELSE = auto()
    MATCH = auto()
    LOOP = auto()
    WHILE = auto()
    FOR = auto()
    IN = auto()
    BREAK = auto()
    CONTINUE = auto()
    RETURN = auto()
    
    # Operators
    PLUS = auto()
    MINUS = auto()
    STAR = auto()
    SLASH = auto()
    PERCENT = auto()
    EQ = auto()
    NE = auto()
    LT = auto()
    GT = auto()
    LE = auto()
    GE = auto()
    AND = auto()
    OR = auto()
    NOT = auto()
    
    # Punctuation
    LPAREN = auto()
    RPAREN = auto()
    LBRACE = auto()
    RBRACE = auto()
    LBRACKET = auto()
    RBRACKET = auto()
    SEMICOLON = auto()
    COLON = auto()
    COMMA = auto()
    DOT = auto()
    ARROW = auto()
    ASSIGN = auto()
    
    # Special
    IDENTIFIER = auto()
    EOF = auto()

@dataclass
class Token:
    type: TokenType
    value: any
    line: int
    column: int

class Lexer:
    def __init__(self, source: str):
        self.source = source
        self.position = 0
        self.line = 1
        self.column = 1
        
    def tokenize(self) -> Iterator[Token]:
        while self.position < len(self.source):
            self._skip_whitespace()
            if self.position >= len(self.source):
                break
                
            # Skip comments
            if self._peek() == '#':
                self._skip_comment()
                continue
            
            # String literals with interpolation
            if self._peek() == '"':
                yield self._read_string()
            # Numbers
            elif self._peek().isdigit():
                yield self._read_number()
            # Identifiers and keywords
            elif self._peek().isalpha() or self._peek() == '_':
                yield self._read_identifier()
            # Operators and punctuation
            else:
                yield self._read_operator()
        
        yield Token(TokenType.EOF, None, self.line, self.column)
```

### Parser Implementation

```python
# parser.py
from dataclasses import dataclass
from typing import List, Optional
from ast import *
from lexer import Token, TokenType

class Parser:
    def __init__(self, tokens: List[Token]):
        self.tokens = tokens
        self.current = 0
    
    def parse(self) -> Program:
        statements = []
        while not self._is_at_end():
            stmt = self._parse_statement()
            if stmt:
                statements.append(stmt)
        return Program(statements)
    
    def _parse_statement(self) -> Statement:
        if self._match(TokenType.LET):
            return self._parse_let()
        elif self._match(TokenType.FN):
            return self._parse_function()
        elif self._match(TokenType.TYPE):
            return self._parse_type()
        elif self._match(TokenType.IF):
            return self._parse_if_statement()
        elif self._match(TokenType.LOOP):
            return self._parse_loop()
        elif self._match(TokenType.WHILE):
            return self._parse_while()
        elif self._match(TokenType.FOR):
            return self._parse_for()
        elif self._match(TokenType.RETURN):
            return self._parse_return()
        else:
            return self._parse_expression_statement()
    
    def _parse_expression(self) -> Expression:
        return self._parse_or()
    
    def _parse_or(self) -> Expression:
        expr = self._parse_and()
        while self._match(TokenType.OR):
            op = self._previous()
            right = self._parse_and()
            expr = BinaryOp(expr, op.value, right)
        return expr
    
    # ... more parsing methods
```

### AST Definitions

```python
# ast.py
from dataclasses import dataclass
from typing import List, Optional, Union
from abc import ABC, abstractmethod

class ASTNode(ABC):
    @abstractmethod
    def accept(self, visitor):
        pass

@dataclass
class Program(ASTNode):
    statements: List['Statement']
    
    def accept(self, visitor):
        return visitor.visit_program(self)

# Statements
class Statement(ASTNode):
    pass

@dataclass
class LetStatement(Statement):
    name: str
    mutable: bool
    type_annotation: Optional['Type']
    initializer: 'Expression'
    
    def accept(self, visitor):
        return visitor.visit_let(self)

@dataclass
class FunctionDef(Statement):
    name: str
    params: List['Parameter']
    return_type: 'Type'
    body: List[Statement]
    generic_params: List[str] = None
    
    def accept(self, visitor):
        return visitor.visit_function(self)

# Expressions
class Expression(ASTNode):
    pass

@dataclass
class BinaryOp(Expression):
    left: Expression
    operator: str
    right: Expression
    
    def accept(self, visitor):
        return visitor.visit_binary_op(self)

@dataclass
class FunctionCall(Expression):
    name: str
    args: List[Expression]
    type_args: List['Type'] = None
    
    def accept(self, visitor):
        return visitor.visit_call(self)

# Pattern matching
@dataclass
class MatchExpression(Expression):
    expr: Expression
    arms: List['MatchArm']
    else_clause: Optional[Expression]
    
    def accept(self, visitor):
        return visitor.visit_match(self)
```

### Interpreter Implementation

```python
# interpreter.py
from ast import *
from value import Value
from environment import Environment
from types import Type, TypeChecker
from patterns import PatternMatcher

class Interpreter:
    def __init__(self):
        self.globals = Environment()
        self.current_env = self.globals
        self.type_checker = TypeChecker()
        self.pattern_matcher = PatternMatcher()
        self._setup_builtins()
    
    def interpret(self, program: Program) -> None:
        # Type check first
        if not self.type_checker.check(program):
            raise TypeError("Type checking failed")
        
        # Then interpret
        for statement in program.statements:
            self.evaluate(statement)
    
    def evaluate(self, node: ASTNode) -> Value:
        return node.accept(self)
    
    def visit_program(self, node: Program) -> None:
        for stmt in node.statements:
            self.evaluate(stmt)
    
    def visit_let(self, node: LetStatement) -> None:
        value = self.evaluate(node.initializer)
        self.current_env.define(node.name, value, node.mutable)
    
    def visit_binary_op(self, node: BinaryOp) -> Value:
        left = self.evaluate(node.left)
        right = self.evaluate(node.right)
        
        if node.operator == '+':
            return self._add(left, right)
        elif node.operator == '-':
            return self._subtract(left, right)
        elif node.operator == '*':
            return self._multiply(left, right)
        elif node.operator == '/':
            return self._divide(left, right)
        elif node.operator == '==':
            return self._equal(left, right)
        # ... more operators
    
    def visit_if(self, node: IfExpression) -> Value:
        condition = self.evaluate(node.condition)
        if self._is_truthy(condition):
            return self.evaluate(node.then_branch)
        elif node.else_branch:
            return self.evaluate(node.else_branch)
        return Value.void()
    
    def visit_match(self, node: MatchExpression) -> Value:
        value = self.evaluate(node.expr)
        
        for arm in node.arms:
            if bindings := self.pattern_matcher.match(arm.pattern, value):
                # Create new environment with bindings
                match_env = Environment(self.current_env)
                for name, bound_value in bindings.items():
                    match_env.define(name, bound_value)
                
                # Evaluate arm body in new environment
                prev_env = self.current_env
                self.current_env = match_env
                try:
                    result = self.evaluate(arm.body)
                finally:
                    self.current_env = prev_env
                return result
        
        if node.else_clause:
            return self.evaluate(node.else_clause)
        
        raise RuntimeError("Non-exhaustive match")
```

### Environment (Scoping)

```python
# environment.py
from typing import Dict, Optional
from value import Value

class Environment:
    def __init__(self, parent: Optional['Environment'] = None):
        self.parent = parent
        self.values: Dict[str, Value] = {}
        self.mutability: Dict[str, bool] = {}
    
    def define(self, name: str, value: Value, mutable: bool = False):
        if name in self.values:
            raise RuntimeError(f"Variable '{name}' already defined")
        self.values[name] = value
        self.mutability[name] = mutable
    
    def get(self, name: str) -> Value:
        if name in self.values:
            return self.values[name]
        if self.parent:
            return self.parent.get(name)
        raise RuntimeError(f"Undefined variable '{name}'")
    
    def set(self, name: str, value: Value):
        if name in self.values:
            if not self.mutability.get(name, False):
                raise RuntimeError(f"Cannot assign to immutable variable '{name}'")
            self.values[name] = value
            return
        if self.parent:
            self.parent.set(name, value)
            return
        raise RuntimeError(f"Undefined variable '{name}'")
```

## Testing Strategy

### Unit Tests

```python
# tests/test_lexer.py
import pytest
from thoxa.lexer import Lexer, TokenType

def test_tokenize_integers():
    lexer = Lexer("42 100 0xFF")
    tokens = list(lexer.tokenize())
    assert tokens[0].type == TokenType.INTEGER
    assert tokens[0].value == 42
    assert tokens[1].value == 100
    assert tokens[2].value == 255

def test_tokenize_strings_with_interpolation():
    lexer = Lexer('"Hello {name}!"')
    tokens = list(lexer.tokenize())
    assert tokens[0].type == TokenType.STRING
    assert tokens[0].value == ["Hello ", {"expr": "name"}, "!"]

def test_camel_case_enforcement():
    lexer = Lexer("myVariable PascalCase")
    tokens = list(lexer.tokenize())
    assert tokens[0].type == TokenType.IDENTIFIER
    assert tokens[0].value == "myVariable"
```

### Integration Tests

```python
# tests/integration/test_programs.py
import pytest
from thoxa import interpret_file

def test_fibonacci():
    result = interpret_file("examples/fibonacci.thx")
    assert result == 55  # fib(10)

def test_error_handling():
    result = interpret_file("examples/error_handling.thx")
    assert result.type.name == "Trial"
    assert result.data["variant"] == "Pass"

def test_pattern_matching():
    result = interpret_file("examples/shapes.thx")
    assert "Circle" in result
```

## Running the Interpreter

### Command Line Interface

```python
# __main__.py
import sys
import argparse
from pathlib import Path
from .lexer import Lexer
from .parser import Parser
from .interpreter import Interpreter
from .repl import REPL

def main():
    parser = argparse.ArgumentParser(description="Thoxa Interpreter")
    parser.add_argument("file", nargs="?", help="Thoxa file to run")
    parser.add_argument("--ast", action="store_true", help="Print AST")
    parser.add_argument("--check", action="store_true", help="Type check only")
    args = parser.parse_args()
    
    if args.file:
        run_file(args.file, args.ast, args.check)
    else:
        REPL().run()

def run_file(filename: str, show_ast: bool, check_only: bool):
    source = Path(filename).read_text()
    
    # Lex
    lexer = Lexer(source)
    tokens = list(lexer.tokenize())
    
    # Parse
    parser = Parser(tokens)
    ast = parser.parse()
    
    if show_ast:
        print(ast)
        return
    
    # Type check
    interpreter = Interpreter()
    if not interpreter.type_checker.check(ast):
        print("Type checking failed")
        sys.exit(1)
    
    if check_only:
        print("Type checking passed")
        return
    
    # Interpret
    interpreter.interpret(ast)

if __name__ == "__main__":
    main()
```

### REPL Implementation

```python
# repl.py
from .lexer import Lexer
from .parser import Parser
from .interpreter import Interpreter

class REPL:
    def __init__(self):
        self.interpreter = Interpreter()
        self.history = []
    
    def run(self):
        print("Thoxa REPL v0.1.0")
        print("Type 'exit' to quit")
        
        while True:
            try:
                # Read
                line = input("thoxa> ")
                if line == "exit":
                    break
                
                # Lex
                lexer = Lexer(line)
                tokens = list(lexer.tokenize())
                
                # Parse
                parser = Parser(tokens)
                ast = parser.parse()
                
                # Eval
                result = self.interpreter.interpret(ast)
                
                # Print
                if result and result.type.name != "Void":
                    print(result)
                
                self.history.append(line)
                
            except KeyboardInterrupt:
                print("\nUse 'exit' to quit")
            except Exception as e:
                print(f"Error: {e}")
```

## Development Workflow

### Setup

```bash
# Clone repository
git clone https://github.com/yourusername/thoxa-python
cd thoxa-python

# Create virtual environment
python3 -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install in development mode
pip install -e .

# Install dev dependencies
pip install pytest mypy black isort
```

### Testing

```bash
# Run all tests
pytest

# Run with coverage
pytest --cov=thoxa

# Run specific test
pytest tests/test_lexer.py::test_tokenize_integers

# Type checking
mypy thoxa
```

### Code Style

```bash
# Format code
black thoxa tests

# Sort imports
isort thoxa tests

# Lint
pylint thoxa
```

## Performance Optimization

### Current Approach
- Direct AST interpretation
- Python's built-in data structures
- Minimal optimization

### Future Optimizations
1. **Bytecode Compilation**
   - Compile AST to bytecode
   - Simple stack-based VM
   - Faster execution

2. **PyPy Support**
   - JIT compilation
   - Significant speedup for long-running programs

3. **Cython Extensions**
   - Compile performance-critical parts
   - Keep Python flexibility

4. **LLVM Backend**
   - Generate LLVM IR
   - Near-native performance
   - More complex implementation

## Debugging Support

### Error Messages

```python
class ThoxaError(Exception):
    def __init__(self, message: str, line: int, column: int):
        self.message = message
        self.line = line
        self.column = column
        super().__init__(self._format_error())
    
    def _format_error(self):
        return f"Error at {self.line}:{self.column}: {self.message}"

class TypeError(ThoxaError):
    pass

class RuntimeError(ThoxaError):
    pass
```

### Stack Traces

```python
class CallStack:
    def __init__(self):
        self.frames = []
    
    def push(self, function_name: str, location: Location):
        self.frames.append(Frame(function_name, location))
    
    def pop(self):
        self.frames.pop()
    
    def format_trace(self) -> str:
        lines = ["Stack trace:"]
        for frame in reversed(self.frames):
            lines.append(f"  at {frame.function_name} ({frame.location})")
        return "\n".join(lines)
```

## Next Steps

1. **Complete Core Features**
   - Finish pattern matching
   - Implement ? operator
   - Add array methods
   - Module system

2. **Tooling**
   - Language server protocol
   - Syntax highlighting
   - Debugger interface
   - Documentation generator

3. **Optimization**
   - Profile and optimize hot paths
   - Consider bytecode compilation
   - Implement caching

4. **Extensions**
   - FFI for Python libraries
   - Standard library
   - Package manager