/* File publication fault injection; the transaction/recovery algorithm is Aether. */
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifndef __APPLE__
#include <dlfcn.h>
#endif
static FILE *records;
static int rollback, record_failed;
static int mode(const char *name) {
    const char *value = getenv("AETHER_JXZ_FAULT");
    return value && strcmp(value, name) == 0;
}
static int ends(const char *text, const char *suffix) {
    size_t a = strlen(text), b = strlen(suffix);
    return a >= b && strcmp(text + a - b, suffix) == 0;
}
#ifdef __APPLE__
#define REAL(name, type) name
#define REPLACE(name) test_##name
#define INTERPOSE(name) __attribute__((used)) static struct {const void *replacement; const void *original;} pair_##name __attribute__((section("__DATA,__interpose"))) = {(const void *)test_##name, (const void *)name};
#else
#define REAL(name, type) ((type)dlsym(RTLD_NEXT, #name))
#define REPLACE(name) name
#define INTERPOSE(name)
#endif
FILE *REPLACE(fopen)(const char *path, const char *flags) {
    int selected = ends(path, "/.jxz-build/records") && flags[0] == 'w';
    if (selected && mode("record-open")) { errno = EACCES; return NULL; }
    FILE *result = REAL(fopen, FILE *(*)(const char *, const char *))(path, flags);
    if (selected) records = result;
    return result;
}
int REPLACE(fputc)(int byte, FILE *file) {
    if (file == records && records && mode("record-write")) { errno = ENOSPC; return EOF; }
    return REAL(fputc, int (*)(int, FILE *))(byte, file);
}
int REPLACE(fputs)(const char *text, FILE *file) {
    if (file == records && records && mode("record-write")) { errno = ENOSPC; return EOF; }
    return REAL(fputs, int (*)(const char *, FILE *))(text, file);
}
int REPLACE(fclose)(FILE *file) {
    int selected = file == records && records;
    if (selected) records = NULL;
    int result = REAL(fclose, int (*)(FILE *))(file);
    if (selected && mode("record-close")) { errno = EIO; return EOF; }
    return result;
}
int REPLACE(rename)(const char *old, const char *next) {
    if (ends(old, "/.jxz-dependencies.tmp/record") && mode("kill-before-dependency-record")) {
        kill(getpid(), SIGKILL); _exit(99);
    }
    int publishing_record = ends(old, "/.jxz-build/records") && ends(next, "/.jxz-outputs");
    int publishing_binary = ends(old, "/.jxz-build/program");
    if (publishing_record && mode("kill-before-record")) { kill(getpid(), SIGKILL); _exit(99); }
    if (publishing_record && !record_failed && (mode("record-rename") || mode("rollback-failure"))) {
        record_failed = 1;
        rollback = 1; errno = EIO; return -1;
    }
    if (publishing_binary && mode("binary-rename") && !rollback) {
        rollback = 1; errno = EIO; return -1;
    }
    if (publishing_binary && mode("rollback-failure") && rollback) {
        errno = EIO; return -1;
    }
    return REAL(rename, int (*)(const char *, const char *))(old, next);
}
int REPLACE(symlink)(const char *target, const char *path) {
    if ((ends(path, "/.jxz-build/owner") && mode("kill-before-owner")) ||
        (ends(path, "/.jxz-build/ready") && mode("kill-before-ready")) ||
        (ends(path, "/.jxz-build/settled") && mode("kill-before-settled"))) {
        kill(getpid(), SIGKILL); _exit(99);
    }
    int result = REAL(symlink, int (*)(const char *, const char *))(target, path);
    if (result == 0 && ends(path, "/.jxz-dependencies-owner") && mode("kill-after-dependency-owner")) {
        kill(getpid(), SIGKILL); _exit(99);
    }
    if (result == 0 && ends(path, "/.jxz-build/owner") && mode("kill-after-owner")) {
        kill(getpid(), SIGKILL); _exit(99);
    }
    return result;
}
void REPLACE(perror)(const char *text) {
    REAL(perror, void (*)(const char *))(text);
    if (mode("perror-errno")) {
        static const char marker[] = "injected diagnostic errno change\n";
        (void)write(2, marker, sizeof marker - 1);
        errno = EIO;
    }
}
INTERPOSE(fopen)
INTERPOSE(fputc)
INTERPOSE(fputs)
INTERPOSE(fclose)
INTERPOSE(rename)
INTERPOSE(symlink)
INTERPOSE(perror)
