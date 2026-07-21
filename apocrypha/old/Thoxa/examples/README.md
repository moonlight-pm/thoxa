# Thoxa Examples

This directory contains example Thoxa programs organized by category:

## Directory Structure

### `/demos`
Basic demonstrations of language features:
- `hello.thx`, `hello_world.thx` - Simple output examples
- `calculator.thx` - Basic arithmetic calculator
- `counter.thx` - Counter implementation
- `add.thx`, `arithmetic.thx` - Math operations
- `functions.thx` - Function definitions and calls
- `if_else.thx` - Conditional statements
- `loop_control.thx`, `while_loop.thx`, `simple_break.thx` - Loop constructs
- `early_return.thx` - Return statement usage
- `print_demo.thx` - Print functionality

### `/tests`
Test cases for specific features:
- `test_*.thx` - Various test files
- `match_*.thx` - Pattern matching tests
- `*_test.thx` - Feature-specific tests
- `minimal.thx` - Minimal program test
- `auto_refcount.thx` - Reference counting test
- `string_concat.thx` - String operations test
- `test_boxing.thx` - Boxing/unboxing for value types
- `test_unit.thx` - Unit definition and field access

### `/future`
Complete feature examples showing Thoxa's goals at 1.0:
- `cli_parser.thx` - Command-line argument parsing library demonstrating:
  - Advanced pattern matching
  - Error handling with Trial<T,E>
  - Generic units and functions
  - Module system with exports
- `image_processor.thx` - Image manipulation tool showcasing:
  - FFI integration with C libraries
  - Efficient memory management
  - Parallel processing capabilities
  - File I/O and binary data handling
- `web_server.thx` - HTTP server implementation featuring:
  - Async/await concurrency model
  - Network I/O
  - Protocol implementation
  - Real-world application structure

These future examples represent the complete feature set planned for Thoxa 1.0, demonstrating the language's capability to handle real-world systems programming tasks with safety and performance.

## Running Examples

To compile and run an example:

```bash
# Compile to QBE
./target/debug/thoxa -c examples/demos/hello.thx -o tmp/hello

# Run the compiled program
./tmp/hello
```

For debugging with QBE output visible:
```bash
SHOW_QBE=1 ./target/debug/thoxa -c examples/demos/hello.thx -o tmp/hello
```