#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

// Type tags from thoxa_memory.c
extern void* thoxa_alloc(size_t size, uint16_t type_tag);
extern void thoxa_incref(void* ptr);
extern void thoxa_decref(void* ptr);

// Trial type tag
#define TYPE_TRIAL 50

// Trial discriminants
#define TRIAL_PASS 0
#define TRIAL_FAIL 1

// Trial structure (follows our discriminated union pattern)
typedef struct {
    // Standard 16-byte header is added by thoxa_alloc
    uint32_t discriminant;  // Which variant: Pass(0) or Fail(1)
    void* value;           // The payload (heap pointer)
} thoxa_trial_t;

// Create a Pass variant
thoxa_trial_t* thoxa_trial_pass(void* value) {
    // Allocate Trial structure
    thoxa_trial_t* trial = (thoxa_trial_t*)thoxa_alloc(
        sizeof(thoxa_trial_t),
        TYPE_TRIAL
    );
    
    if (!trial) {
        fprintf(stderr, "thoxa_trial_pass: out of memory\n");
        exit(1);
    }
    
    // Initialize as Pass variant
    trial->discriminant = TRIAL_PASS;
    trial->value = value;
    
    // Don't increment reference count - the value is either:
    // 1. A raw integer (not a heap object)
    // 2. Already referenced by the caller
    // TODO: This needs proper handling once we have boxed integers
    
    return trial;
}

// Create a Fail variant
thoxa_trial_t* thoxa_trial_fail(void* error) {
    // Allocate Trial structure
    thoxa_trial_t* trial = (thoxa_trial_t*)thoxa_alloc(
        sizeof(thoxa_trial_t),
        TYPE_TRIAL
    );
    
    if (!trial) {
        fprintf(stderr, "thoxa_trial_fail: out of memory\n");
        exit(1);
    }
    
    // Initialize as Fail variant
    trial->discriminant = TRIAL_FAIL;
    trial->value = error;
    
    // Don't increment reference count - the value is either:
    // 1. A raw integer (not a heap object)  
    // 2. Already referenced by the caller
    // TODO: This needs proper handling once we have boxed integers
    
    return trial;
}

// Check if Trial is Pass
int thoxa_trial_is_pass(thoxa_trial_t* trial) {
    if (!trial) {
        return 0;
    }
    return trial->discriminant == TRIAL_PASS;
}

// Check if Trial is Fail
int thoxa_trial_is_fail(thoxa_trial_t* trial) {
    if (!trial) {
        return 0;
    }
    return trial->discriminant == TRIAL_FAIL;
}

// Get the value from a Pass variant (returns NULL if Fail)
void* thoxa_trial_get_pass(thoxa_trial_t* trial) {
    if (!trial || trial->discriminant != TRIAL_PASS) {
        return NULL;
    }
    return trial->value;
}

// Get the error from a Fail variant (returns NULL if Pass)
void* thoxa_trial_get_fail(thoxa_trial_t* trial) {
    if (!trial || trial->discriminant != TRIAL_FAIL) {
        return NULL;
    }
    return trial->value;
}

// Unwrap a Pass variant (exits if Fail)
void* thoxa_trial_unwrap(thoxa_trial_t* trial) {
    if (!trial || trial->discriminant != TRIAL_PASS) {
        fprintf(stderr, "Unwrap called on Fail variant\n");
        exit(1);
    }
    return trial->value;
}

// Unwrap a Fail variant (exits if Pass)
void* thoxa_trial_unwrap_err(thoxa_trial_t* trial) {
    if (!trial || trial->discriminant != TRIAL_FAIL) {
        fprintf(stderr, "Unwrap_err called on Pass variant\n");
        exit(1);
    }
    return trial->value;
}

// Clean up Trial (called by memory system)
void thoxa_trial_cleanup(thoxa_trial_t* trial) {
    // Don't decref the value - it's either:
    // 1. A raw integer (not a heap object)
    // 2. Not owned by the Trial
    // TODO: This needs proper handling once we have boxed integers
}