// Runtime type system for Thoxa
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

// Maximum number of types we can register
#define MAX_TYPES 1024

// Type flags
#define TYPE_FLAG_HEAP_ALLOCATED (1 << 0)
#define TYPE_FLAG_HAS_FIELDS     (1 << 1)
#define TYPE_FLAG_GENERIC        (1 << 2)

// Type information structure
typedef struct TypeInfo {
    uint32_t id;           // Type ID (matches TypeId in Rust)
    const char* name;      // Human-readable name
    size_t size;           // Size in bytes (for heap-allocated types)
    uint32_t flags;        // Type flags
    uint32_t field_count;  // Number of fields (for units)
    struct TypeInfo** field_types; // Types of fields (for units)
    const char** field_names;      // Names of fields (for units)
} TypeInfo;

// Global type table
static TypeInfo* type_table[MAX_TYPES] = {0};
static uint32_t next_type_id = 100; // Start after built-in types

// Initialize the type system with built-in types
void thoxa_init_types() {
    // Register built-in types
    // Signed integers
    type_table[0] = malloc(sizeof(TypeInfo));
    *type_table[0] = (TypeInfo){0, "Int8", 1, 0, 0, NULL, NULL};
    
    type_table[1] = malloc(sizeof(TypeInfo));
    *type_table[1] = (TypeInfo){1, "Int16", 2, 0, 0, NULL, NULL};
    
    type_table[2] = malloc(sizeof(TypeInfo));
    *type_table[2] = (TypeInfo){2, "Int32", 4, 0, 0, NULL, NULL};
    
    type_table[3] = malloc(sizeof(TypeInfo));
    *type_table[3] = (TypeInfo){3, "Int64", 8, 0, 0, NULL, NULL};
    
    // Unsigned integers
    type_table[4] = malloc(sizeof(TypeInfo));
    *type_table[4] = (TypeInfo){4, "UInt8", 1, 0, 0, NULL, NULL};
    
    type_table[5] = malloc(sizeof(TypeInfo));
    *type_table[5] = (TypeInfo){5, "UInt16", 2, 0, 0, NULL, NULL};
    
    type_table[6] = malloc(sizeof(TypeInfo));
    *type_table[6] = (TypeInfo){6, "UInt32", 4, 0, 0, NULL, NULL};
    
    type_table[7] = malloc(sizeof(TypeInfo));
    *type_table[7] = (TypeInfo){7, "UInt64", 8, 0, 0, NULL, NULL};
    
    // Floating point
    type_table[8] = malloc(sizeof(TypeInfo));
    *type_table[8] = (TypeInfo){8, "Float32", 4, 0, 0, NULL, NULL};
    
    type_table[9] = malloc(sizeof(TypeInfo));
    *type_table[9] = (TypeInfo){9, "Float64", 8, 0, 0, NULL, NULL};
    
    // Other primitives
    type_table[10] = malloc(sizeof(TypeInfo));
    *type_table[10] = (TypeInfo){10, "Bool", 4, 0, 0, NULL, NULL}; // Word-sized in QBE
    
    type_table[11] = malloc(sizeof(TypeInfo));
    *type_table[11] = (TypeInfo){11, "Char", 4, 0, 0, NULL, NULL}; // 32-bit Unicode
    
    type_table[12] = malloc(sizeof(TypeInfo));
    *type_table[12] = (TypeInfo){12, "String", 8, TYPE_FLAG_HEAP_ALLOCATED, 0, NULL, NULL};
    
    type_table[13] = malloc(sizeof(TypeInfo));
    *type_table[13] = (TypeInfo){13, "Void", 0, 0, 0, NULL, NULL};
    
    type_table[14] = malloc(sizeof(TypeInfo));
    *type_table[14] = (TypeInfo){14, "Pointer", 8, TYPE_FLAG_HEAP_ALLOCATED, 0, NULL, NULL};
    
    type_table[15] = malloc(sizeof(TypeInfo));
    *type_table[15] = (TypeInfo){15, "Trial", 8, TYPE_FLAG_HEAP_ALLOCATED | TYPE_FLAG_GENERIC, 0, NULL, NULL};
}

// Register a new unit type
uint32_t thoxa_register_type(const char* name, uint32_t field_count, 
                             uint32_t* field_type_ids, const char** field_names) {
    if (next_type_id >= MAX_TYPES) {
        fprintf(stderr, "Error: Type table full\n");
        exit(1);
    }
    
    uint32_t id = next_type_id++;
    
    TypeInfo* info = malloc(sizeof(TypeInfo));
    info->id = id;
    info->name = strdup(name);
    info->size = 8 + 16 * field_count; // Type ptr + fields (16 bytes each)
    info->flags = TYPE_FLAG_HEAP_ALLOCATED | TYPE_FLAG_HAS_FIELDS;
    info->field_count = field_count;
    
    if (field_count > 0) {
        info->field_types = malloc(sizeof(TypeInfo*) * field_count);
        info->field_names = malloc(sizeof(char*) * field_count);
        
        for (uint32_t i = 0; i < field_count; i++) {
            info->field_types[i] = type_table[field_type_ids[i]];
            info->field_names[i] = strdup(field_names[i]);
        }
    } else {
        info->field_types = NULL;
        info->field_names = NULL;
    }
    
    type_table[id] = info;
    return id;
}

// Get type info by ID
TypeInfo* thoxa_get_type_info(uint32_t type_id) {
    if (type_id >= MAX_TYPES) {
        return NULL;
    }
    return type_table[type_id];
}

// Get type name for debugging
const char* thoxa_type_name(uint32_t type_id) {
    TypeInfo* info = thoxa_get_type_info(type_id);
    if (info) {
        return info->name;
    }
    return "<unknown>";
}

// Runtime type checking
int thoxa_check_type(void* ptr, uint32_t expected_type_id) {
    if (!ptr) {
        return 0; // NULL is never the right type
    }
    
    // Read the type ID from the object
    uint32_t* type_ptr = (uint32_t*)ptr;
    uint32_t actual_type_id = *type_ptr;
    
    return actual_type_id == expected_type_id;
}

// Safe cast with type checking
void* thoxa_cast(void* ptr, uint32_t from_type_id, uint32_t to_type_id) {
    if (!ptr) {
        return NULL;
    }
    
    // For now, just check that the cast is valid
    if (!thoxa_check_type(ptr, from_type_id)) {
        fprintf(stderr, "Type error: Cast from %s to %s failed - object is not of expected source type\n",
                thoxa_type_name(from_type_id), thoxa_type_name(to_type_id));
        exit(1);
    }
    
    // TODO: Check if cast is allowed (e.g., upcast/downcast rules)
    
    return ptr;
}

// Debug: Print type information
void thoxa_print_type(uint32_t type_id) {
    TypeInfo* info = thoxa_get_type_info(type_id);
    if (!info) {
        printf("Type %u: <not found>\n", type_id);
        return;
    }
    
    printf("Type %u: %s (size=%zu, flags=0x%x)\n", 
           info->id, info->name, info->size, info->flags);
    
    if (info->field_count > 0) {
        printf("  Fields:\n");
        for (uint32_t i = 0; i < info->field_count; i++) {
            printf("    %s: %s\n", info->field_names[i], 
                   info->field_types[i] ? info->field_types[i]->name : "<unknown>");
        }
    }
}

// Debug: Print all registered types
void thoxa_print_all_types() {
    printf("=== Registered Types ===\n");
    for (int i = 0; i < MAX_TYPES; i++) {
        if (type_table[i]) {
            thoxa_print_type(i);
        }
    }
    printf("========================\n");
}