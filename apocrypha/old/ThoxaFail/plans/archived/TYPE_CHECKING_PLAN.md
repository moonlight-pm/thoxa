# Type Checking Implementation Plan

## Overview

This document outlines the plan for implementing type checking in Thoxa. The type checker will run after parsing but before code generation, ensuring type safety at compile time.

## Current Status (July 2025)

✅ **Phase 1 Complete**: Basic type checking infrastructure is now implemented and integrated into the compiler pipeline.

### Completed Features:
- Type enum representation with all basic types
- TypedExpr wrapper for AST nodes with type information
- Type environment (TypeEnv) for tracking variable, function, and module types
- Type checker module that validates expressions and produces typed AST
- Integration into compilation pipeline (both JIT and AOT)
- Type error reporting with clear error messages
- Support for new type names (Int64, Uint64, String, etc.)
- Built-in types initialized in type environment
- Type resolution for identifiers, let bindings, and module members

### Working Examples:
```thoxa
# Type annotations work correctly
let x: Int64 = 42;
let s: String = "Hello";

# Type errors are caught
let bad: UnknownType = 42;  # Error: Unknown type: UnknownType

# Module member types are tracked
module Point {
    export let x: Int64 = 10;
}
let p: Point;
p.x  # Type: Int64
```

## Phase 1: Basic Infrastructure

### 1.1 Type Representation

We need to add a type representation that can handle:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    // Primitive types
    Void,
    String,
    Int { signed: bool, bits: u8 },  // Int8..Int64, Uint8..Uint64
    
    // Type of types
    Type,
    
    // Module types
    Module {
        name: String,
        members: HashMap<String, MemberType>,
    },
    
    // Function types (for future)
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    
    // Unknown (for error recovery)
    Unknown,
}
```

### 1.2 AST Changes

Each expression node needs an optional type field:

```rust
pub struct TypedExpr {
    pub expr: Expr,
    pub ty: Option<Type>,
}
```

### 1.3 Type Environment

Track types of variables and functions:

```rust
pub struct TypeEnv {
    // Variable types
    variables: HashMap<String, Type>,
    
    // Function signatures
    functions: HashMap<String, FunctionType>,
    
    // Module types
    modules: HashMap<String, ModuleType>,
    
    // Parent scope (for nested scopes)
    parent: Option<Box<TypeEnv>>,
}
```

## Phase 2: Type Checking Rules

### 2.1 Literals

```thoxa
42         # Type: Int64 (default integer type)
"hello"    # Type: String
```

### 2.2 Variables

```thoxa
let x: Int32 = 42;    # Check: 42 can coerce to Int32
let y: String = x;    # Error: Int32 cannot convert to String
```

### 2.3 Binary Operations

```thoxa
# Both operands must have same type
let a: Int32 = 10;
let b: Int32 = 20;
let c: Int32 = a + b;  # OK

let d: Int64 = a + b;  # Error: Int32 + Int32 produces Int32, not Int64
```

### 2.4 Function Calls

```thoxa
module Math {
    export fn add(a: Int32, b: Int32) -> Int32 { a + b };
}

let m: Math;
let x: Int32 = m.add(1, 2);     # OK
let y: Int64 = m.add(1, 2);     # Error: return type mismatch
m.add(1, "hello");              # Error: argument type mismatch
```

### 2.5 Module Field Access

```thoxa
module Point {
    export let x: Int32 = 0;
    export let y: Int32 = 0;
}

let p: Point;
let x: Int32 = p.x;    # OK
let y: String = p.y;   # Error: type mismatch
```

## Phase 3: Implementation Steps

### Step 1: Add Type to Symbol Table

Update the symbol table to store type information:

```rust
pub enum SymbolInfo {
    Variable { 
        ty: Type,
        value: Option<Expr>,
        category: ValueCategory 
    },
    Function {
        params: Vec<(String, Type)>,
        return_type: Type,
        body: Option<Expr>,
    },
    Module {
        ty: ModuleType,
    },
}
```

### Step 2: Type Inference Pass

Create a type inference pass that:
1. Walks the AST
2. Infers types for expressions
3. Checks type consistency
4. Reports type errors

```rust
pub struct TypeChecker {
    env: TypeEnv,
    errors: Vec<TypeError>,
}

impl TypeChecker {
    pub fn check_expr(&mut self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Number(_) => Ok(Type::Int { signed: true, bits: 64 }),
            Expr::StringLiteral(_) => Ok(Type::String),
            Expr::Identifier(name) => self.env.lookup(name),
            Expr::Binary { left, op, right } => {
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;
                self.check_binary_op(left_ty, *op, right_ty)
            }
            // ... other cases
        }
    }
}
```

### Step 3: Error Reporting

Create clear error messages:

```
Type Error at line 10:
    Expected type 'String', but found 'Int32'
    
    10 | let message: String = 42;
       |                       ^^ Int32 cannot be assigned to String
```

## Phase 4: Integration

### 4.1 Update Parser

The parser needs to handle the new type names:
- Recognize `Int64`, `Uint32`, etc. as type identifiers
- Support importing types from std/Types

### 4.2 Update Code Generation

Code generation needs to use type information:
- Generate correct cranelift types based on Thoxa types
- Handle string operations differently from integers
- Ensure correct calling conventions

### 4.3 Standard Library Integration

```thoxa
# Types should be importable
import { Int32, String } from "../std/Types";

# And usable as regular types
let x: Int32 = 42;
let msg: String = "Hello";
```

## Testing Strategy

### Unit Tests
- Type inference for each expression type
- Type checking for valid programs
- Error detection for invalid programs

### Integration Tests
- Full programs with complex type interactions
- Module system with typed exports
- Function calls with type checking

### Example Test Cases

```thoxa
# Valid: should pass type checking
module TestValid {
    export fn identity(x: Int32) -> Int32 { x };
    export fn add(a: Int64, b: Int64) -> Int64 { a + b };
}

# Invalid: should produce type errors
module TestInvalid {
    export fn bad1() -> String { 42 };  # Error: returning Int64, expected String
    export fn bad2(x: Int32) -> Int32 { x + "hello" };  # Error: can't add Int32 and String
}
```

## Migration Plan

1. **Keep backward compatibility** initially (accept both I64 and Int64)
2. **Add warnings** for deprecated type names
3. **Update all examples** to use new type names
4. **Remove legacy support** in future version

## Open Questions

1. **Type coercion**: Should we allow automatic widening (Int32 → Int64)?
2. **Type aliases**: How to handle `type MyInt = Int32;`?
3. **Const evaluation**: When to evaluate type expressions?
4. **Error recovery**: How to continue checking after type errors?