#ifndef THOXA_RT_H
#define THOXA_RT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* --- I/O (R0) ----------------------------------------------------------- */

void thoxa_print(const char *s);
void thoxa_println(const char *s);
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
