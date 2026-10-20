#define _DEFAULT_SOURCE
#include "thoxa_rt.h"

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

int32_t thoxa_errno(void) {
    return (int32_t)errno;
}

/* A panic inside a `-->` task ends that task, not the process (R4). */
static void thoxa_task_end_by_panic(void);

void thoxa_panic(const char *msg) {
    fprintf(stderr, "thoxa panic: %s\n", msg ? msg : "(null)");
    thoxa_task_end_by_panic();
    abort();
}

/* One panic at a time: the message buffer is never in use twice. */
static char thoxa_narrow_message[96];

void thoxa_narrow_fail_signed(int64_t value, const char *to) {
    snprintf(thoxa_narrow_message, sizeof thoxa_narrow_message, "%lld does not fit in %s",
             (long long)value, to);
    thoxa_panic(thoxa_narrow_message);
}

void thoxa_narrow_fail_unsigned(uint64_t value, const char *to) {
    snprintf(thoxa_narrow_message, sizeof thoxa_narrow_message, "%llu does not fit in %s",
             (unsigned long long)value, to);
    thoxa_panic(thoxa_narrow_message);
}

int thoxa_exit_status(int64_t status) {
    if (status < 0 || status > 255) {
        snprintf(thoxa_narrow_message, sizeof thoxa_narrow_message,
                 "main returned %lld; an exit status is 0 to 255", (long long)status);
        thoxa_panic(thoxa_narrow_message);
    }
    return (int)status;
}

static size_t size_add(size_t a, size_t b) {
    if (a > SIZE_MAX - b) {
        thoxa_panic("allocation size overflow");
    }
    return a + b;
}

static size_t size_mul(size_t a, size_t b) {
    if (a != 0 && b > SIZE_MAX / a) {
        thoxa_panic("allocation size overflow");
    }
    return a * b;
}

static size_t size_dbl(size_t n) {
    if (n > SIZE_MAX / 2) {
        thoxa_panic("allocation size overflow");
    }
    return n * 2;
}

static size_t string_payload_size(size_t len) {
    return size_add(sizeof(ThoxaString), size_add(len, 1));
}

static size_t bytes_payload_size(size_t len) {
    return size_add(sizeof(size_t), len);
}

static void *grow_buf(void *ptr, size_t *cap, size_t need) {
    if (ptr && need <= *cap) {
        return ptr;
    }
    size_t ncap = *cap ? *cap : 8;
    while (ncap < need) {
        ncap = size_dbl(ncap);
    }
    void *grown = realloc(ptr, ncap);
    if (!grown) {
        thoxa_panic("out of memory");
    }
    *cap = ncap;
    return grown;
}

void *thoxa_alloc(size_t payload_size, uint16_t type_tag) {
    size_t total = size_add(sizeof(ThoxaHeader), payload_size);
    ThoxaHeader *h = (ThoxaHeader *)calloc(1, total);
    if (!h) {
        thoxa_panic("out of memory");
    }
    h->refcount = 1;
    h->type_tag = type_tag;
    h->flags = 0;
    h->reserved = 0;
    h->dtor = NULL;
    return (void *)(h + 1);
}

static ThoxaHeader *header_of(void *ptr) {
    if (!ptr) {
        return NULL;
    }
    return ((ThoxaHeader *)ptr) - 1;
}

void thoxa_set_dtor(void *ptr, void (*dtor)(void *payload)) {
    ThoxaHeader *h = header_of(ptr);
    if (!h) {
        return;
    }
    h->dtor = dtor;
}

void thoxa_dup(void *ptr) {
    ThoxaHeader *h = header_of(ptr);
    if (!h) {
        return;
    }
    if (h->refcount == SIZE_MAX) {
        thoxa_panic("refcount overflow");
    }
    h->refcount += 1;
}

static void thoxa_html_free_payload(ThoxaHtml *n);

void thoxa_array_drop(ThoxaArray *a);
void thoxa_array_drop_ex(ThoxaArray *a, ThoxaArrayElemFn elem_drop);
void thoxa_map_drop(ThoxaMap *m);
void thoxa_map_drop_ex(ThoxaMap *m, ThoxaArrayElemFn elem_drop);

void thoxa_drop(void *ptr) {
    ThoxaHeader *h = header_of(ptr);
    if (!h) {
        return;
    }
    if (h->type_tag == THOXA_TAG_ARRAY) {
        thoxa_array_drop((ThoxaArray *)ptr);
        return;
    }
    if (h->type_tag == THOXA_TAG_MAP) {
        thoxa_map_drop((ThoxaMap *)ptr);
        return;
    }
    if (h->refcount == 0) {
        thoxa_panic("drop on zero refcount");
    }
    h->refcount -= 1;
    if (h->refcount == 0) {
        void (*dtor)(void *) = h->dtor;
        h->dtor = NULL;
        if (dtor) {
            dtor(ptr);
        }
        if (h->type_tag == THOXA_TAG_HTML) {
            thoxa_html_free_payload((ThoxaHtml *)ptr);
        }
        free(h);
    }
}

/* --- Function values (D132) --------------------------------------------- */

void *thoxa_fn_payload(ThoxaFnBox *box) {
    if (!box) {
        return NULL;
    }
    return (void *)(box + 1);
}

ThoxaFn thoxa_fn_free(void *code) {
    ThoxaFn f;
    f.recv = NULL;
    f.code = code;
    return f;
}

ThoxaFn thoxa_fn_bind(
    const void *payload,
    uint32_t size,
    void (*retain)(void *payload),
    void (*release)(void *payload),
    void *code
) {
    ThoxaFnBox *box = (ThoxaFnBox *)calloc(1, size_add(sizeof(ThoxaFnBox), (size_t)size));
    if (!box) {
        thoxa_panic("out of memory");
    }
    box->rc = 1;
    box->release = release;
    if (payload && size > 0) {
        memcpy(thoxa_fn_payload(box), payload, (size_t)size);
    }
    if (retain) {
        retain(thoxa_fn_payload(box));
    }
    ThoxaFn f;
    f.recv = box;
    f.code = code;
    return f;
}

void thoxa_fn_retain(ThoxaFn *fn) {
    if (!fn || !fn->recv) {
        return;
    }
    if (fn->recv->rc == UINT32_MAX) {
        thoxa_panic("refcount overflow");
    }
    fn->recv->rc += 1;
}

void thoxa_fn_release(ThoxaFn *fn) {
    if (!fn || !fn->recv) {
        return;
    }
    if (fn->recv->rc == 0) {
        thoxa_panic("fn release on zero refcount");
    }
    fn->recv->rc -= 1;
    if (fn->recv->rc == 0) {
        if (fn->recv->release) {
            fn->recv->release(thoxa_fn_payload(fn->recv));
        }
        free(fn->recv);
    }
    fn->recv = NULL;
}

/* --- String ------------------------------------------------------------- */

/*
 * Every String holds valid UTF-8 and knows its scalar count (D195):
 * thoxa_string_new replaces each maximal ill-formed subsequence with U+FFFD
 * (Unicode §3.9, as WHATWG decoders do), so a String from Bytes, C, or a
 * file is always text.
 */

/* One UTF-8 sequence at p (n > 0 bytes left): sets *code and returns its
 * length. An ill-formed sequence sets *code = UINT32_MAX and returns the
 * length of its maximal subpart (at least 1). */
static size_t utf8_decode(const uint8_t *p, size_t n, uint32_t *code) {
    uint8_t lead = p[0];
    if (lead < 0x80) {
        *code = lead;
        return 1;
    }
    size_t len;
    uint32_t value;
    /* Well-formed range of the second byte (Unicode Table 3-7). */
    uint8_t low = 0x80;
    uint8_t high = 0xBF;
    if (lead >= 0xC2 && lead <= 0xDF) {
        len = 2;
        value = lead & 0x1F;
    } else if (lead >= 0xE0 && lead <= 0xEF) {
        len = 3;
        value = lead & 0x0F;
        low = lead == 0xE0 ? 0xA0 : 0x80;
        high = lead == 0xED ? 0x9F : 0xBF;
    } else if (lead >= 0xF0 && lead <= 0xF4) {
        len = 4;
        value = lead & 0x07;
        low = lead == 0xF0 ? 0x90 : 0x80;
        high = lead == 0xF4 ? 0x8F : 0xBF;
    } else {
        *code = UINT32_MAX;
        return 1;
    }
    for (size_t i = 1; i < len; i++) {
        if (i >= n || p[i] < low || p[i] > high) {
            *code = UINT32_MAX;
            return i;
        }
        value = (value << 6) | (p[i] & 0x3F);
        low = 0x80;
        high = 0xBF;
    }
    *code = value;
    return len;
}

/* Scalars in data[0..len), or SIZE_MAX when it is not valid UTF-8. */
static size_t utf8_count(const char *data, size_t len) {
    const uint8_t *p = (const uint8_t *)data;
    size_t count = 0;
    size_t i = 0;
    while (i < len) {
        if (len - i >= 8) {
            uint64_t word;
            memcpy(&word, p + i, 8);
            if ((word & 0x8080808080808080ull) == 0) {
                i += 8;
                count += 8;
                continue;
            }
        }
        if (p[i] < 0x80) {
            i++;
        } else {
            uint32_t code;
            i += utf8_decode(p + i, len - i, &code);
            if (code == UINT32_MAX) {
                return SIZE_MAX;
            }
        }
        count++;
    }
    return count;
}

/* An uninitialized String of `len` bytes holding `count` scalars. */
static ThoxaString *string_alloc(size_t len, size_t count) {
    ThoxaString *s = (ThoxaString *)thoxa_alloc(string_payload_size(len), THOXA_TAG_STRING);
    s->len = len;
    s->count = count;
    s->data[len] = '\0';
    return s;
}

/* data[0..len) with each maximal ill-formed subsequence as U+FFFD. */
static ThoxaString *string_repaired(const char *data, size_t len) {
    static const char replacement[] = "\xEF\xBF\xBD";
    const uint8_t *p = (const uint8_t *)data;
    size_t out_len = 0;
    size_t count = 0;
    for (size_t i = 0; i < len; count++) {
        uint32_t code;
        size_t step = utf8_decode(p + i, len - i, &code);
        out_len = size_add(out_len, code == UINT32_MAX ? 3 : step);
        i += step;
    }
    ThoxaString *s = string_alloc(out_len, count);
    size_t used = 0;
    for (size_t i = 0; i < len;) {
        uint32_t code;
        size_t step = utf8_decode(p + i, len - i, &code);
        if (code == UINT32_MAX) {
            memcpy(s->data + used, replacement, 3);
            used += 3;
        } else {
            memcpy(s->data + used, data + i, step);
            used += step;
        }
        i += step;
    }
    return s;
}

ThoxaString *thoxa_string_new(const char *bytes, size_t len) {
    if (!bytes) {
        /* `len` NUL bytes: valid, one scalar each. */
        return string_alloc(len, len);
    }
    size_t count = utf8_count(bytes, len);
    if (count == SIZE_MAX) {
        return string_repaired(bytes, len);
    }
    ThoxaString *s = string_alloc(len, count);
    if (len > 0) {
        memcpy(s->data, bytes, len);
    }
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

ThoxaString *thoxa_string_from_u64(uint64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof(buf), "%llu", (unsigned long long)v);
    if (n < 0) {
        thoxa_panic("snprintf failed");
    }
    return thoxa_string_new(buf, (size_t)n);
}

double thoxa_f64_from_bits(uint64_t bits) {
    double d;
    memcpy(&d, &bits, sizeof d);
    return d;
}

uint64_t thoxa_f64_to_bits(double v) {
    uint64_t bits;
    memcpy(&bits, &v, sizeof bits);
    return bits;
}

double thoxa_f64_from_string(const char *s) {
    if (!s) {
        return 0.0;
    }
    char *end = NULL;
    return strtod(s, &end);
}

int32_t thoxa_f64_is_finite(double v) {
    uint64_t bits = thoxa_f64_to_bits(v);
    return (bits & 0x7ff0000000000000ull) != 0x7ff0000000000000ull;
}

ThoxaString *thoxa_string_from_f64(double v) {
    char buf[64];
    uint64_t bits = thoxa_f64_to_bits(v);
    if ((bits & 0x7ff0000000000000ull) == 0x7ff0000000000000ull) {
        if ((bits & 0x000fffffffffffffull) != 0) {
            return thoxa_string_from_cstr("NaN");
        }
        return thoxa_string_from_cstr((bits >> 63) != 0 ? "-inf" : "inf");
    }
    /* Shortest decimal that round-trips (so 0.15 prints as 0.15, not 0.1499…):
       the fewest significant digits, found in scientific form. */
    int prec = 1;
    for (; prec <= 17; prec++) {
        if (snprintf(buf, sizeof(buf), "%.*e", prec - 1, v) < 0) {
            thoxa_panic("snprintf failed");
        }
        char *end = NULL;
        double back = strtod(buf, &end);
        if (end && *end == '\0' && thoxa_f64_to_bits(back) == bits) {
            break;
        }
    }
    if (prec > 17) {
        prec = 17;
    }
    /* `buf` is `[-]d[.ddd]e±XX`. Collect the digits and the exponent. */
    char digits[24];
    int count = 0;
    const char *at = buf;
    int negative = *at == '-';
    if (negative) {
        at++;
    }
    while (*at && *at != 'e') {
        if (*at != '.') {
            digits[count++] = *at;
        }
        at++;
    }
    int exponent = *at == 'e' ? atoi(at + 1) : 0;
    while (count > 1 && digits[count - 1] == '0') {
        count--;
    }
    int n = 0;
    if (exponent < -7 || exponent >= 21) {
        /* Far from 1: exponent form, as `%g` writes it (`1e+21`, `1.5e-08`). */
        n = snprintf(buf, sizeof(buf), "%.*g", count, v);
        if (n < 0) {
            thoxa_panic("snprintf failed");
        }
        return thoxa_string_new(buf, (size_t)n);
    }
    char out[64];
    if (negative) {
        out[n++] = '-';
    }
    if (exponent < 0) {
        /* 0.000ddd */
        out[n++] = '0';
        out[n++] = '.';
        for (int i = -1; i > exponent; i--) {
            out[n++] = '0';
        }
        for (int i = 0; i < count; i++) {
            out[n++] = digits[i];
        }
    } else {
        /* ddd[000].ddd — an integral value keeps `.0`. */
        for (int i = 0; i <= exponent; i++) {
            out[n++] = i < count ? digits[i] : '0';
        }
        out[n++] = '.';
        if (count > exponent + 1) {
            for (int i = exponent + 1; i < count; i++) {
                out[n++] = digits[i];
            }
        } else {
            out[n++] = '0';
        }
    }
    return thoxa_string_new(out, (size_t)n);
}

ThoxaString *thoxa_string_from_bool(bool v) {
    return thoxa_string_from_cstr(v ? "true" : "false");
}

ThoxaString *thoxa_string_concat(const ThoxaString *a, const ThoxaString *b) {
    size_t alen = a ? a->len : 0;
    size_t blen = b ? b->len : 0;
    size_t len = size_add(alen, blen);
    ThoxaString *s = string_alloc(len, (a ? a->count : 0) + (b ? b->count : 0));
    if (alen > 0) {
        memcpy(s->data, a->data, alen);
    }
    if (blen > 0) {
        memcpy(s->data + alen, b->data, blen);
    }
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

size_t thoxa_string_count(const ThoxaString *s) {
    return s ? s->count : 0;
}

/* --- String byte primitives (D196) ---------------------------------------
 *
 * String's methods are written in Thoxa (std/string.thx). These are the
 * byte-level operations they are built on.
 */

int64_t thoxa_string_byte_at(const ThoxaString *s, int64_t index) {
    if (index < 0 || (uint64_t)index >= thoxa_string_len(s)) {
        thoxa_panic("String.byteAt: index out of range");
    }
    return (uint8_t)s->data[index];
}

static size_t clamp_offset(int64_t offset, size_t len) {
    if (offset <= 0) {
        return 0;
    }
    return (uint64_t)offset > len ? len : (size_t)offset;
}

ThoxaString *thoxa_string_byte_slice(const ThoxaString *s, int64_t start, int64_t end) {
    size_t len = thoxa_string_len(s);
    size_t first = clamp_offset(start, len);
    size_t last = clamp_offset(end, len);
    if (first >= last) {
        return thoxa_string_new("", 0);
    }
    return thoxa_string_new(s->data + first, last - first);
}

int64_t thoxa_string_byte_index_of(const ThoxaString *s, const ThoxaString *needle, int64_t from) {
    size_t len = thoxa_string_len(s);
    size_t needle_len = thoxa_string_len(needle);
    if (from < 0) {
        from = 0;
    }
    if ((uint64_t)from > len || needle_len > len - (size_t)from) {
        return -1;
    }
    if (needle_len == 0) {
        return from;
    }
    const char *data = s->data;
    const char *last = data + (len - needle_len);
    const char *at = data + from;
    while (at <= last) {
        at = (const char *)memchr(at, needle->data[0], (size_t)(last - at) + 1);
        if (!at) {
            return -1;
        }
        if (memcmp(at, needle->data, needle_len) == 0) {
            return (int64_t)(at - data);
        }
        at++;
    }
    return -1;
}

int32_t thoxa_write_line_fd(int32_t fd, const char *s) {
    if (fd < 0) {
        return -1;
    }
    if (s && s[0] != '\0') {
        const uint8_t *p = (const uint8_t *)s;
        size_t left = strlen(s);
        while (left > 0) {
            ssize_t n = write(fd, p, left);
            if (n < 0) {
                if (errno == EINTR) {
                    continue;
                }
                return -1;
            }
            if (n == 0) {
                return -1;
            }
            p += (size_t)n;
            left -= (size_t)n;
        }
    }
    for (;;) {
        ssize_t n = write(fd, "\n", 1);
        if (n < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        if (n == 1) {
            break;
        }
    }
    /* Best-effort flush for stdio-backed fds; ignore failures. */
    if (fd == 1) {
        fflush(stdout);
    } else if (fd == 2) {
        fflush(stderr);
    }
    return 0;
}

/* --- Bytes -------------------------------------------------------------- */

ThoxaBytes *thoxa_bytes_from_string(const ThoxaString *s) {
    size_t len = s ? s->len : 0;
    size_t payload = bytes_payload_size(len);
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
    size_t payload = bytes_payload_size(n);
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
    size_t payload = bytes_payload_size(n);
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

int thoxa_bytes_equals(const ThoxaBytes *a, const ThoxaBytes *b) {
    if (a == b) {
        return 1;
    }
    if (!a || !b) {
        return 0;
    }
    if (a->len != b->len) {
        return 0;
    }
    if (a->len == 0) {
        return 1;
    }
    return memcmp(a->data, b->data, a->len) == 0;
}

ThoxaBytes *thoxa_bytes_insert_at(const ThoxaBytes *b, int64_t at, int64_t byte) {
    size_t old_len = b ? b->len : 0;
    if (at < 0 || (size_t)at > old_len) {
        thoxa_panic("Bytes.insert out of range");
    }
    size_t pos = (size_t)at;
    size_t n = size_add(old_len, 1);
    size_t payload = bytes_payload_size(n);
    ThoxaBytes *out = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    out->len = n;
    if (pos > 0 && b) {
        memcpy(out->data, b->data, pos);
    }
    out->data[pos] = (uint8_t)(byte & 0xFF);
    if (pos < old_len && b) {
        memcpy(out->data + pos + 1, b->data + pos, old_len - pos);
    }
    return out;
}

ThoxaBytes *thoxa_bytes_remove_at(const ThoxaBytes *b, int64_t at) {
    if (!b || b->len == 0) {
        thoxa_panic("Bytes.removeAt out of range");
    }
    if (at < 0 || (size_t)at >= b->len) {
        thoxa_panic("Bytes.removeAt out of range");
    }
    size_t pos = (size_t)at;
    size_t n = b->len - 1;
    size_t payload = bytes_payload_size(n);
    ThoxaBytes *out = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    out->len = n;
    if (pos > 0) {
        memcpy(out->data, b->data, pos);
    }
    if (pos < n) {
        memcpy(out->data + pos, b->data + pos + 1, n - pos);
    }
    return out;
}

/* --- Buffer ------------------------------------------------------------- */

static size_t buffer_payload_size(size_t cap) {
    return size_add(size_add(sizeof(size_t), sizeof(size_t)), cap);
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
        c = size_dbl(c);
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
    b = buffer_reserve_unique(b, size_add(old_len, 1));
    b->data[b->len] = (uint8_t)(byte & 0xFF);
    b->len += 1;
    return b;
}

ThoxaBuffer *thoxa_buffer_append_bytes(ThoxaBuffer *b, const ThoxaBytes *src) {
    size_t add = src ? src->len : 0;
    size_t old_len = b ? b->len : 0;
    b = buffer_reserve_unique(b, size_add(old_len, add));
    if (add > 0 && src) {
        memcpy(b->data + b->len, src->data, add);
        b->len += add;
    }
    return b;
}

ThoxaBytes *thoxa_buffer_freeze(const ThoxaBuffer *b) {
    size_t n = b ? b->len : 0;
    size_t payload = bytes_payload_size(n);
    ThoxaBytes *out = (ThoxaBytes *)thoxa_alloc(payload, THOXA_TAG_BYTES);
    out->len = n;
    if (n > 0 && b) {
        memcpy(out->data, b->data, n);
    }
    return out;
}

/* --- Array -------------------------------------------------------------- */

static size_t array_payload_size(size_t cap, uint32_t elem_size) {
    /* offsetof accounts for padding before the flexible data[] array. */
    return size_add(offsetof(ThoxaArray, data), size_mul(cap, (size_t)elem_size));
}

static ThoxaArray *array_alloc(size_t len, size_t cap, uint32_t flags, uint32_t elem_size) {
    if (elem_size == 0) {
        thoxa_panic("array elem_size must be non-zero");
    }
    ThoxaArray *a =
        (ThoxaArray *)thoxa_alloc(array_payload_size(cap, elem_size), THOXA_TAG_ARRAY);
    a->len = len;
    a->cap = cap;
    a->flags = flags;
    a->elem_size = elem_size;
    return a;
}

static void *array_slot(ThoxaArray *a, size_t i) {
    return a->data + i * (size_t)a->elem_size;
}

static const void *array_slot_c(const ThoxaArray *a, size_t i) {
    return a->data + i * (size_t)a->elem_size;
}

static int64_t *array_i64(ThoxaArray *a, size_t i) {
    return (int64_t *)array_slot(a, i);
}

static const int64_t *array_i64_c(const ThoxaArray *a, size_t i) {
    return (const int64_t *)array_slot_c(a, i);
}

static void array_bounds(const ThoxaArray *a, int64_t i) {
    if (!a || i < 0 || (size_t)i >= a->len) {
        thoxa_panic("array index out of bounds");
    }
}

static int array_heap_elems(const ThoxaArray *a) {
    return a && (a->flags & THOXA_ARRAY_FLAG_HEAP_ELEMS) != 0;
}

/* Copy n elements; for HEAP_ELEMS, dup each pointer slot. */
static void array_copy_items(ThoxaArray *dst, const ThoxaArray *src, size_t n) {
    if (n == 0 || !src) {
        return;
    }
    if (array_heap_elems(src)) {
        for (size_t i = 0; i < n; i++) {
            int64_t slot = *array_i64_c(src, i);
            void *p = (void *)(intptr_t)slot;
            thoxa_dup(p);
            *array_i64(dst, i) = slot;
        }
    } else {
        memcpy(dst->data, src->data, size_mul(n, (size_t)src->elem_size));
    }
}

/* Copy `n` elements from src[src_off..] into dst[dst_off..] (heap-aware). */
static void array_copy_range(
    ThoxaArray *dst,
    size_t dst_off,
    const ThoxaArray *src,
    size_t src_off,
    size_t n
) {
    if (n == 0 || !src) {
        return;
    }
    if (array_heap_elems(src) || array_heap_elems(dst)) {
        for (size_t i = 0; i < n; i++) {
            int64_t slot = *array_i64_c(src, src_off + i);
            void *p = (void *)(intptr_t)slot;
            thoxa_dup(p);
            *array_i64(dst, dst_off + i) = slot;
        }
    } else {
        memcpy(
            array_slot(dst, dst_off),
            array_slot_c(src, src_off),
            size_mul(n, (size_t)src->elem_size)
        );
    }
}

/* Shallow copy n elements then optionally field-dup each destination slot. */
static void array_copy_items_ex(
    ThoxaArray *dst,
    const ThoxaArray *src,
    size_t n,
    ThoxaArrayElemFn elem_dup
) {
    if (n == 0 || !src) {
        return;
    }
    memcpy(dst->data, src->data, size_mul(n, (size_t)src->elem_size));
    if (elem_dup) {
        for (size_t i = 0; i < n; i++) {
            elem_dup(array_slot(dst, i));
        }
    }
}

/* Range variant of array_copy_items_ex (User inline elems). */
static void array_copy_range_ex(
    ThoxaArray *dst,
    size_t dst_off,
    const ThoxaArray *src,
    size_t src_off,
    size_t n,
    ThoxaArrayElemFn elem_dup
) {
    if (n == 0 || !src) {
        return;
    }
    memcpy(
        array_slot(dst, dst_off),
        array_slot_c(src, src_off),
        size_mul(n, (size_t)src->elem_size)
    );
    if (elem_dup) {
        for (size_t i = 0; i < n; i++) {
            elem_dup(array_slot(dst, dst_off + i));
        }
    }
}

ThoxaArray *thoxa_array_new(int heap_elems) {
    return array_alloc(
        0,
        0,
        heap_elems ? THOXA_ARRAY_FLAG_HEAP_ELEMS : 0,
        (uint32_t)sizeof(int64_t)
    );
}

ThoxaArray *thoxa_array_new_ex(uint32_t elem_size, uint32_t flags) {
    return array_alloc(0, 0, flags, elem_size);
}

ThoxaArray *thoxa_array_from_i64(const int64_t *items, size_t len) {
    ThoxaArray *a = array_alloc(len, len, 0, (uint32_t)sizeof(int64_t));
    if (len > 0 && items) {
        memcpy(a->data, items, size_mul(len, sizeof(int64_t)));
    }
    return a;
}

ThoxaArray *thoxa_array_from_ptrs(void *const *items, size_t len) {
    ThoxaArray *a =
        array_alloc(len, len, THOXA_ARRAY_FLAG_HEAP_ELEMS, (uint32_t)sizeof(int64_t));
    for (size_t i = 0; i < len; i++) {
        void *p = items ? items[i] : NULL;
        thoxa_dup(p);
        *array_i64(a, i) = (int64_t)(intptr_t)p;
    }
    return a;
}

ThoxaArray *thoxa_array_from_elems(
    const void *items,
    size_t len,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup
) {
    ThoxaArray *a = array_alloc(len, len, flags, elem_size);
    if (len > 0 && items) {
        memcpy(a->data, items, size_mul(len, (size_t)elem_size));
    }
    if (elem_dup) {
        for (size_t i = 0; i < len; i++) {
            elem_dup(array_slot(a, i));
        }
    }
    return a;
}

int64_t thoxa_array_count(const ThoxaArray *a) {
    return a ? (int64_t)a->len : 0;
}

int64_t thoxa_array_get_i64(const ThoxaArray *a, int64_t i) {
    array_bounds(a, i);
    return *array_i64_c(a, (size_t)i);
}

static ThoxaArray *array_clone(const ThoxaArray *a, ThoxaArrayElemFn elem_dup) {
    if (!a) {
        return array_alloc(0, 0, 0, (uint32_t)sizeof(int64_t));
    }
    ThoxaArray *n = array_alloc(a->len, a->len, a->flags, a->elem_size);
    if (elem_dup) {
        array_copy_items_ex(n, a, a->len, elem_dup);
    } else {
        array_copy_items(n, a, a->len);
    }
    return n;
}

static ThoxaArray *array_ensure_unique(
    ThoxaArray *a,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    if (!a) {
        return array_alloc(0, 0, 0, (uint32_t)sizeof(int64_t));
    }
    ThoxaHeader *h = header_of(a);
    if (h->refcount == 1) {
        return a;
    }
    ThoxaArray *n = array_clone(a, elem_dup);
    if (elem_drop) {
        thoxa_array_drop_ex(a, elem_drop);
    } else {
        thoxa_array_drop(a);
    }
    return n;
}

ThoxaArray *thoxa_array_set_i64(ThoxaArray *a, int64_t i, int64_t v) {
    a = array_ensure_unique(a, NULL, NULL);
    array_bounds(a, i);
    *array_i64(a, (size_t)i) = v;
    return a;
}

void *thoxa_array_get_ptr(const ThoxaArray *a, int64_t i) {
    array_bounds(a, i);
    return (void *)(intptr_t)(*array_i64_c(a, (size_t)i));
}

ThoxaArray *thoxa_array_set_ptr(ThoxaArray *a, int64_t i, void *p) {
    a = array_ensure_unique(a, NULL, NULL);
    array_bounds(a, i);
    void *old = (void *)(intptr_t)(*array_i64(a, (size_t)i));
    thoxa_dup(p);
    *array_i64(a, (size_t)i) = (int64_t)(intptr_t)p;
    thoxa_drop(old);
    return a;
}

void thoxa_array_get_elem(
    const ThoxaArray *a,
    int64_t i,
    void *out,
    ThoxaArrayElemFn elem_dup
) {
    array_bounds(a, i);
    if (!out) {
        thoxa_panic("array get_elem null out");
    }
    memcpy(out, array_slot_c(a, (size_t)i), (size_t)a->elem_size);
    if (elem_dup) {
        elem_dup(out);
    }
}

ThoxaArray *thoxa_array_set_elem(
    ThoxaArray *a,
    int64_t i,
    const void *elem,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    a = array_ensure_unique(a, elem_dup, elem_drop);
    array_bounds(a, i);
    if (!elem) {
        thoxa_panic("array set_elem null elem");
    }
    /* Drop fields of old element in place, then overwrite and own new fields. */
    if (elem_drop) {
        elem_drop(array_slot(a, (size_t)i));
    }
    memcpy(array_slot(a, (size_t)i), elem, (size_t)a->elem_size);
    if (elem_dup) {
        elem_dup(array_slot(a, (size_t)i));
    }
    return a;
}

/* An unshared array with spare capacity takes an append in place. Otherwise
 * append copies into a new array with doubled capacity, so building an array
 * one element at a time is amortized O(1) per append, not O(n). */
static int array_append_in_place(const ThoxaArray *a) {
    return a && header_of((void *)a)->refcount == 1 && a->len < a->cap;
}

static size_t array_grown_cap(size_t len) {
    return len < 4 ? 4 : size_dbl(len);
}

ThoxaArray *thoxa_array_append_i64(ThoxaArray *a, int64_t v) {
    if (array_append_in_place(a) && a->elem_size == (uint32_t)sizeof(int64_t)) {
        if (array_heap_elems(a)) {
            thoxa_dup((void *)(intptr_t)v);
        }
        *array_i64(a, a->len) = v;
        a->len += 1;
        return a;
    }
    size_t old_len = a ? a->len : 0;
    uint32_t flags = a ? a->flags : 0;
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n =
        array_alloc(new_len, array_grown_cap(old_len), flags, (uint32_t)sizeof(int64_t));
    array_copy_items(n, a, old_len);
    if (array_heap_elems(n)) {
        void *p = (void *)(intptr_t)v;
        thoxa_dup(p);
        *array_i64(n, old_len) = v;
    } else {
        *array_i64(n, old_len) = v;
    }
    thoxa_array_drop(a);
    return n;
}

ThoxaArray *thoxa_array_append_ptr(ThoxaArray *a, void *p) {
    if (array_append_in_place(a) && array_heap_elems(a)) {
        thoxa_dup(p);
        *array_i64(a, a->len) = (int64_t)(intptr_t)p;
        a->len += 1;
        return a;
    }
    size_t old_len = a ? a->len : 0;
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n = array_alloc(
        new_len,
        array_grown_cap(old_len),
        THOXA_ARRAY_FLAG_HEAP_ELEMS,
        (uint32_t)sizeof(int64_t)
    );
    /* Source may be empty non-heap from thoxa_array_new(0) mistype — force heap. */
    if (a && array_heap_elems(a)) {
        array_copy_items(n, a, old_len);
    } else if (a && old_len > 0) {
        /* Copy slots without element dup if source was non-heap (shouldn't happen). */
        memcpy(n->data, a->data, size_mul(old_len, sizeof(int64_t)));
    }
    thoxa_dup(p);
    *array_i64(n, old_len) = (int64_t)(intptr_t)p;
    thoxa_array_drop(a);
    return n;
}

ThoxaArray *thoxa_array_append_elem(
    ThoxaArray *a,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    size_t old_len = a ? a->len : 0;
    if (a) {
        elem_size = a->elem_size;
        flags = a->flags;
    }
    if (!elem) {
        thoxa_panic("array append_elem null elem");
    }
    if (array_append_in_place(a)) {
        memcpy(array_slot(a, old_len), elem, (size_t)elem_size);
        if (elem_dup) {
            elem_dup(array_slot(a, old_len));
        }
        a->len += 1;
        return a;
    }
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n = array_alloc(new_len, array_grown_cap(old_len), flags, elem_size);
    array_copy_items_ex(n, a, old_len, elem_dup);
    memcpy(array_slot(n, old_len), elem, (size_t)elem_size);
    if (elem_dup) {
        elem_dup(array_slot(n, old_len));
    }
    if (elem_drop) {
        thoxa_array_drop_ex(a, elem_drop);
    } else {
        thoxa_array_drop(a);
    }
    return n;
}

ThoxaArray *thoxa_array_drop_last(ThoxaArray *a) {
    if (!a || a->len == 0) {
        thoxa_panic("Array.dropLast on empty array");
    }
    size_t new_len = a->len - 1;
    ThoxaArray *n = array_alloc(new_len, new_len, a->flags, a->elem_size);
    array_copy_items(n, a, new_len);
    thoxa_array_drop(a);
    return n;
}

/* argv → Array<String> for `fn main(arguments: Array<String>)`. */
ThoxaArray *thoxa_argv_to_array(int argc, char **argv) {
    size_t n = argc > 0 ? (size_t)argc : 0;
    ThoxaArray *a =
        array_alloc(n, n, THOXA_ARRAY_FLAG_HEAP_ELEMS, (uint32_t)sizeof(int64_t));
    for (size_t i = 0; i < n; i++) {
        const char *s = (argv && argv[i]) ? argv[i] : "";
        ThoxaString *str = thoxa_string_new(s, strlen(s));
        *array_i64(a, i) = (int64_t)(intptr_t)str;
    }
    return a;
}

ThoxaArray *thoxa_array_drop_last_ex(
    ThoxaArray *a,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    if (!a || a->len == 0) {
        thoxa_panic("Array.dropLast on empty array");
    }
    size_t new_len = a->len - 1;
    ThoxaArray *n = array_alloc(new_len, new_len, a->flags, a->elem_size);
    array_copy_items_ex(n, a, new_len, elem_dup);
    if (elem_drop) {
        thoxa_array_drop_ex(a, elem_drop);
    } else {
        thoxa_array_drop(a);
    }
    return n;
}

ThoxaArray *thoxa_array_insert_at_i64(ThoxaArray *a, int64_t at, int64_t v) {
    size_t old_len = a ? a->len : 0;
    if (at < 0 || (size_t)at > old_len) {
        thoxa_panic("Array.insert out of range");
    }
    size_t pos = (size_t)at;
    uint32_t flags = a ? a->flags : 0;
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n =
        array_alloc(new_len, new_len, flags, (uint32_t)sizeof(int64_t));
    array_copy_range(n, 0, a, 0, pos);
    if (array_heap_elems(n)) {
        void *p = (void *)(intptr_t)v;
        thoxa_dup(p);
        *array_i64(n, pos) = v;
    } else {
        *array_i64(n, pos) = v;
    }
    array_copy_range(n, pos + 1, a, pos, old_len - pos);
    thoxa_array_drop(a);
    return n;
}

ThoxaArray *thoxa_array_insert_at_ptr(ThoxaArray *a, int64_t at, void *p) {
    size_t old_len = a ? a->len : 0;
    if (at < 0 || (size_t)at > old_len) {
        thoxa_panic("Array.insert out of range");
    }
    size_t pos = (size_t)at;
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n = array_alloc(
        new_len,
        new_len,
        THOXA_ARRAY_FLAG_HEAP_ELEMS,
        (uint32_t)sizeof(int64_t)
    );
    if (a && array_heap_elems(a)) {
        array_copy_range(n, 0, a, 0, pos);
        array_copy_range(n, pos + 1, a, pos, old_len - pos);
    } else if (a && old_len > 0) {
        if (pos > 0) {
            memcpy(n->data, a->data, pos * sizeof(int64_t));
        }
        if (pos < old_len) {
            memcpy(
                n->data + (pos + 1) * sizeof(int64_t),
                a->data + pos * sizeof(int64_t),
                (old_len - pos) * sizeof(int64_t)
            );
        }
    }
    thoxa_dup(p);
    *array_i64(n, pos) = (int64_t)(intptr_t)p;
    thoxa_array_drop(a);
    return n;
}

ThoxaArray *thoxa_array_insert_at_elem(
    ThoxaArray *a,
    int64_t at,
    const void *elem,
    uint32_t elem_size,
    uint32_t flags,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    size_t old_len = a ? a->len : 0;
    if (at < 0 || (size_t)at > old_len) {
        thoxa_panic("Array.insert out of range");
    }
    if (a) {
        elem_size = a->elem_size;
        flags = a->flags;
    }
    if (!elem) {
        thoxa_panic("array insert_at_elem null elem");
    }
    size_t pos = (size_t)at;
    size_t new_len = size_add(old_len, 1);
    ThoxaArray *n = array_alloc(new_len, new_len, flags, elem_size);
    array_copy_range_ex(n, 0, a, 0, pos, elem_dup);
    memcpy(array_slot(n, pos), elem, (size_t)elem_size);
    if (elem_dup) {
        elem_dup(array_slot(n, pos));
    }
    array_copy_range_ex(n, pos + 1, a, pos, old_len - pos, elem_dup);
    if (elem_drop) {
        thoxa_array_drop_ex(a, elem_drop);
    } else {
        thoxa_array_drop(a);
    }
    return n;
}

ThoxaArray *thoxa_array_remove_at(ThoxaArray *a, int64_t at) {
    if (!a || a->len == 0) {
        thoxa_panic("Array.removeAt out of range");
    }
    if (at < 0 || (size_t)at >= a->len) {
        thoxa_panic("Array.removeAt out of range");
    }
    size_t pos = (size_t)at;
    size_t new_len = a->len - 1;
    ThoxaArray *n = array_alloc(new_len, new_len, a->flags, a->elem_size);
    array_copy_range(n, 0, a, 0, pos);
    array_copy_range(n, pos, a, pos + 1, new_len - pos);
    thoxa_array_drop(a);
    return n;
}

ThoxaArray *thoxa_array_remove_at_ex(
    ThoxaArray *a,
    int64_t at,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    if (!a || a->len == 0) {
        thoxa_panic("Array.removeAt out of range");
    }
    if (at < 0 || (size_t)at >= a->len) {
        thoxa_panic("Array.removeAt out of range");
    }
    size_t pos = (size_t)at;
    size_t new_len = a->len - 1;
    ThoxaArray *n = array_alloc(new_len, new_len, a->flags, a->elem_size);
    array_copy_range_ex(n, 0, a, 0, pos, elem_dup);
    array_copy_range_ex(n, pos, a, pos + 1, new_len - pos, elem_dup);
    if (elem_drop) {
        thoxa_array_drop_ex(a, elem_drop);
    } else {
        thoxa_array_drop(a);
    }
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
                thoxa_drop((void *)(intptr_t)(*array_i64(a, i)));
            }
        }
        free(h);
    }
}

void thoxa_array_drop_ex(ThoxaArray *a, ThoxaArrayElemFn elem_drop) {
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
        if (elem_drop) {
            for (size_t i = 0; i < a->len; i++) {
                elem_drop(array_slot(a, i));
            }
        }
        free(h);
    }
}

/* --- Map ---------------------------------------------------------------- */

#define MAP_IDX_EMPTY ((int32_t)-1)
#define MAP_IDX_TOMB ((int32_t)-2)

/* Slots keep insertion order. `remove` marks its slot removed (and its
 * index entry a tombstone) instead of moving another slot into the hole;
 * `map_compact` packs the live slots, in order, when removed ones pass half
 * of `len`, when the slots run out, and before a read by position. */
struct ThoxaMap {
    size_t len;
    size_t slot_cap;
    size_t idx_cap;
    size_t tombs;
    /* Removed slots among the first `len`. */
    size_t dead;
    uint32_t key_kind;
    uint32_t val_kind;
    uint32_t val_size;
    uint32_t flags;
    /* THOXA_MAP_KEY_USER only (D179). */
    const ThoxaMapKeyOps *key_ops;
    uint8_t data[];
};

typedef struct {
    uint64_t hash;
    int64_t key;
    uint64_t removed;
} MapSlotHead;

static size_t map_align8(size_t n) {
    return (n + 7u) & ~(size_t)7u;
}

static uint32_t map_val_size_of(uint32_t val_size) {
    return val_size == 0 ? (uint32_t)sizeof(int64_t) : val_size;
}

static size_t map_slot_size_sz(uint32_t val_size) {
    return sizeof(MapSlotHead) + map_align8(map_val_size_of(val_size));
}

static size_t map_slot_size(const ThoxaMap *m) {
    return map_slot_size_sz(m->val_size);
}

static size_t map_payload_size(size_t slot_cap, size_t idx_cap, uint32_t val_size) {
    size_t slots = size_mul(slot_cap, map_slot_size_sz(val_size));
    size_t idx = size_mul(idx_cap, sizeof(int32_t));
    return size_add(offsetof(ThoxaMap, data), size_add(slots, idx));
}

static MapSlotHead *map_slot(ThoxaMap *m, size_t i) {
    return (MapSlotHead *)(m->data + i * map_slot_size(m));
}

static const MapSlotHead *map_slot_c(const ThoxaMap *m, size_t i) {
    return (const MapSlotHead *)(m->data + i * map_slot_size(m));
}

static void *map_slot_val(ThoxaMap *m, size_t i) {
    return (uint8_t *)map_slot(m, i) + sizeof(MapSlotHead);
}

static const void *map_slot_val_c(const ThoxaMap *m, size_t i) {
    return (const uint8_t *)map_slot_c(m, i) + sizeof(MapSlotHead);
}

static int32_t *map_index(ThoxaMap *m) {
    return (int32_t *)(m->data + m->slot_cap * map_slot_size(m));
}

static const int32_t *map_index_c(const ThoxaMap *m) {
    return (const int32_t *)(m->data + m->slot_cap * map_slot_size(m));
}

static uint64_t map_hash_i64(int64_t k) {
    uint64_t x = (uint64_t)k;
    x ^= x >> 30;
    x *= 0xbf58476d1ce4e5b9ULL;
    x ^= x >> 27;
    x *= 0x94d049bb133111ebULL;
    x ^= x >> 31;
    return x;
}

static uint64_t map_hash_str(const ThoxaString *s) {
    const unsigned char *p = (const unsigned char *)(s ? s->data : "");
    size_t n = s ? s->len : 0;
    uint64_t h = 14695981039346656037ULL;
    for (size_t i = 0; i < n; i++) {
        h ^= (uint64_t)p[i];
        h *= 1099511628211ULL;
    }
    return h;
}

uint64_t thoxa_hash_string(const ThoxaString *s) {
    return map_hash_str(s);
}

uint64_t thoxa_hash_bytes(const ThoxaBytes *b) {
    uint64_t h = 14695981039346656037ULL;
    size_t n = b ? b->len : 0;
    for (size_t i = 0; i < n; i++) {
        h ^= (uint64_t)b->data[i];
        h *= 1099511628211ULL;
    }
    return h;
}

static uint64_t map_hash(const ThoxaMap *m, int64_t key) {
    if (m->key_kind == THOXA_MAP_KEY_STR) {
        return map_hash_str((const ThoxaString *)(intptr_t)key);
    }
    if (m->key_kind == THOXA_MAP_KEY_USER) {
        /* Spread the generated hash the way an Int64 key is spread. */
        return map_hash_i64((int64_t)m->key_ops->hash((const void *)(intptr_t)key));
    }
    return map_hash_i64(key);
}

static int map_key_eq(const ThoxaMap *m, int64_t a, int64_t b) {
    if (m->key_kind == THOXA_MAP_KEY_STR) {
        return thoxa_string_eq(
            (const ThoxaString *)(intptr_t)a,
            (const ThoxaString *)(intptr_t)b
        );
    }
    if (m->key_kind == THOXA_MAP_KEY_USER) {
        return m->key_ops->equal((const void *)(intptr_t)a, (const void *)(intptr_t)b);
    }
    return a == b;
}

/* Stored keys are retained values: a String, or a user key's box. */
static void map_dup_key(const ThoxaMap *m, int64_t key) {
    if (m->key_kind == THOXA_MAP_KEY_STR || m->key_kind == THOXA_MAP_KEY_USER) {
        thoxa_dup((void *)(intptr_t)key);
    }
}

static void map_drop_key(const ThoxaMap *m, int64_t key) {
    if (m->key_kind == THOXA_MAP_KEY_STR || m->key_kind == THOXA_MAP_KEY_USER) {
        thoxa_drop((void *)(intptr_t)key);
    }
}

/* The key bits a new slot stores: a String is retained; a user key (the
 * caller's struct) is copied into a box that owns its fields (D179). */
static int64_t map_own_key(const ThoxaMap *m, int64_t key) {
    if (m->key_kind != THOXA_MAP_KEY_USER) {
        map_dup_key(m, key);
        return key;
    }
    const ThoxaMapKeyOps *ops = m->key_ops;
    void *box = thoxa_alloc(ops->size, THOXA_TAG_USER);
    memcpy(box, (const void *)(intptr_t)key, ops->size);
    ops->dup(box);
    thoxa_set_dtor(box, ops->drop);
    return (int64_t)(intptr_t)box;
}

static void map_dup_val_ptr(void *p) {
    thoxa_dup(p);
}

static void map_drop_val_ptr(void *p) {
    thoxa_drop(p);
}

static ThoxaMap *map_alloc(
    size_t slot_cap,
    size_t idx_cap,
    uint32_t key_kind,
    const ThoxaMapKeyOps *key_ops,
    uint32_t val_kind,
    uint32_t val_size
) {
    if (idx_cap < 8) {
        idx_cap = 8;
    }
    if (slot_cap < 4) {
        slot_cap = 4;
    }
    val_size = map_val_size_of(val_size);
    ThoxaMap *m = (ThoxaMap *)thoxa_alloc(
        map_payload_size(slot_cap, idx_cap, val_size),
        THOXA_TAG_MAP
    );
    m->len = 0;
    m->slot_cap = slot_cap;
    m->idx_cap = idx_cap;
    m->tombs = 0;
    m->dead = 0;
    m->key_kind = key_kind;
    m->key_ops = key_ops;
    m->val_kind = val_kind;
    m->val_size = val_size;
    m->flags = 0;
    int32_t *idx = map_index(m);
    for (size_t i = 0; i < idx_cap; i++) {
        idx[i] = MAP_IDX_EMPTY;
    }
    return m;
}

static void map_reindex(ThoxaMap *m) {
    int32_t *idx = map_index(m);
    for (size_t i = 0; i < m->idx_cap; i++) {
        idx[i] = MAP_IDX_EMPTY;
    }
    m->tombs = 0;
    size_t mask = m->idx_cap - 1;
    for (size_t s = 0; s < m->len; s++) {
        MapSlotHead *sl = map_slot(m, s);
        if (sl->removed) {
            continue;
        }
        size_t pos = (size_t)sl->hash & mask;
        while (idx[pos] != MAP_IDX_EMPTY) {
            pos = (pos + 1) & mask;
        }
        idx[pos] = (int32_t)s;
    }
}

static int32_t map_find_slot(
    const ThoxaMap *m,
    int64_t key,
    uint64_t hash,
    int32_t *idx_pos
) {
    if (!m || m->idx_cap == 0) {
        if (idx_pos) {
            *idx_pos = -1;
        }
        return -1;
    }
    size_t mask = m->idx_cap - 1;
    size_t pos = (size_t)hash & mask;
    int32_t first_tomb = -1;
    const int32_t *idx = map_index_c(m);
    for (size_t n = 0; n < m->idx_cap; n++) {
        int32_t s = idx[pos];
        if (s == MAP_IDX_EMPTY) {
            if (idx_pos) {
                *idx_pos = first_tomb >= 0 ? first_tomb : (int32_t)pos;
            }
            return -1;
        }
        if (s == MAP_IDX_TOMB) {
            if (first_tomb < 0) {
                first_tomb = (int32_t)pos;
            }
        } else {
            const MapSlotHead *sl = map_slot_c(m, (size_t)s);
            if (sl->hash == hash && map_key_eq(m, sl->key, key)) {
                if (idx_pos) {
                    *idx_pos = (int32_t)pos;
                }
                return s;
            }
        }
        pos = (pos + 1) & mask;
    }
    if (idx_pos) {
        *idx_pos = first_tomb;
    }
    return -1;
}

static void map_copy_slots(
    ThoxaMap *dst,
    const ThoxaMap *src,
    ThoxaArrayElemFn elem_dup
) {
    size_t out = 0;
    for (size_t i = 0; i < src->len; i++) {
        const MapSlotHead *s = map_slot_c(src, i);
        if (s->removed) {
            continue;
        }
        MapSlotHead *d = map_slot(dst, out);
        *d = *s;
        map_dup_key(src, s->key);
        memcpy(map_slot_val(dst, out), map_slot_val_c(src, i), (size_t)src->val_size);
        if (src->val_kind == THOXA_MAP_VAL_PTR) {
            int64_t bits;
            memcpy(&bits, map_slot_val_c(src, i), sizeof(int64_t));
            map_dup_val_ptr((void *)(intptr_t)bits);
        } else if (src->val_kind == THOXA_MAP_VAL_USER && elem_dup) {
            elem_dup(map_slot_val(dst, out));
        }
        out += 1;
    }
    dst->len = out;
    dst->dead = 0;
    map_reindex(dst);
}

static ThoxaMap *map_clone(const ThoxaMap *m, ThoxaArrayElemFn elem_dup) {
    size_t slot_cap = m->slot_cap;
    size_t idx_cap = m->idx_cap;
    ThoxaMap *n =
        map_alloc(slot_cap, idx_cap, m->key_kind, m->key_ops, m->val_kind, m->val_size);
    map_copy_slots(n, m, elem_dup);
    return n;
}

static void map_drop_contents(ThoxaMap *m, ThoxaArrayElemFn elem_drop) {
    for (size_t i = 0; i < m->len; i++) {
        MapSlotHead *sl = map_slot(m, i);
        if (sl->removed) {
            continue;
        }
        map_drop_key(m, sl->key);
        if (m->val_kind == THOXA_MAP_VAL_PTR) {
            int64_t bits;
            memcpy(&bits, map_slot_val(m, i), sizeof(int64_t));
            map_drop_val_ptr((void *)(intptr_t)bits);
        } else if (m->val_kind == THOXA_MAP_VAL_USER && elem_drop) {
            elem_drop(map_slot_val(m, i));
        }
    }
}

static ThoxaMap *map_ensure_unique(ThoxaMap *m, ThoxaArrayElemFn elem_dup, ThoxaArrayElemFn elem_drop) {
    if (!m) {
        thoxa_panic("Map write on null map");
    }
    ThoxaHeader *h = header_of(m);
    if (h->refcount == 1) {
        return m;
    }
    ThoxaMap *n = map_clone(m, elem_dup);
    if (elem_drop) {
        thoxa_map_drop_ex(m, elem_drop);
    } else {
        thoxa_map_drop(m);
    }
    return n;
}

static ThoxaMap *map_grow_slots(ThoxaMap *m, ThoxaArrayElemFn elem_dup, ThoxaArrayElemFn elem_drop) {
    size_t new_slots = size_dbl(m->slot_cap);
    size_t new_idx = m->idx_cap;
    if (m->len > SIZE_MAX / 4 || new_idx > SIZE_MAX / 3) {
        thoxa_panic("allocation size overflow");
    }
    if (m->len * 4 >= new_idx * 3) {
        new_idx = size_dbl(new_idx);
    }
    ThoxaMap *n =
        map_alloc(new_slots, new_idx, m->key_kind, m->key_ops, m->val_kind, m->val_size);
    map_copy_slots(n, m, elem_dup);
    if (elem_drop) {
        thoxa_map_drop_ex(m, elem_drop);
    } else {
        thoxa_map_drop(m);
    }
    return n;
}

/* Pack the live slots to the front, in order, and rebuild the index. The
 * entries do not change, so this is safe on a shared map. */
static void map_compact(ThoxaMap *m) {
    if (!m || m->dead == 0) {
        return;
    }
    size_t slot_size = map_slot_size(m);
    size_t out = 0;
    for (size_t i = 0; i < m->len; i++) {
        if (map_slot(m, i)->removed) {
            continue;
        }
        if (out != i) {
            memmove(map_slot(m, out), map_slot(m, i), slot_size);
        }
        out += 1;
    }
    m->len = out;
    m->dead = 0;
    map_reindex(m);
}

/* Reads by position (`for k, v`, `key_at`) see the live slots only. */
static const ThoxaMap *map_settled(const ThoxaMap *m) {
    if (m && m->dead > 0) {
        map_compact((ThoxaMap *)m);
    }
    return m;
}

static ThoxaMap *map_prepare_insert(
    ThoxaMap *m,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    m = map_ensure_unique(m, elem_dup, elem_drop);
    if (m->len >= m->slot_cap && m->dead > 0) {
        /* Removed slots hold the room: pack the live ones first. */
        map_compact(m);
    }
    if (m->len >= m->slot_cap || m->len * 4 >= m->idx_cap * 3) {
        m = map_grow_slots(m, elem_dup, elem_drop);
    } else if (m->tombs > m->idx_cap / 2) {
        /* Too many tombstones but room for live entries: rebuild the index
         * at the same size. Growing here doubled an empty map without bound. */
        map_reindex(m);
    }
    return m;
}

ThoxaMap *thoxa_map_new(uint32_t key_kind, uint32_t val_kind, uint32_t val_size) {
    if (key_kind != THOXA_MAP_KEY_I64 && key_kind != THOXA_MAP_KEY_STR) {
        thoxa_panic("Map key kind not supported");
    }
    if (val_kind > THOXA_MAP_VAL_USER) {
        thoxa_panic("Map value kind not supported");
    }
    return map_alloc(8, 16, key_kind, NULL, val_kind, val_size);
}

ThoxaMap *thoxa_map_new_keyed(const ThoxaMapKeyOps *key_ops, uint32_t val_kind, uint32_t val_size) {
    if (!key_ops || key_ops->size == 0) {
        thoxa_panic("Map key ops missing");
    }
    if (val_kind > THOXA_MAP_VAL_USER) {
        thoxa_panic("Map value kind not supported");
    }
    return map_alloc(8, 16, THOXA_MAP_KEY_USER, key_ops, val_kind, val_size);
}

int64_t thoxa_map_count(const ThoxaMap *m) {
    return m ? (int64_t)(m->len - m->dead) : 0;
}

int thoxa_map_has(const ThoxaMap *m, int64_t key_bits) {
    if (!m) {
        return 0;
    }
    uint64_t h = map_hash(m, key_bits);
    return map_find_slot(m, key_bits, h, NULL) >= 0;
}

int thoxa_map_get_i64(const ThoxaMap *m, int64_t key_bits, int64_t *out) {
    if (!m) {
        return 0;
    }
    uint64_t h = map_hash(m, key_bits);
    int32_t s = map_find_slot(m, key_bits, h, NULL);
    if (s < 0) {
        return 0;
    }
    if (out) {
        memcpy(out, map_slot_val_c(m, (size_t)s), sizeof(int64_t));
    }
    return 1;
}

int thoxa_map_get_ptr(const ThoxaMap *m, int64_t key_bits, void **out) {
    int64_t bits = 0;
    if (!thoxa_map_get_i64(m, key_bits, &bits)) {
        return 0;
    }
    if (out) {
        *out = (void *)(intptr_t)bits;
    }
    return 1;
}

int thoxa_map_get_elem(
    const ThoxaMap *m,
    int64_t key_bits,
    void *out,
    ThoxaArrayElemFn elem_dup
) {
    if (!m) {
        return 0;
    }
    uint64_t h = map_hash(m, key_bits);
    int32_t s = map_find_slot(m, key_bits, h, NULL);
    if (s < 0) {
        return 0;
    }
    if (out) {
        memcpy(out, map_slot_val_c(m, (size_t)s), (size_t)m->val_size);
        if (elem_dup) {
            elem_dup(out);
        }
    }
    return 1;
}

static ThoxaMap *map_set_raw(
    ThoxaMap *m,
    int64_t key_bits,
    const void *val,
    size_t val_n,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop,
    int val_is_ptr
) {
    uint64_t h = map_hash(m, key_bits);
    int32_t ipos = -1;
    int32_t found = map_find_slot(m, key_bits, h, &ipos);
    if (found >= 0) {
        m = map_ensure_unique(m, elem_dup, elem_drop);
        found = map_find_slot(m, key_bits, h, NULL);
        void *dst = map_slot_val(m, (size_t)found);
        if (m->val_kind == THOXA_MAP_VAL_PTR) {
            int64_t old = 0;
            memcpy(&old, dst, sizeof(int64_t));
            if (val_is_ptr) {
                int64_t bits;
                memcpy(&bits, val, sizeof(int64_t));
                map_dup_val_ptr((void *)(intptr_t)bits);
            }
            memcpy(dst, val, val_n);
            map_drop_val_ptr((void *)(intptr_t)old);
        } else if (m->val_kind == THOXA_MAP_VAL_USER) {
            if (elem_drop) {
                elem_drop(dst);
            }
            memcpy(dst, val, val_n);
            if (elem_dup) {
                elem_dup(dst);
            }
        } else {
            memcpy(dst, val, val_n);
        }
        return m;
    }
    m = map_prepare_insert(m, elem_dup, elem_drop);
    h = map_hash(m, key_bits);
    found = map_find_slot(m, key_bits, h, &ipos);
    if (found >= 0) {
        /* grew/rehashed; treat as overwrite (shouldn't happen for new keys). */
        memcpy(map_slot_val(m, (size_t)found), val, val_n);
        return m;
    }
    if (ipos < 0) {
        thoxa_panic("Map index full");
    }
    int32_t *idx = map_index(m);
    if (idx[ipos] == MAP_IDX_TOMB) {
        m->tombs -= 1;
    }
    size_t slot = m->len;
    MapSlotHead *sl = map_slot(m, slot);
    sl->hash = h;
    sl->key = map_own_key(m, key_bits);
    sl->removed = 0;
    memcpy(map_slot_val(m, slot), val, val_n);
    if (m->val_kind == THOXA_MAP_VAL_PTR && val_is_ptr) {
        int64_t bits;
        memcpy(&bits, val, sizeof(int64_t));
        map_dup_val_ptr((void *)(intptr_t)bits);
    } else if (m->val_kind == THOXA_MAP_VAL_USER && elem_dup) {
        elem_dup(map_slot_val(m, slot));
    }
    idx[ipos] = (int32_t)slot;
    m->len += 1;
    return m;
}

ThoxaMap *thoxa_map_set_i64(ThoxaMap *m, int64_t key_bits, int64_t val) {
    return map_set_raw(m, key_bits, &val, sizeof(int64_t), NULL, NULL, 0);
}

ThoxaMap *thoxa_map_set_ptr(ThoxaMap *m, int64_t key_bits, void *val) {
    int64_t bits = (int64_t)(intptr_t)val;
    return map_set_raw(m, key_bits, &bits, sizeof(int64_t), NULL, NULL, 1);
}

ThoxaMap *thoxa_map_set_elem(
    ThoxaMap *m,
    int64_t key_bits,
    const void *val,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    if (!val) {
        thoxa_panic("Map.set null value");
    }
    return map_set_raw(
        m,
        key_bits,
        val,
        (size_t)m->val_size,
        elem_dup,
        elem_drop,
        0
    );
}

static ThoxaMap *map_remove_raw(
    ThoxaMap *m,
    int64_t key_bits,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    if (!m) {
        return m;
    }
    uint64_t h = map_hash(m, key_bits);
    int32_t ipos = -1;
    int32_t found = map_find_slot(m, key_bits, h, &ipos);
    if (found < 0) {
        return m;
    }
    m = map_ensure_unique(m, elem_dup, elem_drop);
    h = map_hash(m, key_bits);
    found = map_find_slot(m, key_bits, h, &ipos);
    if (found < 0) {
        return m;
    }
    MapSlotHead *sl = map_slot(m, (size_t)found);
    map_drop_key(m, sl->key);
    if (m->val_kind == THOXA_MAP_VAL_PTR) {
        int64_t bits = 0;
        memcpy(&bits, map_slot_val(m, (size_t)found), sizeof(int64_t));
        map_drop_val_ptr((void *)(intptr_t)bits);
    } else if (m->val_kind == THOXA_MAP_VAL_USER && elem_drop) {
        elem_drop(map_slot_val(m, (size_t)found));
    }
    int32_t *idx = map_index(m);
    if (ipos >= 0) {
        idx[ipos] = MAP_IDX_TOMB;
        m->tombs += 1;
    }
    /* Keep the order: mark the slot, and pack once half are removed. */
    sl->removed = 1;
    m->dead += 1;
    if (m->dead * 2 > m->len) {
        map_compact(m);
    }
    return m;
}

ThoxaMap *thoxa_map_remove(ThoxaMap *m, int64_t key_bits) {
    return map_remove_raw(m, key_bits, NULL, NULL);
}

ThoxaMap *thoxa_map_remove_ex(
    ThoxaMap *m,
    int64_t key_bits,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    return map_remove_raw(m, key_bits, elem_dup, elem_drop);
}

int64_t thoxa_map_key_at(const ThoxaMap *m, int64_t i) {
    m = map_settled(m);
    if (!m || i < 0 || (size_t)i >= m->len) {
        thoxa_panic("Map index out of range");
    }
    return map_slot_c(m, (size_t)i)->key;
}

int64_t thoxa_map_val_i64_at(const ThoxaMap *m, int64_t i) {
    m = map_settled(m);
    if (!m || i < 0 || (size_t)i >= m->len) {
        thoxa_panic("Map index out of range");
    }
    int64_t v = 0;
    memcpy(&v, map_slot_val_c(m, (size_t)i), sizeof(int64_t));
    return v;
}

void *thoxa_map_val_ptr_at(const ThoxaMap *m, int64_t i) {
    return (void *)(intptr_t)thoxa_map_val_i64_at(m, i);
}

void thoxa_map_val_elem_at(
    const ThoxaMap *m,
    int64_t i,
    void *out,
    ThoxaArrayElemFn elem_dup
) {
    m = map_settled(m);
    if (!m || i < 0 || (size_t)i >= m->len) {
        thoxa_panic("Map index out of range");
    }
    if (!out) {
        thoxa_panic("Map val_elem_at null out");
    }
    memcpy(out, map_slot_val_c(m, (size_t)i), (size_t)m->val_size);
    if (elem_dup) {
        elem_dup(out);
    }
}

ThoxaArray *thoxa_map_keys(const ThoxaMap *m) {
    m = map_settled(m);
    size_t n = m ? m->len : 0;
    if (m && m->key_kind == THOXA_MAP_KEY_USER) {
        /* Array<K> of inline structs: each element is a retained copy. */
        const ThoxaMapKeyOps *ops = m->key_ops;
        ThoxaArray *a = thoxa_array_new_ex(ops->size, 0u);
        for (size_t i = 0; i < n; i++) {
            const void *box = (const void *)(intptr_t)map_slot_c(m, i)->key;
            a = thoxa_array_append_elem(a, box, ops->size, 0u, ops->dup, ops->drop);
        }
        return a;
    }
    int heap = m && m->key_kind == THOXA_MAP_KEY_STR;
    ThoxaArray *a = thoxa_array_new(heap);
    for (size_t i = 0; i < n; i++) {
        int64_t k = map_slot_c(m, i)->key;
        if (heap) {
            a = thoxa_array_append_ptr(a, (void *)(intptr_t)k);
        } else {
            a = thoxa_array_append_i64(a, k);
        }
    }
    return a;
}

ThoxaArray *thoxa_map_values_i64(const ThoxaMap *m) {
    m = map_settled(m);
    size_t n = m ? m->len : 0;
    ThoxaArray *a = thoxa_array_new(0);
    for (size_t i = 0; i < n; i++) {
        int64_t v = 0;
        memcpy(&v, map_slot_val_c(m, i), sizeof(int64_t));
        a = thoxa_array_append_i64(a, v);
    }
    return a;
}

ThoxaArray *thoxa_map_values_ptr(const ThoxaMap *m) {
    m = map_settled(m);
    size_t n = m ? m->len : 0;
    ThoxaArray *a = thoxa_array_new(1);
    for (size_t i = 0; i < n; i++) {
        int64_t v = 0;
        memcpy(&v, map_slot_val_c(m, i), sizeof(int64_t));
        a = thoxa_array_append_ptr(a, (void *)(intptr_t)v);
    }
    return a;
}

ThoxaArray *thoxa_map_values_elem(
    const ThoxaMap *m,
    ThoxaArrayElemFn elem_dup,
    ThoxaArrayElemFn elem_drop
) {
    m = map_settled(m);
    size_t n = m ? m->len : 0;
    uint32_t es = m ? m->val_size : (uint32_t)sizeof(int64_t);
    ThoxaArray *a = thoxa_array_new_ex(es, 0);
    for (size_t i = 0; i < n; i++) {
        a = thoxa_array_append_elem(a, map_slot_val_c(m, i), es, 0, elem_dup, elem_drop);
    }
    return a;
}

void thoxa_map_dup(ThoxaMap *m) {
    thoxa_dup(m);
}

void thoxa_map_drop(ThoxaMap *m) {
    thoxa_map_drop_ex(m, NULL);
}

void thoxa_map_drop_ex(ThoxaMap *m, ThoxaArrayElemFn elem_drop) {
    if (!m) {
        return;
    }
    ThoxaHeader *h = header_of(m);
    if (!h) {
        return;
    }
    if (h->refcount == 0) {
        thoxa_panic("drop on zero refcount");
    }
    h->refcount -= 1;
    if (h->refcount == 0) {
        map_drop_contents(m, elem_drop);
        free(h);
    }
}

/* --- Process start ------------------------------------------------------ */

/* The kernel drops a signal whose action is the default when it is sent to
 * PID 1, so a program that is a container's init never ends on `podman
 * stop`'s SIGTERM and is killed after the timeout. As PID 1, end on SIGTERM
 * and SIGINT the way any other process does: at once, with the status a
 * shell reports for that signal (D170). */
static void thoxa_init_signal_exit(int sig) {
    _exit(128 + sig);
}

void thoxa_program_start(void) {
    if (getpid() != 1) {
        return;
    }
    static const int signals[] = {SIGTERM, SIGINT};
    for (size_t i = 0; i < sizeof signals / sizeof signals[0]; i++) {
        struct sigaction current;
        if (sigaction(signals[i], NULL, &current) != 0 || current.sa_handler != SIG_DFL) {
            continue; /* ignored on purpose, or someone else's handler */
        }
        struct sigaction action;
        memset(&action, 0, sizeof action);
        action.sa_handler = thoxa_init_signal_exit;
        sigemptyset(&action.sa_mask);
        (void)sigaction(signals[i], &action, NULL);
    }
}

/* --- Clock and host name (std/env) -------------------------------------- */

/* Plain statics: the runtime is single-threaded. */
const char *thoxa_local_hms(void) {
    static char buf[16];
    time_t t = time(NULL);
    struct tm tm;
    if (localtime_r(&t, &tm) == NULL) {
        memcpy(buf, "??:??:??", 9);
        return buf;
    }
    if (strftime(buf, sizeof buf, "%H:%M:%S", &tm) == 0) {
        memcpy(buf, "??:??:??", 9);
        return buf;
    }
    return buf;
}

const char *thoxa_hostname(void) {
    static char buf[256];
    if (gethostname(buf, sizeof buf) != 0) {
        memcpy(buf, "localhost", 10);
        return buf;
    }
    buf[sizeof buf - 1] = '\0';
    return buf;
}

/* --- HTML tree (builtin Node) ------------------------------------------- */

static void thoxa_attr_dup(void *p) {
    Thoxa_Attr *a = (Thoxa_Attr *)p;
    thoxa_dup(a->name);
    thoxa_dup(a->value);
}

static void thoxa_attr_drop(void *p) {
    Thoxa_Attr *a = (Thoxa_Attr *)p;
    thoxa_drop(a->name);
    thoxa_drop(a->value);
}

static void thoxa_html_free_payload(ThoxaHtml *n) {
    if (!n) {
        return;
    }
    thoxa_drop(n->tag);
    thoxa_drop(n->text);
    thoxa_array_drop_ex(n->attrs, thoxa_attr_drop);
    thoxa_array_drop(n->kids);
}

static ThoxaArray *html_own_kid_arr(ThoxaArray *a) {
    if (!a) {
        return thoxa_array_new(1);
    }
    thoxa_array_dup(a);
    return a;
}

static ThoxaArray *html_own_attr_arr(ThoxaArray *a) {
    if (!a) {
        return thoxa_array_new_ex((uint32_t)sizeof(Thoxa_Attr), 0);
    }
    thoxa_array_dup(a);
    return a;
}

ThoxaHtml *thoxa_html_el(ThoxaString *tag, ThoxaArray *attrs, ThoxaArray *kids) {
    ThoxaHtml *n = (ThoxaHtml *)thoxa_alloc(sizeof(ThoxaHtml), THOXA_TAG_HTML);
    n->kind = THOXA_HTML_EL;
    n->tag = tag;
    thoxa_dup(tag);
    n->text = NULL;
    n->attrs = html_own_attr_arr(attrs);
    n->kids = html_own_kid_arr(kids);
    return n;
}

static ThoxaString *html_escape_text(const ThoxaString *s) {
    if (!s || s->len == 0) {
        return thoxa_string_new("", 0);
    }
    size_t extra = 0;
    for (size_t i = 0; i < s->len; i++) {
        char c = s->data[i];
        if (c == '&') {
            extra += 4;
        } else if (c == '<' || c == '>') {
            extra += 3;
        }
    }
    if (extra == 0) {
        thoxa_dup((void *)s);
        return (ThoxaString *)s;
    }
    size_t nlen = s->len + extra;
    char *buf = (char *)malloc(nlen);
    if (!buf) {
        thoxa_panic("out of memory");
    }
    size_t o = 0;
    for (size_t i = 0; i < s->len; i++) {
        char c = s->data[i];
        if (c == '&') {
            memcpy(buf + o, "&amp;", 5);
            o += 5;
        } else if (c == '<') {
            memcpy(buf + o, "&lt;", 4);
            o += 4;
        } else if (c == '>') {
            memcpy(buf + o, "&gt;", 4);
            o += 4;
        } else {
            buf[o++] = c;
        }
    }
    ThoxaString *out = thoxa_string_new(buf, o);
    free(buf);
    return out;
}

ThoxaHtml *thoxa_html_text(ThoxaString *s) {
    ThoxaHtml *n = (ThoxaHtml *)thoxa_alloc(sizeof(ThoxaHtml), THOXA_TAG_HTML);
    n->kind = THOXA_HTML_TEXT;
    n->tag = NULL;
    n->text = html_escape_text(s);
    n->attrs = NULL;
    n->kids = NULL;
    return n;
}

ThoxaHtml *thoxa_html_frag(ThoxaArray *kids) {
    ThoxaHtml *n = (ThoxaHtml *)thoxa_alloc(sizeof(ThoxaHtml), THOXA_TAG_HTML);
    n->kind = THOXA_HTML_FRAG;
    n->tag = NULL;
    n->text = NULL;
    n->attrs = NULL;
    n->kids = html_own_kid_arr(kids);
    return n;
}

int64_t thoxa_html_kind(const ThoxaHtml *n) {
    return n ? (int64_t)n->kind : 0;
}

ThoxaString *thoxa_html_tag(const ThoxaHtml *n) {
    if (!n || !n->tag) {
        return thoxa_string_new("", 0);
    }
    thoxa_dup(n->tag);
    return n->tag;
}

ThoxaString *thoxa_html_get_text(const ThoxaHtml *n) {
    if (!n || !n->text) {
        return thoxa_string_new("", 0);
    }
    thoxa_dup(n->text);
    return n->text;
}

ThoxaArray *thoxa_html_attrs(const ThoxaHtml *n) {
    if (!n || !n->attrs) {
        return thoxa_array_new_ex((uint32_t)sizeof(Thoxa_Attr), 0);
    }
    thoxa_array_dup(n->attrs);
    return n->attrs;
}

ThoxaArray *thoxa_html_kids(const ThoxaHtml *n) {
    if (!n || !n->kids) {
        return thoxa_array_new(1);
    }
    thoxa_array_dup(n->kids);
    return n->kids;
}

static int html_is_void(const ThoxaString *tag) {
    if (!tag) {
        return 0;
    }
    const char *s = thoxa_string_cstr(tag);
    return strcmp(s, "br") == 0 || strcmp(s, "hr") == 0 || strcmp(s, "img") == 0
        || strcmp(s, "input") == 0 || strcmp(s, "meta") == 0
        || strcmp(s, "link") == 0;
}

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} HtmlBuf;

static void html_buf_grow(HtmlBuf *b, size_t more) {
    b->data = (char *)grow_buf(b->data, &b->cap, size_add(b->len, more));
}

static void html_buf_app(HtmlBuf *b, const char *s, size_t n) {
    if (!s || n == 0) {
        return;
    }
    html_buf_grow(b, n);
    memcpy(b->data + b->len, s, n);
    b->len += n;
}

static void html_buf_cstr(HtmlBuf *b, const char *s) {
    if (s) {
        html_buf_app(b, s, strlen(s));
    }
}

static void html_buf_esc_attr(HtmlBuf *b, const ThoxaString *s) {
    if (!s) {
        return;
    }
    for (size_t i = 0; i < s->len; i++) {
        char c = s->data[i];
        if (c == '&') {
            html_buf_cstr(b, "&amp;");
        } else if (c == '"') {
            html_buf_cstr(b, "&quot;");
        } else {
            html_buf_app(b, &c, 1);
        }
    }
}

static void html_render_into(const ThoxaHtml *n, HtmlBuf *b);

static void html_render_kids(const ThoxaArray *kids, HtmlBuf *b) {
    if (!kids) {
        return;
    }
    int64_t n = thoxa_array_count(kids);
    for (int64_t i = 0; i < n; i++) {
        ThoxaHtml *k = (ThoxaHtml *)thoxa_array_get_ptr(kids, i);
        html_render_into(k, b);
    }
}

static void html_render_into(const ThoxaHtml *n, HtmlBuf *b) {
    if (!n) {
        return;
    }
    if (n->kind == THOXA_HTML_TEXT) {
        if (n->text) {
            html_buf_app(b, n->text->data, n->text->len);
        }
        return;
    }
    if (n->kind == THOXA_HTML_FRAG) {
        html_render_kids(n->kids, b);
        return;
    }
    html_buf_cstr(b, "<");
    if (n->tag) {
        html_buf_app(b, n->tag->data, n->tag->len);
    }
    int64_t nattr = thoxa_array_count(n->attrs);
    for (int64_t i = 0; i < nattr; i++) {
        Thoxa_Attr a;
        thoxa_array_get_elem(n->attrs, i, &a, NULL);
        html_buf_cstr(b, " ");
        if (a.name) {
            html_buf_app(b, a.name->data, a.name->len);
        }
        /* Empty value = boolean attribute (`disabled`, not `disabled="disabled"`). */
        if (a.value && a.value->len > 0) {
            html_buf_cstr(b, "=\"");
            html_buf_esc_attr(b, a.value);
            html_buf_cstr(b, "\"");
        }
    }
    html_buf_cstr(b, ">");
    if (!html_is_void(n->tag)) {
        html_render_kids(n->kids, b);
        html_buf_cstr(b, "</");
        if (n->tag) {
            html_buf_app(b, n->tag->data, n->tag->len);
        }
        html_buf_cstr(b, ">");
    }
}

ThoxaString *thoxa_html_render(const ThoxaHtml *n) {
    HtmlBuf b = {0};
    html_render_into(n, &b);
    ThoxaString *s = thoxa_string_new(b.data ? b.data : "", b.len);
    free(b.data);
    return s;
}

/* --- Cooperative tasks (M:1 fibers) ------------------------------------ */

/* Usable stack per task. Pages are committed on first touch, so this is
 * address space, not resident memory. A PROT_NONE guard page sits below it. */
#define THOXA_FIBER_STACK (512u * 1024u)
/* Freed stacks kept for reuse, so a busy server does not mmap per connection. */
#define THOXA_STACK_POOL 32
#define THOXA_ACCEPT_BACKOFF_MS 50
/* 2^62 ns in ms (about 146 years): now_ns + ms * 1e6 stays below INT64_MAX. */
#define THOXA_WAIT_MS_MAX (INT64_C(4611686018427))

typedef struct ThoxaFiber {
    ucontext_t ctx;
    /* Start of the mapping: guard page(s), then THOXA_FIBER_STACK bytes. */
    void *stack;
    void (*fn)(void *);
    void *data;
    /* Sleep deadline; with `wait_fd` >= 0, the fd wait's deadline (0 = none). */
    int64_t wake_at_ns;
    int32_t wait_fd;
    short wait_events;
    /* Set when an fd wait ended at its deadline instead of readiness. */
    int timed_out;
    int done;
    /* Sockets this task owns (`thoxa_task_own_socket`), closed if it panics. */
    int owned_fds;
    /* Under LeakSanitizer a panicked task's stack stays mapped as a root. */
    int keep_stack;
    struct ThoxaFiber *next;
} ThoxaFiber;

static ucontext_t thoxa_sched_ctx;
static ThoxaFiber *thoxa_current;
static ThoxaFiber *thoxa_ready_head;
static ThoxaFiber *thoxa_ready_tail;
static ThoxaFiber *thoxa_sleeping;
static int thoxa_pumping;

static void thoxa_pump(void);
static void thoxa_wake_due(int64_t now);
static int64_t thoxa_next_wake_ns(void);
static int thoxa_idle(int extra_fd, short extra_events, int timeout_ms);
static int thoxa_wait_fd_until(int32_t fd, int32_t events, int64_t deadline_ns);

static int64_t thoxa_now_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        thoxa_panic("clock_gettime failed");
    }
    return (int64_t)ts.tv_sec * 1000000000LL + (int64_t)ts.tv_nsec;
}

static void thoxa_nsleep(int64_t ns) {
    if (ns <= 0) {
        return;
    }
    struct timespec ts;
    ts.tv_sec = (time_t)(ns / 1000000000LL);
    ts.tv_nsec = (long)(ns % 1000000000LL);
    while (nanosleep(&ts, &ts) != 0) {
        if (errno != EINTR) {
            break;
        }
    }
}

static void thoxa_enqueue_ready(ThoxaFiber *f) {
    f->next = NULL;
    f->wake_at_ns = 0;
    f->wait_fd = -1;
    f->wait_events = 0;
    if (!thoxa_ready_tail) {
        thoxa_ready_head = thoxa_ready_tail = f;
    } else {
        thoxa_ready_tail->next = f;
        thoxa_ready_tail = f;
    }
}

static ThoxaFiber *thoxa_dequeue_ready(void) {
    ThoxaFiber *f = thoxa_ready_head;
    if (!f) {
        return NULL;
    }
    thoxa_ready_head = f->next;
    if (!thoxa_ready_head) {
        thoxa_ready_tail = NULL;
    }
    f->next = NULL;
    return f;
}

static void *thoxa_stack_pool[THOXA_STACK_POOL];
static int thoxa_stack_pool_count;

static size_t thoxa_guard_size(void) {
    static size_t page;
    if (!page) {
        long size = sysconf(_SC_PAGESIZE);
        page = size > 0 ? (size_t)size : 4096u;
    }
    return page;
}

static char thoxa_overflow_altstack[64 * 1024];

/* A fault inside the current task's guard page is a stack overflow: say so,
 * then let the default action end the process. Other faults pass through. */
static void thoxa_segv_handler(int sig, siginfo_t *info, void *context) {
    (void)context;
    if (thoxa_current) {
        char *guard = (char *)thoxa_current->stack;
        char *addr = (char *)info->si_addr;
        if (addr >= guard && addr < guard + thoxa_guard_size()) {
            static const char msg[] = "thoxa panic: stack overflow in task\n";
            ssize_t written = write(STDERR_FILENO, msg, sizeof msg - 1);
            (void)written;
        }
    }
    signal(sig, SIG_DFL);
}

static void thoxa_install_overflow_handler(void) {
    static int installed;
    if (installed) {
        return;
    }
    installed = 1;
    struct sigaction current;
    if (sigaction(SIGSEGV, NULL, &current) != 0 || current.sa_handler != SIG_DFL) {
        return; /* someone else (a sanitizer, a host) owns SIGSEGV */
    }
    stack_t alt;
    alt.ss_sp = thoxa_overflow_altstack;
    alt.ss_size = sizeof thoxa_overflow_altstack;
    alt.ss_flags = 0;
    if (sigaltstack(&alt, NULL) != 0) {
        return;
    }
    struct sigaction action;
    memset(&action, 0, sizeof action);
    action.sa_sigaction = thoxa_segv_handler;
    action.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_RESETHAND;
    sigemptyset(&action.sa_mask);
    (void)sigaction(SIGSEGV, &action, NULL);
}

static void *thoxa_stack_alloc(void) {
    if (thoxa_stack_pool_count > 0) {
        return thoxa_stack_pool[--thoxa_stack_pool_count];
    }
    size_t guard = thoxa_guard_size();
    void *map = mmap(
        NULL,
        guard + THOXA_FIBER_STACK,
        PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE | MAP_STACK,
        -1,
        0
    );
    if (map == MAP_FAILED) {
        thoxa_panic("out of memory");
    }
    if (mprotect(map, guard, PROT_NONE) != 0) {
        thoxa_panic("mprotect failed for task stack guard");
    }
    return map;
}

static void thoxa_stack_free(void *map) {
    if (thoxa_stack_pool_count < THOXA_STACK_POOL) {
        thoxa_stack_pool[thoxa_stack_pool_count++] = map;
        return;
    }
    (void)munmap(map, thoxa_guard_size() + THOXA_FIBER_STACK);
}

static void thoxa_fiber_free(ThoxaFiber *f) {
    if (!f->keep_stack) {
        thoxa_stack_free(f->stack);
    }
    free(f);
}

/* Task-owned sockets (D166), indexed by fd: the owning task and the
 * socket's box. A `std/net` Socket box starts with its `fd: Int64` field. */
typedef struct ThoxaOwnedFd {
    ThoxaFiber *task;
    int64_t *fd_field;
} ThoxaOwnedFd;

static ThoxaOwnedFd *thoxa_fd_owner;
static int32_t thoxa_fd_owner_cap;
static int64_t thoxa_owned_fd_count;

void thoxa_task_own_socket(void *socket) {
    int64_t *fd_field = (int64_t *)socket;
    if (!thoxa_current || !fd_field || *fd_field < 0 || *fd_field > INT32_MAX) {
        return;
    }
    int32_t fd = (int32_t)*fd_field;
    if (fd >= thoxa_fd_owner_cap) {
        int32_t cap = thoxa_fd_owner_cap ? thoxa_fd_owner_cap : 64;
        while (cap <= fd) {
            cap *= 2;
        }
        ThoxaOwnedFd *grown =
            (ThoxaOwnedFd *)realloc(thoxa_fd_owner, (size_t)cap * sizeof(ThoxaOwnedFd));
        if (!grown) {
            thoxa_panic("out of memory");
        }
        memset(grown + thoxa_fd_owner_cap, 0,
               (size_t)(cap - thoxa_fd_owner_cap) * sizeof(ThoxaOwnedFd));
        thoxa_fd_owner = grown;
        thoxa_fd_owner_cap = cap;
    }
    ThoxaOwnedFd *slot = &thoxa_fd_owner[fd];
    if (slot->task) {
        slot->task->owned_fds--;
    } else {
        thoxa_owned_fd_count++;
    }
    slot->task = thoxa_current;
    slot->fd_field = fd_field;
    thoxa_current->owned_fds++;
}

static void thoxa_fd_forget(int32_t fd) {
    if (fd < 0 || fd >= thoxa_fd_owner_cap || !thoxa_fd_owner[fd].task) {
        return;
    }
    thoxa_fd_owner[fd].task->owned_fds--;
    thoxa_fd_owner[fd].task = NULL;
    thoxa_fd_owner[fd].fd_field = NULL;
    thoxa_owned_fd_count--;
}

int32_t thoxa_close(int32_t fd) {
    thoxa_fd_forget(fd);
    return (int32_t)close(fd);
}

int64_t thoxa_task_owned_fds(void) {
    return thoxa_owned_fd_count;
}

/* A task that returns gives up its sockets. One that panicked closes them
 * and sets each box's fd to -1, so every copy of the Socket (a registry, an
 * SSE subscriber list) sees it closed and none can reach a reused fd. */
static int thoxa_task_release_fds(ThoxaFiber *f, int close_them) {
    int closed = 0;
    for (int32_t fd = 0; f->owned_fds > 0 && fd < thoxa_fd_owner_cap; fd++) {
        if (thoxa_fd_owner[fd].task != f) {
            continue;
        }
        int64_t *fd_field = thoxa_fd_owner[fd].fd_field;
        thoxa_fd_forget(fd);
        if (close_them) {
            (void)close(fd);
            *fd_field = -1;
            closed++;
        }
    }
    return closed;
}

/* Present only when the program links LeakSanitizer (THOXA_SANITIZE=leak). */
extern void __lsan_register_root_region(const void *p, size_t size) __attribute__((weak));

/* Called from `thoxa_panic` on the task's own stack. The task never resumes:
 * its frames are abandoned where they stand, so what they held leaks, by
 * design (R4). Its packed arguments, owned sockets, and stack are given back,
 * and the scheduler runs the other tasks. Outside a task this returns. */
static void thoxa_task_end_by_panic(void) {
    ThoxaFiber *f = thoxa_current;
    if (!f) {
        return;
    }
    int closed = thoxa_task_release_fds(f, 1);
    thoxa_task_free(f->data);
    f->data = NULL;
    if (closed > 0) {
        fprintf(stderr, "thoxa: task ended by panic; closed %d socket%s\n",
                closed, closed == 1 ? "" : "s");
    } else {
        fputs("thoxa: task ended by panic\n", stderr);
    }
    if (__lsan_register_root_region) {
        /* Count what the dead frames still point at as reachable, so the
         * leak gate reports real leaks, not this one. */
        f->keep_stack = 1;
        __lsan_register_root_region((char *)f->stack + thoxa_guard_size(), THOXA_FIBER_STACK);
    }
    f->done = 1;
    swapcontext(&f->ctx, &thoxa_sched_ctx);
    abort(); /* the scheduler frees a done task; it never comes back */
}

static void thoxa_fiber_entry(void) {
    ThoxaFiber *f = thoxa_current;
    f->fn(f->data);
    thoxa_task_release_fds(f, 0);
    f->done = 1;
    swapcontext(&f->ctx, &thoxa_sched_ctx);
}

static void thoxa_pump(void) {
    for (;;) {
        ThoxaFiber *f = thoxa_dequeue_ready();
        if (!f) {
            break;
        }
        thoxa_current = f;
        swapcontext(&thoxa_sched_ctx, &f->ctx);
        thoxa_current = NULL;
        if (f->done) {
            thoxa_fiber_free(f);
        }
    }
}

void *thoxa_task_alloc(size_t nbytes) {
    void *p = malloc(nbytes ? nbytes : 1);
    if (!p) {
        thoxa_panic("out of memory");
    }
    return p;
}

void thoxa_task_free(void *ptr) {
    free(ptr);
}

void thoxa_task_start(void (*fn)(void *), void *data) {
    ThoxaFiber *f = (ThoxaFiber *)calloc(1, sizeof(ThoxaFiber));
    if (!f) {
        thoxa_panic("out of memory");
    }
    thoxa_install_overflow_handler();
    f->stack = thoxa_stack_alloc();
    f->fn = fn;
    f->data = data;
    f->wait_fd = -1;
    if (getcontext(&f->ctx) != 0) {
        thoxa_panic("getcontext failed");
    }
    f->ctx.uc_stack.ss_sp = (char *)f->stack + thoxa_guard_size();
    f->ctx.uc_stack.ss_size = THOXA_FIBER_STACK;
    f->ctx.uc_link = &thoxa_sched_ctx;
    makecontext(&f->ctx, thoxa_fiber_entry, 0);
    thoxa_enqueue_ready(f);
    if (!thoxa_pumping) {
        thoxa_pumping = 1;
        thoxa_pump();
        thoxa_pumping = 0;
    }
}

static int thoxa_ns_to_poll_ms(int64_t ns) {
    if (ns < 0) {
        return -1;
    }
    if (ns == 0) {
        return 0;
    }
    int64_t ms = (ns + 999999LL) / 1000000LL;
    if (ms > 86400000LL) {
        ms = 86400000LL;
    }
    return (int)ms;
}

static short thoxa_poll_events(int32_t ev) {
    short p = (short)(POLLERR | POLLHUP);
    if (ev & THOXA_WAIT_READ) {
        p = (short)(p | POLLIN);
    }
    if (ev & THOXA_WAIT_WRITE) {
        p = (short)(p | POLLOUT);
    }
    return p;
}

static int thoxa_sleeping_has_fd(void) {
    for (ThoxaFiber *f = thoxa_sleeping; f; f = f->next) {
        if (f->wait_fd >= 0) {
            return 1;
        }
    }
    return 0;
}

static int thoxa_idle(int extra_fd, short extra_events, int timeout_ms) {
    int nfd = 0;
    for (ThoxaFiber *f = thoxa_sleeping; f; f = f->next) {
        if (f->wait_fd >= 0) {
            nfd++;
        }
    }
    int extra = extra_fd >= 0 ? 1 : 0;
    int n = nfd + extra;
    if (n == 0) {
        if (timeout_ms > 0) {
            thoxa_nsleep((int64_t)timeout_ms * 1000000LL);
        }
        return 0;
    }
    struct pollfd *pfds = (struct pollfd *)calloc((size_t)n, sizeof(struct pollfd));
    ThoxaFiber **own = (ThoxaFiber **)calloc((size_t)n, sizeof(ThoxaFiber *));
    if (!pfds || !own) {
        thoxa_panic("out of memory");
    }
    int i = 0;
    for (ThoxaFiber *f = thoxa_sleeping; f; f = f->next) {
        if (f->wait_fd < 0) {
            continue;
        }
        pfds[i].fd = f->wait_fd;
        pfds[i].events = f->wait_events;
        own[i] = f;
        i++;
    }
    if (extra) {
        pfds[i].fd = extra_fd;
        pfds[i].events = extra_events;
        own[i] = NULL;
    }
    int pr;
    do {
        pr = poll(pfds, (nfds_t)n, timeout_ms);
    } while (pr < 0 && errno == EINTR);
    int extra_ready = 0;
    if (pr > 0) {
        ThoxaFiber *keep = NULL;
        ThoxaFiber *f = thoxa_sleeping;
        while (f) {
            ThoxaFiber *next = f->next;
            int wake = 0;
            if (f->wait_fd >= 0) {
                for (int j = 0; j < nfd; j++) {
                    if (own[j] == f && pfds[j].revents) {
                        wake = 1;
                        break;
                    }
                }
            }
            if (wake) {
                thoxa_enqueue_ready(f);
            } else {
                f->next = keep;
                keep = f;
            }
            f = next;
        }
        thoxa_sleeping = keep;
        if (extra && pfds[nfd].revents) {
            extra_ready = 1;
        }
    }
    free(pfds);
    free(own);
    return extra_ready;
}

/* Run tasks until `until_ns` (-1 = until idle) or until `extra_fd` is ready.
 * Returns 1 when `extra_fd` became ready. */
static int thoxa_host_idle_until(int64_t until_ns, int extra_fd, short extra_events) {
    int saved = thoxa_pumping;
    int extra_ready = 0;
    thoxa_pumping = 1;
    for (;;) {
        int64_t now = thoxa_now_ns();
        thoxa_wake_due(now);
        thoxa_pump();
        now = thoxa_now_ns();
        if (until_ns >= 0 && now >= until_ns) {
            break;
        }
        if (!thoxa_sleeping && extra_fd < 0) {
            break;
        }
        int64_t next = thoxa_next_wake_ns();
        int has_fd = thoxa_sleeping_has_fd() || extra_fd >= 0;
        int64_t slice = -1;
        if (until_ns >= 0) {
            slice = until_ns - now;
        }
        if (next >= 0 && (slice < 0 || next - now < slice)) {
            slice = next - now;
        }
        if (!has_fd) {
            if (slice < 0) {
                break;
            }
            thoxa_nsleep(slice);
            continue;
        }
        if (thoxa_idle(extra_fd, extra_events, thoxa_ns_to_poll_ms(slice))) {
            extra_ready = 1;
            break;
        }
    }
    thoxa_pumping = saved;
    return extra_ready;
}

void thoxa_wait_ms(int64_t ms) {
    if (ms < 0) {
        ms = 0;
    }
    /* Keep now + ms in range; a wrapped deadline would never wake. */
    if (ms > THOXA_WAIT_MS_MAX) {
        ms = THOXA_WAIT_MS_MAX;
    }
    if (!thoxa_current) {
        thoxa_host_idle_until(thoxa_now_ns() + ms * 1000000LL, -1, 0);
        return;
    }
    if (ms == 0) {
        thoxa_enqueue_ready(thoxa_current);
    } else {
        thoxa_current->wait_fd = -1;
        thoxa_current->wait_events = 0;
        thoxa_current->wake_at_ns = thoxa_now_ns() + ms * 1000000LL;
        thoxa_current->next = thoxa_sleeping;
        thoxa_sleeping = thoxa_current;
    }
    swapcontext(&thoxa_current->ctx, &thoxa_sched_ctx);
}

void thoxa_wait_fd(int32_t fd, int32_t events) {
    (void)thoxa_wait_fd_until(fd, events, 0);
}

/* Park until `fd` is ready or `deadline_ns` (monotonic; 0 = none) passes.
 * Returns 1 when ready, 0 at the deadline. */
static int thoxa_wait_fd_until(int32_t fd, int32_t events, int64_t deadline_ns) {
    short pe = thoxa_poll_events(events);
    if (!thoxa_current) {
        return thoxa_host_idle_until(deadline_ns > 0 ? deadline_ns : -1, fd, pe);
    }
    thoxa_current->wait_fd = fd;
    thoxa_current->wait_events = pe;
    thoxa_current->wake_at_ns = deadline_ns > 0 ? deadline_ns : 0;
    thoxa_current->timed_out = 0;
    thoxa_current->next = thoxa_sleeping;
    thoxa_sleeping = thoxa_current;
    swapcontext(&thoxa_current->ctx, &thoxa_sched_ctx);
    return !thoxa_current->timed_out;
}

static void thoxa_wake_due(int64_t now) {
    ThoxaFiber *keep = NULL;
    ThoxaFiber *f = thoxa_sleeping;
    while (f) {
        ThoxaFiber *next = f->next;
        if (f->wake_at_ns > 0 && f->wake_at_ns <= now) {
            /* A sleep is due, or an fd wait reached its deadline. */
            f->timed_out = f->wait_fd >= 0;
            thoxa_enqueue_ready(f);
        } else {
            f->next = keep;
            keep = f;
        }
        f = next;
    }
    thoxa_sleeping = keep;
}

static int64_t thoxa_next_wake_ns(void) {
    int64_t best = -1;
    for (ThoxaFiber *f = thoxa_sleeping; f; f = f->next) {
        if (f->wake_at_ns <= 0) {
            continue;
        }
        if (best < 0 || f->wake_at_ns < best) {
            best = f->wake_at_ns;
        }
    }
    return best;
}

void thoxa_sched_drain(void) {
    thoxa_host_idle_until(-1, -1, 0);
}

static int thoxa_set_nonblock(int fd) {
    int fl = fcntl(fd, F_GETFL, 0);
    if (fl < 0) {
        return -1;
    }
    if (fl & O_NONBLOCK) {
        return 0;
    }
    return fcntl(fd, F_SETFL, fl | O_NONBLOCK);
}

int32_t thoxa_accept(int32_t fd, void *addr, void *addrlen) {
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    for (;;) {
        int c = accept(fd, (struct sockaddr *)addr, (socklen_t *)addrlen);
        if (c >= 0) {
            (void)thoxa_set_nonblock(c);
            return (int32_t)c;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            thoxa_wait_fd(fd, THOXA_WAIT_READ);
            continue;
        }
        if (errno == EMFILE || errno == ENFILE || errno == ENOBUFS || errno == ENOMEM) {
            /* Out of descriptors: let other tasks run (and close theirs)
             * before retrying, instead of spinning on accept. */
            thoxa_wait_ms(THOXA_ACCEPT_BACKOFF_MS);
            continue;
        }
        return -1;
    }
}

int64_t thoxa_recv(int32_t fd, void *buf, uint64_t len, int32_t flags) {
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    for (;;) {
        ssize_t n = recv(fd, buf, (size_t)len, flags);
        if (n >= 0) {
            return (int64_t)n;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            thoxa_wait_fd(fd, THOXA_WAIT_READ);
            continue;
        }
        return -1;
    }
}

int64_t thoxa_send(int32_t fd, const void *buf, uint64_t len, int32_t flags) {
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    for (;;) {
        /* MSG_NOSIGNAL: a peer reset is an error return, not SIGPIPE. */
        ssize_t n = send(fd, buf, (size_t)len, flags | MSG_NOSIGNAL);
        if (n >= 0) {
            return (int64_t)n;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            thoxa_wait_fd(fd, THOXA_WAIT_WRITE);
            continue;
        }
        return -1;
    }
}

int64_t thoxa_now_ms(void) {
    return thoxa_now_ns() / 1000000LL;
}

/* recv / send that give up at `deadline_ms` (thoxa_now_ms clock; <= 0 = none):
 * -1 with errno ETIMEDOUT. A server's read and write deadlines. */
int64_t thoxa_recv_until(int32_t fd, void *buf, uint64_t len, int32_t flags, int64_t deadline_ms) {
    if (deadline_ms <= 0) {
        return thoxa_recv(fd, buf, len, flags);
    }
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    int64_t deadline_ns = deadline_ms * 1000000LL;
    for (;;) {
        ssize_t n = recv(fd, buf, (size_t)len, flags);
        if (n >= 0) {
            return (int64_t)n;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            if (thoxa_now_ns() >= deadline_ns
                || !thoxa_wait_fd_until(fd, THOXA_WAIT_READ, deadline_ns)) {
                errno = ETIMEDOUT;
                return -1;
            }
            continue;
        }
        return -1;
    }
}

int64_t thoxa_send_until(int32_t fd, const void *buf, uint64_t len, int32_t flags, int64_t deadline_ms) {
    if (deadline_ms <= 0) {
        return thoxa_send(fd, buf, len, flags);
    }
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    int64_t deadline_ns = deadline_ms * 1000000LL;
    for (;;) {
        ssize_t n = send(fd, buf, (size_t)len, flags | MSG_NOSIGNAL);
        if (n >= 0) {
            return (int64_t)n;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EAGAIN || errno == EWOULDBLOCK) {
            if (thoxa_now_ns() >= deadline_ns
                || !thoxa_wait_fd_until(fd, THOXA_WAIT_WRITE, deadline_ns)) {
                errno = ETIMEDOUT;
                return -1;
            }
            continue;
        }
        return -1;
    }
}

int32_t thoxa_connect(int32_t fd, const void *addr, uint32_t addrlen) {
    if (thoxa_set_nonblock(fd) < 0) {
        return -1;
    }
    for (;;) {
        int r = connect(fd, (const struct sockaddr *)addr, (socklen_t)addrlen);
        if (r == 0) {
            return 0;
        }
        if (errno == EINTR) {
            continue;
        }
        if (errno == EINPROGRESS) {
            thoxa_wait_fd(fd, THOXA_WAIT_WRITE);
            int err = 0;
            socklen_t elen = (socklen_t)sizeof(err);
            if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &elen) < 0) {
                return -1;
            }
            if (err != 0) {
                errno = err;
                return -1;
            }
            return 0;
        }
        return -1;
    }
}
