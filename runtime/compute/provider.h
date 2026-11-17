#ifndef AETHER_COMPUTE_PROVIDER_H
#define AETHER_COMPUTE_PROVIDER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum { AETHER_GPU_ABI_V1 = 1, AETHER_GPU_METAL = 2, AETHER_GPU_VULKAN = 3 };
enum { AETHER_GPU_ARGUMENT = 1, AETHER_GPU_OVERFLOW = 4, AETHER_GPU_BOUNDS = 5,
       AETHER_GPU_BUDGET = 6, AETHER_GPU_ALLOCATION = 7, AETHER_GPU_DEVICE = 10,
       AETHER_GPU_EXECUTION = 11 };
enum { AETHER_GPU_PENDING = 1, AETHER_GPU_COMPLETE = 2, AETHER_GPU_FAILED = 3 };

typedef struct {
    uint64_t m, n, k, ao, ar, ac, bo, br, bc, co, cr, cc;
} AetherGemm;

typedef struct {
    uint64_t device_id, heap_bytes, max_buffer_bytes, available_bytes;
    uint32_t vendor_id, fp64, unified_memory, queue_family;
    char name[256];
} AetherGpuCaps;

// All handles are owned by the Aether queue, buffer and event layers. A provider
// must reject charges larger than remaining *before* allocating. Mapped data is
// coherent on return from wait or terminal status. available_bytes may be zero
// when the SDK does not report an available budget. Sizes are bytes.
typedef struct {
    uint32_t version, struct_bytes;
    int32_t backend;
    void *(*open)(int32_t *code, int64_t *native);
    void (*close)(void *ctx);
    void (*caps)(void *ctx, AetherGpuCaps *caps);
    void *(*buffer_new)(void *ctx, uint64_t bytes, uint64_t remaining,
                        uint64_t *charge, int32_t *code, int64_t *native);
    void *(*buffer_contents)(void *ctx, void *buffer);
    void (*buffer_free)(void *ctx, void *buffer);
    void *(*submit_gemm)(void *ctx, void *a, void *b, void *c, const AetherGemm *params,
                         uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native);
    int32_t (*event_status)(void *ctx, void *event, int64_t *native);
    int32_t (*event_wait)(void *ctx, void *event, int64_t *native);
    void (*event_free)(void *ctx, void *event);
} AetherGpuProviderV1;

// The only symbol that a provider shared library needs to export.
const AetherGpuProviderV1 *aether_gpu_provider_v1(void);

// Plain C ABI used by stdlib/compute. Explicit path selects the installed
// provider. The loader verifies its ABI and backend before calling any hook.
void *aether_gpu_open(int32_t backend, const char *path, int32_t *code, int64_t *native);
void aether_gpu_close(void *ctx);
void aether_gpu_caps(void *ctx, AetherGpuCaps *caps);
uint64_t aether_gpu_device_id(void *ctx);
void *aether_gpu_buffer_new(void *ctx, uint64_t bytes, uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native);
void *aether_gpu_buffer_contents(void *ctx, void *buffer);
void aether_gpu_buffer_free(void *ctx, void *buffer);
void *aether_gpu_submit_gemm(void *ctx, void *a, void *b, void *c, const AetherGemm *params,
                             uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native);
int32_t aether_gpu_event_status(void *ctx, void *event, int64_t *native);
int32_t aether_gpu_event_wait(void *ctx, void *event, int64_t *native);
void aether_gpu_event_free(void *ctx, void *event);

#ifdef __cplusplus
}
#endif
#endif
