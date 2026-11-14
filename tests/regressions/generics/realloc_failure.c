/* Test-only allocator failure. Language/library logic stays in AetherLang. */
#include <stdlib.h>
#include <stddef.h>
#ifdef __APPLE__
static void *test_realloc(void *pointer, size_t size) {
    if (size == 8192) return NULL;
    return realloc(pointer, size);
}
__attribute__((used)) static struct { const void *replacement; const void *original; }
interpose __attribute__((section("__DATA,__interpose"))) = { (const void *)test_realloc, (const void *)realloc };
#else
#include <dlfcn.h>
void *realloc(void *pointer, size_t size) {
    if (size == 8192) return NULL;
    void *(*original)(void *, size_t) = dlsym(RTLD_NEXT, "realloc");
    return original(pointer, size);
}
#endif
