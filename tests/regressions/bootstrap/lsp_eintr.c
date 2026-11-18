/* Interrupt transport syscalls only. All framing/recovery is implemented in Aether. */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#ifndef __APPLE__
#include <dlfcn.h>
#endif
static unsigned observed;
#ifdef __APPLE__
#define REAL(name, type) name
#define REPLACE(name) test_##name
#define INTERPOSE(name) __attribute__((used)) static struct {const void *replacement; const void *original;} pair_##name __attribute__((section("__DATA,__interpose"))) = {(const void *)test_##name, (const void *)name};
#else
#define REAL(name, type) ((type)dlsym(RTLD_NEXT, #name))
#define REPLACE(name) name
#define INTERPOSE(name)
#endif
ssize_t REPLACE(read)(int fd, void *buffer, size_t count) {
    unsigned bit = count == 1 ? 1u : 2u;
    if (fd == 0 && !(observed & bit)) { observed |= bit; errno = EINTR; return -1; }
    return REAL(read, ssize_t (*)(int,void *,size_t))(fd, buffer, count);
}
INTERPOSE(read)
ssize_t REPLACE(write)(int fd, const void *buffer, size_t count) {
    unsigned bit = count <= 64 ? 4u : 8u;
    if (fd == 1 && !(observed & bit)) { observed |= bit; errno = EINTR; return -1; }
    return REAL(write, ssize_t (*)(int,const void *,size_t))(fd, buffer, count);
}
INTERPOSE(write)
__attribute__((destructor)) static void receipt(void) {
    const char *path = getenv("AETHER_LSP_EINTR_RECEIPT");
    if (!path) return;
    FILE *file = fopen(path, "wb");
    if (file) { fprintf(file, "%u\n", observed); fclose(file); }
}
