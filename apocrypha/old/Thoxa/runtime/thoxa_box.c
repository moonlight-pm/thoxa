// Boxing and unboxing for value types
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

// Box an integer value (allocate heap memory and store the value)
void* thoxa_box_int(int64_t value) {
    int64_t* box = (int64_t*)malloc(sizeof(int64_t));
    if (!box) {
        // Out of memory
        return NULL;
    }
    *box = value;
    return box;
}

// Box a 32-bit integer
void* thoxa_box_int32(int32_t value) {
    int32_t* box = (int32_t*)malloc(sizeof(int32_t));
    if (!box) {
        return NULL;
    }
    *box = value;
    return box;
}

// Box a 16-bit integer
void* thoxa_box_int16(int16_t value) {
    int16_t* box = (int16_t*)malloc(sizeof(int16_t));
    if (!box) {
        return NULL;
    }
    *box = value;
    return box;
}

// Box an 8-bit integer
void* thoxa_box_int8(int8_t value) {
    int8_t* box = (int8_t*)malloc(sizeof(int8_t));
    if (!box) {
        return NULL;
    }
    *box = value;
    return box;
}

// Box a boolean (as 1 byte)
void* thoxa_box_bool(int value) {
    uint8_t* box = (uint8_t*)malloc(sizeof(uint8_t));
    if (!box) {
        return NULL;
    }
    *box = value ? 1 : 0;
    return box;
}

// Box a float
void* thoxa_box_float(double value) {
    double* box = (double*)malloc(sizeof(double));
    if (!box) {
        return NULL;
    }
    *box = value;
    return box;
}

// Box a 32-bit float
void* thoxa_box_float32(float value) {
    float* box = (float*)malloc(sizeof(float));
    if (!box) {
        return NULL;
    }
    *box = value;
    return box;
}

// Unbox an integer value (dereference the pointer)
int64_t thoxa_unbox_int(void* box) {
    if (!box) return 0;
    return *(int64_t*)box;
}

// Unbox a 32-bit integer
int32_t thoxa_unbox_int32(void* box) {
    if (!box) return 0;
    return *(int32_t*)box;
}

// Unbox a 16-bit integer
int16_t thoxa_unbox_int16(void* box) {
    if (!box) return 0;
    return *(int16_t*)box;
}

// Unbox an 8-bit integer
int8_t thoxa_unbox_int8(void* box) {
    if (!box) return 0;
    return *(int8_t*)box;
}

// Unbox a boolean
int thoxa_unbox_bool(void* box) {
    if (!box) return 0;
    return *(uint8_t*)box;
}

// Unbox a float
double thoxa_unbox_float(void* box) {
    if (!box) return 0.0;
    return *(double*)box;
}

// Unbox a 32-bit float
float thoxa_unbox_float32(void* box) {
    if (!box) return 0.0f;
    return *(float*)box;
}

// Free a boxed value (just a regular free)
void thoxa_unbox_free(void* box) {
    free(box);
}