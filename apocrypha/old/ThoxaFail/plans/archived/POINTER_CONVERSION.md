# Pointer System Conversion: Fat Pointers to Tagged Pointers

## Overview

This document outlines the conversion from the fat pointer system (128-bit) to the tagged pointer system (64-bit) in Thoxa. The partial conversion is causing memory corruption and incorrect behavior.

## Previous System: Fat Pointers (128-bit)

### Structure
- **Size**: 128 bits (16 bytes)
- **Layout**: 
  - First 64 bits: Type tag (BuiltinTypeId)
  - Second 64 bits: Value or pointer

### Implementation Details
```rust
// Creating a fat pointer
fn make_fat_pointer(builder: &mut FunctionBuilder, type_tag: Value, value: Value) -> Value {
    let pair_type = types::I128;
    let type_tag_128 = builder.ins().uextend(types::I128, type_tag);
    let value_128 = builder.ins().uextend(types::I128, value);
    let shifted = builder.ins().ishl_imm(value_128, 64);
    builder.ins().bor(type_tag_128, shifted)
}

// Extracting from fat pointer
fn extract_type(builder: &mut FunctionBuilder, fat_ptr: Value) -> Value {
    builder.ins().ireduce(types::I64, fat_ptr)
}

fn extract_value(builder: &mut FunctionBuilder, fat_ptr: Value) -> Value {
    let shifted = builder.ins().ushr_imm(fat_ptr, 64);
    builder.ins().ireduce(types::I64, shifted)
}
```

### Array Storage with Fat Pointers
- Array elements: 16 bytes each (full fat pointer)
- Array layout: `[8 bytes: length][16 bytes * n: elements]`
- Total size: `8 + (n * 16)` bytes

## New System: Tagged Pointers (64-bit)

### Structure
- **Size**: 64 bits (8 bytes)
- **Layout for integers**: 
  - Bits 0-2: Tag (001 for integers)
  - Bits 3-63: Integer value (shifted left by 3)
- **Layout for pointers**:
  - Bits 0-2: Tag (000 for pointers)
  - Bits 3-63: Pointer address (already 8-byte aligned, so low 3 bits are 0)

### Implementation Details
```rust
// Tag constants
const INT_TAG: i64 = 0b001;
const PTR_TAG: i64 = 0b000;

// Creating tagged values
fn tag_int_value(builder: &mut FunctionBuilder, value: Value) -> Value {
    let shifted = builder.ins().ishl_imm(value, 3);
    let tag = builder.ins().iconst(types::I64, INT_TAG);
    builder.ins().bor(shifted, tag)
}

// Extracting from tagged pointer
fn extract_value(builder: &mut FunctionBuilder, tagged: Value) -> Value {
    if USE_TAGGED_POINTERS {
        // Check if it's an integer (low 3 bits == 001)
        let tag_mask = builder.ins().iconst(types::I64, 0b111);
        let tag = builder.ins().band(tagged, tag_mask);
        let int_tag = builder.ins().iconst(types::I64, INT_TAG);
        let is_int = builder.ins().icmp(IntCC::Equal, tag, int_tag);
        
        // If integer, shift right by 3
        // If pointer, just clear tag bits
        let shifted = builder.ins().sshr_imm(tagged, 3);
        let mask = builder.ins().iconst(types::I64, !0b111);
        let cleared = builder.ins().band(tagged, mask);
        
        builder.ins().select(is_int, shifted, cleared)
    } else {
        // Old fat pointer extraction
        let shifted = builder.ins().ushr_imm(tagged, 64);
        builder.ins().ireduce(types::I64, shifted)
    }
}
```

### Array Storage with Tagged Pointers
- Array elements: 8 bytes each (tagged value)
- Array layout: `[8 bytes: length][8 bytes * n: elements]`
- Total size: `8 + (n * 8)` bytes

## Conversion Issues Found

### 1. Mixed Storage Sizes
**Problem**: Some code paths store fat pointers (16 bytes) while others expect tagged pointers (8 bytes).

**Example**: Variadic array creation
```rust
// Current problematic code
let element_size = if USE_FAT_POINTERS { 16 } else { 8 };
let total_size = 8 + (element_count * element_size);
```

### 2. Inconsistent Tagging
**Problem**: Some values are tagged, others are not, leading to incorrect interpretation.

**Example**: Array length field
- Stored as raw i64
- Sometimes returned as tagged integer
- Sometimes returned as raw value

### 3. String Pointer Corruption
**Problem**: String pointers are being incorrectly tagged or extracted.

**Symptoms**:
- `<invalid utf-8>` output
- Partial string output with garbage

### 4. Array Access Confusion
**Problem**: Array indexing code expects consistent element sizes but gets mixed sizes.

## Conversion Plan

### Phase 1: Establish Clear Boundaries
1. Define `USE_TAGGED_POINTERS` constant (currently missing clear definition)
2. Ensure `USE_FAT_POINTERS` and `USE_TAGGED_POINTERS` are mutually exclusive
3. Create clear conversion points at FFI boundaries

### Phase 2: Fix Storage Consistency
1. Update all array creation to use 8-byte elements
2. Fix variadic array creation to store tagged values
3. Ensure array indexing uses correct offsets

### Phase 3: Fix Value Tagging
1. Audit all value creation points
2. Ensure consistent tagging for:
   - Integer literals
   - Array lengths
   - Function returns
   - Variable storage

### Phase 4: Fix String Handling
1. Strings should be stored as pointers (tag 000)
2. String lengths should be tagged integers
3. Fix @printStr to handle tagged string pointers

### Phase 5: Testing Strategy
1. Create isolated tests for each pointer operation
2. Test value round-trips (create → store → load → extract)
3. Test array operations independently
4. Test string operations independently

## Test Cases Needed

### 1. Basic Tagged Value Tests
```thx
// Test integer tagging
let x: Int64 = 42;
@print(x);  // Should print 42

// Test pointer tagging
let arr = [1, 2, 3];
@print(arr.length());  // Should print 3
```

### 2. Array Storage Tests
```thx
// Test array element storage
let arr = [10, 20, 30];
@print(arr[0]);  // Should print 10
@print(arr[1]);  // Should print 20
@print(arr[2]);  // Should print 30
```

### 3. String Tests
```thx
// Test string storage and retrieval
let s: String = "Hello";
@printStr(s);  // Should print "Hello"
```

### 4. Mixed Operations Tests
```thx
// Test that different types don't interfere
let x: Int64 = 100;
let s: String = "Test";
let arr = [1, 2];

@print(x);        // Should print 100
@printStr(s);     // Should print "Test"
@print(arr[0]);   // Should print 1
```

## Success Criteria

1. All values consistently use either fat pointers OR tagged pointers
2. No memory corruption or invalid UTF-8 errors
3. Array lengths report correctly
4. All basic operations work as expected
5. Existing tests pass without modification

## CRITICAL UPDATE: Root Cause Analysis Complete

### December 2024 Investigation Results

After systematic testing with completely untagged values, the issue is **NOT** related to pointer tagging at all. The real problem has been identified:

**ROOT CAUSE: Loop Variable Persistence Failure**
- Variables assigned inside loops don't persist their values for the next iteration
- Loop variables reset to initial values on each iteration instead of accumulating changes
- This causes infinite loops because condition variables never update

**Testing Evidence:**
1. ✅ Basic operations work correctly: arithmetic, assignments, conditions, if statements
2. ✅ Tagged pointer system works correctly for all non-loop operations  
3. ✅ All operations work correctly when used outside of loops
4. ❌ Mutable variable assignments inside loops don't persist across iterations

**Technical Details:**
- Issue occurs even with completely untagged 64-bit values
- **PRECISE ROOT CAUSE**: Variable assignments fail completely inside loop blocks
- Assignment `i = 5` inside loop doesn't take effect (variable retains old value)
- Assignment `i = i + 1` inside loop evaluates `i + 1` correctly but assignment fails
- Same assignments work perfectly outside loops
- **NOT** a phi node or SSA iteration issue - assignments are fundamentally broken in loop context
- Problem is in Cranelift code generation within loop blocks specifically

### Updated Fix Priority
1. **IMMEDIATE**: Fix loop variable environment/SSA construction issue
2. Later: Complete pointer system cleanup (currently not blocking functionality)

## Next Steps

1. ~~Implement clear separation between pointer systems~~ (DEPRIORITIZED)
2. **URGENT**: Fix Cranelift loop SSA construction for variable persistence
3. Investigate variable environment handling in loop contexts
4. ~~Create isolated test cases~~ (COMPLETED - revealed real issue)
5. ~~Fix each subsystem independently~~ (POINTER SYSTEM WORKS)
6. Test loop functionality after SSA fix