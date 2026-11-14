#include <stdint.h>
#include <stdlib.h>
static int64_t remaining = -1;
static int64_t live = 0;
void test_fail_after(int64_t count) { remaining = count; }
int64_t test_live(void) { return live; }
static int fail(void) {
    if (remaining < 0) return 0;
    if (remaining == 0) return 1;
    --remaining;
    return 0;
}
void *test_malloc(uint64_t size) {
    if (fail()) return NULL;
    void *p = malloc((size_t)size);
    if (p) ++live;
    return p;
}
void *test_realloc(void *old, uint64_t size) {
    if (fail()) return NULL;
    void *p = realloc(old, (size_t)size);
    if (p && !old) ++live;
    return p;
}
void test_free(void *p) {
    if (p) --live;
    free(p);
}
