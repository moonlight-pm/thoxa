# Thoxa Examples

This directory contains example programs demonstrating various features of the Thoxa programming language.

## Main Examples

These numbered examples demonstrate key language features in order of complexity:

- `01_HelloWorld.thx` - Basic "Hello, World!" program
- `02_TypesAndConversions.thx` - Type system and conversions
- `03_ArraysAndVariadics.thx` - Arrays and variadic functions
- `04_ControlFlow.thx` - If/else statements and loops
- `05_ModulesAndImports.thx` - Module system and imports
- `06_StringsAndTemplates.thx` - String manipulation and template literals
- `07_Showcase.thx` - Complete showcase of language features
- `08_PolymorphicAny.thx` - Polymorphic programming with Any type and type guards

## Support Files

- `Math.thx` - Mathematical functions module (used by other examples)

## Test Cases

The `tests/` subdirectory contains smaller test files used for compiler development and validation:

- Basic functionality tests
- Type system tests  
- Runtime type information tests
- Type guard tests
- Fat pointer implementation tests

## Running Examples

To run any example:

```bash
thoxa examples/01_HelloWorld.thx
```

Or compile to a binary:

```bash
thoxa -c examples/01_HelloWorld.thx -o hello
./hello
```