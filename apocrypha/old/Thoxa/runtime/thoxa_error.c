// Error handling and panic infrastructure for Thoxa
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdarg.h>
#include <execinfo.h>
#include <unistd.h>
#include <signal.h>
#include <limits.h>

// Configuration
#define MAX_STACK_FRAMES 50
#define MAX_PANIC_MSG_LEN 1024

// Global state for error handling
static bool panic_in_progress = false;
static bool debug_mode = false;

// Function to check if we're in debug mode
static void init_debug_mode() {
    static bool initialized = false;
    if (!initialized) {
        const char* debug_env = getenv("THOXA_DEBUG");
        debug_mode = (debug_env && strcmp(debug_env, "1") == 0);
        initialized = true;
    }
}

// Print a stack trace
void thoxa_print_stack_trace() {
    void* frames[MAX_STACK_FRAMES];
    int frame_count = backtrace(frames, MAX_STACK_FRAMES);
    
    fprintf(stderr, "\n=== Stack Trace ===\n");
    
    // Get symbol names
    char** symbols = backtrace_symbols(frames, frame_count);
    if (symbols) {
        for (int i = 0; i < frame_count; i++) {
            fprintf(stderr, "  [%2d] %s\n", i, symbols[i]);
        }
        free(symbols);
    } else {
        fprintf(stderr, "  (Unable to get symbol names)\n");
    }
    
    fprintf(stderr, "==================\n\n");
}

// Main panic function
void thoxa_panic(const char* msg) {
    // Prevent recursive panics
    if (panic_in_progress) {
        fprintf(stderr, "\n!!! DOUBLE PANIC !!!\n");
        abort();
    }
    panic_in_progress = true;
    
    // Print panic message
    fprintf(stderr, "\n");
    fprintf(stderr, "╔════════════════════════════════════════╗\n");
    fprintf(stderr, "║           THOXA PANIC                   ║\n");
    fprintf(stderr, "╚════════════════════════════════════════╝\n");
    fprintf(stderr, "\n");
    
    if (msg) {
        fprintf(stderr, "Message: %s\n", msg);
    } else {
        fprintf(stderr, "Message: (no message provided)\n");
    }
    
    // Print stack trace
    thoxa_print_stack_trace();
    
    // Exit with error code
    exit(101);
}

// Panic with formatted message
void thoxa_panic_fmt(const char* fmt, ...) {
    char buffer[MAX_PANIC_MSG_LEN];
    va_list args;
    va_start(args, fmt);
    vsnprintf(buffer, sizeof(buffer), fmt, args);
    va_end(args);
    
    thoxa_panic(buffer);
}

// Assert with message
void thoxa_assert(bool condition, const char* msg) {
    if (!condition) {
        thoxa_panic_fmt("Assertion failed: %s", msg ? msg : "no message");
    }
}

// Bounds checking for arrays
void thoxa_check_bounds(int64_t index, int64_t length, const char* array_name) {
    if (index < 0 || index >= length) {
        thoxa_panic_fmt(
            "Index out of bounds: %s[%lld] (length = %lld)",
            array_name ? array_name : "array",
            (long long)index,
            (long long)length
        );
    }
}

// Null pointer check
void thoxa_check_not_null(void* ptr, const char* var_name) {
    if (!ptr) {
        thoxa_panic_fmt(
            "Null pointer dereference: %s",
            var_name ? var_name : "unknown"
        );
    }
}

// Division by zero check
void thoxa_check_div_zero(int64_t divisor) {
    if (divisor == 0) {
        thoxa_panic("Division by zero");
    }
}

// Overflow checking for arithmetic
bool thoxa_add_overflow_i64(int64_t a, int64_t b, int64_t* result) {
    // Check for signed overflow
    if (b > 0 && a > INT64_MAX - b) {
        return true; // Would overflow
    }
    if (b < 0 && a < INT64_MIN - b) {
        return true; // Would underflow
    }
    *result = a + b;
    return false;
}

bool thoxa_mul_overflow_i64(int64_t a, int64_t b, int64_t* result) {
    if (a == 0 || b == 0) {
        *result = 0;
        return false;
    }
    
    // Check for multiplication overflow
    if (a > 0 && b > 0 && a > INT64_MAX / b) {
        return true;
    }
    if (a > 0 && b < 0 && b < INT64_MIN / a) {
        return true;
    }
    if (a < 0 && b > 0 && a < INT64_MIN / b) {
        return true;
    }
    if (a < 0 && b < 0 && a < INT64_MAX / b) {
        return true;
    }
    
    *result = a * b;
    return false;
}

// Checked arithmetic operations
int64_t thoxa_add_checked(int64_t a, int64_t b) {
    int64_t result;
    if (thoxa_add_overflow_i64(a, b, &result)) {
        thoxa_panic_fmt("Integer overflow in addition: %lld + %lld", 
                       (long long)a, (long long)b);
    }
    return result;
}

int64_t thoxa_mul_checked(int64_t a, int64_t b) {
    int64_t result;
    if (thoxa_mul_overflow_i64(a, b, &result)) {
        thoxa_panic_fmt("Integer overflow in multiplication: %lld * %lld",
                       (long long)a, (long long)b);
    }
    return result;
}

int64_t thoxa_div_checked(int64_t a, int64_t b) {
    thoxa_check_div_zero(b);
    
    // Check for INT64_MIN / -1 which would overflow
    if (a == INT64_MIN && b == -1) {
        thoxa_panic("Integer overflow in division: INT64_MIN / -1");
    }
    
    return a / b;
}

// Signal handler for segfaults
static void handle_sigsegv(int sig) {
    (void)sig;
    fprintf(stderr, "\n!!! SEGMENTATION FAULT !!!\n");
    thoxa_print_stack_trace();
    exit(139); // Standard segfault exit code
}

// Signal handler for abort
static void handle_sigabrt(int sig) {
    (void)sig;
    fprintf(stderr, "\n!!! ABORT SIGNAL !!!\n");
    thoxa_print_stack_trace();
    exit(134); // Standard abort exit code
}

// Initialize error handling system
__attribute__((constructor))
static void thoxa_error_init() {
    init_debug_mode();
    
    // Install signal handlers
    signal(SIGSEGV, handle_sigsegv);
    signal(SIGABRT, handle_sigabrt);
    
    if (debug_mode) {
        fprintf(stderr, "[THOXA] Debug mode enabled\n");
    }
}

// Debug logging functions
void thoxa_debug(const char* fmt, ...) {
    if (!debug_mode) return;
    
    fprintf(stderr, "[DEBUG] ");
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
    fprintf(stderr, "\n");
}

// Unreachable code marker
void thoxa_unreachable(const char* msg) {
    thoxa_panic_fmt("Unreachable code reached: %s", 
                   msg ? msg : "no message");
}

// TODO marker that panics in debug mode
void thoxa_todo(const char* msg) {
    if (debug_mode) {
        thoxa_panic_fmt("TODO not implemented: %s",
                       msg ? msg : "no message");
    } else {
        fprintf(stderr, "Warning: TODO: %s\n", msg ? msg : "no message");
    }
}