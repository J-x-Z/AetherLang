// Compile with -fobjc-arc and link Metal and Foundation. Each returned handle
// owns one reference. buffer_contents is borrowed until the buffer is released.
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdint.h>
#include <stdbool.h>

#define HANDLE(expression) @autoreleasepool { return (__bridge_retained void *)(expression); }
#define OBJ(protocol, pointer) ((__bridge id<protocol>)(pointer))

void *aether_mtl_device(void) { HANDLE(MTLCreateSystemDefaultDevice()) }
void aether_mtl_release(void *handle) {
    if (handle) { @autoreleasepool { (void)(__bridge_transfer id)handle; } }
}
void *aether_mtl_create_command_queue(void *device) { HANDLE([OBJ(MTLDevice, device) newCommandQueue]) }
void *aether_mtl_create_command_buffer(void *queue) { HANDLE([OBJ(MTLCommandQueue, queue) commandBuffer]) }
void *aether_mtl_create_buffer(void *device, uint64_t length) {
    if (!length || length > OBJ(MTLDevice, device).maxBufferLength) return NULL;
    HANDLE([OBJ(MTLDevice, device) newBufferWithLength:length options:MTLResourceStorageModeShared])
}
void *aether_mtl_buffer_contents(void *buffer) { return OBJ(MTLBuffer, buffer).contents; }
void *aether_mtl_create_library(void *device, const char *source) {
    if (!source) return NULL;
    @autoreleasepool {
        NSString *text = [NSString stringWithUTF8String:source];
        if (!text) return NULL;
        return (__bridge_retained void *)[OBJ(MTLDevice, device) newLibraryWithSource:text options:nil error:NULL];
    }
}
void *aether_mtl_get_function(void *library, const char *name) {
    if (!name) return NULL;
    @autoreleasepool {
        NSString *key = [NSString stringWithUTF8String:name];
        if (!key) return NULL;
        return (__bridge_retained void *)[OBJ(MTLLibrary, library) newFunctionWithName:key];
    }
}
void *aether_mtl_create_pipeline(void *device, void *function) {
    if (!function) return NULL;
    HANDLE([OBJ(MTLDevice, device) newComputePipelineStateWithFunction:OBJ(MTLFunction, function) error:NULL])
}
void *aether_mtl_create_compute_encoder(void *command) {
    HANDLE([OBJ(MTLCommandBuffer, command) computeCommandEncoder])
}
void aether_mtl_set_pipeline(void *encoder, void *pipeline) {
    [OBJ(MTLComputeCommandEncoder, encoder) setComputePipelineState:OBJ(MTLComputePipelineState, pipeline)];
}
void aether_mtl_set_buffer(void *encoder, void *buffer, uint32_t index) {
    [OBJ(MTLComputeCommandEncoder, encoder) setBuffer:OBJ(MTLBuffer, buffer) offset:0 atIndex:index];
}
void aether_mtl_dispatch_threads(void *encoder, uint32_t x, uint32_t y, uint32_t z) {
    [OBJ(MTLComputeCommandEncoder, encoder) dispatchThreads:MTLSizeMake(x, y, z)
                                      threadsPerThreadgroup:MTLSizeMake(1, 1, 1)];
}
bool aether_mtl_dispatch_1d(void *encoder, void *pipeline, uint32_t count) {
    if (!encoder || !pipeline || !count) return false;
    id<MTLComputePipelineState> state = OBJ(MTLComputePipelineState, pipeline);
    NSUInteger lanes = MIN(count, MIN(state.threadExecutionWidth, state.maxTotalThreadsPerThreadgroup));
    if (!lanes) return false;
    [OBJ(MTLComputeCommandEncoder, encoder) dispatchThreads:MTLSizeMake(count, 1, 1)
                                      threadsPerThreadgroup:MTLSizeMake(lanes, 1, 1)];
    return true;
}
void aether_mtl_end_encoding(void *encoder) { [OBJ(MTLComputeCommandEncoder, encoder) endEncoding]; }
void aether_mtl_commit(void *command) { [OBJ(MTLCommandBuffer, command) commit]; }
void aether_mtl_wait_until_completed(void *command) { [OBJ(MTLCommandBuffer, command) waitUntilCompleted]; }
bool aether_mtl_completed(void *command) { return OBJ(MTLCommandBuffer, command).status == MTLCommandBufferStatusCompleted; }
int32_t aether_mtl_command_status(void *command) {
    return command ? (int32_t)OBJ(MTLCommandBuffer, command).status : -1;
}
int64_t aether_mtl_command_error(void *command) {
    return command ? (int64_t)OBJ(MTLCommandBuffer, command).error.code : 0;
}
uint64_t aether_mtl_registry_id(void *device) { return OBJ(MTLDevice, device).registryID; }
