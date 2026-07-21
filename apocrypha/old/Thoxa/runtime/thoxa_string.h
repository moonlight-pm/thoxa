#ifndef THOXA_STRING_H
#define THOXA_STRING_H

#include <stdint.h>
#include <stdbool.h>

// Forward declaration of string type
typedef struct ThoxaString ThoxaString;

// String creation and destruction
ThoxaString* thoxa_string_new(const char* cstr);
ThoxaString* thoxa_string_with_capacity(int64_t capacity);
void thoxa_string_destroy(ThoxaString* str);

// String operations
const char* thoxa_string_cstr(ThoxaString* str);
int64_t thoxa_string_len(ThoxaString* str);
ThoxaString* thoxa_string_clone(ThoxaString* str);

// String concatenation
ThoxaString* thoxa_string_concat(ThoxaString* left, ThoxaString* right);
void thoxa_string_append(ThoxaString* str, ThoxaString* other);
void thoxa_string_append_cstr(ThoxaString* str, const char* cstr);

// String comparison
int thoxa_string_compare(ThoxaString* left, ThoxaString* right);
bool thoxa_string_equals(ThoxaString* left, ThoxaString* right);

// String manipulation
ThoxaString* thoxa_string_substring(ThoxaString* str, int64_t start, int64_t len);
int64_t thoxa_string_find(ThoxaString* haystack, ThoxaString* needle);
char thoxa_string_char_at(ThoxaString* str, int64_t index);

// UTF-8 support
bool thoxa_string_is_valid_utf8(ThoxaString* str);

// Output
void thoxa_string_print(ThoxaString* str);
void thoxa_string_println(ThoxaString* str);

#endif // THOXA_STRING_H