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
#define THOXA_TAG_MAP    6
#define THOXA_TAG_USER   7

/* Element slots: heap pointers (String/Bytes) bitcast into 8-byte slots. */
#define THOXA_ARRAY_FLAG_HEAP_ELEMS 1u
/* Optional callback for User-inline arrays (field-level RC). */
typedef void (*ThoxaArrayElemFn)(void *elem);

/* --- I/O ---------------------------------------------------------------- */

/* noreturn lets the C compiler drop the "store after panic" path, so a bounds
 * check is never optimized away as if the out-of-range store were reachable. */
#if defined(__GNUC__) || defined(__clang__) || defined(__TINYC__)
#define THOXA_NORETURN __attribute__((noreturn))
#else
#define THOXA_NORETURN
#endif

THOXA_NORETURN void thoxa_panic(const char *msg);

/* Int64 `/` and `%`: division by zero panics; INT64_MIN / -1 wraps like -fwrapv. */
static inline int64_t thoxa_int_div(int64_t left, int64_t right) {
    if (right == 0) {
        thoxa_panic("division by zero");
    }
    if (right == -1) {
        return (int64_t)(UINT64_C(0) - (uint64_t)left);
    }
    return left / right;
}

/* Fixed-size Array<T, N> subscript: an index outside 0..N-1 panics. */
static inline int64_t thoxa_fixed_index(int64_t index, int64_t len) {
    if (index < 0 || index >= len) {
        thoxa_panic("fixed array index out of bounds");
    }
    return index;
}

static inline int64_t thoxa_int_rem(int64_t left, int64_t right) {
    if (right == 0) {
        thoxa_panic("division by zero");
    }
    if (right == -1) {
        return 0;
    }
    return left % right;
}

/* Int64 `<<` and `>>` (D161): a count of 64 or more shifts every bit out
 * (`<<` gives 0, `>>` gives 0 or -1 by sign); a negative count panics.
 * `<<` shifts the unsigned bits so it wraps like `*` under -fwrapv. */
static inline int64_t thoxa_int_shl(int64_t left, int64_t count) {
    if (count < 0) {
        thoxa_panic("negative shift count");
    }
    if (count >= 64) {
        return 0;
    }
    return (int64_t)((uint64_t)left << count);
}

static inline int64_t thoxa_int_shr(int64_t left, int64_t count) {
    if (count < 0) {
        thoxa_panic("negative shift count");
    }
    if (count >= 64) {
        return left < 0 ? -1 : 0;
    }
    return left >> count;
}

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
 * Session (REPL) only: run one compiled step. A panic or a fault (bad memory
 * access, stack overflow, arithmetic trap) inside `step` returns here instead
 * of ending the shell: `*crashed` is set and the result is 134 for a panic or
 * 128 + signal for a fault. Memory held by the abandoned step is leaked.
 */
int64_t thoxa_session_call(int64_t (*step)(void *), void *state, int32_t *crashed);

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
 * Cooperative tasks (D148). One OS thread; `thoxa_task_start` enqueues
 * a thunk; `thoxa_wait_ms` parks the current task; `thoxa_sched_drain` runs
 * until idle. Generated `main` drains after `thoxa_fn_main`.
 *
 * A panic inside a task ends that task only (R4): its frames are abandoned
 * (what they held leaks), the fds it owns are closed, and the other tasks
 * and `main` keep running. A panic in `main` still aborts.
 */
void thoxa_task_start(void (*fn)(void *), void *data);
void *thoxa_task_alloc(size_t nbytes);
void thoxa_task_free(void *ptr);
/* The running task owns `fd`: closed if the task panics. No-op outside a
 * task. Ownership ends at `thoxa_close(fd)` or when the task returns. */
void thoxa_task_own_fd(int32_t fd);
/* close(2) that also ends task ownership of `fd`. */
int32_t thoxa_close(int32_t fd);
/* Open fds owned by tasks, across all tasks. */
int64_t thoxa_task_owned_fds(void);
void thoxa_wait_ms(int64_t ms);
#define THOXA_WAIT_READ  1
#define THOXA_WAIT_WRITE 2
void thoxa_wait_fd(int32_t fd, int32_t events);
int32_t thoxa_accept(int32_t fd, void *addr, void *addrlen);
int64_t thoxa_recv(int32_t fd, void *buf, uint64_t len, int32_t flags);
int64_t thoxa_send(int32_t fd, const void *buf, uint64_t len, int32_t flags);
/* Monotonic milliseconds, and recv / send that fail with ETIMEDOUT once that
 * clock reaches `deadline_ms` (<= 0 = no deadline). */
int64_t thoxa_now_ms(void);
int64_t thoxa_recv_until(int32_t fd, void *buf, uint64_t len, int32_t flags, int64_t deadline_ms);
int64_t thoxa_send_until(int32_t fd, const void *buf, uint64_t len, int32_t flags, int64_t deadline_ms);
int32_t thoxa_connect(int32_t fd, const void *addr, uint32_t addrlen);
void thoxa_sched_drain(void);

/* --- Heap header (RC) --------------------------------------------------- */

typedef struct ThoxaHeader {
    size_t refcount;
    uint16_t type_tag;
    uint16_t flags;
    uint32_t reserved;
    void (*dtor)(void *payload);
} ThoxaHeader;

/* Panics if `sizeof(ThoxaHeader) + payload_size` wraps. */
void *thoxa_alloc(size_t payload_size, uint16_t type_tag);
void thoxa_set_dtor(void *ptr, void (*dtor)(void *payload));
void thoxa_dup(void *ptr);
void thoxa_drop(void *ptr);

/* --- Function values (D132): unboxed {recv, code} pair ------------------- */

typedef struct ThoxaFnBox {
    uint32_t rc;
    void (*release)(void *payload);
    /* payload follows */
} ThoxaFnBox;

typedef struct ThoxaFn {
    ThoxaFnBox *recv; /* NULL for free / unbound */
    void *code;
} ThoxaFn;

void *thoxa_fn_payload(ThoxaFnBox *box);
ThoxaFn thoxa_fn_free(void *code);
ThoxaFn thoxa_fn_bind(
    const void *payload,
    uint32_t size,
    void (*retain)(void *payload),
    void (*release)(void *payload),
    void *code
);
void thoxa_fn_retain(ThoxaFn *fn);
void thoxa_fn_release(ThoxaFn *fn);

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
ThoxaString *thoxa_string_from_u64(uint64_t v);
ThoxaString *thoxa_string_from_f64(double v);
ThoxaString *thoxa_string_from_bool(bool v);
/* IEEE-754 bitcast helpers (array/map slots store Float64 as i64 bits). */
double thoxa_f64_from_bits(uint64_t bits);
uint64_t thoxa_f64_to_bits(double v);
/* strtod wrapper for std/json float tokens (already syntax-checked). */
double thoxa_f64_from_string(const char *s);
/* 1 when finite (not NaN / ±Inf). */
int32_t thoxa_f64_is_finite(double v);
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
 * copies via thoxa_string_from_cstr. Buffers are static (single-threaded runtime). */
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

/* --- Array (dynamic, RC heap; Swift/Buffer COW writes — D133) ----------- */

/*
 * Layout: [ThoxaHeader][ThoxaArray { len, cap, flags, elem_size, data[cap*elem_size] }]
 * A ThoxaArray* points at the payload (after the header).
 *
 * D65: variable element size. Int64/Bool/String/Bytes use elem_size=8 with
 * pointers bitcast into slots; HEAP_ELEMS walks slots with thoxa_dup/drop.
 * User structs store inline (elem_size = sizeof(C struct)); cgen supplies
 * ThoxaArrayElemFn for field-level RC on copy/get/set/drop.
 *
 * Writes (append / dropLast / insertAt / removeAt / index set) copy-on-write
 * when refcount > 1 and return the (possibly new) unique array — assign the
 * result back to the place (D133).
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
/* COW index set Int64 slot. OOB → panic. Returns unique array. */
ThoxaArray *thoxa_array_set_i64(ThoxaArray *a, int64_t i, int64_t v);
/* Index get heap pointer. OOB → panic. Does not dup. */
void *thoxa_array_get_ptr(const ThoxaArray *a, int64_t i);
/* COW index set heap pointer: dups `p`, drops old slot. Returns unique array. */
ThoxaArray *thoxa_array_set_ptr(ThoxaArray *a, int64_t i, void *p);
/* Typed element get: memcpy into `out` (elem_size bytes), optional field-dup. */
void thoxa_array_get_elem(
    const ThoxaArray *a,
    int64_t i,
    void *out,
    ThoxaArrayElemFn elem_dup
);
/* COW typed element set: drop old, memcpy new, optional field-dup of new. */
ThoxaArray *thoxa_array_set_elem(
    ThoxaArray *a,
    int64_t i,
    const void *elem,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* COW: append `v` (int64 or bitcast ptr). Drops caller's share of `a`. */
ThoxaArray *thoxa_array_append_i64(ThoxaArray *a, int64_t v);
/* COW: append heap ptr (dups `p`). Drops caller's share of `a`. */
ThoxaArray *thoxa_array_append_ptr(ThoxaArray *a, void *p);
/*
 * Functional append of a raw element. Shallow-copies prior elems + new, then
 * `elem_dup` each slot of the result when non-NULL.
 * When `a` is non-NULL, elem_size/flags are taken from `a`.
 */
ThoxaArray *thoxa_array_append_elem(
    ThoxaArray *a,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* COW: drop last element. Empty → panic. Drops caller's share of `a`. */
ThoxaArray *thoxa_array_drop_last(ThoxaArray *a);
/* Like drop_last but field-dups surviving elements (User arrays). */
ThoxaArray *thoxa_array_drop_last_ex(
    ThoxaArray *a,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* COW insertAt: 0 ≤ at ≤ count (at == count appends). */
ThoxaArray *thoxa_array_insert_at_i64(ThoxaArray *a, int64_t at, int64_t v);
ThoxaArray *thoxa_array_insert_at_ptr(ThoxaArray *a, int64_t at, void *p);
ThoxaArray *thoxa_array_insert_at_elem(
    ThoxaArray *a,
    int64_t at,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* COW removeAt: 0 ≤ at < count. */
ThoxaArray *thoxa_array_remove_at(ThoxaArray *a, int64_t at);
ThoxaArray *thoxa_array_remove_at_ex(
    ThoxaArray *a,
    int64_t at,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
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

/* --- Map (dynamic, RC heap; Swift/Buffer COW writes — D134) ------------- */

#define THOXA_MAP_KEY_I64 0u
#define THOXA_MAP_KEY_STR 1u
#define THOXA_MAP_VAL_I64 0u
#define THOXA_MAP_VAL_PTR 1u
#define THOXA_MAP_VAL_USER 2u

/*
 * Layout: [ThoxaHeader][ThoxaMap { len, slot_cap, idx_cap, kinds, … data[] }]
 * Slots are insertion-ordered. An open-addressed index maps hash → slot.
 * Writes copy-on-write when refcount > 1 and return the unique map.
 */
typedef struct ThoxaMap ThoxaMap;

ThoxaMap *thoxa_map_new(uint32_t key_kind, uint32_t val_kind, uint32_t val_size);
int64_t thoxa_map_count(const ThoxaMap *m);
int thoxa_map_has(const ThoxaMap *m, int64_t key_bits);
int thoxa_map_get_i64(const ThoxaMap *m, int64_t key_bits, int64_t *out);
int thoxa_map_get_ptr(const ThoxaMap *m, int64_t key_bits, void **out);
int thoxa_map_get_elem(
    const ThoxaMap *m,
    int64_t key_bits,
    void *out,
    ThoxaArrayElemFn elem_dup
);
ThoxaMap *thoxa_map_set_i64(ThoxaMap *m, int64_t key_bits, int64_t val);
ThoxaMap *thoxa_map_set_ptr(ThoxaMap *m, int64_t key_bits, void *val);
ThoxaMap *thoxa_map_set_elem(
    ThoxaMap *m,
    int64_t key_bits,
    const void *val,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
ThoxaMap *thoxa_map_remove(ThoxaMap *m, int64_t key_bits);
ThoxaMap *thoxa_map_remove_ex(
    ThoxaMap *m,
    int64_t key_bits,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
/* Insertion-order access. OOB → panic. Key is i64 or pointer bitcast. */
int64_t thoxa_map_key_at(const ThoxaMap *m, int64_t i);
int64_t thoxa_map_val_i64_at(const ThoxaMap *m, int64_t i);
void *thoxa_map_val_ptr_at(const ThoxaMap *m, int64_t i);
void thoxa_map_val_elem_at(
    const ThoxaMap *m,
    int64_t i,
    void *out,
    ThoxaArrayElemFn elem_dup
);
ThoxaArray *thoxa_map_keys(const ThoxaMap *m);
ThoxaArray *thoxa_map_values_i64(const ThoxaMap *m);
ThoxaArray *thoxa_map_values_ptr(const ThoxaMap *m);
ThoxaArray *thoxa_map_values_elem(
    const ThoxaMap *m,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
);
void thoxa_map_dup(ThoxaMap *m);
void thoxa_map_drop(ThoxaMap *m);
void thoxa_map_drop_ex(ThoxaMap *m, ThoxaArrayElemFn elem_drop);
/* Display `{k: v, …}` for i64/bool/string keys and i64/bool/string values. */
ThoxaString *thoxa_map_to_string(const ThoxaMap *m, int val_is_string);

/* --- HTML tree (builtin `Node`, JSX runtime) ---------------------------- */

#define THOXA_HTML_EL 0
#define THOXA_HTML_TEXT 1
#define THOXA_HTML_FRAG 2

/* Ambient `Attr(name, value)` — layout must match compiler TypeLayout "Attr". */
typedef struct Thoxa_Attr {
    ThoxaString *name;
    ThoxaString *value;
} Thoxa_Attr;

typedef struct ThoxaHtml {
    uint8_t kind;
    ThoxaString *tag;
    ThoxaString *text;
    ThoxaArray *attrs; /* Array<Attr>, or NULL */
    ThoxaArray *kids;
} ThoxaHtml;

ThoxaHtml *thoxa_html_el(ThoxaString *tag, ThoxaArray *attrs, ThoxaArray *kids);
/* Escapes `s` as text. */
ThoxaHtml *thoxa_html_text(ThoxaString *s);
ThoxaHtml *thoxa_html_frag(ThoxaArray *kids);
ThoxaString *thoxa_html_render(const ThoxaHtml *n);
int64_t thoxa_html_kind(const ThoxaHtml *n);
ThoxaString *thoxa_html_tag(const ThoxaHtml *n);
ThoxaString *thoxa_html_get_text(const ThoxaHtml *n);
ThoxaArray *thoxa_html_attrs(const ThoxaHtml *n);
ThoxaArray *thoxa_html_kids(const ThoxaHtml *n);

/* Quote `s` as a SQL string literal (`'` doubled). Used by sql"…" (D144). */
ThoxaString *thoxa_sql_quote(const ThoxaString *s);

/* --- SQLite (amalgamation in vendor/sqlite; handles are sqlite3* as Int64) */

/* 0 = fail; errmsg via thoxa_sql_errmsg. Path ":memory:" is allowed. */
int64_t thoxa_sql_open(const char *path);
int32_t thoxa_sql_close(int64_t handle);
/* Last error from any sqlite helper (static buffer; copied by String returns). */
const char *thoxa_sql_errmsg(void);
int32_t thoxa_sql_exec(int64_t handle, const char *sql);
int64_t thoxa_sql_last_insert_rowid(int64_t handle);
int64_t thoxa_sql_changes(int64_t handle);
/* 0 = fail. Step: 100 = row, 101 = done, else error (same as SQLite). */
int64_t thoxa_sql_prepare(int64_t handle, const char *sql);
int32_t thoxa_sql_step(int64_t stmt);
int32_t thoxa_sql_finalize(int64_t stmt);
int32_t thoxa_sql_column_count(int64_t stmt);
const char *thoxa_sql_column_name(int64_t stmt, int32_t i);
/* 1 integer, 2 float, 3 text, 4 blob, 5 null. */
int32_t thoxa_sql_column_type(int64_t stmt, int32_t i);
int64_t thoxa_sql_column_i64(int64_t stmt, int32_t i);
double thoxa_sql_column_f64(int64_t stmt, int32_t i);
const char *thoxa_sql_column_text(int64_t stmt, int32_t i);

#ifdef __cplusplus
}
#endif

#endif /* THOXA_RT_H */
