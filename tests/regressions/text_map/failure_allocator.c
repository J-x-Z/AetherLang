/* The optimizer may fold malloc + zero initialization into calloc. Interpose
 * all three allocation entry points so optimized tests use one allocator. */
#include "../legacy_runtime/failure_allocator.c"
void *calloc(size_t count, size_t size) {
    if (size != 0 && count > SIZE_MAX / size) return NULL;
    size_t total = count * size;
    void *pointer = malloc(total);
    if (pointer) {
        for (size_t i = 0; i < total; ++i) ((unsigned char *)pointer)[i] = 0;
    }
    return pointer;
}
