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
