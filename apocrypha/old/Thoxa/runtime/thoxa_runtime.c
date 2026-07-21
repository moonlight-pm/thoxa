// Thoxa Runtime Library
// Provides built-in functions for I/O and other operations

#include <stdio.h>
#include <stdint.h>

// Print a null-terminated string
void print(const char* str) {
    printf("%s", str);
}

// Print a null-terminated string followed by a newline
void println(const char* str) {
    printf("%s\n", str);
}

// Print an integer followed by a newline
void printInt(int64_t value) {
    printf("%lld\n", (long long)value);
}

// Print a character
void printChar(uint32_t ch) {
    if (ch <= 0x7F) {
        putchar((char)ch);
    } else {
        // For now, just print as Unicode codepoint
        printf("U+%04X", ch);
    }
}