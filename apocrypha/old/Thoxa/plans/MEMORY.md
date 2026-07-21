# Memory Management

## Overview

Thoxa uses automatic reference counting (ARC) with a unified discriminated union approach for all heap allocations. This provides deterministic memory deallocation without a garbage collector, while maintaining flexibility for future optimizations.

## Core Design: Discriminated Union

All heap-allocated values share a common header structure with type-specific variants:

### Universal Header
```c
typedef struct {
    atomic_size_t refcount;  // 8 bytes: Atomic reference count
    uint16_t type_tag;       // 2 bytes: Type identifier
    uint16_t flags;          // 2 bytes: Type-specific optimization flags
    uint32_t reserved;       // 4 bytes: Reserved for future use/alignment
} thoxa_header_t;          // Total: 16 bytes
```

### Type Tags
```c
enum {
    TYPE_SCALAR  = 10,   // Simple values (future)
    TYPE_STRING  = 20,   // String types
    TYPE_ARRAY   = 30,   // Array types  
    TYPE_SLICE   = 40,   // Slice views
    TYPE_TRIAL   = 50,   // Trial<T,E> variants
    TYPE_CUSTOM  = 100,  // User-defined types
    // Gaps reserved for future variants
};
```

### Flags (Per Type)
```c
// String flags
#define STRING_HEAP      0x0000  // Normal heap string
#define STRING_SSO       0x0001  // Small string optimization (future)
#define STRING_INTERNED  0x0002  // Interned string (future)
#define STRING_ROPE      0x0004  // Rope/cord structure (future)
#define STRING_STATIC    0x0008  // Points to static data

// Array flags  
#define ARRAY_NORMAL     0x0000  // Standard array
#define ARRAY_FIXED      0x0001  // Fixed-size array (future)
#define ARRAY_PACKED     0x0002  // Packed representation (future)
```

## Memory Layout

### String Layout
```c
typedef struct {
    thoxa_header_t header;
    union {
        struct {  // HEAP variant (flags = STRING_HEAP)
            size_t length;
            size_t capacity;
            char* data;
        } heap;
        struct {  // SSO variant (flags = STRING_SSO) - future
            uint8_t length;
            char data[23];
        } sso;
        struct {  // STATIC variant (flags = STRING_STATIC)
            size_t length;
            const char* data;
        } static_ref;
    } variant;
} thoxa_string_t;
```

### Array Layout
```c
typedef struct {
    thoxa_header_t header;
    size_t length;           // Current number of elements
    size_t capacity;         // Allocated capacity
    size_t element_size;     // Size of each element
    type_info_t* elem_type;  // Type information for elements
    void* data;             // Pointer to element data
} thoxa_array_t;
```

### Type Dispatch Table
```c
typedef struct {
    void (*destructor)(void* ptr);
    void* (*clone)(void* ptr);
    size_t (*size_of)(void* ptr);
    bool (*equals)(void* a, void* b);
    // ... other operations
} type_ops_t;

// Global registry indexed by type_tag
type_ops_t* type_operations[256];
```

## Remaining Work

### Memory Optimizations
- [ ] Small string optimization (SSO)
- [ ] String interning table
- [ ] Packed array representations
- [ ] Custom allocator support

## API Design

### Core Memory Functions
```c
// Allocate with type tag
void* thoxa_alloc(size_t size, uint16_t type_tag);

// Reference counting (works with any type)
void thoxa_incref(void* ptr);
void thoxa_decref(void* ptr);

// Type introspection
uint16_t thoxa_get_type(void* ptr);
uint16_t thoxa_get_flags(void* ptr);
```

### String Functions (Thin Wrappers)
```c
thoxa_string_t* thoxa_string_new(const char* str);
thoxa_string_t* thoxa_string_concat(thoxa_string_t* a, thoxa_string_t* b);
thoxa_string_t* thoxa_string_slice(thoxa_string_t* s, size_t start, size_t end);
```

### Array Functions (Thin Wrappers)
```c
thoxa_array_t* thoxa_array_new(size_t capacity, size_t elem_size, uint16_t elem_type);
void thoxa_array_push(thoxa_array_t* arr, void* elem);
void* thoxa_array_get(thoxa_array_t* arr, size_t index);
```

## Extensibility Strategy

### Adding New Optimizations
1. Define new flag constant
2. Implement variant-specific operations
3. Update constructors to choose variant
4. Old code continues working unchanged

### Example: Adding SSO Later
```c
thoxa_string_t* thoxa_string_new(const char* str) {
    size_t len = strlen(str);
    
    // Choose representation based on size
    if (len <= 23) {  // SSO threshold
        // Allocate with SSO flag
        thoxa_string_t* s = thoxa_alloc(sizeof(thoxa_string_t), TYPE_STRING);
        s->header.flags = STRING_SSO;
        s->variant.sso.length = len;
        memcpy(s->variant.sso.data, str, len + 1);
        return s;
    } else {
        // Use heap variant as before
        // ...
    }
}
```

## Benefits of This Approach

1. **Unified Interface**: Single allocation API for all types
2. **Type Safety**: Type tags enable runtime type checking
3. **Extensibility**: Flags field allows future optimizations
4. **No Premature Optimization**: Start simple, optimize based on profiling
5. **Binary Compatibility**: Header format is stable, internal variants can change
6. **Performance**: Type-specific fast paths via dispatch table
7. **Memory Tracking**: Single place to monitor all allocations

## Migration Path

For existing code:
1. Current thoxa_alloc becomes thoxa_alloc_raw (deprecated)
2. New thoxa_alloc takes type_tag parameter
3. Compiler updated to emit type tags
4. Runtime functions become thin wrappers

## Future Considerations

### Reserved for Later
- Concurrent GC for cycle collection (using reserved bytes)
- Compression flags for arrays
- Memory pool identifiers
- Generation counters for optimization

### ABI Stability
Once v1.0 ships:
- Header layout is frozen (16 bytes)
- Type tag values are stable
- New flags can be added
- Internal layouts can change if interface is maintained

## Example: Complete String Implementation

```c
// User code (unchanged regardless of internal optimization)
fn main() {
    let s1 = "Hello"           # Static string
    let s2 = String::new(" ")  # Heap string (might use SSO)
    let s3 = s1 + s2          # Concatenation
    println(s3)
}

// Runtime chooses optimal representation:
// - s1: STRING_STATIC flag, no allocation
// - s2: STRING_SSO flag if small, STRING_HEAP if large  
// - s3: STRING_HEAP flag (or STRING_ROPE for large strings)
```

## Testing Strategy

### Phase-by-Phase Testing
1. **Phase 2**: Test type dispatch, header layout
2. **Phase 3**: String operations, memory leaks
3. **Phase 4**: Array bounds, element cleanup
4. **Phase 5**: Optimization correctness

### Performance Benchmarks
- Measure overhead of discriminated union
- Compare with specialized implementations
- Profile real-world programs
- Guide optimization priorities

## Implementation Order

1. **Immediate** (Phase 2): Core discriminated union
2. **Next** (Phase 3): Strings with basic heap variant
3. **Then** (Phase 4): Arrays and slices
4. **Later** (Phase 5): Optimizations based on profiling
5. **Future**: Additional types as needed