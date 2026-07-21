# Fat Pointer Migration Plan

## Overview

This document outlines the systematic migration from raw values to uniform fat pointers throughout the Thoxa compiler.

## Current State

- Values are currently raw 64-bit values
- Type information is only available at compile time
- The TypedExpr refactoring provides type info to codegen

## Target State

- ALL values are 128-bit fat pointers: `[type tag][value]`
- Type information is always available at runtime
- Any type requires no special handling

## Migration Strategy

### Phase 1: Infrastructure (COMPLETE)
- [x] Create fat pointer generation functions
- [x] Type tag generation from Type
- [x] TypedExpr threading (partial)

### Phase 2: TypedExpr Refactoring (IN PROGRESS)

#### Step 1: Restructure TypedExpr (COMPLETE)
- [x] Created new TypedExpr enum with typed subexpressions
- [x] Updated type checker to produce new TypedExpr
- [x] Fixed all `.ty` and `.expr` accesses

**BLOCKER RESOLVED**: TypedExpr now contains typed subexpressions!

#### Step 2: Update remaining AST nodes
- [x] Number, StringLiteral, Identifier
- [x] Binary operations
- [x] Sequence and Block
- [x] LetBinding and Assignment
- [ ] FunctionCall, Print, PrintStr
- [ ] If, Loop, Break, Continue
- [ ] FieldAccess, MethodCall
- [ ] Array operations
- [ ] Conversion operations

### Phase 3: Gradual Value Representation Change

#### Step 1: Update Cranelift types
- [x] Use stack allocation for fat pointers (chosen approach)
- [x] Create helpers to extract value/type from fat pointers
- [ ] Update all Value generation to return pointers to fat pointers

#### Step 2: Update value generation
- [ ] Numbers: Return fat pointer instead of raw value
- [ ] Strings: Return fat pointer with String tag
- [ ] All literals create fat pointers

#### Step 3: Update value consumption
- [ ] Arithmetic operations: Extract values from fat pointers
- [ ] Comparisons: Extract values from fat pointers
- [ ] Function calls: Pass fat pointers

#### Step 4: Update storage
- [ ] Variables store 16 bytes (fat pointers)
- [ ] Let bindings handle fat pointers
- [ ] Module fields are fat pointers

#### Step 5: Update builtins
- [ ] @print: Extract value from fat pointer
- [ ] @printStr: Extract pointer from fat pointer
- [ ] All builtins handle fat pointers

### Phase 3: Complete TypedExpr Threading
- [ ] All recursive calls use TypedExpr
- [ ] Remove old translate_expr function
- [ ] Type information available everywhere

### Phase 4: Implement Type Operations
- [ ] Type guards: `if x is Type`
- [ ] Safe casts: `x as Type` after guard
- [ ] Runtime conversions with type info

## Implementation Order

1. **Start small**: Begin with number literals
2. **Test incrementally**: Ensure each step works
3. **Maintain compatibility**: Keep old path working during transition
4. **Complete systematically**: Don't leave half-migrated code

## Technical Considerations

### Cranelift Representation

Fat pointers need special handling in Cranelift:
```rust
// Option 1: Use struct types (not well supported)
// Option 2: Pass as two separate i64 values
// Option 3: Use i128 (if supported)
// Option 4: Always use pointers to stack-allocated fat pointers
```

We'll use Option 4: Stack allocation for reliability.

### Performance Impact

- 2x memory usage for all values
- Additional indirection through pointers
- Can optimize later with better Cranelift integration

### Debugging Benefits

- Can always inspect value types at runtime
- Better error messages
- Foundation for future reflection

## Success Criteria

The migration is complete when:
- [ ] All values are fat pointers
- [ ] Any type works without special cases
- [ ] Type guards can check runtime types
- [ ] All tests pass
- [ ] Performance is acceptable

## Risks

1. **Complexity**: This touches every part of codegen
2. **Performance**: May be slower initially
3. **Bugs**: Easy to miss edge cases

## Mitigation

- Incremental approach with testing
- Keep old code path during transition
- Extensive testing at each phase