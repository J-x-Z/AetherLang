// Deterministic failure injection, never presented as GPU execution evidence.
#include "../../../runtime/compute/provider.h"
#include <stdlib.h>
#include <string.h>
typedef struct { unsigned buffers, events, submissions; } Context;
typedef struct { Context *owner; float *data; } Buffer;
typedef struct { Context *owner; int state; } Event;
static void *open_device(int32_t *code,int64_t *native) { (void)code;(void)native;return calloc(1,sizeof(Context)); }
static void close_device(void *raw) { Context *c=raw;if(c->buffers||c->events)abort();free(c); }
static void caps(void *raw,AetherGpuCaps *out) {(void)raw;memset(out,0,sizeof(*out));}
static void *buffer_new(void *raw,uint64_t bytes,uint64_t remaining,uint64_t *charge,int32_t *code,int64_t *native) {
    (void)native;if(bytes>remaining){*code=AETHER_GPU_BUDGET;return NULL;}
    Buffer *b=calloc(1,sizeof(*b));if(!b)return NULL;
    b->data=calloc(1,(size_t)bytes);if(!b->data){free(b);return NULL;}
    b->owner=raw;b->owner->buffers++;*charge=bytes;return b;
}
static void *contents(void *raw,void *value){Buffer*b=value;if(b->owner!=raw)abort();return b->data;}
static void buffer_free(void *raw,void *value){Buffer*b=value;if(b->owner!=raw)abort();b->owner->buffers--;free(b->data);free(b);}
static void *submit(void *raw,void *a,void *b,void *c,const AetherGemm*p,uint64_t remaining,uint64_t*charge,int32_t*code,int64_t*native){
    (void)a;(void)b;(void)remaining;(void)code;(void)native;Context*ctx=raw;
    Event*e=calloc(1,sizeof(*e));if(!e)return NULL;
    e->owner=ctx;e->state=ctx->submissions++==0?AETHER_GPU_FAILED:AETHER_GPU_COMPLETE;ctx->events++;*charge=0;
    if(e->state==AETHER_GPU_COMPLETE){Buffer*out=c;for(uint64_t i=0;i<p->m;i++)for(uint64_t j=0;j<p->n;j++)out->data[p->co+i*p->cr+j*p->cc]=9.0f;}
    return e;
}
static int32_t status(void *raw,void *event,int64_t*native){Event*e=event;if(e->owner!=raw)abort();*native=e->state==AETHER_GPU_FAILED?77:0;return e->state;}
static void event_free(void*raw,void*event){Event*e=event;if(e->owner!=raw)abort();e->owner->events--;free(e);}
const AetherGpuProviderV1 *aether_gpu_provider_v1(void){
    static const AetherGpuProviderV1 api={1,sizeof(AetherGpuProviderV1),AETHER_GPU_METAL,open_device,close_device,caps,buffer_new,contents,buffer_free,submit,status,status,event_free};return &api;
}
