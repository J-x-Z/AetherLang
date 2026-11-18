/* Independent C caller and callee for the default Windows x64 calling convention. */
#include <stdint.h>
typedef struct { uint8_t x; } Byte;
typedef struct { uint16_t x; } Half;
typedef struct { float x; } FloatBox;
typedef union { uint64_t x; double real; } Word;
typedef struct { uint8_t x, y, z; } Three;
typedef struct { int64_t x; double y; int64_t z; } Large;
_Static_assert(sizeof(Byte)==1 && sizeof(Half)==2 && sizeof(FloatBox)==4 && sizeof(Word)==8,"register sizes");
_Static_assert(sizeof(Three)==3 && sizeof(Large)==24,"indirect sizes");

extern FloatBox a_small(Byte,Half,FloatBox,Word);
extern Three a_three(Three);
extern _Bool a_bool(int8_t,uint16_t,Byte,_Bool);
extern Large a_mixed(int64_t,double,Large,FloatBox,Word,double);
FloatBox c_small(Byte a, Half b, FloatBox c, Word d) {
    return (FloatBox){a.x+b.x+c.x+(float)d.real};
}
Three c_three(Three v) { return (Three){v.y,v.z,v.x}; }
_Bool c_bool(int8_t a, uint16_t b, Byte small, _Bool q) { return a==-9 && b==65000 && small.x==170 && q; }
Large c_mixed(int64_t a, double b, Large v, FloatBox d, Word e, double f) {
    v.x+=a; v.y+=b+d.x+f; v.z+=(int64_t)e.x;
    return v;
}

int check_aether(void) {
    FloatBox small=a_small((Byte){251},(Half){513},(FloatBox){1.5f},(Word){.real=2.25});
    if (small.x!=767.75f) return 11;
    Three odd=a_three((Three){1,2,3});
    if (odd.x!=2 || odd.y!=3 || odd.z!=1) return 12;
    if (!a_bool(-9,65000,(Byte){170},1) || a_bool(-9,65000,(Byte){170},0)) return 14;
    Large source={-20,3.25,90};
    Large out=a_mixed(6,2.5,source,(FloatBox){4.0f},(Word){.x=8},0.75);
    return out.x==-14 && out.y==10.5 && out.z==98 && source.x==-20 ? 0 : 13;
}
