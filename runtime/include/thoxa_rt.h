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
#define THOXA_TAG_HTML   5

/* Element slots: heap pointers (String/Bytes) bitcast into 8-byte slots. */
#define THOXA_ARRAY_FLAG_HEAP_ELEMS 1u
/* Optional callback for User-inline arrays (field-level RC). */
typedef void (*ThoxaArrayElemFn)(void *elem);

/* --- I/O ---------------------------------------------------------------- */

void thoxa_panic(const char *msg);

/* Thread-local errno after a failed libc call (no named `ffi` root — D80). */
int32_t thoxa_errno(void);

/*
 * fork + execv + waitpid for dogfood shells (thoxash).
 * `argv_blob` is consecutive NUL-terminated strings (total `argv_len` bytes,
 * each string including its trailing NUL). Used because Array is not yet
 * FFI-marshalable. Returns:
 *   0..255     — child exit status (WEXITSTATUS)
 *   128+sig    — child killed by signal
 *   -1         — fork failed (errno set; readable via thoxa_errno)
 * Exec failure in the child exits 127 (shell convention).
 */
int64_t thoxa_spawn_wait(const char *path, const void *argv_blob, int64_t argv_len);

/*
 * Like thoxa_spawn_wait with optional file redirections (session `>` / `>>` / `<`).
 * stdin_path / stdout_path: NULL or empty string → inherit that fd.
 * stdout_append: 0 = O_TRUNC ( `>` ), non-zero = O_APPEND ( `>>` ).
 * Open failures in the child yield exit 127 (same as exec failure).
 */
int64_t thoxa_spawn_wait_io(
    const char *path,
    const void *argv_blob,
    int64_t argv_len,
    const char *stdin_path,
    const char *stdout_path,
    int64_t stdout_append
);

/*
 * OS pipeline: stages separated by an empty NUL-terminated field (double NUL).
 * Within a stage, consecutive NUL-terminated words form argv (like spawn_wait).
 * Example for `echo hi | wc -c`:
 *   "echo\0hi\0\0wc\0-c\0"
 * First word of each stage is the executable (PATH search via execvp).
 * First stage inherits stdin; last inherits stdout; intermediate stages are
 * connected with pipe(2). stderr inherits for all. Returns last stage status
 * (shell convention), or -1 on setup failure (errno set).
 */
int64_t thoxa_pipeline(const void *stages_blob, int64_t stages_len);

/*
 * Session / REPL helpers (plan PR3+, D93 v2).
 *
 * Host-held shell state for in-process `step` shared objects.
 * Layout is fixed: one Int64 status (matches `type Shell(status: Int64)` field
 * packing for the status slot). Generated session libs export:
 *
 *   int64_t thoxa_session_step(ThoxaShellState *state);
 *
 * which reads `state->status`, runs `Shell.step`, writes back status, and
 * returns it. Optional `void thoxa_session_init(void)` runs module-level inits
 * (idempotent per loaded image).
 */
typedef struct ThoxaShellState {
    int64_t status;
} ThoxaShellState;

/* pipe(2) wrapper — low-level; prefer thoxa_pipeline for session pipelines. */
int32_t thoxa_pipe(int32_t fds_out[2]);

/*
 * Cooperative tasks (experiment/implicit-async). One OS thread; `start` enqueues
 * a thunk; `thoxa_wait_ms` parks the current task; `thoxa_sched_drain` runs
 * until idle. Generated `main` drains after `thoxa_fn_main`.
 */
void thoxa_task_start(void (*fn)(void *), void *data);
void *thoxa_task_alloc(size_t nbytes);
void thoxa_task_free(void *ptr);
void thoxa_wait_ms(int64_t ms);
#define THOXA_WAIT_READ  1
#define THOXA_WAIT_WRITE 2
void thoxa_wait_fd(int32_t fd, int32_t events);
int32_t thoxa_accept(int32_t fd, void *addr, void *addrlen);
int64_t thoxa_recv(int32_t fd, void *buf, uint64_t len, int32_t flags);
int64_t thoxa_send(int32_t fd, const void *buf, uint64_t len, int32_t flags);
int32_t thoxa_connect(int32_t fd, const void *addr, uint32_t addrlen);
void thoxa_sched_drain(void);

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

/* Best-effort write of C string bytes + '\n' to fd (1/2/file). Returns 0 / -1.
 * Takes const char* so Thoxa `extern … from "c"` String args marshal cleanly. */
int32_t thoxa_write_line_fd(int32_t fd, const char *s);

/* Session prompt helpers (D96): return cstr; Thoxa extern -> String
 * copies via thoxa_string_from_cstr. Buffers are thread-local. */
const char *thoxa_local_hms(void); /* local "HH:MM:SS" */
const char *thoxa_hostname(void);  /* gethostname(2), or "localhost" */

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
/* Content equality: same length and same bytes (embedded-NUL safe). */
int thoxa_bytes_equals(const ThoxaBytes *a, const ThoxaBytes *b);
/* Functional: new Bytes with `byte` (masked 0–255) inserted at `at`.
 * Bounds: 0 ≤ at ≤ count (at == count appends). Source unchanged. */
ThoxaBytes *thoxa_bytes_insert_at(const ThoxaBytes *b, int64_t at, int64_t byte);
/* Functional: new Bytes without the byte at `at`.
 * Bounds: 0 ≤ at < count. Source unchanged. */
ThoxaBytes *thoxa_bytes_remove_at(const ThoxaBytes *b, int64_t at);

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

/* --- Array (dynamic, RC heap; functional append/dropLast/insertAt/removeAt) */

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
/* Functional insertAt: 0 ≤ at ≤ count (at == count appends). */
ThoxaArray *thoxa_array_insert_at_i64(const ThoxaArray *a, int64_t at, int64_t v);
ThoxaArray *thoxa_array_insert_at_ptr(const ThoxaArray *a, int64_t at, void *p);
ThoxaArray *thoxa_array_insert_at_elem(
    const ThoxaArray *a,
    int64_t at,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup
);
/* Functional removeAt: 0 ≤ at < count. */
ThoxaArray *thoxa_array_remove_at(const ThoxaArray *a, int64_t at);
ThoxaArray *thoxa_array_remove_at_ex(const ThoxaArray *a, int64_t at, ThoxaArrayElemFn elem_dup);
/* Build argv as Array<String> for `fn main(arguments: Array<String>)`. */
ThoxaArray *thoxa_argv_to_array(int argc, char **argv);
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

/* --- HTML tree (builtin `Node`, JSX runtime) ---------------------------- */

#define THOXA_HTML_EL 0
#define THOXA_HTML_TEXT 1
#define THOXA_HTML_FRAG 2

typedef struct ThoxaHtml {
    uint8_t kind;
    ThoxaString *tag;
    ThoxaString *text;
    ThoxaArray *attr_names;
    ThoxaArray *attr_vals;
    ThoxaArray *kids;
} ThoxaHtml;

ThoxaHtml *thoxa_html_el(
    ThoxaString *tag,
    ThoxaArray *attr_names,
    ThoxaArray *attr_vals,
    ThoxaArray *kids
);
/* Escapes `s` as text. */
ThoxaHtml *thoxa_html_text(ThoxaString *s);
ThoxaHtml *thoxa_html_frag(ThoxaArray *kids);
ThoxaString *thoxa_html_render(const ThoxaHtml *n);

#ifdef __cplusplus
}
#endif

#endif /* THOXA_RT_H */
