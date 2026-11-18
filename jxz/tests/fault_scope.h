/* Test preload applies to JXZ only. Its compiler, tools and programs run normally. */
#ifndef JXZ_TEST_FAULT_SCOPE_H
#define JXZ_TEST_FAULT_SCOPE_H

#include <stdlib.h>

__attribute__((constructor))
static void isolate_fault_injection(void) {
#ifdef __APPLE__
    unsetenv("DYLD_INSERT_LIBRARIES");
#else
    unsetenv("LD_PRELOAD");
#endif
}

#endif
