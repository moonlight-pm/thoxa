#include "thoxa_rt.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void thoxa_print(const char *s) {
    if (s) {
        fputs(s, stdout);
    }
}

void thoxa_println(const char *s) {
    if (s) {
        fputs(s, stdout);
    }
    fputc('\n', stdout);
    fflush(stdout);
}

void thoxa_print_i64(int64_t v) {
    printf("%lld", (long long)v);
}

void thoxa_println_i64(int64_t v) {
    printf("%lld\n", (long long)v);
    fflush(stdout);
}

void thoxa_print_bool(bool v) {
    fputs(v ? "true" : "false", stdout);
}

void thoxa_println_bool(bool v) {
    fputs(v ? "true\n" : "false\n", stdout);
    fflush(stdout);
}

void thoxa_panic(const char *msg) {
    fprintf(stderr, "thoxa panic: %s\n", msg ? msg : "(null)");
    abort();
}

void *thoxa_alloc(size_t payload_size, uint16_t type_tag) {
    size_t total = sizeof(ThoxaHeader) + payload_size;
    ThoxaHeader *h = (ThoxaHeader *)calloc(1, total);
    if (!h) {
        thoxa_panic("out of memory");
    }
    h->refcount = 1;
    h->type_tag = type_tag;
    h->flags = 0;
    h->reserved = 0;
    return (void *)(h + 1);
}

static ThoxaHeader *header_of(void *ptr) {
    if (!ptr) {
        return NULL;
    }
    return ((ThoxaHeader *)ptr) - 1;
}

void thoxa_dup(void *ptr) {
    ThoxaHeader *h = header_of(ptr);
    if (!h) {
        return;
    }
    h->refcount += 1;
}

void thoxa_drop(void *ptr) {
    ThoxaHeader *h = header_of(ptr);
    if (!h) {
        return;
    }
    if (h->refcount == 0) {
        thoxa_panic("drop on zero refcount");
    }
    h->refcount -= 1;
    if (h->refcount == 0) {
        free(h);
    }
}

/* --- String ------------------------------------------------------------- */

ThoxaString *thoxa_string_new(const char *bytes, size_t len) {
    size_t payload = sizeof(size_t) + len + 1;
    ThoxaString *s = (ThoxaString *)thoxa_alloc(payload, THOXA_TAG_STRING);
    s->len = len;
    if (len > 0 && bytes) {
        memcpy(s->data, bytes, len);
    }
    s->data[len] = '\0';
    return s;
}

ThoxaString *thoxa_string_from_cstr(const char *s) {
    if (!s) {
        return thoxa_string_new("", 0);
    }
    return thoxa_string_new(s, strlen(s));
}

ThoxaString *thoxa_string_from_i64(int64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof(buf), "%lld", (long long)v);
    if (n < 0) {
        thoxa_panic("snprintf failed");
    }
    return thoxa_string_new(buf, (size_t)n);
}

ThoxaString *thoxa_string_from_bool(bool v) {
    return thoxa_string_from_cstr(v ? "true" : "false");
}

ThoxaString *thoxa_string_concat(const ThoxaString *a, const ThoxaString *b) {
    size_t alen = a ? a->len : 0;
    size_t blen = b ? b->len : 0;
    size_t len = alen + blen;
    size_t payload = sizeof(size_t) + len + 1;
    ThoxaString *s = (ThoxaString *)thoxa_alloc(payload, THOXA_TAG_STRING);
    s->len = len;
    if (alen > 0) {
        memcpy(s->data, a->data, alen);
    }
    if (blen > 0) {
        memcpy(s->data + alen, b->data, blen);
    }
    s->data[len] = '\0';
    return s;
}

bool thoxa_string_eq(const ThoxaString *a, const ThoxaString *b) {
    size_t alen = a ? a->len : 0;
    size_t blen = b ? b->len : 0;
    if (alen != blen) {
        return false;
    }
    if (alen == 0) {
        return true;
    }
    return memcmp(a->data, b->data, alen) == 0;
}

const char *thoxa_string_cstr(const ThoxaString *s) {
    return s ? s->data : "";
}

size_t thoxa_string_len(const ThoxaString *s) {
    return s ? s->len : 0;
}

void thoxa_print_string(const ThoxaString *s) {
    if (s && s->len > 0) {
        fwrite(s->data, 1, s->len, stdout);
    }
}

void thoxa_println_string(const ThoxaString *s) {
    thoxa_print_string(s);
    fputc('\n', stdout);
    fflush(stdout);
}

/* --- Bytes -------------------------------------------------------------- */

ThoxaBytes *thoxa_bytes_from_string(const ThoxaString *s) {
    size_t len = s ? s->len : 0;
    size_t payload = sizeof(size_t) + len;
    ThoxaBytes *b = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    b->len = len;
    if (len > 0 && s) {
        memcpy(b->data, s->data, len);
    }
    return b;
}

ThoxaBytes *thoxa_bytes_new(int64_t len) {
    if (len < 0) {
        thoxa_panic("Bytes length must be non-negative");
    }
    size_t n = (size_t)len;
    size_t payload = sizeof(size_t) + n;
    ThoxaBytes *b = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    b->len = n;
    /* thoxa_alloc uses calloc — payload is already zeroed. */
    return b;
}

size_t thoxa_bytes_count(const ThoxaBytes *b) {
    return b ? b->len : 0;
}

uint8_t *thoxa_bytes_data(ThoxaBytes *b) {
    return b ? b->data : NULL;
}

static void bytes_check_index(const ThoxaBytes *b, int64_t i) {
    if (!b || i < 0 || (size_t)i >= b->len) {
        thoxa_panic("Bytes index out of range");
    }
}

int64_t thoxa_bytes_get(const ThoxaBytes *b, int64_t i) {
    bytes_check_index(b, i);
    return (int64_t)b->data[(size_t)i];
}

void thoxa_bytes_set(ThoxaBytes *b, int64_t i, int64_t byte) {
    bytes_check_index(b, i);
    b->data[(size_t)i] = (uint8_t)(byte & 0xFF);
}

ThoxaBytes *thoxa_bytes_slice(const ThoxaBytes *b, int64_t start, int64_t end) {
    if (!b) {
        thoxa_panic("Bytes.slice on null");
    }
    if (start < 0 || end < start || (size_t)end > b->len) {
        thoxa_panic("Bytes.slice out of range");
    }
    size_t n = (size_t)(end - start);
    size_t payload = sizeof(size_t) + n;
    ThoxaBytes *out = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    out->len = n;
    if (n > 0) {
        memcpy(out->data, b->data + (size_t)start, n);
    }
    return out;
}

int64_t thoxa_bytes_find(const ThoxaBytes *hay, const ThoxaBytes *needle, int64_t from) {
    if (!hay) {
        return -1;
    }
    if (!needle || needle->len == 0) {
        /* Empty needle matches at `from` if in range, else -1. */
        if (from < 0 || (size_t)from > hay->len) {
            return -1;
        }
        return from;
    }
    if (from < 0) {
        from = 0;
    }
    if ((size_t)from > hay->len || needle->len > hay->len - (size_t)from) {
        return -1;
    }
    size_t last = hay->len - needle->len;
    for (size_t i = (size_t)from; i <= last; i++) {
        if (memcmp(hay->data + i, needle->data, needle->len) == 0) {
            return (int64_t)i;
        }
    }
    return -1;
}

ThoxaString *thoxa_bytes_to_string(const ThoxaBytes *b) {
    if (!b || b->len == 0) {
        return thoxa_string_new("", 0);
    }
    return thoxa_string_new((const char *)b->data, b->len);
}

/* --- Buffer ------------------------------------------------------------- */

static size_t buffer_payload_size(size_t cap) {
    return sizeof(size_t) + sizeof(size_t) + cap;
}

static ThoxaBuffer *buffer_alloc(size_t len, size_t cap) {
    ThoxaBuffer *b = (ThoxaBuffer *)thoxa_alloc(buffer_payload_size(cap), THOXA_TAG_BUFFER);
    b->len = len;
    b->cap = cap;
    return b;
}

ThoxaBuffer *thoxa_buffer_new(void) {
    return buffer_alloc(0, 0);
}

ThoxaBuffer *thoxa_buffer_with_capacity(int64_t capacity) {
    if (capacity < 0) {
        thoxa_panic("Buffer capacity must be non-negative");
    }
    return buffer_alloc(0, (size_t)capacity);
}

int64_t thoxa_buffer_count(const ThoxaBuffer *b) {
    return b ? (int64_t)b->len : 0;
}

int64_t thoxa_buffer_capacity(const ThoxaBuffer *b) {
    return b ? (int64_t)b->cap : 0;
}

static size_t buffer_next_cap(size_t cap, size_t need) {
    size_t c = cap == 0 ? 8 : cap;
    while (c < need) {
        if (c > (SIZE_MAX / 2)) {
            thoxa_panic("Buffer capacity overflow");
        }
        c *= 2;
    }
    return c;
}

static ThoxaBuffer *buffer_clone(const ThoxaBuffer *b) {
    size_t len = b ? b->len : 0;
    size_t cap = b ? b->cap : 0;
    /* Clone keeps capacity so growth schedule matches a unique buffer. */
    ThoxaBuffer *n = buffer_alloc(len, cap);
    if (len > 0 && b) {
        memcpy(n->data, b->data, len);
    }
    return n;
}

/* If shared, replace with a private clone and drop the shared ref. */
static ThoxaBuffer *buffer_ensure_unique(ThoxaBuffer *b) {
    if (!b) {
        return thoxa_buffer_new();
    }
    ThoxaHeader *h = header_of(b);
    if (h->refcount == 1) {
        return b;
    }
    ThoxaBuffer *n = buffer_clone(b);
    thoxa_drop(b);
    return n;
}

static ThoxaBuffer *buffer_reserve_unique(ThoxaBuffer *b, size_t need) {
    b = buffer_ensure_unique(b);
    if (b->cap >= need) {
        return b;
    }
    size_t new_cap = buffer_next_cap(b->cap, need);
    ThoxaBuffer *n = buffer_alloc(b->len, new_cap);
    if (b->len > 0) {
        memcpy(n->data, b->data, b->len);
    }
    thoxa_drop(b);
    return n;
}

ThoxaBuffer *thoxa_buffer_append(ThoxaBuffer *b, int64_t byte) {
    size_t old_len = b ? b->len : 0;
    b = buffer_reserve_unique(b, old_len + 1);
    b->data[b->len] = (uint8_t)(byte & 0xFF);
    b->len += 1;
    return b;
}

ThoxaBuffer *thoxa_buffer_append_bytes(ThoxaBuffer *b, const ThoxaBytes *src) {
    size_t add = src ? src->len : 0;
    size_t old_len = b ? b->len : 0;
    b = buffer_reserve_unique(b, old_len + add);
    if (add > 0 && src) {
        memcpy(b->data + b->len, src->data, add);
        b->len += add;
    }
    return b;
}

ThoxaBytes *thoxa_buffer_freeze(const ThoxaBuffer *b) {
    size_t n = b ? b->len : 0;
    size_t payload = sizeof(size_t) + n;
    ThoxaBytes *out = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    out->len = n;
    if (n > 0 && b) {
        memcpy(out->data, b->data, n);
    }
    return out;
}

/* --- Array -------------------------------------------------------------- */

static size_t array_payload_size(size_t cap) {
    /* offsetof accounts for padding before the flexible items[] array. */
    return offsetof(ThoxaArray, items) + cap * sizeof(int64_t);
}

static ThoxaArray *array_alloc(size_t len, size_t cap, uint32_t flags) {
    ThoxaArray *a = (ThoxaArray *)thoxa_alloc(array_payload_size(cap), THOXA_TAG_ARRAY);
    a->len = len;
    a->cap = cap;
    a->flags = flags;
    return a;
}

static void array_bounds(const ThoxaArray *a, int64_t i) {
    if (!a || i < 0 || (size_t)i >= a->len) {
        thoxa_panic("array index out of bounds");
    }
}

static int array_heap_elems(const ThoxaArray *a) {
    return a && (a->flags & THOXA_ARRAY_FLAG_HEAP_ELEMS) != 0;
}

static void array_copy_items(ThoxaArray *dst, const ThoxaArray *src, size_t n) {
    if (n == 0 || !src) {
        return;
    }
    if (array_heap_elems(src)) {
        for (size_t i = 0; i < n; i++) {
            void *p = (void *)(intptr_t)src->items[i];
            thoxa_dup(p);
            dst->items[i] = src->items[i];
        }
    } else {
        memcpy(dst->items, src->items, n * sizeof(int64_t));
    }
}

ThoxaArray *thoxa_array_new(int heap_elems) {
    return array_alloc(0, 0, heap_elems ? THOXA_ARRAY_FLAG_HEAP_ELEMS : 0);
}

ThoxaArray *thoxa_array_from_i64(const int64_t *items, size_t len) {
    ThoxaArray *a = array_alloc(len, len, 0);
    if (len > 0 && items) {
        memcpy(a->items, items, len * sizeof(int64_t));
    }
    return a;
}

ThoxaArray *thoxa_array_from_ptrs(void *const *items, size_t len) {
    ThoxaArray *a = array_alloc(len, len, THOXA_ARRAY_FLAG_HEAP_ELEMS);
    for (size_t i = 0; i < len; i++) {
        void *p = items ? items[i] : NULL;
        thoxa_dup(p);
        a->items[i] = (int64_t)(intptr_t)p;
    }
    return a;
}

int64_t thoxa_array_count(const ThoxaArray *a) {
    return a ? (int64_t)a->len : 0;
}

int64_t thoxa_array_get_i64(const ThoxaArray *a, int64_t i) {
    array_bounds(a, i);
    return a->items[(size_t)i];
}

void thoxa_array_set_i64(ThoxaArray *a, int64_t i, int64_t v) {
    array_bounds(a, i);
    a->items[(size_t)i] = v;
}

void *thoxa_array_get_ptr(const ThoxaArray *a, int64_t i) {
    array_bounds(a, i);
    return (void *)(intptr_t)a->items[(size_t)i];
}

void thoxa_array_set_ptr(ThoxaArray *a, int64_t i, void *p) {
    array_bounds(a, i);
    void *old = (void *)(intptr_t)a->items[(size_t)i];
    thoxa_dup(p);
    a->items[(size_t)i] = (int64_t)(intptr_t)p;
    thoxa_drop(old);
}

ThoxaArray *thoxa_array_append_i64(const ThoxaArray *a, int64_t v) {
    size_t old_len = a ? a->len : 0;
    uint32_t flags = a ? a->flags : 0;
    ThoxaArray *n = array_alloc(old_len + 1, old_len + 1, flags);
    array_copy_items(n, a, old_len);
    if (array_heap_elems(n)) {
        void *p = (void *)(intptr_t)v;
        thoxa_dup(p);
        n->items[old_len] = v;
    } else {
        n->items[old_len] = v;
    }
    return n;
}

ThoxaArray *thoxa_array_append_ptr(const ThoxaArray *a, void *p) {
    size_t old_len = a ? a->len : 0;
    ThoxaArray *n = array_alloc(old_len + 1, old_len + 1, THOXA_ARRAY_FLAG_HEAP_ELEMS);
    /* Source may be empty non-heap from thoxa_array_new(0) mistype — force heap. */
    if (a && array_heap_elems(a)) {
        array_copy_items(n, a, old_len);
    } else if (a && old_len > 0) {
        /* Copy slots without element dup if source was non-heap (shouldn't happen). */
        memcpy(n->items, a->items, old_len * sizeof(int64_t));
    }
    thoxa_dup(p);
    n->items[old_len] = (int64_t)(intptr_t)p;
    return n;
}

ThoxaArray *thoxa_array_drop_last(const ThoxaArray *a) {
    if (!a || a->len == 0) {
        thoxa_panic("Array.dropLast on empty array");
    }
    size_t new_len = a->len - 1;
    ThoxaArray *n = array_alloc(new_len, new_len, a->flags);
    array_copy_items(n, a, new_len);
    return n;
}

void thoxa_array_dup(ThoxaArray *a) {
    thoxa_dup(a);
}

void thoxa_array_drop(ThoxaArray *a) {
    if (!a) {
        return;
    }
    ThoxaHeader *h = header_of(a);
    if (!h) {
        return;
    }
    if (h->refcount == 0) {
        thoxa_panic("drop on zero refcount");
    }
    h->refcount -= 1;
    if (h->refcount == 0) {
        if (array_heap_elems(a)) {
            for (size_t i = 0; i < a->len; i++) {
                thoxa_drop((void *)(intptr_t)a->items[i]);
            }
        }
        free(h);
    }
}

ThoxaString *thoxa_array_i64_to_string(const ThoxaArray *a) {
    /* Build via snprintf into a growable temp, then own as ThoxaString. */
    size_t n = a ? a->len : 0;
    size_t cap = 16 + n * 24;
    char *buf = (char *)malloc(cap);
    if (!buf) {
        thoxa_panic("out of memory");
    }
    size_t used = 0;
    buf[used++] = '[';
    for (size_t i = 0; i < n; i++) {
        if (i > 0) {
            if (used + 2 >= cap) {
                cap *= 2;
                buf = (char *)realloc(buf, cap);
                if (!buf) {
                    thoxa_panic("out of memory");
                }
            }
            buf[used++] = ',';
            buf[used++] = ' ';
        }
        char tmp[32];
        int w = snprintf(tmp, sizeof(tmp), "%lld", (long long)a->items[i]);
        if (w < 0) {
            free(buf);
            thoxa_panic("array toString failed");
        }
        while (used + (size_t)w + 1 >= cap) {
            cap *= 2;
            buf = (char *)realloc(buf, cap);
            if (!buf) {
                thoxa_panic("out of memory");
            }
        }
        memcpy(buf + used, tmp, (size_t)w);
        used += (size_t)w;
    }
    if (used + 1 >= cap) {
        cap = used + 2;
        buf = (char *)realloc(buf, cap);
        if (!buf) {
            thoxa_panic("out of memory");
        }
    }
    buf[used++] = ']';
    buf[used] = '\0';
    ThoxaString *s = thoxa_string_new(buf, used);
    free(buf);
    return s;
}

ThoxaString *thoxa_array_string_to_string(const ThoxaArray *a) {
    size_t n = a ? a->len : 0;
    /* Rough capacity: brackets + quotes + commas + string bytes. */
    size_t cap = 16;
    for (size_t i = 0; i < n; i++) {
        ThoxaString *el = (ThoxaString *)(intptr_t)a->items[i];
        cap += (el ? el->len : 0) + 4;
    }
    char *buf = (char *)malloc(cap);
    if (!buf) {
        thoxa_panic("out of memory");
    }
    size_t used = 0;
    buf[used++] = '[';
    for (size_t i = 0; i < n; i++) {
        if (i > 0) {
            buf[used++] = ',';
            buf[used++] = ' ';
        }
        ThoxaString *el = (ThoxaString *)(intptr_t)a->items[i];
        const char *cstr = el ? thoxa_string_cstr(el) : "";
        size_t elen = el ? el->len : 0;
        while (used + elen + 3 >= cap) {
            cap *= 2;
            buf = (char *)realloc(buf, cap);
            if (!buf) {
                thoxa_panic("out of memory");
            }
        }
        /* Oracle display of String is bare content without quotes in arrays? */
        /* Python repr of list of str uses quotes; oracle arrays.out uses bare ints.
         * For strings: match Python list display with quotes. */
        buf[used++] = '"';
        if (elen > 0) {
            memcpy(buf + used, cstr, elen);
            used += elen;
        }
        buf[used++] = '"';
    }
    buf[used++] = ']';
    buf[used] = '\0';
    ThoxaString *s = thoxa_string_new(buf, used);
    free(buf);
    return s;
}
