// POSIX provider loading. Scheduling, tensor layouts and ownership live in Aether.
#include "provider.h"
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    void *library, *device;
    const AetherGpuProviderV1 *api;
} Provider;

void *aether_gpu_open(int32_t backend, const char *path, int32_t *code, int64_t *native) {
    *code = AETHER_GPU_ARGUMENT;
    *native = 0;
    if (!path || !strchr(path, '/')) return NULL;
    *code = AETHER_GPU_DEVICE;
    void *library = dlopen(path, RTLD_LOCAL | RTLD_NOW);
    if (!library) return NULL;
    void *symbol = dlsym(library, "aether_gpu_provider_v1");
    const AetherGpuProviderV1 *(*entry)(void) = NULL;
    _Static_assert(sizeof(entry) == sizeof(symbol), "POSIX function pointer ABI");
    memcpy(&entry, &symbol, sizeof(entry));
    const AetherGpuProviderV1 *api = entry ? entry() : NULL;
    if (!api || api->version != AETHER_GPU_ABI_V1 || api->struct_bytes < sizeof(*api)
        || api->backend != backend || !api->open || !api->close || !api->caps
        || !api->buffer_new || !api->buffer_contents || !api->buffer_free
        || !api->submit_gemm || !api->event_status || !api->event_wait || !api->event_free) {
        dlclose(library);
        return NULL;
    }
    Provider *provider = malloc(sizeof(*provider));
    if (!provider) {
        *code = AETHER_GPU_ALLOCATION;
        dlclose(library);
        return NULL;
    }
    void *device = api->open(code, native);
    if (!device) {
        if (!*code) *code = AETHER_GPU_DEVICE;
        free(provider);
        dlclose(library);
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
    dlclose(p->library);
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
