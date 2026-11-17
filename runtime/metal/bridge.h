#ifndef AETHER_METAL_BRIDGE_H
#define AETHER_METAL_BRIDGE_H
#include <stdint.h>
#include <stdbool.h>
void *aether_mtl_device(void);
void aether_mtl_release(void *handle);
void *aether_mtl_create_command_queue(void *device);
void *aether_mtl_create_command_buffer(void *queue);
void *aether_mtl_create_buffer(void *device, uint64_t length);
void *aether_mtl_buffer_contents(void *buffer);
void *aether_mtl_create_library(void *device, const char *source);
void *aether_mtl_get_function(void *library, const char *name);
void *aether_mtl_create_pipeline(void *device, void *function);
void *aether_mtl_create_compute_encoder(void *command);
void aether_mtl_set_pipeline(void *encoder, void *pipeline);
void aether_mtl_set_buffer(void *encoder, void *buffer, uint32_t index);
void aether_mtl_dispatch_threads(void *encoder, uint32_t x, uint32_t y, uint32_t z);
bool aether_mtl_dispatch_1d(void *encoder, void *pipeline, uint32_t count);
void aether_mtl_end_encoding(void *encoder);
void aether_mtl_commit(void *command);
void aether_mtl_wait_until_completed(void *command);
bool aether_mtl_completed(void *command);
int32_t aether_mtl_command_status(void *command);
int64_t aether_mtl_command_error(void *command);
uint64_t aether_mtl_registry_id(void *device);
#endif
