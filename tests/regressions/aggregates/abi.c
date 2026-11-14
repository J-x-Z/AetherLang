/* An independent native caller/callee, not an implementation of the workload. */
#include <stdint.h>
#include <stddef.h>
typedef struct { int64_t a,b; } Pair;
typedef struct { uint8_t a,b,c; } Tiny;
typedef struct __attribute__((packed)) { uint8_t tag; int64_t value; } Odd;
typedef struct { float values[3]; } Floats;
typedef struct { double values[2][2]; } Doubles;
typedef struct { int64_t values[3]; } Large;
_Static_assert(sizeof(Tiny)==3 && sizeof(Odd)==9 && offsetof(Odd,value)==1,"ABI test layout");
Pair c_pair(Pair x) { return (Pair){x.a+1,x.b+2}; }
Tiny c_tiny(Tiny x) { return (Tiny){x.a+1,x.b+2,x.c+3}; }
Odd c_odd(Odd x) { return (Odd){x.tag+1,x.value+10}; }
Floats c_floats(Floats x) { return (Floats){{x.values[0]+1,x.values[1]+2,x.values[2]+3}}; }
Doubles c_doubles(Doubles x) { return (Doubles){{{x.values[0][0]+1,x.values[0][1]+2},{x.values[1][0]+3,x.values[1][1]+4}}}; }
Large c_large(Large x) { x.values[0]++; x.values[1]+=2; x.values[2]+=3; return x; }
Pair c_spill(int64_t a,int64_t b,int64_t c,int64_t d,int64_t e,int64_t f,int64_t g,Pair x) { return (Pair){x.a+a+b+c+d+e+f+g,x.b+2}; }
extern Pair aeth_pair(Pair);
extern Tiny aeth_tiny(Tiny);
extern Odd aeth_odd(Odd);
extern Floats aeth_floats(Floats);
extern Doubles aeth_doubles(Doubles);
extern Large aeth_large(Large);
extern Pair aeth_spill(int64_t,int64_t,int64_t,int64_t,int64_t,int64_t,int64_t,Pair);
int32_t abi_check(void) {
    Pair p=aeth_pair((Pair){10,20});
    Tiny t=aeth_tiny((Tiny){10,20,30});
    Odd o=aeth_odd((Odd){10,1000});
    Floats f=aeth_floats((Floats){{10,20,30}});
    Doubles d=aeth_doubles((Doubles){{{10,20},{30,40}}});
    Large input={{10,20,30}}, l=aeth_large(input);
    Pair s=aeth_spill(1,2,3,4,5,6,7,(Pair){10,20});
    if(p.a!=11||p.b!=22) return 11;
    if(t.a!=11||t.b!=22||t.c!=33) return 12;
    if(o.tag!=11||o.value!=1010) return 13;
    if(f.values[0]!=11||f.values[1]!=22||f.values[2]!=33) return 14;
    if(d.values[0][0]!=11||d.values[0][1]!=22||d.values[1][0]!=33||d.values[1][1]!=44) return 15;
    if(input.values[0]!=10||l.values[0]!=11||l.values[1]!=22||l.values[2]!=33) return 16;
    if(s.a!=38||s.b!=22) return 17;
    return 0;
}
