# Thoxa Interpreter Architecture

## Overview

The Thoxa interpreter is implemented in Python as a tree-walking interpreter, providing rapid prototyping and validation of language design decisions with the flexibility and clarity of Python's dynamic features.

## Interpretation Pipeline

1. **Lexical Analysis** - Tokenization with camelCase identifier enforcement
2. **Parsing** - Recursive descent parser building AST
3. **Type Checking** - Type validation and inference
4. **Interpretation** - Direct AST evaluation with Python-based value representation

## Core Components

### Lexer (`thoxa/lexer.py`)
- Hand-written tokenizer with lookahead capabilities
- Enforces camelCase/PascalCase naming conventions
- Supports string literals with escape sequences and interpolation
- Integer and float literals with type suffixes
- Clean token stream generation using Python generators

### Parser (`thoxa/parser.py`)
- Recursive descent parser with clear, readable implementation
- Generates strongly-typed AST using Python dataclasses
- Supports expressions, statements, and control flow
- Pattern matching with constructor parameter destructuring
- Union type syntax parsing with Python's type annotations

### AST (`thoxa/ast.py`)
- Complete representation using Python dataclasses and type hints
- Clean algebraic data types via `@dataclass` decorators
- Variant types for enum/union representations
- Match expressions with exhaustiveness checking
- Support for generic types using Python's typing module

### Interpreter (`thoxa/interpreter.py`)
- Direct AST evaluation using visitor pattern
- Python-based value representation with dynamic dispatch
- Environment-based variable binding using dictionaries
- Native Python functions for built-in operations
- Clean separation of concerns with modular design

### Type System (`thoxa/types.py`)
- Built-in types defined as Python classes
- Type registry using dictionaries for efficient lookup
- Union types represented as composite type objects
- Generic type parameters using Python's typing system
- Pattern matching with runtime type checking

### Value System (`thoxa/value.py`)
- Base Value class with type and data attributes
- Python's garbage collection (automatic memory management)
- Union types as tagged values
- Pattern matching using Python's structural pattern matching (3.10+)

## Key Design Decisions

### Memory Management
Leverages Python's garbage collector:
- Automatic memory management with reference counting + cycle detection
- No manual memory management needed
- Values represented as Python objects
- Efficient and safe by default

### Value Representation
```python
@dataclass
class Value:
    type: Type
    data: Any  # Actual Python value or wrapped object
```

- Primitives: int, float, bool, str
- Compound types: lists, custom objects
- Functions as first-class values (Python callables)

### Union Types and Error Handling
- Union types: Composite type objects containing variant types
- Types are standalone, unions compose them
- Trial<T,E> and May<T> as union types:
  - Trial for error handling with Pass/Fail variants
  - May for optional values with Has/Not variants
- Pattern matching using Python's match statement (3.10+) or if/elif chains
- The ? operator for Trial propagation (via exception handling)

### Control Flow
- Structured control flow (if, while, loop)
- Break/continue with proper control flow management
- For-in loops for collection iteration
- Exception-based control flow for ? operator

## Implementation Structure

```
thoxa/
├── __main__.py       # Entry point
├── lexer.py          # Tokenization
├── parser.py         # AST generation
├── ast.py            # AST definitions
├── types.py          # Type system
├── value.py          # Value representation
├── interpreter.py    # Evaluation engine
├── environment.py    # Variable scoping
├── builtins.py       # Built-in functions
└── utils/
    ├── errors.py     # Error handling
    └── visitor.py    # AST visitor base
```

## Running the Interpreter

### Direct Execution
```bash
python -m thoxa file.thx          # Run a Thoxa file
python -m thoxa                   # Start REPL
python -m thoxa --ast file.thx    # Show AST
python -m thoxa --check file.thx  # Type check only
```

### Installation
```bash
pip install -e .                   # Install in development mode
thoxa file.thx                     # Run after installation
```

### Dependencies
- Python 3.10+ (for pattern matching)
- No external runtime dependencies for core interpreter
- Optional: pytest for testing, mypy for type checking

## Testing Strategy

### Unit Tests
```python
# tests/test_lexer.py
def test_tokenize_identifiers():
    tokens = list(tokenize("let myVar = 42"))
    assert tokens[1].type == TokenType.IDENTIFIER
    assert tokens[1].value == "myVar"

# tests/test_parser.py
def test_parse_let_statement():
    ast = parse("let x = 10")
    assert isinstance(ast.statements[0], LetStatement)
```

### Integration Tests
- End-to-end interpretation tests
- Language feature validation
- Error handling verification
- Type system consistency checks

## Python-Specific Advantages

### Development Speed
- Rapid prototyping with dynamic typing
- Interactive REPL for testing
- Clear, readable code structure
- Excellent debugging tools

### Library Ecosystem
- Rich standard library for file I/O, networking, etc.
- Easy integration with external tools
- Simple packaging and distribution
- Cross-platform compatibility out of the box

### Code Clarity
```python
# Clear visitor pattern for AST traversal
class Interpreter(ASTVisitor):
    def visit_BinaryOp(self, node: BinaryOp) -> Value:
        left = self.visit(node.left)
        right = self.visit(node.right)
        
        if node.op == '+':
            return self.add_values(left, right)
        elif node.op == '-':
            return self.subtract_values(left, right)
        # ...
```

## Performance Considerations

While Python is slower than compiled languages, it's ideal for:
- Language design validation
- Prototype implementation
- Educational purposes
- Small to medium programs

Future optimization paths:
- PyPy for JIT compilation
- Cython for performance-critical sections
- Bytecode compilation and caching
- Eventually compile to native code or LLVM

## Future Enhancements

### Near Term
- Pattern matching for Trial<T,E>
- The ? operator for error propagation
- User-defined enums
- Generic type parameters
- Module system implementation

### Long Term
- Bytecode compiler and VM
- JIT compilation via PyPy
- FFI for C extensions
- Debugger integration
- Language server protocol support

## Related Documentation

- [Language Specification](LANGUAGE.md) - Syntax and semantics
- [Type System](TYPE_SYSTEM.md) - Type system design
- [Implementation Guide](IMPLEMENTATION.md) - Development details