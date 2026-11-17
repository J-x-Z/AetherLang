/* Link the generated C object's allocation calls to independent counters.
   These wrappers leave allocation, alignment and deallocation to the host. */
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

static size_t live;
static size_t allocations;
void *counted_malloc(size_t n) {
    void *p = malloc(n);
    if (p) { live++; allocations++; }
    return p;
}
void *counted_realloc(void *p, size_t n) {
    /* This workload never requests a zero-size reallocation. */
    if (!n) abort();
    int new_allocation = p == NULL;
    void *q = realloc(p, n);
    if (new_allocation && q) { live++; allocations++; }
    return q;
}
void counted_free(void *p) {
    if (p) {
        if (!live) abort();
        live--;
    }
    free(p);
}
extern int owned_resource_program(void);
int main(void) {
    int status = owned_resource_program();
    printf("allocations=%zu live=%zu\n", allocations, live);
    return status ? status : (live || allocations < 128 ? 99 : 0);
}
