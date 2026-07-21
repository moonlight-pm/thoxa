// Runtime string operations for Thoxa - Wrapper around thoxa_memory.c string implementation
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

// Forward declarations for existing string functions in thoxa_memory.c
typedef void thoxa_string_t;
extern thoxa_string_t* thoxa_string_new(const char* cstr);
extern const char* thoxa_string_cstr(thoxa_string_t* str);
extern thoxa_string_t* thoxa_string_concat(thoxa_string_t* a, thoxa_string_t* b);
extern size_t thoxa_string_length(thoxa_string_t* str);

// Use the existing implementation's type
typedef thoxa_string_t ThoxaString;

// Additional wrapper functions

// Get string length (wrapper)
int64_t thoxa_string_len(ThoxaString* str) {
    if (!str) return 0;
    return (int64_t)thoxa_string_length(str);
}

// Clone a string
ThoxaString* thoxa_string_clone(ThoxaString* str) {
    if (!str) return thoxa_string_new("");
    return thoxa_string_new(thoxa_string_cstr(str));
}

// String comparison
int thoxa_string_compare(ThoxaString* left, ThoxaString* right) {
    if (!left && !right) return 0;
    if (!left) return -1;
    if (!right) return 1;
    
    return strcmp(thoxa_string_cstr(left), thoxa_string_cstr(right));
}

// String equality
bool thoxa_string_equals(ThoxaString* left, ThoxaString* right) {
    if (!left && !right) return true;
    if (!left || !right) return false;
    
    const char* left_str = thoxa_string_cstr(left);
    const char* right_str = thoxa_string_cstr(right);
    
    return strcmp(left_str, right_str) == 0;
}

// Substring extraction
ThoxaString* thoxa_string_substring(ThoxaString* str, int64_t start, int64_t len) {
    if (!str || start < 0 || len <= 0) {
        return thoxa_string_new("");
    }
    
    const char* cstr = thoxa_string_cstr(str);
    int64_t str_len = thoxa_string_len(str);
    
    if (start >= str_len) {
        return thoxa_string_new("");
    }
    
    // Clamp length to available characters
    if (start + len > str_len) {
        len = str_len - start;
    }
    
    // Create a temporary buffer for the substring
    char* buffer = (char*)malloc(len + 1);
    if (!buffer) return NULL;
    
    memcpy(buffer, cstr + start, len);
    buffer[len] = '\0';
    
    ThoxaString* result = thoxa_string_new(buffer);
    free(buffer);
    
    return result;
}

// Find substring - returns index or -1
int64_t thoxa_string_find(ThoxaString* haystack, ThoxaString* needle) {
    if (!haystack || !needle) return -1;
    
    const char* haystack_cstr = thoxa_string_cstr(haystack);
    const char* needle_cstr = thoxa_string_cstr(needle);
    
    if (strlen(needle_cstr) == 0) return 0;
    
    char* pos = strstr(haystack_cstr, needle_cstr);
    if (!pos) return -1;
    
    return pos - haystack_cstr;
}

// Character access
char thoxa_string_char_at(ThoxaString* str, int64_t index) {
    if (!str || index < 0) return '\0';
    
    const char* cstr = thoxa_string_cstr(str);
    int64_t len = thoxa_string_len(str);
    
    if (index >= len) return '\0';
    return cstr[index];
}

// UTF-8 validation
bool thoxa_string_is_valid_utf8(ThoxaString* str) {
    if (!str) return true;
    
    const char* cstr = thoxa_string_cstr(str);
    if (!cstr) return true;
    
    const unsigned char* bytes = (const unsigned char*)cstr;
    int64_t len = thoxa_string_len(str);
    int64_t i = 0;
    
    while (i < len) {
        unsigned char c = bytes[i];
        
        // ASCII
        if (c <= 0x7F) {
            i++;
            continue;
        }
        
        // Multi-byte sequence
        int expected_bytes = 0;
        if ((c & 0xE0) == 0xC0) expected_bytes = 2;
        else if ((c & 0xF0) == 0xE0) expected_bytes = 3;
        else if ((c & 0xF8) == 0xF0) expected_bytes = 4;
        else return false; // Invalid start byte
        
        if (i + expected_bytes > len) return false;
        
        // Check continuation bytes
        for (int j = 1; j < expected_bytes; j++) {
            if ((bytes[i + j] & 0xC0) != 0x80) return false;
        }
        
        i += expected_bytes;
    }
    
    return true;
}

// Print string (for debugging)
void thoxa_string_print(ThoxaString* str) {
    if (str) {
        printf("%s", thoxa_string_cstr(str));
    }
}

// Print string with newline
void thoxa_string_println(ThoxaString* str) {
    if (str) {
        printf("%s\n", thoxa_string_cstr(str));
    } else {
        printf("\n");
    }
}

// Type conversion functions for string interpolation

// Convert integer to string
ThoxaString* thoxa_int_to_string(int64_t value) {
    char buffer[32]; // Enough for any int64
    snprintf(buffer, sizeof(buffer), "%lld", value);
    return thoxa_string_new(buffer);
}

// Convert boolean to string  
ThoxaString* thoxa_bool_to_string(int32_t value) {
    return thoxa_string_new(value ? "true" : "false");
}

// Convert float to string
ThoxaString* thoxa_float_to_string(double value) {
    char buffer[64]; // Enough for reasonable precision
    snprintf(buffer, sizeof(buffer), "%g", value);
    return thoxa_string_new(buffer);
}

// Convert char to string
ThoxaString* thoxa_char_to_string(char value) {
    char buffer[2] = {value, '\0'};
    return thoxa_string_new(buffer);
}