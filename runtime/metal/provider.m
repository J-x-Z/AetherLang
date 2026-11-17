// Platform-only adapter for the common compute provider ABI. Operator metadata,
// graph policy, budget ownership and the CPU algorithm remain in Aether.
// Shared no-copy buffers use page-aligned host backing. Its exact rounded length
// is charged before allocation; Metal mapping/driver metadata is outside budget.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include "bridge.h"
#include "../compute/provider.h"
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <errno.h>
#include <unistd.h>

typedef struct { void *device, *queue, *pipeline; } MetalContext;
typedef struct {
    MetalContext *owner;
    void *handle, *memory;
    uint64_t bytes, charge, refs;
} MetalBuffer;
typedef struct {
    MetalContext *owner;
    void *command;
    MetalBuffer *workspace, *a, *b, *c;
} MetalEvent;
static void fail(int32_t *code, int64_t *native, int32_t value, int64_t detail) {
    if (code) *code=value;
    if (native) *native=detail;
}
static void close_context(void *raw) {
    MetalContext *ctx=raw;
    if (!ctx) return;
    aether_mtl_release(ctx->pipeline);
    aether_mtl_release(ctx->queue);
    aether_mtl_release(ctx->device);
    free(ctx);
}
static void *open_context(int32_t *code, int64_t *native) {
    fail(code,native,0,0);
    MetalContext *ctx=calloc(1,sizeof(*ctx));
    if (!ctx) { fail(code,native,AETHER_GPU_ALLOCATION,ENOMEM); return NULL; }
    ctx->device=aether_mtl_device();
    ctx->queue=aether_mtl_create_command_queue(ctx->device);
    if (!ctx->device || !ctx->queue) { close_context(ctx); fail(code,native,AETHER_GPU_DEVICE,0); return NULL; }
    @autoreleasepool {
        NSString *source=@"#include <metal_stdlib>\nusing namespace metal;\nstruct P { ulong m,n,k,ao,ar,ac,bo,br,bc,co,cr,cc; };\nkernel void gemm(device const float *a [[buffer(0)]], device const float *b [[buffer(1)]], device float *c [[buffer(2)]], constant P &p [[buffer(3)]], uint id [[thread_position_in_grid]]) { ulong row=ulong(id)/p.n,col=ulong(id)%p.n; if(row>=p.m) return; float sum=0.0f; for(ulong k=0;k<p.k;k++) sum+=a[p.ao+row*p.ar+k*p.ac]*b[p.bo+k*p.br+col*p.bc]; c[p.co+row*p.cr+col*p.cc]=sum; }\n";
        NSError *error=nil;
        MTLCompileOptions *options=[MTLCompileOptions new];
        options.fastMathEnabled=NO;
        id<MTLLibrary> library=[(__bridge id<MTLDevice>)ctx->device newLibraryWithSource:source options:options error:&error];
        if (!library) { fail(code,native,AETHER_GPU_DEVICE,error.code); close_context(ctx); return NULL; }
        id<MTLFunction> function=[library newFunctionWithName:@"gemm"];
        ctx->pipeline=aether_mtl_create_pipeline(ctx->device,(__bridge void *)function);
    }
    if (!ctx->pipeline) { close_context(ctx); fail(code,native,AETHER_GPU_DEVICE,0); return NULL; }
    return ctx;
}
static void caps(void *raw,AetherGpuCaps *out) {
    memset(out,0,sizeof(*out));
    MetalContext *ctx=raw;
    id<MTLDevice> device=(__bridge id<MTLDevice>)ctx->device;
    out->device_id=aether_mtl_registry_id(ctx->device);
    out->max_buffer_bytes=device.maxBufferLength;
    out->unified_memory=device.hasUnifiedMemory;
    // Metal does not expose a portable physical heap or free-byte quantity.
    // recommendedMaxWorkingSetSize is a recommendation, not a memory guarantee.
    const char *name=device.name.UTF8String;
    if (name) { strncpy(out->name,name,sizeof(out->name)-1); }
}
static void *buffer_new(void *raw,uint64_t bytes,uint64_t remaining,uint64_t *charge,int32_t *code,int64_t *native) {
    MetalContext *ctx=raw;
    *charge=0;fail(code,native,0,0);
    long page=sysconf(_SC_PAGESIZE);
    if (!bytes || page<=0 || ((uint64_t)page&((uint64_t)page-1))) { fail(code,native,AETHER_GPU_ARGUMENT,0); return NULL; }
    uint64_t mask=(uint64_t)page-1;
    if (bytes>UINT64_MAX-mask) { fail(code,native,AETHER_GPU_OVERFLOW,0); return NULL; }
    uint64_t length=(bytes+mask)&~mask;
    if (length>SIZE_MAX || length>((__bridge id<MTLDevice>)ctx->device).maxBufferLength) { fail(code,native,AETHER_GPU_BOUNDS,0); return NULL; }
    if (length>remaining) { fail(code,native,AETHER_GPU_BUDGET,0); return NULL; }
    MetalBuffer *result=calloc(1,sizeof(*result));
    if (!result) { fail(code,native,AETHER_GPU_ALLOCATION,ENOMEM); return NULL; }
    int status=posix_memalign(&result->memory,(size_t)page,(size_t)length);
    if (status) { free(result); fail(code,native,AETHER_GPU_ALLOCATION,status); return NULL; }
    @autoreleasepool {
        id<MTLBuffer> buffer=[(__bridge id<MTLDevice>)ctx->device newBufferWithBytesNoCopy:result->memory length:(NSUInteger)length options:MTLResourceStorageModeShared deallocator:nil];
        result->handle=(__bridge_retained void *)buffer;
    }
    if (!result->handle) { free(result->memory); free(result); fail(code,native,AETHER_GPU_ALLOCATION,0); return NULL; }
    result->owner=ctx;result->bytes=bytes;result->charge=length;result->refs=1;*charge=length;
    return result;
}
static void *buffer_contents(void *raw,void *value) {
    MetalBuffer *buffer=value;
    return buffer && buffer->owner==raw ? buffer->memory : NULL;
}
static void buffer_free(void *raw,void *value) {
    MetalBuffer *buffer=value;
    if (!buffer || buffer->owner!=raw) return;
    if (--buffer->refs) return;
    aether_mtl_release(buffer->handle);free(buffer->memory);free(buffer);
}
static bool within(const MetalBuffer *buffer,uint64_t rows,uint64_t cols,uint64_t offset,uint64_t row,uint64_t col) {
    if (!rows || !cols) return offset<=buffer->bytes/sizeof(float);
    uint64_t dr,dc,last;
    return !__builtin_mul_overflow(rows-1,row,&dr) && !__builtin_mul_overflow(cols-1,col,&dc)
        && !__builtin_add_overflow(offset,dr,&last) && !__builtin_add_overflow(last,dc,&last)
        && last<buffer->bytes/sizeof(float);
}
static void *submit(void *raw,void *av,void *bv,void *cv,const AetherGemm *p,uint64_t remaining,uint64_t *charge,int32_t *code,int64_t *native) {
    MetalContext *ctx=raw;MetalBuffer *a=av,*b=bv,*c=cv;
    *charge=0;fail(code,native,0,0);
    if (!a || !b || !c || !p || a->owner!=ctx || b->owner!=ctx || c->owner!=ctx || a==c || b==c || !p->m || !p->n) {
        fail(code,native,AETHER_GPU_ARGUMENT,0);return NULL;
    }
    uint64_t count;
    if (__builtin_mul_overflow(p->m,p->n,&count) || count>UINT32_MAX) { fail(code,native,AETHER_GPU_OVERFLOW,0);return NULL; }
    if (!within(a,p->m,p->k,p->ao,p->ar,p->ac) || !within(b,p->k,p->n,p->bo,p->br,p->bc) || !within(c,p->m,p->n,p->co,p->cr,p->cc)) {
        fail(code,native,AETHER_GPU_BOUNDS,0);return NULL;
    }
    MetalEvent *event=calloc(1,sizeof(*event));
    if (!event) { fail(code,native,AETHER_GPU_ALLOCATION,ENOMEM);return NULL; }
    uint64_t amount=0;
    event->workspace=buffer_new(ctx,sizeof(*p),remaining,&amount,code,native);
    if (!event->workspace) { free(event);return NULL; }
    memcpy(event->workspace->memory,p,sizeof(*p));
    event->command=aether_mtl_create_command_buffer(ctx->queue);
    void *encoder=event->command?aether_mtl_create_compute_encoder(event->command):NULL;
    if (!encoder) {
        aether_mtl_release(event->command);buffer_free(ctx,event->workspace);free(event);
        fail(code,native,AETHER_GPU_ALLOCATION,0);return NULL;
    }
    aether_mtl_set_pipeline(encoder,ctx->pipeline);
    aether_mtl_set_buffer(encoder,a->handle,0);aether_mtl_set_buffer(encoder,b->handle,1);
    aether_mtl_set_buffer(encoder,c->handle,2);aether_mtl_set_buffer(encoder,event->workspace->handle,3);
    bool dispatched=aether_mtl_dispatch_1d(encoder,ctx->pipeline,(uint32_t)count);
    aether_mtl_end_encoding(encoder);aether_mtl_release(encoder);
    if (!dispatched) {
        aether_mtl_release(event->command);buffer_free(ctx,event->workspace);free(event);
        fail(code,native,AETHER_GPU_DEVICE,0);return NULL;
    }
    event->owner=ctx;event->a=a;event->b=b;event->c=c;
    a->refs++;b->refs++;c->refs++;*charge=amount;
    aether_mtl_commit(event->command);return event;
}
static int32_t event_status(void *raw,void *value,int64_t *native) {
    MetalEvent *event=value;*native=0;
    if (!event || event->owner!=raw) { *native=EINVAL;return AETHER_GPU_FAILED; }
    int32_t status=aether_mtl_command_status(event->command);
    if (status==4) return AETHER_GPU_COMPLETE;
    if (status==5) { *native=aether_mtl_command_error(event->command);return AETHER_GPU_FAILED; }
    return AETHER_GPU_PENDING;
}
static int32_t event_wait(void *raw,void *value,int64_t *native) {
    MetalEvent *event=value;
    if (event && event->owner==raw) aether_mtl_wait_until_completed(event->command);
    return event_status(raw,value,native);
}
static void event_free(void *raw,void *value) {
    MetalEvent *event=value;
    if (!event || event->owner!=raw) return;
    int64_t native;
    event_wait(raw,value,&native);aether_mtl_release(event->command);
    buffer_free(raw,event->workspace);buffer_free(raw,event->a);buffer_free(raw,event->b);buffer_free(raw,event->c);
    free(event);
}
static const AetherGpuProviderV1 provider={
    .version=AETHER_GPU_ABI_V1,.struct_bytes=sizeof(AetherGpuProviderV1),.backend=AETHER_GPU_METAL,
    .open=open_context,.close=close_context,.caps=caps,
    .buffer_new=buffer_new,.buffer_contents=buffer_contents,.buffer_free=buffer_free,
    .submit_gemm=submit,.event_status=event_status,.event_wait=event_wait,.event_free=event_free,
};
const AetherGpuProviderV1 *aether_gpu_provider_v1(void) { return &provider; }
