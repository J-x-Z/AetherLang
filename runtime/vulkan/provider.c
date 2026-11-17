// Vulkan device adapter for the Aether compute provider ABI. No window system.
#include "../compute/provider.h"
#include <vulkan/vulkan.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const uint32_t gemm_spv[] =
#include "gemm_spv.h"
;

typedef struct {
    VkInstance instance;
    VkPhysicalDevice gpu;
    VkPhysicalDeviceProperties props;
    VkPhysicalDeviceMemoryProperties memory;
    VkDevice device;
    PFN_vkGetPhysicalDeviceMemoryProperties2KHR get_memory;
    uint32_t family;
    VkQueue queue;
    VkCommandPool commands;
    VkDescriptorSetLayout bindings;
    VkPipelineLayout layout;
    VkPipeline pipeline;
} Device;

typedef struct {
    VkBuffer buffer;
    VkDeviceMemory memory;
    VkDeviceSize bytes;
    void *mapped;
} Buffer;

typedef struct {
    VkDescriptorPool descriptors;
    VkCommandBuffer command;
    VkFence fence;
} Event;

#define CHECK(call) do { result = (call); if (result != VK_SUCCESS) goto fail; } while (0)

static void close_device(void *ctx) {
    Device *d = ctx;
    if (d->device) {
        vkDestroyPipeline(d->device, d->pipeline, NULL);
        vkDestroyPipelineLayout(d->device, d->layout, NULL);
        vkDestroyDescriptorSetLayout(d->device, d->bindings, NULL);
        vkDestroyCommandPool(d->device, d->commands, NULL);
        vkDestroyDevice(d->device, NULL);
    }
    if (d->instance) vkDestroyInstance(d->instance, NULL);
    free(d);
}

static int has_extension(VkPhysicalDevice device, const char *name) {
    uint32_t count = 0;
    VkResult result = device ? vkEnumerateDeviceExtensionProperties(device, NULL, &count, NULL)
                             : vkEnumerateInstanceExtensionProperties(NULL, &count, NULL);
    if (result != VK_SUCCESS || !count) return 0;
    VkExtensionProperties *names = calloc(count, sizeof(*names));
    if (!names) return 0;
    result = device ? vkEnumerateDeviceExtensionProperties(device, NULL, &count, names)
                    : vkEnumerateInstanceExtensionProperties(NULL, &count, names);
    int found = 0;
    if (result == VK_SUCCESS)
        for (uint32_t i = 0; i < count; ++i)
            if (strcmp(names[i].extensionName, name) == 0) found = 1;
    free(names);
    return found;
}

static VkResult select_gpu(Device *d) {
    // A multi-GPU host can explicitly select e.g. 10de or 5143. Never select
    // a software CPU device for this provider.
    uint32_t vendor = 0, count = 0;
    const char *filter = getenv("AETHER_VK_VENDOR");
    if (filter && *filter) {
        char *end;
        unsigned long value = strtoul(filter, &end, 16);
        if (*end || !value || value > UINT32_MAX) return VK_ERROR_INITIALIZATION_FAILED;
        vendor = (uint32_t)value;
    }
    VkResult result = vkEnumeratePhysicalDevices(d->instance, &count, NULL);
    if (result != VK_SUCCESS || !count) return result != VK_SUCCESS ? result : VK_ERROR_FEATURE_NOT_PRESENT;
    VkPhysicalDevice *devices = calloc(count, sizeof(*devices));
    if (!devices) return VK_ERROR_OUT_OF_HOST_MEMORY;
    result = vkEnumeratePhysicalDevices(d->instance, &count, devices);
    if (result == VK_SUCCESS) {
        for (uint32_t i = 0; i < count && !d->gpu; ++i) {
            VkPhysicalDeviceProperties props;
            vkGetPhysicalDeviceProperties(devices[i], &props);
            if (props.deviceType != VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU && props.deviceType != VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) continue;
            if (vendor && props.vendorID != vendor) continue;
            uint32_t families = 0;
            vkGetPhysicalDeviceQueueFamilyProperties(devices[i], &families, NULL);
            VkQueueFamilyProperties *queues = calloc(families, sizeof(*queues));
            if (!queues) { result = VK_ERROR_OUT_OF_HOST_MEMORY; break; }
            vkGetPhysicalDeviceQueueFamilyProperties(devices[i], &families, queues);
            for (uint32_t q = 0; q < families; ++q) {
                if (queues[q].queueCount && (queues[q].queueFlags & VK_QUEUE_COMPUTE_BIT)) {
                    d->gpu = devices[i]; d->family = q; d->props = props;
                    break;
                }
            }
            free(queues);
        }
    }
    free(devices);
    if (result == VK_SUCCESS && !d->gpu) return VK_ERROR_FEATURE_NOT_PRESENT;
    return result;
}

static void *open_device(int32_t *code, int64_t *native) {
    *code = AETHER_GPU_ALLOCATION; *native = 0;
    Device *d = calloc(1, sizeof(*d));
    if (!d) return NULL;
    *code = AETHER_GPU_DEVICE;
    VkResult result;
    VkShaderModule shader = VK_NULL_HANDLE;
    VkApplicationInfo app = {.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "Aether Compute", .apiVersion = VK_API_VERSION_1_0};
    VkInstanceCreateInfo instance = {.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, .pApplicationInfo = &app};
    const char *extensions[2];
    if (has_extension(VK_NULL_HANDLE, VK_KHR_GET_PHYSICAL_DEVICE_PROPERTIES_2_EXTENSION_NAME))
        extensions[instance.enabledExtensionCount++] = VK_KHR_GET_PHYSICAL_DEVICE_PROPERTIES_2_EXTENSION_NAME;
#ifdef VK_KHR_portability_enumeration
    if (has_extension(VK_NULL_HANDLE, VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME)) {
        instance.flags = VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR;
        extensions[instance.enabledExtensionCount++] = VK_KHR_PORTABILITY_ENUMERATION_EXTENSION_NAME;
    }
#endif
    instance.ppEnabledExtensionNames = extensions;
    CHECK(vkCreateInstance(&instance, NULL, &d->instance));
    CHECK(select_gpu(d));
    if (has_extension(d->gpu, VK_EXT_MEMORY_BUDGET_EXTENSION_NAME))
        d->get_memory = (PFN_vkGetPhysicalDeviceMemoryProperties2KHR)
            vkGetInstanceProcAddr(d->instance, "vkGetPhysicalDeviceMemoryProperties2KHR");
    vkGetPhysicalDeviceMemoryProperties(d->gpu, &d->memory);
    const float priority = 1.0f;
    VkDeviceQueueCreateInfo queue = {.sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = d->family, .queueCount = 1, .pQueuePriorities = &priority};
    VkDeviceCreateInfo device = {.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, .queueCreateInfoCount = 1, .pQueueCreateInfos = &queue};
    const char *subset = "VK_KHR_portability_subset";
    if (has_extension(d->gpu, subset)) {
        device.enabledExtensionCount = 1; device.ppEnabledExtensionNames = &subset;
    }
    CHECK(vkCreateDevice(d->gpu, &device, NULL, &d->device));
    vkGetDeviceQueue(d->device, d->family, 0, &d->queue);
    VkCommandPoolCreateInfo pool = {.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_TRANSIENT_BIT, .queueFamilyIndex = d->family};
    CHECK(vkCreateCommandPool(d->device, &pool, NULL, &d->commands));
    VkDescriptorSetLayoutBinding slots[3] = {0};
    for (uint32_t i = 0; i < 3; ++i) {
        slots[i].binding = i; slots[i].descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
        slots[i].descriptorCount = 1; slots[i].stageFlags = VK_SHADER_STAGE_COMPUTE_BIT;
    }
    VkDescriptorSetLayoutCreateInfo bindings = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 3, .pBindings = slots};
    CHECK(vkCreateDescriptorSetLayout(d->device, &bindings, NULL, &d->bindings));
    VkPushConstantRange push = {.stageFlags = VK_SHADER_STAGE_COMPUTE_BIT, .size = 12 * sizeof(uint32_t)};
    VkPipelineLayoutCreateInfo layout = {.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1, .pSetLayouts = &d->bindings, .pushConstantRangeCount = 1, .pPushConstantRanges = &push};
    CHECK(vkCreatePipelineLayout(d->device, &layout, NULL, &d->layout));
    VkShaderModuleCreateInfo module = {.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = sizeof(gemm_spv), .pCode = gemm_spv};
    CHECK(vkCreateShaderModule(d->device, &module, NULL, &shader));
    VkComputePipelineCreateInfo pipeline = {.sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
        .stage = {.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = shader, .pName = "main"}, .layout = d->layout};
    CHECK(vkCreateComputePipelines(d->device, VK_NULL_HANDLE, 1, &pipeline, NULL, &d->pipeline));
    vkDestroyShaderModule(d->device, shader, NULL);
    *code = 0; return d;
fail:
    if (shader) vkDestroyShaderModule(d->device, shader, NULL);
    *native = result;
    close_device(d);
    return NULL;
}

static const VkMemoryPropertyFlags mapped_flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;

static void caps(void *ctx, AetherGpuCaps *caps) {
    Device *d = ctx;
    VkPhysicalDeviceFeatures features;
    vkGetPhysicalDeviceFeatures(d->gpu, &features);
    *caps = (AetherGpuCaps){.device_id = ((uint64_t)d->props.vendorID << 32) | d->props.deviceID,
        .max_buffer_bytes = d->props.limits.maxStorageBufferRange, .vendor_id = d->props.vendorID,
        .fp64 = features.shaderFloat64, .queue_family = d->family,
        .unified_memory = d->props.deviceType == VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU};
    snprintf(caps->name, sizeof(caps->name), "%s", d->props.deviceName);
    VkPhysicalDeviceMemoryBudgetPropertiesEXT budget = {.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MEMORY_BUDGET_PROPERTIES_EXT};
    if (d->get_memory) {
        VkPhysicalDeviceMemoryProperties2KHR info = {.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MEMORY_PROPERTIES_2_KHR, .pNext = &budget};
        d->get_memory(d->gpu, &info);
    }
    // Describe the coherent mapped heap the allocation policy prefers.
    int preferred = -1;
    for (uint32_t i = 0; i < d->memory.memoryTypeCount; ++i) {
        VkMemoryType t = d->memory.memoryTypes[i];
        if ((t.propertyFlags & mapped_flags) != mapped_flags) continue;
        int local = (t.propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) != 0;
        if (local <= preferred) continue;
        preferred = local;
        uint32_t h = t.heapIndex;
        caps->heap_bytes = d->memory.memoryHeaps[h].size;
        caps->available_bytes = budget.heapBudget[h] > budget.heapUsage[h] ? budget.heapBudget[h]-budget.heapUsage[h] : 0;
    }
}

static void free_buffer(void *ctx, void *handle) {
    Device *d = ctx; Buffer *b = handle;
    if (!b) return;
    if (b->mapped) vkUnmapMemory(d->device, b->memory);
    vkDestroyBuffer(d->device, b->buffer, NULL);
    vkFreeMemory(d->device, b->memory, NULL);
    free(b);
}

static void *buffer_new(void *ctx, uint64_t bytes, uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native) {
    Device *d = ctx; *charge = 0; *native = 0;
    if (!bytes || bytes > d->props.limits.maxStorageBufferRange) { *code = AETHER_GPU_BOUNDS; return NULL; }
    *code = AETHER_GPU_ALLOCATION;
    Buffer *b = calloc(1, sizeof(*b));
    if (!b) return NULL;
    VkResult result;
    VkBufferCreateInfo create = {.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = bytes,
        .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT, .sharingMode = VK_SHARING_MODE_EXCLUSIVE};
    CHECK(vkCreateBuffer(d->device, &create, NULL, &b->buffer));
    VkMemoryRequirements requirements;
    vkGetBufferMemoryRequirements(d->device, b->buffer, &requirements);
    if (requirements.size > remaining) { *code = AETHER_GPU_BUDGET; free_buffer(d,b); return NULL; }
    uint32_t type = UINT32_MAX;
    for (uint32_t i = 0; i < d->memory.memoryTypeCount; ++i) {
        VkMemoryPropertyFlags flags = d->memory.memoryTypes[i].propertyFlags;
        if ((requirements.memoryTypeBits & (1u << i)) && (flags & mapped_flags) == mapped_flags) {
            if (type == UINT32_MAX) type = i;
            if (flags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) { type = i; break; }
        }
    }
    if (type == UINT32_MAX) { result = VK_ERROR_FEATURE_NOT_PRESENT; goto fail; }
    VkMemoryAllocateInfo allocation = {.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = requirements.size, .memoryTypeIndex = type};
    CHECK(vkAllocateMemory(d->device, &allocation, NULL, &b->memory));
    CHECK(vkBindBufferMemory(d->device, b->buffer, b->memory, 0));
    CHECK(vkMapMemory(d->device, b->memory, 0, requirements.size, 0, &b->mapped));
    b->bytes = bytes; *charge = requirements.size; *code = 0;
    return b;
fail:
    *native = result; free_buffer(d,b); return NULL;
}

static void *buffer_contents(void *ctx, void *buffer) {
    (void)ctx; return ((Buffer *)buffer)->mapped;
}

static void free_event(void *ctx, void *handle) {
    Device *d = ctx; Event *e = handle;
    if (!e) return;
    vkDestroyFence(d->device, e->fence, NULL);
    if (e->command) vkFreeCommandBuffers(d->device, d->commands, 1, &e->command);
    vkDestroyDescriptorPool(d->device, e->descriptors, NULL);
    free(e);
}

static int covers(const Buffer *b, uint64_t rows, uint64_t cols, uint64_t offset, uint64_t rs, uint64_t cs) {
    const uint64_t elements = b->bytes / sizeof(float);
    if (!rows || !cols) return offset <= elements;
    if (offset >= elements) return 0;
    uint64_t room = elements - 1 - offset;
    if (rows > 1 && rs > room / (rows - 1)) return 0;
    room -= (rows - 1) * rs;
    return cols == 1 || cs <= room / (cols - 1);
}

static void *submit(void *ctx, void *a, void *b, void *c, const AetherGemm *p,
                    uint64_t remaining, uint64_t *charge, int32_t *code, int64_t *native) {
    Device *d = ctx;
    (void)remaining; *charge = 0; *native = 0; // Parameters use push constants.
    uint64_t wide[12]; uint32_t words[12];
    _Static_assert(sizeof(wide) == sizeof(*p), "GEMM parameter ABI");
    memcpy(wide, p, sizeof(wide));
    for (size_t i = 0; i < 12; ++i) {
        if (wide[i] > UINT32_MAX) { *code = AETHER_GPU_OVERFLOW; return NULL; }
        words[i] = (uint32_t)wide[i];
    }
    uint64_t groups_x = (p->n + 7)/8, groups_y = (p->m + 7)/8;
    if (!groups_x || !groups_y || groups_x > d->props.limits.maxComputeWorkGroupCount[0] ||
        groups_y > d->props.limits.maxComputeWorkGroupCount[1] || c == a || c == b ||
        !covers(a,p->m,p->k,p->ao,p->ar,p->ac) || !covers(b,p->k,p->n,p->bo,p->br,p->bc) ||
        !covers(c,p->m,p->n,p->co,p->cr,p->cc)) { *code = AETHER_GPU_BOUNDS; return NULL; }
    *code = AETHER_GPU_ALLOCATION;
    Event *e = calloc(1, sizeof(*e));
    if (!e) return NULL;
    VkResult result;
    VkDescriptorPoolSize size = {VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, 3};
    VkDescriptorPoolCreateInfo pool = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
        .maxSets = 1, .poolSizeCount = 1, .pPoolSizes = &size};
    CHECK(vkCreateDescriptorPool(d->device, &pool, NULL, &e->descriptors));
    VkDescriptorSetAllocateInfo set_info = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
        .descriptorPool = e->descriptors, .descriptorSetCount = 1, .pSetLayouts = &d->bindings};
    VkDescriptorSet set;
    CHECK(vkAllocateDescriptorSets(d->device, &set_info, &set));
    Buffer *buffers[] = {a,b,c};
    VkDescriptorBufferInfo info[3];
    VkWriteDescriptorSet writes[3];
    for (uint32_t i = 0; i < 3; ++i) {
        info[i] = (VkDescriptorBufferInfo){buffers[i]->buffer, 0, buffers[i]->bytes};
        writes[i] = (VkWriteDescriptorSet){.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            .dstSet = set, .dstBinding = i, .descriptorCount = 1,
            .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo = &info[i]};
    }
    vkUpdateDescriptorSets(d->device, 3, writes, 0, NULL);
    VkFenceCreateInfo fence = {.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
    CHECK(vkCreateFence(d->device, &fence, NULL, &e->fence));
    VkCommandBufferAllocateInfo allocate = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = d->commands, .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = 1};
    CHECK(vkAllocateCommandBuffers(d->device, &allocate, &e->command));
    VkCommandBufferBeginInfo begin = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
        .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT};
    *code = AETHER_GPU_EXECUTION;
    CHECK(vkBeginCommandBuffer(e->command, &begin));
    VkMemoryBarrier input = {.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_HOST_WRITE_BIT | VK_ACCESS_SHADER_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT};
    vkCmdPipelineBarrier(e->command, VK_PIPELINE_STAGE_HOST_BIT | VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
                         VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, 0, 1, &input, 0, NULL, 0, NULL);
    vkCmdBindPipeline(e->command, VK_PIPELINE_BIND_POINT_COMPUTE, d->pipeline);
    vkCmdBindDescriptorSets(e->command, VK_PIPELINE_BIND_POINT_COMPUTE, d->layout, 0, 1, &set, 0, NULL);
    vkCmdPushConstants(e->command, d->layout, VK_SHADER_STAGE_COMPUTE_BIT, 0, sizeof(words), words);
    vkCmdDispatch(e->command, (uint32_t)groups_x, (uint32_t)groups_y, 1);
    VkMemoryBarrier output = {.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT, .dstAccessMask = VK_ACCESS_HOST_READ_BIT};
    vkCmdPipelineBarrier(e->command, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_PIPELINE_STAGE_HOST_BIT,
                         0, 1, &output, 0, NULL, 0, NULL);
    CHECK(vkEndCommandBuffer(e->command));
    VkSubmitInfo submit_info = {.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
        .commandBufferCount = 1, .pCommandBuffers = &e->command};
    CHECK(vkQueueSubmit(d->queue, 1, &submit_info, e->fence));
    *code = 0;
    return e;
fail:
    *native = result; free_event(d,e); return NULL;
}

static int32_t event_state(VkResult result, int64_t *native) {
    if (result == VK_SUCCESS) return AETHER_GPU_COMPLETE;
    if (result == VK_NOT_READY || result == VK_TIMEOUT) return AETHER_GPU_PENDING;
    *native = result;
    return AETHER_GPU_FAILED;
}

static int32_t status(void *ctx, void *event, int64_t *native) {
    Device *d = ctx; Event *e = event;
    return event_state(vkGetFenceStatus(d->device, e->fence), native);
}

static int32_t wait_event(void *ctx, void *event, int64_t *native) {
    Device *d = ctx; Event *e = event;
    VkResult result;
    do { result = vkWaitForFences(d->device, 1, &e->fence, VK_TRUE, UINT64_MAX); } while (result == VK_TIMEOUT);
    return event_state(result, native);
}

const AetherGpuProviderV1 *aether_gpu_provider_v1(void) {
    static const AetherGpuProviderV1 provider = {
        .version=AETHER_GPU_ABI_V1, .struct_bytes=sizeof(AetherGpuProviderV1), .backend=AETHER_GPU_VULKAN,
        .open=open_device, .close=close_device, .caps=caps, .buffer_new=buffer_new,
        .buffer_contents=buffer_contents, .buffer_free=free_buffer, .submit_gemm=submit,
        .event_status=status, .event_wait=wait_event, .event_free=free_event,
    };
    return &provider;
}
