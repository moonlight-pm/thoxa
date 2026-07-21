# Thoxa Memory System

## Overview

Thoxa uses automatic reference counting (ARC) with a unified memory model for all heap-allocated types. This provides deterministic memory management without garbage collection pauses.

## Key Concepts

### Discriminated Union Approach

All heap-allocated values in Thoxa share a common header structure, allowing for:
- Unified memory tracking and debugging
- Type-safe runtime operations
- Future optimizations without breaking changes
- Single allocation API for all types

### Reference Counting

Every heap allocation maintains an atomic reference count:
- **Allocation**: Starts with refcount = 1
- **Assignment**: Increments refcount
- **Scope Exit**: Decrements refcount
- **Deallocation**: Automatic when refcount reaches 0

## Memory Header

Every heap allocation has a 16-byte header:

```
[8 bytes] Atomic reference count
[2 bytes] Type tag (STRING, ARRAY, etc.)
[2 bytes] Flags (optimization hints)
[4 bytes] Reserved for future use
```

## Type System Integration

### Built-in Types

Stack-allocated (no refcounting):
- `Int`, `Float`, `Bool`, `Char`
- Small fixed-size values

Heap-allocated (automatic refcounting):
- `String` - Text data with various optimizations
- `Array<T>` - Dynamic arrays with type safety
- `Trial<T,E>` - Error handling variants
- User-defined types containing heap data

### Compiler Integration

The Thoxa compiler automatically:
- Generates incref calls on assignment
- Generates decref calls on scope exit
- Optimizes away redundant ref operations
- Chooses optimal representation based on context

## String Optimizations

Strings can use different internal representations transparently:

```thoxa
let s1 = "Hello"        # Static data, no allocation
let s2 = "Hi"          # May use small string optimization
let s3 = s1 + " World" # Heap allocated
```

The runtime chooses the best representation:
- **Static**: String literals point to program data
- **Small String Optimization (SSO)**: Strings ≤ 23 bytes stored inline
- **Heap**: Normal heap-allocated buffer
- **Interned**: Frequently used strings share storage (future)

## Safety Guarantees

The memory system prevents:
- **Use-after-free**: Reference counting keeps memory alive
- **Double-free**: Automatic deallocation only at refcount=0
- **Memory leaks**: Automatic cleanup (except reference cycles)
- **Buffer overflows**: Bounds checking in debug mode

## Performance Characteristics

- **Allocation**: O(1) with thread-local caching
- **Reference counting**: Atomic increment/decrement
- **Deallocation**: O(1) plus type destructor
- **No GC pauses**: Deterministic deallocation

## Example Usage

### Strings
```thoxa
fn processData(data: String) -> String {
    # 'data' refcount incremented on entry
    let result = data + " processed"  # New allocation
    # 'data' refcount decremented on exit
    result  # Ownership transferred to caller
}

fn main() {
    let input = "Hello"              # Static, no allocation
    let output = processData(input)  # Automatic memory management
    println(output)
    # 'output' deallocated here when refcount → 0
}
```

### Arrays
```thoxa
fn testArrays() {
    # Create array with initial capacity
    let arr = thoxaArrayNew(10, 8, 0)  # capacity=10, elem_size=8, type=INT
    
    # Array operations
    let length = thoxaArrayLength(arr)
    let capacity = thoxaArrayCapacity(arr)
    
    # Arrays of strings automatically manage references
    let stringArr = thoxaArrayNew(5, 8, 20)  # TYPE_STRING = 20
    # Push/pop operations handle refcounting automatically
    
    # Cleanup is automatic on scope exit
}
```

## Implementation Details

### Phase 1: Foundation ✅
Basic allocation and reference counting infrastructure.

### Phase 2: Type System ✅
Discriminated union with type tags and dispatch tables.

### Phase 3: Strings ✅
Full string support with heap and static variants, concatenation, and automatic memory management.

### Phase 4: Arrays ✅
Dynamic arrays with type information, automatic growth, element cleanup, and bounds checking.

### Phase 5: Optimizations (Future)
SSO, interning, custom allocators, and performance tuning.

## Debugging Support

The memory system provides:
- Memory leak detection in debug builds
- Allocation tracking and statistics
- Type-safe runtime checks
- Clear error messages for memory issues

## Future Extensions

The design reserves space for:
- Weak references for breaking cycles
- Custom allocators and memory pools
- Concurrent garbage collection for cycles
- Memory compression and compaction

## API Stability

The memory header format is designed for long-term stability:
- Fixed 16-byte size
- Reserved bytes for future features
- Type tags with gaps for new types
- Flags field for optimizations

This ensures binary compatibility across Thoxa versions while allowing internal improvements.