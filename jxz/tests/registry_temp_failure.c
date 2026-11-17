/* Inject a failed temporary-file acquisition without changing its template.
 * Cleanup must never unlink a pathname for which mkstemp returned no fd. */
#include <dlfcn.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#ifdef __APPLE__
#define REPLACE test_mkstemp
#else
#define REPLACE mkstemp
#endif
int REPLACE(char *path) {
    const char *mode = getenv("JXZ_REGISTRY_TEMP_FAULT");
    if (mode && ((strcmp(mode, "lock") == 0 && strstr(path, ".jxz-lock-")) ||
                 (strcmp(mode, "index") == 0 && strstr(path, ".jxz-index-")))) {
        errno = EACCES;
        return -1;
    }
#ifdef __APPLE__
    int (*real_mkstemp)(char *) = mkstemp;
#else
    int (*real_mkstemp)(char *) = (int (*)(char *))dlsym(RTLD_NEXT, "mkstemp");
#endif
    if (!real_mkstemp) { errno = ENOSYS; return -1; }
    return real_mkstemp(path);
}

#ifdef __APPLE__
__attribute__((used)) static struct { const void *replacement; const void *original; }
    pair_mkstemp __attribute__((section("__DATA,__interpose"))) =
        {(const void *)test_mkstemp, (const void *)mkstemp};
#endif
