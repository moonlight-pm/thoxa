# Thoxa Programming Language Project

## Overview
Thoxa is an experimental programming language with its own interpreter written in Python and an interactive web-based notebook for testing and learning the language.

## Project Structure

### Root Level - Python Interpreter
- **Language**: Python 3.13+
- **Dependencies**: Lark (parsing), Rich (REPL), Click (CLI)
- **Package**: Installable via `pip` as `thoxa`
- **Entry point**: `python -m thoxa` or `thoxa` CLI command

### Core Components

#### `/thoxa` - Interpreter Implementation
- `parser.py` - Lark-based parser for Thoxa syntax
- `interpreter.py` - AST interpreter and runtime
- `ast_nodes.py` - Abstract syntax tree node definitions
- `type_checker.py` - Static type checking system
- `repl.py` - Interactive Read-Eval-Print Loop
- `cli.py` - Command-line interface

#### `/notebook` - Interactive Web Application
- **Framework**: Waku (React SSR framework) with Bun runtime
- **UI**: React with TypeScript, TailwindCSS for styling
- **Editor**: CodeMirror with custom Thoxa syntax highlighting
- **Database**: SQLite for storing code samples (migrated from YAML)
- **Routes**:
  - `/` - Landing page
  - `/learn` - Interactive learning environment
  - `/playground` - Code experimentation sandbox

#### `/docs` - Documentation
- `LANGUAGE.md` - Complete language specification
- `TYPE_SYSTEM.md` - Type system details
- `ARCHITECTURE.md` - System architecture
- `IMPLEMENTATION.md` - Implementation notes

#### `/examples` - Code Examples
- Test files (`*.thx`) demonstrating language features
- Pattern matching, loops, functions, arrays, type system

## Language Features

### Core Principles
1. **Everything is an expression** - Most constructs return values
2. **Static typing with inference** - Compile-time type checking
3. **Named parameters by default** - Self-documenting function calls
4. **Automatic memory management** - No manual allocation
5. **Pattern matching** - Structural control flow

### Syntax Highlights
- **Variables**: `let x = 42`, `mut sum = 0`
- **Functions**: `fn name(param: Type) -> ReturnType { ... }`
- **Arrays**: `[1, 2, 3]` with `.count` property
- **Control Flow**: `if/else`, `match`, `for in`, `while`, `loop`
- **Types**: Primitives (Int, Float, Bool, String), custom types, interfaces
- **Special Keywords**: `Has`, `Not`, `Pass`, `Fail` for testing/assertions

### Type System
- Basic types: Int8-64, UInt8-64, Float32/64, Bool, String, Char
- Composite types: Arrays, tuples, structs, unions
- Type inference with explicit annotations where needed
- Interface-based polymorphism

## Development Workflow

### Running the Interpreter
```bash
# Install
uv sync  # or pip install -e .

# Run REPL
python -m thoxa

# Execute file
python -m thoxa examples/tests/for_in_demo.thx
```

### Running the Notebook
```bash
cd notebook
bun install
bun run dev  # Start development server
```

### Testing
- Test files in `/examples/tests/*.thx`
- Run via `./run_tests.sh` script
- Python tests with pytest

## Recent Updates
- Migration from YAML to SQLite for notebook content storage
- TypeScript and Biome linting fixes
- Enhanced language features and notebook improvements

## Build Artifacts
- Python wheel: `dist/thoxa-0.1.0-py3-none-any.whl`
- Source distribution: `dist/thoxa-0.1.0.tar.gz`