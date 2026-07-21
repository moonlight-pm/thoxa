# Thoxa

A modern systems programming language with explicit type conversions, compiles to native code, and prioritizes simplicity and safety.

## Quick Start

```bash
# Install
cargo build --release

# Run a program
thoxa examples/HelloWorld.thx

# Compile to binary
thoxa -c examples/HelloWorld.thx -o hello

# Evaluate expression
thoxa -e "1 + 2 * 3"
```

## Key Features

- **Everything is an expression** - No statements, everything returns a value
- **Explicit type conversions** - No implicit coercion, use `as` for conversions
- **Memory safe** - Compile-time reference validation
- **Fast compilation** - JIT for development, AOT for production
- **Arrays and variadic functions** - Built-in array support with variadic parameters

## Example

```thoxa
# Define a type with functions
type Math {
    export fn factorial(n: Int64) -> Int64 {
        if n <= 1 { 1 } else { n * factorial(n - 1) }
    }
}

# Use the type
let m: Math;
@print(m.factorial(5))  # Prints: 120

# String interpolation with automatic conversion
let result: Int64 = m.factorial(5);
@printStr("The factorial of 5 is ${result}")

# Explicit type conversions
let n: Int64 = 42;
let s: String = n as String;  # Explicit conversion

# Arrays and variadic functions
let numbers = [1, 2, 3, 4, 5];
@print(numbers[0]);       # Array indexing
@print(numbers.length()); # Array methods

# Variadic function example
type IO {
    export fn println(...args: Int64) -> Void {
        # Print all arguments
    }
}
```

## Documentation

- [Language Reference](docs/LANGUAGE.md) - Complete language specification
- [Architecture](docs/ARCHITECTURE.md) - Compiler internals
- [Module System](docs/MODULES.md) - Import system details
- [Type System](docs/TYPES.md) - Type design and implementation
- [Changelog](CHANGELOG.md) - Version history and changes

## Examples

See the `examples/` directory for demonstrations:
- `HelloWorld.thx` - Getting started
- `Working.thx` - Core language features
- `ArraysAndIndexing.thx` - Array operations
- `TypeConversion.thx` - Type conversion examples

## Status

Thoxa is under active development. Core features implemented:
- Type system with explicit conversions
- Arrays with built-in methods
- Variadic functions
- Template literals
- JIT and AOT compilation

## License

MIT