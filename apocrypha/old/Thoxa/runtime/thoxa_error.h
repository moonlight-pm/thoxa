#ifndef THOXA_ERROR_H
#define THOXA_ERROR_H

#include <stdint.h>
#include <stdbool.h>
#include <stdarg.h>

// Panic functions
void thoxa_panic(const char* msg);
void thoxa_panic_fmt(const char* fmt, ...);

// Assertions
void thoxa_assert(bool condition, const char* msg);

// Bounds checking
void thoxa_check_bounds(int64_t index, int64_t length, const char* array_name);

// Null checking
void thoxa_check_not_null(void* ptr, const char* var_name);

// Division by zero
void thoxa_check_div_zero(int64_t divisor);

// Checked arithmetic
int64_t thoxa_add_checked(int64_t a, int64_t b);
int64_t thoxa_mul_checked(int64_t a, int64_t b);
int64_t thoxa_div_checked(int64_t a, int64_t b);

// Overflow detection
bool thoxa_add_overflow_i64(int64_t a, int64_t b, int64_t* result);
bool thoxa_mul_overflow_i64(int64_t a, int64_t b, int64_t* result);

// Stack trace
void thoxa_print_stack_trace();

// Debug functions
void thoxa_debug(const char* fmt, ...);

// Unreachable code
void thoxa_unreachable(const char* msg);

// TODO marker
void thoxa_todo(const char* msg);

#endif // THOXA_ERROR_H