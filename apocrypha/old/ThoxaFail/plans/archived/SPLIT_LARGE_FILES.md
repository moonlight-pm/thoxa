# Plan: Split Large Files

## Overview
Split four large files into smaller, more manageable modules:
- src/codegen/expr.rs (70KB / 1378 lines)
- src/type_checker.rs (58KB / 1349 lines)  
- src/parser.rs (55KB / 1458 lines)
- src/codegen/jit.rs (30KB / 757 lines)

Target: ~300-400 lines per file (max 20KB)

## 1. Split src/codegen/expr.rs

Create subdirectory `src/codegen/expr/` with:

### Modules:
- `basic_values.rs` (~350 lines)
  - Number, Binary operations, Identifier, IntType
  - Sequence, Block, LetBinding, TypeDef
  - Assignment, Import
  
- `field_reference.rs` (~250 lines)
  - FieldAccess operations
  - Reference operations
  - Module field resolution
  
- `function_calls.rs` (~200 lines)
  - Direct function calls
  - Variadic function handling
  - Function resolution
  
- `system_io.rs` (~220 lines)
  - Void, Print, PrintStr
  - StringLiteral handling
  - Syscall, Strlen, Cstr, IntToString
  
- `control_flow.rs` (~200 lines)
  - If, Loop, Break, Continue
  - Control flow block management
  
- `templates_arrays.rs` (~290 lines)
  - TemplateLiteral
  - MethodCall resolution
  - ArrayLiteral, ArrayIndex
  
- `type_operations.rs` (~60 lines)
  - Conversion, CanConvert, TypeGuard

- `mod.rs` (~50 lines)
  - Main translate_expr dispatcher

## 2. Split src/type_checker.rs

Create subdirectory `src/type_checker/` with:

### Modules:
- `expressions.rs` (~400 lines)
  - Basic expression type checking
  - Binary operations, literals
  
- `declarations.rs` (~300 lines)
  - Function declarations
  - Type definitions
  - Let bindings
  
- `control_flow.rs` (~200 lines)
  - If, Loop, Block type checking
  
- `modules.rs` (~250 lines)
  - Module type checking
  - Import resolution
  
- `inference.rs` (~200 lines)
  - Type inference logic
  - Unification
  
- `mod.rs` (~50 lines)
  - Main type checker interface

## 3. Split src/parser.rs

Create subdirectory `src/parser/` with:

### Modules:
- `lexer_integration.rs` (~100 lines)
  - Token stream management
  - Peek/advance operations
  
- `expressions.rs` (~400 lines)
  - Expression parsing
  - Operator precedence
  
- `statements.rs` (~300 lines)
  - Statement parsing
  - Declarations
  
- `types.rs` (~200 lines)
  - Type expression parsing
  - Type annotations
  
- `modules.rs` (~200 lines)
  - Module declarations
  - Import statements
  
- `helpers.rs` (~150 lines)
  - Common parsing utilities
  - Error handling
  
- `mod.rs` (~100 lines)
  - Main parser interface

## 4. Split src/codegen/jit.rs (optional)

Since it's borderline at 757 lines, we could split into:

### Modules:
- `jit_context.rs` (~300 lines)
  - JITCodegen struct
  - Context management
  
- `function_compilation.rs` (~250 lines)
  - Function compilation logic
  - Signature handling
  
- `execution.rs` (~200 lines)
  - Code execution
  - Result handling

## Implementation Order

1. **expr.rs** - Most critical, largest file
2. **type_checker.rs** - Second largest
3. **parser.rs** - Third largest
4. **jit.rs** - Optional, already manageable

## Migration Strategy

1. Create new directory structure
2. Move code sections to new modules
3. Update imports and module declarations
4. Test each step to ensure functionality
5. Remove old monolithic files
6. Update all references in other files