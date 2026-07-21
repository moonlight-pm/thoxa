# Code Generation Module Refactoring Plan

## Overview
Refactor the code generation modules to improve organization and maintainability by:
1. Creating a `src/codegen` directory
2. Moving all codegen files into this directory with cleaner names
3. Breaking up the large `codegen.rs` (2745 lines, 132KB) into smaller, focused modules

## Current State
```
src/codegen.rs                    (2745 lines, 132KB) - Main code generation logic
src/codegen_aot.rs               (139 lines, 4.9KB)  - AOT compilation
src/codegen_jit.rs               (770 lines, 30KB)   - JIT compilation
src/codegen_func.rs              (46 lines, 1.6KB)   - Function helpers
src/codegen_symbols.rs           (91 lines, 3.5KB)   - Symbol management
src/codegen_typed.rs             (98 lines, 3.5KB)   - Typed expression helpers
src/codegen_variable_wrapper.rs  (58 lines, 2.2KB)   - Variable environment wrapper
```

## Proposed Structure
```
src/codegen/
├── mod.rs           - Module exports and core utilities (value_type, extract_value)
├── aot.rs           - AOT compilation (from codegen_aot.rs)
├── jit.rs           - JIT compilation (from codegen_jit.rs)
├── func.rs          - Function helpers (from codegen_func.rs)
├── symbols.rs       - Symbol management (from codegen_symbols.rs)
├── typed.rs         - Typed expression helpers (from codegen_typed.rs)
├── variable.rs      - Variable environment (from codegen_variable_wrapper.rs)
├── tagged_values.rs - Tagged pointer operations (tag_int_value, etc.)
├── expr.rs          - Untyped expression translation (translate_expr)
├── typed_expr/
│   ├── mod.rs       - Main typed expression translation dispatch
│   ├── literals.rs  - Number, StringLiteral, Identifier
│   ├── operators.rs - Binary, Assignment
│   ├── control.rs   - If, Loop, Break, Continue, Block, Sequence
│   ├── functions.rs - MethodCall, function calls
│   ├── arrays.rs    - ArrayLiteral, ArrayIndex
│   ├── types.rs     - TypeGuard, Conversion, type operations
│   ├── builtins.rs  - Print, PrintStr, Syscall, Strlen, Cstr, IntToString
│   └── structs.rs   - FieldAccess, LetBinding (struct instantiation)
└── helpers.rs       - Utility functions (create_any_value, etc.)
```

## Breakdown of codegen.rs

### 1. `tagged_values.rs` (~150 lines)
- `value_type()` - Get Cranelift type for values
- `tag_int_value()` - Tag an integer value
- `untag_int_value()` - Untag an integer value  
- `is_tagged_int()` - Check if value is tagged integer
- `tag_bool_value()` - Tag a boolean value
- `is_tagged_ptr()` - Check if value is tagged pointer
- `extract_value()` - Extract actual value from tagged pointer

### 2. `helpers.rs` (~150 lines)
- `thoxa_type_to_cranelift()` - Convert Thoxa types to Cranelift types
- `create_any_value()` - Create Any type value
- `create_any_value_from_ptr()` - Create Any type from pointer
- Module registry thread-local storage
- Loop control thread-local storage

### 3. `typed_expr/mod.rs` (~100 lines)
- `translate_typed_expr()` - Main dispatch function
- Import necessary types and dispatch to submodules

### 4. `typed_expr/literals.rs` (~150 lines)
- `TypedExpr::Number` - Integer literals
- `TypedExpr::StringLiteral` - String literals  
- `TypedExpr::Identifier` - Variable references

### 5. `typed_expr/operators.rs` (~200 lines)
- `TypedExpr::Binary` - Binary operations
- `TypedExpr::Assignment` - Variable assignment

### 6. `typed_expr/control.rs` (~400 lines)
- `TypedExpr::If` - If/else expressions
- `TypedExpr::Loop` - Loop expressions
- `TypedExpr::Break` - Break statements
- `TypedExpr::Continue` - Continue statements
- `TypedExpr::Block` - Block expressions
- `TypedExpr::Sequence` - Sequence expressions

### 7. `typed_expr/functions.rs` (~300 lines)
- `TypedExpr::MethodCall` - Method calls and function calls
- Function lookup and argument handling
- Variadic argument collection

### 8. `typed_expr/arrays.rs` (~300 lines)
- `TypedExpr::ArrayLiteral` - Array creation
- `TypedExpr::ArrayIndex` - Array indexing with bounds checking

### 9. `typed_expr/types.rs` (~200 lines)
- `TypedExpr::TypeGuard` - Type guard expressions (is operator)
- `TypedExpr::Conversion` - Type conversions (as operator)
- `TypedExpr::Untyped` - Fallback to untyped expression

### 10. `typed_expr/builtins.rs` (~300 lines)
- `TypedExpr::Print` - Print builtin
- `TypedExpr::PrintStr` - Print string builtin
- `TypedExpr::Syscall` - System call builtin
- `TypedExpr::Strlen` - String length builtin
- `TypedExpr::Cstr` - C string conversion
- `TypedExpr::IntToString` - Integer to string conversion

### 11. `typed_expr/structs.rs` (~250 lines)
- `TypedExpr::FieldAccess` - Field access on structs/modules
- `TypedExpr::LetBinding` - Let bindings with struct instantiation
- `TypedExpr::Reference` - Reference expressions

### 12. `expr.rs` (~900 lines)
- `translate_expr()` - Translate untyped expressions
- All untyped expression variants

## Benefits
1. **Better Organization**: Related functionality grouped together
2. **Smaller Files**: No file exceeds 1000 lines
3. **Easier Navigation**: Clear separation of concerns
4. **Improved Maintainability**: Easier to find and modify specific functionality
5. **Cleaner Imports**: More specific imports from submodules

## Migration Steps
1. Create `src/codegen` directory
2. Move and rename existing files
3. Create subdirectories for typed expressions
4. Split `codegen.rs` into the proposed modules
5. Update all imports throughout the codebase
6. Update `lib.rs` to export from new structure
7. Test to ensure everything still works

## Notes
- The line counts are estimates and may vary slightly
- Some functions may need to be made public for cross-module access
- Thread-local storage for loop control may need special handling