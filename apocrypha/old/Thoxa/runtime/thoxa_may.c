#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include "thoxa_runtime.h"

// May type for optional values
// Similar to Trial but for Has/Not instead of Pass/Fail

typedef enum {
    MAY_HAS = 0,
    MAY_NOT = 1
} may_discriminant_t;

// May<T> structure - similar to Trial<T,E>
typedef struct {
    uint32_t discriminant;  // 0 = Has, 1 = Not
    uint32_t padding;       // Padding to ensure 8-byte alignment
    void* value;            // Pointer to value (for Has variant)
} thoxa_may_t;

// Type ID for May (should match the one in Rust)
#define TYPE_MAY 16

// Create a Has variant
thoxa_may_t* thoxa_may_has(void* value) {
    // Allocate May structure
    thoxa_may_t* may = (thoxa_may_t*)thoxa_alloc(
        sizeof(thoxa_may_t),
        TYPE_MAY
    );
    
    if (!may) {
        fprintf(stderr, "thoxa_may_has: out of memory\n");
        exit(1);
    }
    
    // Initialize as Has variant
    may->discriminant = MAY_HAS;
    may->padding = 0;
    may->value = value;
    
    // The value is passed from the caller, who retains it
    // So we don't need to incref it here
    
    return may;
}

// Create a Not variant
thoxa_may_t* thoxa_may_not(void) {
    // Allocate May structure
    thoxa_may_t* may = (thoxa_may_t*)thoxa_alloc(
        sizeof(thoxa_may_t),
        TYPE_MAY
    );
    
    if (!may) {
        fprintf(stderr, "thoxa_may_not: out of memory\n");
        exit(1);
    }
    
    // Initialize as Not variant
    may->discriminant = MAY_NOT;
    may->padding = 0;
    may->value = NULL;  // No value for Not
    
    return may;
}

// Destroy a May value (called by thoxa_decref)
void thoxa_may_destroy(void* may_void) {
    thoxa_may_t* may = (thoxa_may_t*)may_void;
    
    // If it's a Has variant and has a value, decref it
    if (may->discriminant == MAY_HAS && may->value) {
        thoxa_decref(may->value);
    }
    
    // The memory itself will be freed by the caller
}

// Check if a May is Has variant
int thoxa_may_is_has(thoxa_may_t* may) {
    return may && may->discriminant == MAY_HAS;
}

// Check if a May is Not variant
int thoxa_may_is_not(thoxa_may_t* may) {
    return may && may->discriminant == MAY_NOT;
}

// Get the value from a Has variant (returns NULL for Not)
void* thoxa_may_get_value(thoxa_may_t* may) {
    if (may && may->discriminant == MAY_HAS) {
        return may->value;
    }
    return NULL;
}