/* Fault injection only. All process/pipeline and byte processing logic is Aether. */
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#ifndef __APPLE__
#include <dlfcn.h>
#endif
static int mode(const char *wanted) {
    const char *value=getenv("AETHER_PROCESS_FAULT");
    return value && strcmp(value,wanted)==0;
}
#ifdef __APPLE__
#define REAL(name, type) name
#define REPLACE(name, type) test_##name
#define INTERPOSE(name) __attribute__((used)) static struct {const void *replacement;const void *original;} pair_##name __attribute__((section("__DATA,__interpose")))={(const void *)test_##name,(const void *)name};
#else
#define REAL(name, type) ((type)dlsym(RTLD_NEXT, #name))
#define REPLACE(name, type) name
#define INTERPOSE(name)
#endif
pid_t REPLACE(waitpid, unused)(pid_t pid,int *status,int options) {
    static int interrupted;
    if (pid>0 && mode("wait-eintr") && !interrupted++) {errno=EINTR;return -1;}
    return REAL(waitpid,pid_t(*)(pid_t,int*,int))(pid,status,options);
}
ssize_t REPLACE(read, unused)(int fd,void *buffer,size_t size) {
    static int interrupted,failed;
    if(mode("read-eintr") && !interrupted++){errno=EINTR;return -1;}
    if(mode("read-failure") && !failed++){errno=EIO;return -1;}
    return REAL(read,ssize_t(*)(int,void*,size_t))(fd,buffer,size);
}
ssize_t REPLACE(write, unused)(int fd,const void *buffer,size_t size) {
    static int interrupted;
    if(mode("write-eintr") && !interrupted++){errno=EINTR;return -1;}
    if(mode("write-zero"))return 0;
    if(mode("write-short") && size>137)size=137;
    return REAL(write,ssize_t(*)(int,const void*,size_t))(fd,buffer,size);
}
pid_t REPLACE(fork, unused)(void) {
    static int calls;
    if(mode("fork-failure") && ++calls==2){errno=ENOMEM;return -1;}
    return REAL(fork,pid_t(*)(void))();
}
static int first_pipe_descriptor=-1;
int REPLACE(pipe, unused)(int *descriptors) {
    static int calls;
    ++calls;
    if((mode("pipe-failure") && calls==2)||(mode("late-pipe-failure") && calls==4)){errno=EMFILE;return -1;}
    int result=REAL(pipe,int(*)(int*))(descriptors);
    if(result==0 && first_pipe_descriptor<0)first_pipe_descriptor=descriptors[0];
    return result;
}
int REPLACE(close, unused)(int descriptor) {
    static int failed;
    int result=REAL(close,int(*)(int))(descriptor);
    if(mode("close-failure") && descriptor==first_pipe_descriptor && !failed++){errno=EIO;return -1;}
    return result;
}
INTERPOSE(close)
INTERPOSE(waitpid)
INTERPOSE(read)
INTERPOSE(write)
INTERPOSE(fork)
INTERPOSE(pipe)
