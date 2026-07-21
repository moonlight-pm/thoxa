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
#define THOXA_TAG_BUFFER 3
#define THOXA_TAG_ARRAY  4

/* Element slots: heap pointers (String/Bytes) bitcast into 8-byte slots. */
#define THOXA_ARRAY_FLAG_HEAP_ELEMS 1u
/* Optional callback for User-inline arrays (field-level RC). */
typedef void (*ThoxaArrayElemFn)(void *elem);

/* --- I/O ---------------------------------------------------------------- */

void thoxa_print(const char *s);
void thoxa_println(const char *s);
void thoxa_print_i64(int64_t v);
void thoxa_println_i64(int64_t v);
void thoxa_print_bool(bool v);
void thoxa_println_bool(bool v);
void thoxa_panic(const char *msg);

/* Thread-local errno after a failed libc call (no named `ffi` root — D80). */
int32_t thoxa_errno(void);

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
/* Structural equality (length + bytes). NULL equals empty. */
bool thoxa_string_eq(const ThoxaString *a, const ThoxaString *b);
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
/* Owned zero-filled Bytes of length `len` (`Bytes(n)`). Negative len → panic. */
ThoxaBytes *thoxa_bytes_new(int64_t len);
size_t thoxa_bytes_count(const ThoxaBytes *b);
/* Mutable data pointer for FFI (NULL if b is NULL). */
uint8_t *thoxa_bytes_data(ThoxaBytes *b);
/* Byte as Int64 (0–255). Index OOB → panic. */
int64_t thoxa_bytes_get(const ThoxaBytes *b, int64_t i);
/* Masks `byte` to 0–255. Index OOB → panic. */
void thoxa_bytes_set(ThoxaBytes *b, int64_t i, int64_t byte);
/* Half-open slice [start, end) → owned Bytes. OOB / start>end → panic. */
ThoxaBytes *thoxa_bytes_slice(const ThoxaBytes *b, int64_t start, int64_t end);
/* First index of `needle` in `hay` at or after `from`, or -1 if not found. */
int64_t thoxa_bytes_find(const ThoxaBytes *hay, const ThoxaBytes *needle, int64_t from);
/* Fresh owned String with the same bytes (not necessarily valid UTF-8). */
ThoxaString *thoxa_bytes_to_string(const ThoxaBytes *b);

/* --- Buffer (growable, COW, RC heap) ------------------------------------ */

/*
 * Layout: [ThoxaHeader][ThoxaBuffer { len, cap, data[cap] }]
 * A ThoxaBuffer* points at the payload (after the header).
 * Geometric growth: empty cap=0; first growth → 8; then doubles.
 * Mutations (append / appendBytes) are copy-on-write when refcount > 1 and
 * return the (possibly new) unique buffer — assign the result back to the place.
 */
typedef struct ThoxaBuffer {
    size_t len;
    size_t cap;
    uint8_t data[];
} ThoxaBuffer;

/* Empty buffer (count=0, capacity=0). */
ThoxaBuffer *thoxa_buffer_new(void);
/* Pre-reserved capacity (still count=0). Negative → panic. */
ThoxaBuffer *thoxa_buffer_with_capacity(int64_t capacity);
int64_t thoxa_buffer_count(const ThoxaBuffer *b);
int64_t thoxa_buffer_capacity(const ThoxaBuffer *b);
/* COW + append one byte (masked to 0–255). Returns owned unique buffer. */
ThoxaBuffer *thoxa_buffer_append(ThoxaBuffer *b, int64_t byte);
/* COW + append all bytes from `src`. Returns owned unique buffer. */
ThoxaBuffer *thoxa_buffer_append_bytes(ThoxaBuffer *b, const ThoxaBytes *src);
/* Copy live prefix → owned fixed Bytes (buffer left intact). */
ThoxaBytes *thoxa_buffer_freeze(const ThoxaBuffer *b);

/* --- Array (dynamic, RC heap; functional append/dropLast) ---------------- */

/*
 * Layout: [ThoxaHeader][ThoxaArray { len, cap, flags, elem_size, data[cap*elem_size] }]
 * A ThoxaArray* points at the payload (after the header).
 *
 * D65: variable element size. Int64/Bool/String/Bytes use elem_size=8 with
 * pointers bitcast into slots; HEAP_ELEMS walks slots with thoxa_dup/drop.
 * User structs store inline (elem_size = sizeof(C struct)); cgen supplies
 * ThoxaArrayElemFn for field-level RC on copy/get/set/drop.
 *
 * Sharing is refcounted on the array object; index set mutates shared storage
 * (oracle list semantics). append/dropLast always allocate a new array.
 */
typedef struct ThoxaArray {
    size_t len;
    size_t cap;
    uint32_t flags;
    uint32_t elem_size;
    uint8_t data[];
} ThoxaArray;

/* Empty array. `heap_elems` true when slots hold String/Bytes pointers. */
ThoxaArray *thoxa_array_new(int heap_elems);
/* Empty array with explicit element size (User / generic). flags e.g. 0. */
ThoxaArray *thoxa_array_new_ex(uint32_t elem_size, uint32_t flags);
/* Owned array of `len` int64 slots (copied from `items`; NULL ok when len=0). */
ThoxaArray *thoxa_array_from_i64(const int64_t *items, size_t len);
/* Owned array of `len` heap pointers (each is dup'd into the array). */
ThoxaArray *thoxa_array_from_ptrs(void *const *items, size_t len);
/*
 * Owned array from raw element bytes (len * elem_size). Shallow-copies then
 * calls `elem_dup` on each slot when non-NULL (User field RC).
 */
ThoxaArray *thoxa_array_from_elems(
    const void *items,
    size_t len,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup
);
int64_t thoxa_array_count(const ThoxaArray *a);
/* Index get as Int64 (also used for heap pointers bitcast). OOB → panic. */
int64_t thoxa_array_get_i64(const ThoxaArray *a, int64_t i);
/* Index set Int64 slot. OOB → panic. */
void thoxa_array_set_i64(ThoxaArray *a, int64_t i, int64_t v);
/* Index get heap pointer. OOB → panic. Does not dup. */
void *thoxa_array_get_ptr(const ThoxaArray *a, int64_t i);
/* Index set heap pointer: dups `p`, drops old slot. */
void thoxa_array_set_ptr(ThoxaArray *a, int64_t i, void *p);
/* Typed element get: memcpy into `out` (elem_size bytes), optional field-dup. */
void thoxa_array_get_elem(
    const ThoxaArray *a,
    int64_t i,
    void *out,
    ThoxaArrayElemFn elem_dup
);
/* Typed element set: drop old, memcpy new, optional field-dup of new. */
void thoxa_array_set_elem(
    ThoxaArray *a,
    int64_t i,
    const void *elem,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* Functional: new array with `v` appended (int64 or bitcast ptr). */
ThoxaArray *thoxa_array_append_i64(const ThoxaArray *a, int64_t v);
/* Functional: new array with heap ptr appended (dups elems + dups `p`). */
ThoxaArray *thoxa_array_append_ptr(const ThoxaArray *a, void *p);
/*
 * Functional append of a raw element. Shallow-copies prior elems + new, then
 * `elem_dup` each slot of the result when non-NULL.
 * When `a` is non-NULL, elem_size/flags are taken from `a`.
 */
ThoxaArray *thoxa_array_append_elem(
    const ThoxaArray *a,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup
);
/* Functional: new array without last element. Empty → panic. */
ThoxaArray *thoxa_array_drop_last(const ThoxaArray *a);
/* Like drop_last but field-dups surviving elements (User arrays). */
ThoxaArray *thoxa_array_drop_last_ex(const ThoxaArray *a, ThoxaArrayElemFn elem_dup);
/* Display as `[a, b, c]` for Int64 arrays. */
ThoxaString *thoxa_array_i64_to_string(const ThoxaArray *a);
/* Display as `["a", "b"]` for String arrays. */
ThoxaString *thoxa_array_string_to_string(const ThoxaArray *a);
/* Drop with element RC walk when heap_elems. Prefer over bare thoxa_drop. */
void thoxa_array_drop(ThoxaArray *a);
/* Drop calling `elem_drop` on each element first (User inline arrays). */
void thoxa_array_drop_ex(ThoxaArray *a, ThoxaArrayElemFn elem_drop);
/* Share: bump array RC only (elements stay with the shared object). */
void thoxa_array_dup(ThoxaArray *a);

#ifdef __cplusplus
}
#endif

#endif /* THOXA_RT_H */
