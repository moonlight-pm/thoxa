#ifndef THOXA_RT_H
#define THOXA_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- I/O ---------------------------------------------------------------- */

void thoxa_print(const char *s);
void thoxa_println(const char *s);
void thoxa_print_i64(int64_t v);
void thoxa_println_i64(int64_t v);
void thoxa_print_bool(bool v);
void thoxa_println_bool(bool v);
void thoxa_panic(const char *msg);

/* --- Heap header (RC foundation; used from R4+) ------------------------- */

typedef struct ThoxaHeader {
    size_t refcount;
    uint16_t type_tag;
    uint16_t flags;
    uint32_t reserved;
} ThoxaHeader;

void *thoxa_alloc(size_t payload_size, uint16_t type_tag);
void thoxa_dup(void *ptr);
void thoxa_drop(void *ptr);

#ifdef __cplusplus
}
#endif

#endif /* THOXA_RT_H */
