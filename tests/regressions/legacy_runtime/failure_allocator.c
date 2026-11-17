/* Reuse the R01 bounded allocator, including invalid-free/leak accounting.
 * Scheduling one failure after N calls exercises every parser cleanup edge. */
#define malloc tracked_malloc
#define realloc tracked_realloc
#include "../vec_alignment/alignment_allocator.c"
#undef malloc
#undef realloc
static uint64_t countdown;
static int armed;
static void scheduled(void) {
    if (active && armed) {
        if (countdown == 0) { armed = 0; fail_next = 1; }
        else --countdown;
    }
}
void *malloc(size_t size) { scheduled(); return tracked_malloc(size); }
void *realloc(void *pointer, size_t size) { scheduled(); return tracked_realloc(pointer, size); }
void legacy_begin(void) { armed = 0; r01_begin(); }
void legacy_fail_after(uint64_t count) { countdown = count; armed = 1; }
void legacy_allow(void) { armed = 0; r01_allow(); }
int32_t legacy_finish(void) { armed = 0; return r01_finish(); }
void legacy_observe(void *pointer) {
    static void *volatile observed;
    observed = pointer;
}
