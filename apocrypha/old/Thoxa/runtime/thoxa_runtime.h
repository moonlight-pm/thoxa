#ifndef THOXA_RUNTIME_H
#define THOXA_RUNTIME_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

// Memory management
void* thoxa_alloc(int64_t size, int64_t type_id);
void thoxa_incref(void* ptr);
void thoxa_decref(void* ptr);
void thoxa_retain(void* ptr);
void thoxa_release(void* ptr);

// Basic I/O operations
void print(const char* str);
void println(const char* str);
void printInt(int64_t val);
void printFloat(double val);
void printBool(int val);

// String type (forward declaration)
typedef struct ThoxaString ThoxaString;

// String operations
ThoxaString* thoxa_string_new(const char* cstr);
ThoxaString* thoxa_string_concat(ThoxaString* left, ThoxaString* right);
bool thoxa_string_equals(ThoxaString* left, ThoxaString* right);
int thoxa_string_compare(ThoxaString* left, ThoxaString* right);
int64_t thoxa_string_len(ThoxaString* str);
void thoxa_string_print(ThoxaString* str);
void thoxa_string_println(ThoxaString* str);

// Type system
void thoxa_init_types();
uint32_t thoxa_register_type(const char* name, uint32_t field_count, 
                             uint32_t* field_type_ids, const char** field_names);
const char* thoxa_type_name(uint32_t type_id);
int thoxa_check_type(void* ptr, uint32_t expected_type_id);
void* thoxa_cast(void* ptr, uint32_t from_type_id, uint32_t to_type_id);
void thoxa_print_type(uint32_t type_id);
void thoxa_print_all_types();

// Panic/error handling
void thoxa_panic(const char* msg);

#endif // THOXA_RUNTIME_H