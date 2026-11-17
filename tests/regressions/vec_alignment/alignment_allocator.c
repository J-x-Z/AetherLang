/* Extends the R01 audit allocator: valid 16-byte fundamental alignment, but
 * deliberately never 32-byte alignment. Both malloc and realloc are covered,
 * including an optimizer's realloc(NULL, n) -> malloc(n) transformation. */
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

_Static_assert(_Alignof(max_align_t) <= 16, "test allocator must satisfy fundamental alignment");
#define BLOCKS 32
#define CAPACITY 65536
struct block {
    _Alignas(64) unsigned char bytes[CAPACITY + 64];
    size_t size;
    int used, tracked;
};
static struct block blocks[BLOCKS];
static int active, fail_next, live, errors;
static uint64_t calls;

static int find(void *pointer) {
    for (int i = 0; i < BLOCKS; ++i)
        if (pointer == blocks[i].bytes + 16) return i;
    return -1;
}
static void *allocate(size_t size) {
    if (size > CAPACITY) return NULL;
    for (int i = 0; i < BLOCKS; ++i) {
        if (blocks[i].used) continue;
        blocks[i].used = 1;
        blocks[i].tracked = active;
        blocks[i].size = size;
        if (active) ++live;
        return blocks[i].bytes + 16;
    }
    return NULL;
}
void *malloc(size_t size) {
    if (active) {
        ++calls;
        if (fail_next) { fail_next = 0; return NULL; }
    }
    return allocate(size);
}
void free(void *pointer) {
    if (!pointer) return;
    int i = find(pointer);
    if (i < 0 || !blocks[i].used) { if (active) ++errors; return; }
    if (blocks[i].tracked) --live;
    blocks[i].used = 0;
}
void *realloc(void *pointer, size_t size) {
    if (active) {
        ++calls;
        if (fail_next) { fail_next = 0; return NULL; }
    }
    if (!pointer) return allocate(size);
    int i = find(pointer);
    if (i < 0 || !blocks[i].used) { if (active) ++errors; return NULL; }
    void *next = allocate(size);
    if (!next) return NULL;
    size_t copy = size < blocks[i].size ? size : blocks[i].size;
    for (size_t n = 0; n < copy; ++n) ((unsigned char *)next)[n] = ((unsigned char *)pointer)[n];
    free(pointer);
    return next;
}
void r01_begin(void) { active = 1; fail_next = 0; live = 0; errors = 0; calls = 0; }
void r01_fail_next(void) { fail_next = 1; }
void r01_allow(void) { fail_next = 0; }
uint64_t r01_calls(void) { return calls; }
int32_t r01_finish(void) {
    int result = errors ? 91 : live ? 92 : 0;
    active = 0;
    return result;
}
