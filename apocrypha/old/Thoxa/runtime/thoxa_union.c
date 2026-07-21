// Runtime support for Thoxa union types (discriminated unions)

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include "thoxa_runtime.h"

// Union type tag - identifies union values
#define TYPE_UNION 200

// Discriminated union structure
typedef struct {
    // Standard header is added by thoxa_alloc
    uint16_t union_type_id;   // ID of the union type (e.g., Shape)
    uint16_t variant_type_id;  // ID of the actual variant type (e.g., Circle)
    uint32_t size;             // Size of the value
    void* value;               // Pointer to the actual value (boxed if primitive)
} thoxa_union_t;

// Create a union value
void* thoxa_union_create(uint16_t union_type_id, uint16_t variant_type_id, void* value) {
    // Allocate union with our memory management
    thoxa_union_t* uni = (thoxa_union_t*)thoxa_alloc(
        sizeof(thoxa_union_t),
        TYPE_UNION
    );
    
    if (!uni) {
        fprintf(stderr, "thoxa_union_create: out of memory\n");
        exit(1);
    }
    
    // Initialize union
    uni->union_type_id = union_type_id;
    uni->variant_type_id = variant_type_id;
    uni->size = sizeof(void*);  // We store pointers
    uni->value = value;
    
    // Increment refcount of the value if it's heap-allocated
    if (value) {
        thoxa_incref(value);
    }
    
    return uni;
}

// Get the variant type of a union value
uint16_t thoxa_union_get_variant_type(void* union_ptr) {
    if (!union_ptr) return 0;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    return uni->variant_type_id;
}

// Get the union type ID
uint16_t thoxa_union_get_type(void* union_ptr) {
    if (!union_ptr) return 0;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    return uni->union_type_id;
}

// Get the value from a union
void* thoxa_union_get_value(void* union_ptr) {
    if (!union_ptr) return NULL;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    return uni->value;
}

// Check if a union value matches a specific variant type
int thoxa_union_is_variant(void* union_ptr, uint16_t variant_type_id) {
    if (!union_ptr) return 0;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    return uni->variant_type_id == variant_type_id;
}

// Cast a value to a union type (performs implicit conversion)
void* thoxa_union_cast(void* value, uint16_t value_type_id, uint16_t union_type_id) {
    // For now, just wrap the value in a union
    // In the future, we'll validate that value_type_id is a valid variant of union_type_id
    return thoxa_union_create(union_type_id, value_type_id, value);
}

// Extract a value from a union, checking the type
void* thoxa_union_extract(void* union_ptr, uint16_t expected_type) {
    if (!union_ptr) return NULL;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    
    // Type check
    if (uni->variant_type_id != expected_type) {
        fprintf(stderr, "thoxa_union_extract: type mismatch (expected %d, got %d)\n", 
                expected_type, uni->variant_type_id);
        return NULL;
    }
    
    return uni->value;
}

// Free a union value (called by reference counting)
void thoxa_union_free(void* union_ptr) {
    if (!union_ptr) return;
    
    thoxa_union_t* uni = (thoxa_union_t*)union_ptr;
    
    // Decrement refcount of the contained value
    if (uni->value) {
        thoxa_decref(uni->value);
    }
    
    // The union itself will be freed by the memory system
}