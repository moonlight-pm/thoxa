# Code Generation Refactoring Plan

## Overview

This document outlines the refactoring of Thoxa's code generation to use `TypedExpr` instead of `Expr`, enabling runtime type information and laying groundwork for future compiler enhancements.

## Current State

The compilation pipeline currently:
1. Parser produces `Expr` AST
2. Type checker validates and produces `TypedExpr` 
3. Code generation receives `TypedExpr.expr` but **discards type information**
4. All integers compile to `i64`, no runtime type info, limited optimization

## Proposed Architecture

### Core Change
```rust
// Current signature
pub fn translate_expr<M: cranelift_module::Module>(
    expr: &Expr,  // No type info!
    ...
) -> Value

// New signature  
pub fn translate_expr<M: cranelift_module::Module>(
    typed_expr: &TypedExpr,  // Full type information
    ...
) -> Value
```

### Uniform Fat Pointer Design

**Key Decision**: All values in Thoxa are 128-bit fat pointers:
```
[64 bits: type tag][64 bits: value or pointer]
```

Benefits:
- **Simplicity**: No special cases for Any types
- **Uniformity**: All values have the same representation
- **Type Safety**: Runtime type info always available
- **Performance**: Fast type checks with inline tags for built-ins
- **Debugging**: Can always inspect a value's type
- **Future-proof**: Enables reflection, better error messages

Tradeoffs:
- 2x memory usage for primitive values
- All parameters/returns are 16 bytes
- May need optimization pass later for performance-critical code

### Minimal Implementation (Phase 1)

Focus on uniform fat pointer representation:

1. **Thread TypedExpr through codegen**
   - Update all `translate_expr` calls
   - Pass type info to all codegen functions
   - Maintain backward compatibility where possible

2. **Uniform fat pointer generation**
   - ALL values become 128-bit fat pointers
   - Every value: `[64-bit type tag][64-bit value/pointer]`
   - No special handling for Any - it just accepts any fat pointer

3. **Type tag generation**
   ```rust
   fn generate_type_tag(ty: &Type, builder: &mut FunctionBuilder) -> Value {
       match ty {
           Type::Int { signed: true, bits: 64 } => 
               builder.ins().iconst(types::I64, BuiltinTypeId::Int64 as i64),
           Type::String => 
               builder.ins().iconst(types::I64, BuiltinTypeId::String as i64),
           Type::Struct { name, .. } => 
               // Generate pointer to TypeInfo for custom types
               generate_custom_type_tag(name, builder),
           // ...
       }
   }
   ```

### Design Considerations for Future

While implementing the minimal changes, structure code to support:

#### 1. Proper Integer Sizes (Near-term)
```rust
// Structure code so this is easy to add later:
match ty {
    Type::Int { signed, bits } => {
        let cranelift_type = match bits {
            8 => types::I8,
            16 => types::I16,
            32 => types::I32,
            64 => types::I64,
            _ => unreachable!(),
        };
        // Generate appropriate sized operations
    }
}
```

#### 2. Method Resolution (Near-term)
```rust
// Design pattern for method lookups:
struct CodegenContext {
    // ... existing fields ...
    type_info: HashMap<LocalVarId, Type>,  // Track variable types
}

// When generating method calls:
let receiver_type = context.type_info.get(receiver_var);
let method = receiver_type.find_method(method_name);
```

#### 3. Memory Layout (Medium-term)
```rust
// Structure for future memory layout calculations:
trait TypeLayout {
    fn size(&self) -> usize;
    fn alignment(&self) -> usize;
    fn field_offset(&self, field: &str) -> Option<usize>;
}

// Implement for Type enum
impl TypeLayout for Type {
    // Start simple, extend later
}
```

#### 4. Optimization Opportunities (Future)
```rust
// Mark optimization points in code:
// TODO(opt): With type info, could inline small types
// TODO(opt): Could eliminate bounds check if type proves safety
// TODO(opt): Dead code elimination for unused methods
```

#### 5. Generic Preparation (Future)
```rust
// Structure to support eventual monomorphization:
enum Type {
    // ... existing variants ...
    Generic { param: String },  // Future
    Instantiated { base: Box<Type>, args: Vec<Type> }, // Future
}
```

## Implementation Strategy

### Phase 1: Minimal RTI Support (Current Priority)
1. Add `TypedExpr` parameter to `translate_expr`
2. Update all call sites to pass typed expressions
3. Implement Any type fat pointer generation
4. Add type tag generation for built-in types
5. Test with simple Any examples

### Phase 2: Foundation Improvements
1. Generate correct integer sizes
2. Improve memory layout calculations  
3. Add basic method resolution with types
4. Implement type guards (`if x is Type`)

### Phase 3: Advanced Features
1. Generic type support
2. Advanced optimizations
3. Debug information generation
4. Full virtual dispatch

## Code Organization

```
src/
  codegen.rs          - Main translation logic (refactored)
  codegen_types.rs    - Type-specific code generation (new)
  codegen_runtime.rs  - Runtime type info generation (new)
  type_layout.rs      - Memory layout calculations (future)
```

## Testing Strategy

1. **Preserve existing behavior**: All current tests must pass
2. **Add RTI tests**: New tests for Any type assignments
3. **Performance tests**: Ensure no regression
4. **Type-specific tests**: As we add proper integer sizes

## Migration Path

To minimize disruption:
1. Create new `translate_typed_expr` alongside existing
2. Gradually migrate each expression type
3. Remove old `translate_expr` when complete
4. Update documentation

## Risks and Mitigations

**Risk**: Large refactoring could introduce bugs
- **Mitigation**: Incremental migration, extensive testing

**Risk**: Performance regression  
- **Mitigation**: Benchmark before/after, profile hot paths

**Risk**: Breaking changes for users
- **Mitigation**: No external API changes in Phase 1

## Success Criteria

Phase 1 is complete when:
- [ ] Any type assignments generate fat pointers
- [ ] Type tags are correctly generated
- [ ] All existing tests pass
- [ ] Simple RTI example works (`let x: Any = 42`)

## Future Benefits

This refactoring enables:
- Complete runtime type information
- Proper type-specific code generation  
- Foundation for generics
- Better optimization opportunities
- Improved error messages
- Debugging support

## Notes

- Keep changes minimal in Phase 1
- Document future optimization points
- Maintain clean separation of concerns
- Consider compile-time vs runtime costs