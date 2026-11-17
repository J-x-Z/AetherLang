// POSIX loading and filesystem metadata only. Graph semantics, code generation,
// identity hashing, subprocess argv and cache publication live in Aether.
#define _POSIX_C_SOURCE 200809L
#define _DARWIN_C_SOURCE 1
#define _DEFAULT_SOURCE 1
#include <dlfcn.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <unistd.h>
typedef int32_t (*Kernel)(float **,float *,float *);
typedef struct{void*library;Kernel kernel;} Loaded;
void *aether_jit_load(const char*path){
 void*library=dlopen(path,RTLD_NOW|RTLD_LOCAL);if(!library)return NULL;
 void*symbol=dlsym(library,"aether_jit_kernel");Kernel kernel=NULL;
 _Static_assert(sizeof(kernel)==sizeof(symbol),"POSIX function pointers");memcpy(&kernel,&symbol,sizeof(kernel));
 if(!kernel){dlclose(library);return NULL;}Loaded*out=malloc(sizeof(*out));if(!out){dlclose(library);return NULL;}
 *out=(Loaded){library,kernel};return out;
}
int32_t aether_jit_invoke(void*value,float**inputs,float*workspace,float*output){Loaded*v=value;return v?v->kernel(inputs,workspace,output):-1;}
void aether_jit_unload(void*value){Loaded*v=value;if(v){dlclose(v->library);free(v);}}
int32_t aether_jit_kind(const char*path){struct stat s;if(lstat(path,&s)){return errno==ENOENT?0:-1;}return S_ISREG(s.st_mode)?1:S_ISDIR(s.st_mode)?2:3;}
void *aether_jit_cache_lock(const char*path){
 int created=mkdir(path,0700)==0;if(!created&&errno!=EEXIST)return NULL;
 if(aether_jit_kind(path)!=2)return NULL;
 int fd=open(path,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);if(fd<0)return NULL;
 if(flock(fd,LOCK_EX|LOCK_NB)){close(fd);return NULL;}
 const char*identity="AETHER-JIT-CACHE-1";char marker[64];ssize_t n;
 if(created&&symlinkat(identity,fd,"owner")){close(fd);return NULL;}
 n=readlinkat(fd,"owner",marker,sizeof(marker));
 if(n!=(ssize_t)strlen(identity)||memcmp(marker,identity,(size_t)n)){close(fd);return NULL;}
 DIR*directory=fdopendir(fd);if(!directory)close(fd);return directory;
}
void aether_jit_cache_unlock(void*value){if(value)closedir(value);}
// Bound all immediate cache entries, including retained failed build stages.
// Never follow a symlink or recursively delete any content.
int32_t aether_jit_cache_room(void*value){
 DIR *directory = value;
 uint64_t total = 0;
 unsigned entries = 0;
 const uint64_t limit = 48 * 1024 * 1024;
 rewinddir(directory);
 for (;;) {
  errno = 0;
  struct dirent *item = readdir(directory);
  if (!item) return errno == 0;
  if (!strcmp(item->d_name, ".") || !strcmp(item->d_name, "..") || !strcmp(item->d_name, "owner")) continue;
  if (++entries > 32) return 0;
  int fd = openat(dirfd(directory), item->d_name, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
  if (fd < 0) return 0;
  DIR *child = fdopendir(fd);
  if (!child) { close(fd); return 0; }
  for (;;) {
   errno = 0;
   struct dirent *file = readdir(child);
   if (!file) {
    int read_error = errno;
    if (closedir(child) || read_error) return 0;
    break;
   }
   if (!strcmp(file->d_name, ".") || !strcmp(file->d_name, "..")) continue;
   struct stat st;
   if (fstatat(fd, file->d_name, &st, AT_SYMLINK_NOFOLLOW) ||
       !S_ISREG(st.st_mode) || st.st_size < 0 || (uint64_t)st.st_size > limit - total) {
    closedir(child);
    return 0;
   }
   total += (uint64_t)st.st_size;
  }
 }
}
// Native CPU/OS identity joins explicit target and compiler hashes in the key.
int32_t aether_jit_host(char*output,uint64_t capacity){
 struct utsname u;if(uname(&u))return -1;
 size_t a=strlen(u.sysname),b=strlen(u.release),c=strlen(u.machine);
 if(a+b+c+3>capacity)return -1;
 memcpy(output,u.sysname,a);output[a++]=':';memcpy(output+a,u.release,b);a+=b;output[a++]=':';memcpy(output+a,u.machine,c);output[a+c]=0;return 0;
}
int32_t aether_jit_darwin(void){
#ifdef __APPLE__
 return 1;
#else
 return 0;
#endif
}
