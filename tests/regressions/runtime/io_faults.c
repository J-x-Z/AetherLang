/* Test-only libc failure injection; application logic remains AetherLang. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <errno.h>
#include <unistd.h>
#include <dlfcn.h>
#if !defined(__APPLE__)
#define fault_fclose fclose
#define fault_fflush fflush
#define fault_printf printf
#endif

int fault_fclose(FILE *file) {
    static unsigned calls;
#if defined(__APPLE__)
    int (*original)(FILE *) = fclose; /* dyld excludes the replacement image */
#else
    int (*original)(FILE *) = (int (*)(FILE *))dlsym(RTLD_NEXT, "fclose");
#endif
    const char *which = getenv("AETHER_FAULT_CLOSE");
    ++calls;
    if (which && ((strcmp(which, "both") == 0 && calls <= 2) || (unsigned)atoi(which) == calls))
        close(fileno(file));
    return original(file);
}
int fault_fflush(FILE *file) {
    static unsigned global_calls;
#if defined(__APPLE__)
    int (*original)(FILE *) = fflush;
#else
    int (*original)(FILE *) = (int (*)(FILE *))dlsym(RTLD_NEXT, "fflush");
#endif
    if (!file && ++global_calls == 2 && getenv("AETHER_FAULT_FINAL_FLUSH")) {
        errno = EIO;
        return EOF;
    }
    return original(file);
}
int fault_printf(const char *format, ...) {
    if (getenv("AETHER_FAULT_BYE") && strncmp(format, "BYE ", 4) == 0) {
        errno = EIO;
        return -1;
    }
    va_list args;
    va_start(args, format);
    int result = vfprintf(stdout, format, args);
    va_end(args);
    return result;
}
#if defined(__APPLE__)
__attribute__((used)) static struct { const void *replacement; const void *original; }
interpose[] __attribute__((section("__DATA,__interpose"))) = {
    {(const void *)fault_fclose, (const void *)fclose},
    {(const void *)fault_fflush, (const void *)fflush},
    {(const void *)fault_printf, (const void *)printf},
};
#endif
