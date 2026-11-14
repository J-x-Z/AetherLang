/* A test-only ABI fault shim. It contains no GEMM/device implementation and is
 * never linked to gpu_matrix.aeth or presented as evidence of GPU execution. */
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
static int operation, error_code, live, calls;
void mock_failure(int op,int code){operation=op;error_code=code;}
int mock_live(void){return live;}
int mock_calls(void){return calls;}
static int status(int op){++calls;return operation==op?error_code:0;}
int cudaMalloc(void **out,size_t bytes){int error=status(1);if(error)return error;*out=malloc(bytes);if(!*out)return 2;++live;return 0;}
int cudaFree(void *pointer){int error=status(2);if(error)return error;free(pointer);--live;return 0;}
int cudaStreamCreate(void **out){int error=status(3);if(error)return error;*out=malloc(1);if(!*out)return 2;++live;return 0;}
int cudaStreamDestroy(void *pointer){int error=status(4);if(error)return error;free(pointer);--live;return 0;}
int cublasCreate_v2(void **out){int error=status(5);if(error)return error;*out=malloc(1);if(!*out)return 3;++live;return 0;}
int cublasDestroy_v2(void *pointer){int error=status(6);if(error)return error;free(pointer);--live;return 0;}
int cudaMemcpy(void *destination,const void *source,size_t bytes,int kind){(void)kind;int error=status(7);if(error)return error;memcpy(destination,source,bytes);return 0;}
int cudaGetDeviceCount(int *out){int error=status(8);if(error)return error;*out=1;return 0;}
int cudaSetDevice(int device){(void)device;return status(9);}
int cudaDeviceSynchronize(void){return status(10);}
int cudaStreamSynchronize(void *stream){(void)stream;return status(11);}
