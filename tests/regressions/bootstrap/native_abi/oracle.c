#include <stdint.h>
#include <stdarg.h>
typedef struct { int64_t lo,hi; } Pair;
typedef struct { double x; int64_t y; } Mixed;
typedef struct { uint8_t x,y,z; } Tiny;
typedef struct { float xy[3]; } Floats;
typedef struct { double xy[2][2]; } Doubles;
typedef struct { int64_t x,y,z; } Big;
typedef struct {} Empty;
Pair pair(Pair p){return (Pair){p.hi+4,p.lo-3};}
Mixed mixed(Mixed p){return (Mixed){p.x*2,p.y+2};}
Tiny tiny(Tiny p){return (Tiny){p.z,p.x,p.y};}
Floats floats(Floats p){return (Floats){{p.xy[2],p.xy[0],p.xy[1]}};}
Doubles doubles(Doubles p){return (Doubles){{{p.xy[1][0],p.xy[1][1]},{p.xy[0][0],p.xy[0][1]}}};}
Big big(Big p){return (Big){p.z,p.x,p.y};}
int64_t spill(int64_t a,int64_t b,int64_t c,int64_t d,int64_t e,Pair p,int64_t z){return a+b+c+d+e+p.lo+p.hi+z;}
Big sret_spill(int64_t a,int64_t b,int64_t c,int64_t d,Pair p){return (Big){p.lo+a+b,p.hi+c,d};}
int64_t sse_spill(double a,double b,double c,double d,double e,double f,double g,double h,Mixed m){return (int64_t)(a+b+c+d+e+f+g+h+m.x)+m.y;}
int64_t addup(Pair p,...){va_list args;va_start(args,p);int i=va_arg(args,int);double d=va_arg(args,double);va_end(args);return p.lo+p.hi+i+(int)d;}
int empty(Empty p,int x){return x+1;}
extern Pair native_pair(Pair);
extern Mixed native_mixed(Mixed);
extern Tiny native_tiny(Tiny);
extern Floats native_floats(Floats);
extern Doubles native_doubles(Doubles);
extern Big native_big(Big);
extern int64_t native_spill(int64_t,int64_t,int64_t,int64_t,int64_t,Pair,int64_t);
extern Big native_sret(int64_t,int64_t,int64_t,int64_t,Pair);
extern int native_empty(Empty,int);
int inspect(void){
    Pair p=native_pair((Pair){91,17});
    Mixed m=native_mixed((Mixed){0.25,77});
    Tiny t=native_tiny((Tiny){1,3,5});
    Floats f=native_floats((Floats){{2,4,5}});
    Doubles d=native_doubles((Doubles){{{1,2},{3,4}}});
    Big b=native_big((Big){6,7,8});
    Big s=native_sret(1,2,3,4,(Pair){9,11});
    if(p.lo!=21 || p.hi!=88 || m.x!=0.5 || m.y!=79) return 12;
    if(t.x!=5 || t.y!=1 || t.z!=3 || f.xy[0]!=5 || f.xy[2]!=4) return 13;
    if(d.xy[0][1]!=4 || d.xy[1][0]!=1 || b.x!=8 || b.y!=6 || b.z!=7) return 14;
    if(s.x!=12 || s.y!=14 || s.z!=4 || native_spill(1,2,3,4,5,(Pair){9,11},6)!=41) return 15;
    return native_empty((Empty){},36)==37 ? 0 : 16;
}
