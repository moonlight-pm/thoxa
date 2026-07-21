// Unified boxing system for Thoxa value types
// This replaces the ad-hoc boxing functions with a type-aware system

#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <stdio.h>
#include "thoxa_runtime.h"

// Box type tag - identifies boxed values
#define TYPE_BOX 100

// Boxed value structure - uses our standard memory management
typedef struct {
    // Standard header is added by thoxa_alloc
    uint16_t value_type;    // Type ID of the boxed value
    uint16_t padding;       // Padding for alignment
    uint32_t size;          // Size of the value
    union {
        int64_t  i64;
        int32_t  i32;
        int16_t  i16;
        int8_t   i8;
        uint64_t u64;
        uint32_t u32;
        uint16_t u16;
        uint8_t  u8;
        float    f32;
        double   f64;
        void*    ptr;
        char     bytes[8];  // For small values
    } value;
} thoxa_box_t;

// Create a boxed value with proper reference counting
void* thoxa_box_create(uint16_t type_id, void* value, size_t size) {
    // Allocate box with our memory management
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_create: out of memory\n");
        exit(1);
    }
    
    // Initialize box metadata
    box->value_type = type_id;
    box->padding = 0;
    box->size = size;
    
    // Copy value based on size
    if (size <= 8 && value) {
        // Small value - store inline
        memcpy(&box->value, value, size);
    } else if (value) {
        // Large value - would need separate allocation
        // For now, we only handle <= 8 byte values
        fprintf(stderr, "thoxa_box_create: values > 8 bytes not yet supported\n");
        exit(1);
    }
    
    return box;
}

// Type-specific boxing functions that use the unified system

void* thoxa_box_int64_unified(int64_t value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_int64: out of memory\n");
        exit(1);
    }
    
    box->value_type = 1;  // TYPE_INT64
    box->padding = 0;
    box->size = sizeof(int64_t);
    box->value.i64 = value;
    
    return box;
}

void* thoxa_box_int32_unified(int32_t value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_int32: out of memory\n");
        exit(1);
    }
    
    box->value_type = 2;  // TYPE_INT32
    box->padding = 0;
    box->size = sizeof(int32_t);
    box->value.i32 = value;
    
    return box;
}

void* thoxa_box_int16_unified(int16_t value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_int16: out of memory\n");
        exit(1);
    }
    
    box->value_type = 3;  // TYPE_INT16
    box->padding = 0;
    box->size = sizeof(int16_t);
    box->value.i16 = value;
    
    return box;
}

void* thoxa_box_int8_unified(int8_t value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_int8: out of memory\n");
        exit(1);
    }
    
    box->value_type = 4;  // TYPE_INT8
    box->padding = 0;
    box->size = sizeof(int8_t);
    box->value.i8 = value;
    
    return box;
}

void* thoxa_box_bool_unified(int64_t value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_bool: out of memory\n");
        exit(1);
    }
    
    box->value_type = 10;  // TYPE_BOOL
    box->padding = 0;
    box->size = sizeof(uint8_t);
    box->value.u8 = value ? 1 : 0;
    
    return box;
}

void* thoxa_box_float64_unified(double value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_float64: out of memory\n");
        exit(1);
    }
    
    box->value_type = 8;  // TYPE_FLOAT64
    box->padding = 0;
    box->size = sizeof(double);
    box->value.f64 = value;
    
    return box;
}

void* thoxa_box_float32_unified(float value) {
    thoxa_box_t* box = (thoxa_box_t*)thoxa_alloc(
        sizeof(thoxa_box_t),
        TYPE_BOX
    );
    
    if (!box) {
        fprintf(stderr, "thoxa_box_float32: out of memory\n");
        exit(1);
    }
    
    box->value_type = 9;  // TYPE_FLOAT32
    box->padding = 0;
    box->size = sizeof(float);
    box->value.f32 = value;
    
    return box;
}

// Unboxing functions - extract value with type checking

int64_t thoxa_unbox_int64_unified(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    // Type check
    if (box->value_type != 1) {  // TYPE_INT64
        fprintf(stderr, "thoxa_unbox_int64: type mismatch (expected 1, got %d)\n", 
                box->value_type);
        return 0;
    }
    
    return box->value.i64;
}

int32_t thoxa_unbox_int32_unified(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 2) {  // TYPE_INT32
        fprintf(stderr, "thoxa_unbox_int32: type mismatch\n");
        return 0;
    }
    
    return box->value.i32;
}

int16_t thoxa_unbox_int16_unified(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 3) {  // TYPE_INT16
        fprintf(stderr, "thoxa_unbox_int16: type mismatch\n");
        return 0;
    }
    
    return box->value.i16;
}

int8_t thoxa_unbox_int8_unified(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 4) {  // TYPE_INT8
        fprintf(stderr, "thoxa_unbox_int8: type mismatch\n");
        return 0;
    }
    
    return box->value.i8;
}

uint8_t thoxa_unbox_bool_unified(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 10) {  // TYPE_BOOL
        fprintf(stderr, "thoxa_unbox_bool: type mismatch\n");
        return 0;
    }
    
    return box->value.u8;
}

double thoxa_unbox_float64_unified(void* box_ptr) {
    if (!box_ptr) return 0.0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 8) {  // TYPE_FLOAT64
        fprintf(stderr, "thoxa_unbox_float64: type mismatch\n");
        return 0.0;
    }
    
    return box->value.f64;
}

float thoxa_unbox_float32_unified(void* box_ptr) {
    if (!box_ptr) return 0.0f;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    
    if (box->value_type != 9) {  // TYPE_FLOAT32
        fprintf(stderr, "thoxa_unbox_float32: type mismatch\n");
        return 0.0f;
    }
    
    return box->value.f32;
}

// Get the type of a boxed value
uint16_t thoxa_box_get_type(void* box_ptr) {
    if (!box_ptr) return 0;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    return box->value_type;
}

// Generic unbox that returns a pointer to the value
void* thoxa_box_get_value_ptr(void* box_ptr) {
    if (!box_ptr) return NULL;
    
    thoxa_box_t* box = (thoxa_box_t*)box_ptr;
    return &box->value;
}