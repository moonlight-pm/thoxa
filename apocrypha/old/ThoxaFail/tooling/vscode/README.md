# Thoxa Language Support for VS Code

This extension provides syntax highlighting for the Thoxa programming language (`.thx` files).

## Features

- Syntax highlighting for:
  - Keywords (`let`, `mut`, `fn`, `module`, `export`, `if`, `else`, `loop`, `break`, `continue`, etc.)
  - Types (PascalCase identifiers and built-in types like `I64`)
  - Functions and function calls
  - Built-in functions (`@print`, `@printStr`, `@syscall`, `@strlen`, `@intToString`)
  - Numbers (decimal, hexadecimal, binary)
  - String literals with escape sequences
  - Comments (lines starting with `#`)
  - Operators (arithmetic, comparison, assignment)
  - Module field access (`.` operator)
  - Reference operator (`&`)

- Code snippets for common patterns:
  - Variable declarations (`let`, `mut`)
  - Control flow (`if`, `ifelse`, `loop`)
  - Functions and modules
  - Print statements
  - Counter loops

## Installation

### From Source

1. Clone the Thoxa repository
2. Navigate to `tooling/vscode`
3. Run `npm install -g vsce` (if not already installed)
4. Run `vsce package` to create a `.vsix` file
5. In VS Code, run "Extensions: Install from VSIX..." and select the generated file

### Development

1. Open the `tooling/vscode` folder in VS Code
2. Press F5 to launch a new VS Code window with the extension loaded
3. Open any `.thx` file to test syntax highlighting

## Quick Test

Create a file `test.thx`:

```thoxa
# Thoxa example with syntax highlighting
module Point {
    export let x: I64 = 10;
    export let y: I64 = 20;
    
    export fn distance() -> I64 {
        x * x + y * y
    }
}

# Variables and control flow
let p: Point;
mut counter: I64 = 0;

loop {
    counter = counter + 1;
    @print(counter);
    
    if counter >= 5 {
        break
    }
};

# String output
@printStr("Distance: ");
@print(p.distance());
```

## Future Features

- Language Server Protocol (LSP) support
- Code completion
- Go to definition
- Error diagnostics
- Code formatting
- Hover information