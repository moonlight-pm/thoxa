#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdatomic.h>
#include <string.h>
#include <stdbool.h>

// Type tags for discriminated union
enum {
    TYPE_SCALAR  = 10,   // Simple values (future)
    TYPE_STRING  = 20,   // String types
    TYPE_ARRAY   = 30,   // Array types  
    TYPE_SLICE   = 40,   // Slice views
    TYPE_TRIAL   = 50,   // Trial<T,E> variants
    TYPE_CUSTOM  = 100,  // User-defined types
};

// String-specific flags
#define STRING_HEAP      0x0000  // Normal heap string
#define STRING_SSO       0x0001  // Small string optimization (future)
#define STRING_INTERNED  0x0002  // Interned string (future)
#define STRING_ROPE      0x0004  // Rope/cord structure (future)
#define STRING_STATIC    0x0008  // Points to static data

// Array-specific flags
#define ARRAY_NORMAL     0x0000  // Standard array
#define ARRAY_FIXED      0x0001  // Fixed-size array (future)
#define ARRAY_PACKED     0x0002  // Packed representation (future)

// Universal header for all heap allocations
typedef struct {
    atomic_size_t refcount;  // 8 bytes: Atomic reference count
    uint16_t type_tag;       // 2 bytes: Type identifier
    uint16_t flags;          // 2 bytes: Type-specific optimization flags
    uint32_t reserved;       // 4 bytes: Reserved for future use/alignment
} thoxa_header_t;

// Type operation table
typedef struct {
    void (*destructor)(void* ptr);
    void* (*clone)(void* ptr);
    size_t (*size_of)(void* ptr);
    bool (*equals)(void* a, void* b);
} type_ops_t;

// Global type operation registry (indexed by type_tag)
static type_ops_t* type_operations[256] = {0};

// Forward declarations for type-specific operations
static void string_destructor(void* ptr);
static void array_destructor(void* ptr);
static void default_destructor(void* ptr);

// Initialize type operations (called once at startup)
__attribute__((constructor))
static void init_type_operations() {
    // Default operations
    static type_ops_t default_ops = {
        .destructor = default_destructor,
        .clone = NULL,
        .size_of = NULL,
        .equals = NULL
    };
    
    // String operations
    static type_ops_t string_ops = {
        .destructor = string_destructor,
        .clone = NULL,  // TODO: implement
        .size_of = NULL, // TODO: implement
        .equals = NULL   // TODO: implement
    };
    
    // Array operations
    static type_ops_t array_ops = {
        .destructor = array_destructor,
        .clone = NULL,  // TODO: implement
        .size_of = NULL, // TODO: implement
        .equals = NULL   // TODO: implement
    };
    
    // Register operations
    for (int i = 0; i < 256; i++) {
        type_operations[i] = &default_ops;
    }
    type_operations[TYPE_STRING] = &string_ops;
    type_operations[TYPE_ARRAY] = &array_ops;
}

// Get header from user pointer
#define GET_HEADER(ptr) ((thoxa_header_t*)((char*)(ptr) - sizeof(thoxa_header_t)))

// Get user pointer from header
#define GET_DATA(header) ((void*)((char*)(header) + sizeof(thoxa_header_t)))

// Allocate memory with type tag
void* thoxa_alloc(size_t size, uint16_t type_tag) {
    if (size == 0) {
        return NULL;
    }
    
    size_t total_size = size + sizeof(thoxa_header_t);
    thoxa_header_t* header = (thoxa_header_t*)malloc(total_size);
    
    if (!header) {
        fprintf(stderr, "thoxa_alloc: out of memory (requested %zu bytes)\n", size);
        exit(1);
    }
    
    // Initialize header
    atomic_init(&header->refcount, 1);
    header->type_tag = type_tag;
    header->flags = 0;
    header->reserved = 0;
    
    // Return pointer to user data
    return GET_DATA(header);
}

// Backwards compatibility - allocate with no type (defaults to CUSTOM)
void* thoxa_alloc_raw(size_t size) {
    return thoxa_alloc(size, TYPE_CUSTOM);
}

// Free memory (only called when refcount reaches 0)
void thoxa_free(void* ptr) {
    if (!ptr) {
        return;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    
    // Call type-specific destructor if available
    if (header->type_tag < 256 && type_operations[header->type_tag]) {
        type_ops_t* ops = type_operations[header->type_tag];
        if (ops->destructor) {
            ops->destructor(ptr);
        }
    }
    
    free(header);
}

// Increment reference count
void thoxa_incref(void* ptr) {
    if (!ptr) {
        return;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    atomic_fetch_add(&header->refcount, 1);
}

// Decrement reference count and free if it reaches 0
void thoxa_decref(void* ptr) {
    if (!ptr) {
        return;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    size_t old_count = atomic_fetch_sub(&header->refcount, 1);
    
    if (old_count == 1) {
        // Last reference, deallocate
        thoxa_free(ptr);
    }
}

// Better-named aliases for Thoxa language use
void thoxa_retain(void* ptr) {
    thoxa_incref(ptr);
}

void thoxa_release(void* ptr) {
    thoxa_decref(ptr);
}

// Get type tag from pointer
uint16_t thoxa_get_type(void* ptr) {
    if (!ptr) {
        return 0;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    return header->type_tag;
}

// Get flags from pointer
uint16_t thoxa_get_flags(void* ptr) {
    if (!ptr) {
        return 0;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    return header->flags;
}

// Set flags on pointer
void thoxa_set_flags(void* ptr, uint16_t flags) {
    if (!ptr) {
        return;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    header->flags = flags;
}

// Get current reference count (for debugging)
size_t thoxa_refcount(void* ptr) {
    if (!ptr) {
        return 0;
    }
    
    thoxa_header_t* header = GET_HEADER(ptr);
    return atomic_load(&header->refcount);
}

// ============================================================================
// String Implementation
// ============================================================================

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

// String destructor
static void string_destructor(void* ptr) {
    thoxa_string_t* str = (thoxa_string_t*)((char*)ptr - sizeof(thoxa_header_t));
    
    if (str->header.flags == STRING_HEAP) {
        // Free the heap-allocated buffer
        if (str->variant.heap.data) {
            free(str->variant.heap.data);
        }
    }
    // Other variants don't need cleanup
}

// Create a new string from C string
thoxa_string_t* thoxa_string_new(const char* cstr) {
    if (!cstr) {
        cstr = "";
    }
    
    size_t len = strlen(cstr);
    
    // Allocate string structure
    thoxa_string_t* str = (thoxa_string_t*)thoxa_alloc(
        sizeof(thoxa_string_t) - sizeof(thoxa_header_t), 
        TYPE_STRING
    );
    
    // Move back to get full structure including header
    str = (thoxa_string_t*)((char*)str - sizeof(thoxa_header_t));
    
    // For now, always use heap variant (SSO can be added later)
    str->header.flags = STRING_HEAP;
    str->variant.heap.length = len;
    str->variant.heap.capacity = len + 1;
    str->variant.heap.data = (char*)malloc(len + 1);
    
    if (!str->variant.heap.data) {
        fprintf(stderr, "thoxa_string_new: out of memory\n");
        exit(1);
    }
    
    strcpy(str->variant.heap.data, cstr);
    
    // Return pointer to data portion (after header)
    return (thoxa_string_t*)GET_DATA(&str->header);
}

// Get C string from Thoxa string
const char* thoxa_string_cstr(thoxa_string_t* str) {
    if (!str) {
        return "";
    }
    
    // Adjust pointer to get full structure
    str = (thoxa_string_t*)((char*)str - sizeof(thoxa_header_t));
    
    switch (str->header.flags) {
        case STRING_HEAP:
            return str->variant.heap.data;
        case STRING_STATIC:
            return str->variant.static_ref.data;
        case STRING_SSO:
            return str->variant.sso.data;
        default:
            return "";
    }
}

// Get string length
size_t thoxa_string_length(thoxa_string_t* str) {
    if (!str) {
        return 0;
    }
    
    // Adjust pointer to get full structure
    str = (thoxa_string_t*)((char*)str - sizeof(thoxa_header_t));
    
    switch (str->header.flags) {
        case STRING_HEAP:
            return str->variant.heap.length;
        case STRING_STATIC:
            return str->variant.static_ref.length;
        case STRING_SSO:
            return str->variant.sso.length;
        default:
            return 0;
    }
}

// Concatenate two strings
thoxa_string_t* thoxa_string_concat(thoxa_string_t* a, thoxa_string_t* b) {
    if (!a && !b) {
        return thoxa_string_new("");
    }
    
    const char* str_a = a ? thoxa_string_cstr(a) : "";
    const char* str_b = b ? thoxa_string_cstr(b) : "";
    
    size_t len_a = strlen(str_a);
    size_t len_b = strlen(str_b);
    size_t total_len = len_a + len_b;
    
    // Allocate string structure
    thoxa_string_t* result = (thoxa_string_t*)thoxa_alloc(
        sizeof(thoxa_string_t) - sizeof(thoxa_header_t),
        TYPE_STRING
    );
    
    // Move back to get full structure including header
    result = (thoxa_string_t*)((char*)result - sizeof(thoxa_header_t));
    
    // Always use heap variant for concatenation
    result->header.flags = STRING_HEAP;
    result->variant.heap.length = total_len;
    result->variant.heap.capacity = total_len + 1;
    result->variant.heap.data = (char*)malloc(total_len + 1);
    
    if (!result->variant.heap.data) {
        fprintf(stderr, "thoxa_string_concat: out of memory\n");
        exit(1);
    }
    
    // Copy data
    strcpy(result->variant.heap.data, str_a);
    strcat(result->variant.heap.data, str_b);
    
    // Return pointer to data portion (after header)
    return (thoxa_string_t*)GET_DATA(&result->header);
}

// Create a static string (points to existing data, no allocation)
thoxa_string_t* thoxa_string_static(const char* data) {
    if (!data) {
        data = "";
    }
    
    // Allocate string structure
    thoxa_string_t* str = (thoxa_string_t*)thoxa_alloc(
        sizeof(thoxa_string_t) - sizeof(thoxa_header_t),
        TYPE_STRING
    );
    
    // Move back to get full structure including header
    str = (thoxa_string_t*)((char*)str - sizeof(thoxa_header_t));
    
    // Use static variant - no heap allocation
    str->header.flags = STRING_STATIC;
    str->variant.static_ref.length = strlen(data);
    str->variant.static_ref.data = data;
    
    // Return pointer to data portion (after header)
    return (thoxa_string_t*)GET_DATA(&str->header);
}

// ============================================================================
// Array Implementation
// ============================================================================

typedef struct {
    thoxa_header_t header;
    size_t length;          // Current number of elements
    size_t capacity;        // Allocated capacity
    size_t element_size;    // Size of each element
    uint16_t element_type;  // Type tag of elements (for cleanup)
    void* data;            // Pointer to element storage
} thoxa_array_t;

// Forward declaration
static void array_cleanup_elements(thoxa_array_t* arr);

// Array destructor
static void array_destructor(void* ptr) {
    thoxa_array_t* arr = (thoxa_array_t*)((char*)ptr - sizeof(thoxa_header_t));
    
    // Clean up elements if they need reference counting
    if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
        array_cleanup_elements(arr);
    }
    
    if (arr->data) {
        free(arr->data);
    }
}

// Clean up reference-counted elements
static void array_cleanup_elements(thoxa_array_t* arr) {
    if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
        // Elements are pointers that need decref
        void** elements = (void**)arr->data;
        for (size_t i = 0; i < arr->length; i++) {
            if (elements[i]) {
                thoxa_decref(elements[i]);
            }
        }
    }
}

// Create a new array
thoxa_array_t* thoxa_array_new(size_t initial_capacity, size_t element_size, uint16_t element_type) {
    if (element_size == 0) {
        element_size = sizeof(void*); // Default to pointer size
    }
    
    if (initial_capacity == 0) {
        initial_capacity = 8; // Default initial capacity
    }
    
    // Allocate array structure
    thoxa_array_t* arr = (thoxa_array_t*)thoxa_alloc(
        sizeof(thoxa_array_t) - sizeof(thoxa_header_t),
        TYPE_ARRAY
    );
    
    // Move back to get full structure including header
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    // Initialize array
    arr->header.flags = ARRAY_NORMAL;
    arr->length = 0;
    arr->capacity = initial_capacity;
    arr->element_size = element_size;
    arr->element_type = element_type;
    
    // Allocate element storage
    arr->data = calloc(initial_capacity, element_size);
    if (!arr->data) {
        fprintf(stderr, "thoxa_array_new: out of memory\n");
        exit(1);
    }
    
    // Return pointer to data portion (after header)
    return (thoxa_array_t*)GET_DATA(&arr->header);
}

// Get array element type
uint16_t thoxa_array_element_type(thoxa_array_t* arr) {
    if (!arr) {
        return 0;
    }
    thoxa_array_t* real_arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    return real_arr->element_type;
}

// Get array length
size_t thoxa_array_length(thoxa_array_t* arr) {
    if (!arr) {
        return 0;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    return arr->length;
}

// Get array capacity
size_t thoxa_array_capacity(thoxa_array_t* arr) {
    if (!arr) {
        return 0;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    return arr->capacity;
}

// Grow array capacity if needed
static void array_grow(thoxa_array_t* arr) {
    size_t new_capacity = arr->capacity * 2;
    void* new_data = realloc(arr->data, new_capacity * arr->element_size);
    
    if (!new_data) {
        fprintf(stderr, "thoxa_array_grow: out of memory\n");
        exit(1);
    }
    
    // Zero out new memory
    size_t old_bytes = arr->capacity * arr->element_size;
    size_t new_bytes = new_capacity * arr->element_size;
    memset((char*)new_data + old_bytes, 0, new_bytes - old_bytes);
    
    arr->data = new_data;
    arr->capacity = new_capacity;
}

// Push element to array
void thoxa_array_push(thoxa_array_t* arr, void* element) {
    if (!arr) {
        return;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    // Grow if needed
    if (arr->length >= arr->capacity) {
        array_grow(arr);
    }
    
    // Copy element
    void* dest = (char*)arr->data + (arr->length * arr->element_size);
    memcpy(dest, element, arr->element_size);
    
    // If element is reference-counted, incref it
    if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
        void* ptr = *(void**)element;
        if (ptr) {
            thoxa_incref(ptr);
        }
    }
    
    arr->length++;
}

// Get element from array
void* thoxa_array_get(thoxa_array_t* arr, size_t index) {
    if (!arr) {
        return NULL;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    if (index >= arr->length) {
        fprintf(stderr, "thoxa_array_get: index %zu out of bounds (length %zu)\n", 
                index, arr->length);
        return NULL;
    }
    
    return (char*)arr->data + (index * arr->element_size);
}

// Set element in array
void thoxa_array_set(thoxa_array_t* arr, size_t index, void* element) {
    if (!arr) {
        return;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    if (index >= arr->length) {
        fprintf(stderr, "thoxa_array_set: index %zu out of bounds (length %zu)\n", 
                index, arr->length);
        return;
    }
    
    void* dest = (char*)arr->data + (index * arr->element_size);
    
    // Handle reference counting for old value
    if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
        void* old_ptr = *(void**)dest;
        void* new_ptr = *(void**)element;
        
        if (new_ptr) {
            thoxa_incref(new_ptr);
        }
        
        memcpy(dest, element, arr->element_size);
        
        if (old_ptr) {
            thoxa_decref(old_ptr);
        }
    } else {
        // Simple copy for non-reference types
        memcpy(dest, element, arr->element_size);
    }
}

// Pop element from array
int thoxa_array_pop(thoxa_array_t* arr, void* out_element) {
    if (!arr) {
        return 0;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    if (arr->length == 0) {
        return 0; // Empty array
    }
    
    arr->length--;
    void* src = (char*)arr->data + (arr->length * arr->element_size);
    
    if (out_element) {
        memcpy(out_element, src, arr->element_size);
    } else {
        // No output requested, need to decref if reference type
        if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
            void* ptr = *(void**)src;
            if (ptr) {
                thoxa_decref(ptr);
            }
        }
    }
    
    // Clear the popped element
    memset(src, 0, arr->element_size);
    
    return 1; // Success
}

// Clear array (remove all elements)
void thoxa_array_clear(thoxa_array_t* arr) {
    if (!arr) {
        return;
    }
    
    // Adjust pointer to get full structure
    arr = (thoxa_array_t*)((char*)arr - sizeof(thoxa_header_t));
    
    // Clean up elements if they need reference counting
    if (arr->element_type == TYPE_STRING || arr->element_type == TYPE_ARRAY) {
        array_cleanup_elements(arr);
    }
    
    // Clear data and reset length
    memset(arr->data, 0, arr->length * arr->element_size);
    arr->length = 0;
}

// ============================================================================
// Default Operations
// ============================================================================

static void default_destructor(void* ptr) {
    // No additional cleanup needed for unknown types
    (void)ptr;
}

// ============================================================================
// Memory Statistics (for debugging)
// ============================================================================

typedef struct {
    size_t total_allocations;
    size_t total_deallocations;
    size_t current_allocated_bytes;
    size_t peak_allocated_bytes;
} thoxa_mem_stats_t;

static thoxa_mem_stats_t mem_stats = {0, 0, 0, 0};

// Get memory statistics
void thoxa_mem_stats(thoxa_mem_stats_t* stats) {
    if (stats) {
        *stats = mem_stats;
    }
}

// Debug: Print memory statistics
void thoxa_print_mem_stats(void) {
    printf("Memory Statistics:\n");
    printf("  Total allocations: %zu\n", mem_stats.total_allocations);
    printf("  Total deallocations: %zu\n", mem_stats.total_deallocations);
    printf("  Current allocated: %zu bytes\n", mem_stats.current_allocated_bytes);
    printf("  Peak allocated: %zu bytes\n", mem_stats.peak_allocated_bytes);
    printf("  Leaked allocations: %zu\n", 
           mem_stats.total_allocations - mem_stats.total_deallocations);
}