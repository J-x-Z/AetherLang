// Native provider loading. Scheduling, tensor layouts and ownership live in Aether.
#include "provider.h"
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
typedef HMODULE Library;
typedef FARPROC Symbol;

static Library library_open(const char *path, int64_t *native) {
    int n = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, NULL, 0);
    if (!n) { *native = GetLastError(); return NULL; }
    wchar_t *wide = calloc(n, sizeof(*wide));
    if (!wide) { *native = ERROR_OUTOFMEMORY; return NULL; }
    Library library = NULL;
    DWORD needed = 0;
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, wide, n))
        needed = GetFullPathNameW(wide, 0, NULL, NULL);
    if (!needed) {
        *native = GetLastError();
    } else {
        wchar_t *full = calloc(needed, sizeof(*full));
        if (full) {
            DWORD len = GetFullPathNameW(wide, needed, full, NULL);
            if (len && len < needed)
                library = LoadLibraryExW(full, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
            if (!library) *native = len >= needed ? ERROR_INSUFFICIENT_BUFFER : GetLastError();
            free(full);
        } else {
            *native = ERROR_OUTOFMEMORY;
        }
    }
    free(wide);
    return library;
}

static int explicit_path(const char *path) { return strpbrk(path, "/\\") != NULL; }
static Symbol library_entry(Library lib) { return GetProcAddress(lib, "aether_gpu_provider_v1"); }
static void library_close(Library lib) { FreeLibrary(lib); }
#else
#include <dlfcn.h>
typedef void *Library;
typedef void *Symbol;
static Library library_open(const char *path, int64_t *native) {
    (void)native;
    return dlopen(path, RTLD_LOCAL | RTLD_NOW);
}
static int explicit_path(const char *path) { return strchr(path, '/') != NULL; }
static Symbol library_entry(Library lib) { return dlsym(lib, "aether_gpu_provider_v1"); }
static void library_close(Library lib) { dlclose(lib); }
#endif

typedef struct {
    Library library;
    void *device;
    const AetherGpuProviderV1 *api;
} Provider;

void *aether_gpu_open(int32_t backend, const char *path, int32_t *code, int64_t *native) {
    *code = AETHER_GPU_ARGUMENT;
    *native = 0;
    if (!path || !explicit_path(path)) return NULL;
    *code = AETHER_GPU_DEVICE;
    Library library = library_open(path, native);
    if (!library) return NULL;
    Symbol symbol = library_entry(library);
    const AetherGpuProviderV1 *(*entry)(void) = NULL;
    _Static_assert(sizeof(entry) == sizeof(symbol), "provider function pointer ABI");
    memcpy(&entry, &symbol, sizeof(entry));
    const AetherGpuProviderV1 *api = entry ? entry() : NULL;
    if (!api || api->version != AETHER_GPU_ABI_V1 || api->struct_bytes < sizeof(*api)
        || api->backend != backend || !api->open || !api->close || !api->caps
        || !api->buffer_new || !api->buffer_contents || !api->buffer_free
        || !api->submit_gemm || !api->event_status || !api->event_wait || !api->event_free) {
        library_close(library);
        return NULL;
    }
    Provider *provider = malloc(sizeof(*provider));
    if (!provider) {
        *code = AETHER_GPU_ALLOCATION;
        library_close(library);
        return NULL;
    }
    void *device = api->open(code, native);
    if (!device) {
        if (!*code) *code = AETHER_GPU_DEVICE;
        free(provider);
        library_close(library);
        return NULL;
    }
    *provider = (Provider){library, device, api};
    *code = 0;
    return provider;
}

void aether_gpu_close(void *ctx) {
    Provider *p = ctx;
    if (!p) return;
    p->api->close(p->device);
    library_close(p->library);
    free(p);
}

void aether_gpu_caps(void *ctx, AetherGpuCaps *caps) {
    Provider *p = ctx;
    memset(caps, 0, sizeof(*caps));
    if (p) p->api->caps(p->device, caps);
}

uint64_t aether_gpu_device_id(void *ctx) {
    AetherGpuCaps caps;
    aether_gpu_caps(ctx, &caps);
    return caps.device_id;
}

void *aether_gpu_buffer_new(void *ctx, uint64_t bytes, uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native) {
    Provider *p = ctx;
    *charge = 0;
    *native = 0;
    if (!p || !bytes) { *code = AETHER_GPU_ARGUMENT; return NULL; }
    if (bytes > remaining) { *code = AETHER_GPU_BUDGET; return NULL; }
    return p->api->buffer_new(p->device, bytes, remaining, charge, code, native);
}

void *aether_gpu_buffer_contents(void *ctx, void *buffer) {
    Provider *p = ctx;
    return p && buffer ? p->api->buffer_contents(p->device, buffer) : NULL;
}

void aether_gpu_buffer_free(void *ctx, void *buffer) {
    Provider *p = ctx;
    if (p && buffer) p->api->buffer_free(p->device, buffer);
}

void *aether_gpu_submit_gemm(void *ctx, void *a, void *b, void *c, const AetherGemm *params,
                             uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native) {
    Provider *p = ctx;
    *charge = 0;
    *native = 0;
    if (!p || !a || !b || !c || !params) { *code = AETHER_GPU_ARGUMENT; return NULL; }
    return p->api->submit_gemm(p->device, a, b, c, params, remaining, charge, code, native);
}

int32_t aether_gpu_event_status(void *ctx, void *event, int64_t *native) {
    Provider *p = ctx;
    *native = 0;
    if (!p || !event) return AETHER_GPU_FAILED;
    return p->api->event_status(p->device, event, native);
}

int32_t aether_gpu_event_wait(void *ctx, void *event, int64_t *native) {
    Provider *p = ctx;
    *native = 0;
    if (!p || !event) return AETHER_GPU_FAILED;
    return p->api->event_wait(p->device, event, native);
}

void aether_gpu_event_free(void *ctx, void *event) {
    Provider *p = ctx;
    if (p && event) p->api->event_free(p->device, event);
}
