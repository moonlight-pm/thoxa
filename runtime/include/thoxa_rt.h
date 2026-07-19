#ifndef THOXA_RT_H
#define THOXA_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- Type tags ---------------------------------------------------------- */

#define THOXA_TAG_STRING 1
#define THOXA_TAG_BYTES  2

/* --- I/O ---------------------------------------------------------------- */

void thoxa_print(const char *s);
void thoxa_println(const char *s);
void thoxa_print_i64(int64_t v);
void thoxa_println_i64(int64_t v);
void thoxa_print_bool(bool v);
void thoxa_println_bool(bool v);
void thoxa_panic(const char *msg);

/* --- Heap header (RC) --------------------------------------------------- */

typedef struct ThoxaHeader {
    size_t refcount;
    uint16_t type_tag;
    uint16_t flags;
    uint32_t reserved;
} ThoxaHeader;

void *thoxa_alloc(size_t payload_size, uint16_t type_tag);
void thoxa_dup(void *ptr);
void thoxa_drop(void *ptr);

/* --- String (immutable UTF-8, RC heap) ---------------------------------- */

/*
 * Layout: [ThoxaHeader][ThoxaString { len, data[len+1] }]
 * A ThoxaString* points at the payload (after the header).
 */
typedef struct ThoxaString {
    size_t len;
    char data[];
} ThoxaString;

ThoxaString *thoxa_string_new(const char *bytes, size_t len);
ThoxaString *thoxa_string_from_cstr(const char *s);
ThoxaString *thoxa_string_from_i64(int64_t v);
ThoxaString *thoxa_string_from_bool(bool v);
/* Borrows a and b; returns a new owned string (refcount 1). */
ThoxaString *thoxa_string_concat(const ThoxaString *a, const ThoxaString *b);
const char *thoxa_string_cstr(const ThoxaString *s);
size_t thoxa_string_len(const ThoxaString *s);

void thoxa_print_string(const ThoxaString *s);
void thoxa_println_string(const ThoxaString *s);

/* --- Bytes (fixed mutable buffer, RC heap) ------------------------------ */

/*
 * Layout: [ThoxaHeader][ThoxaBytes { len, data[len] }]
 * A ThoxaBytes* points at the payload (after the header).
 * Fixed length after construction; contents are mutable (FFI / set).
 */
typedef struct ThoxaBytes {
    size_t len;
    uint8_t data[];
} ThoxaBytes;

/* Owned Bytes (refcount 1) from a String's UTF-8 bytes (no trailing NUL in payload). */
ThoxaBytes *thoxa_bytes_from_string(const ThoxaString *s);
size_t thoxa_bytes_count(const ThoxaBytes *b);
/* Byte as Int64 (0–255). Index OOB → panic. */
int64_t thoxa_bytes_get(const ThoxaBytes *b, int64_t i);
/* Masks `byte` to 0–255. Index OOB → panic. */
void thoxa_bytes_set(ThoxaBytes *b, int64_t i, int64_t byte);
/* Fresh owned String with the same bytes (not necessarily valid UTF-8). */
ThoxaString *thoxa_bytes_to_string(const ThoxaBytes *b);

#ifdef __cplusplus
}
#endif

#endif /* THOXA_RT_H */
