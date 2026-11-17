/* Independent C ABI oracle, deliberately headerless for cross-object tests. */
typedef long long i64;
typedef unsigned long long u64;
typedef unsigned char u8;
typedef struct { i64 a,b; } Pair;
typedef struct { double a; i64 b; } Mixed;
typedef struct { i64 a,b,c; } Large;
typedef struct { u8 a; double b; } Gap;
typedef struct { float a; double empty[0]; float b; } AlignedGap;
typedef union { double real; u64 bits; } Merge;
_Static_assert(sizeof(i64)==8 && sizeof(Mixed)==16 && sizeof(AlignedGap)==16,"64-bit oracle layout");
Mixed c_mixed(Mixed value){return (Mixed){value.a+1.5,value.b+7};}
extern Mixed aeth_mixed(Mixed value);
Gap c_gap(Gap value){return (Gap){value.a+2,value.b+3.5};}
extern Gap aeth_gap(Gap value);
AlignedGap c_aligned_gap(AlignedGap value){return (AlignedGap){value.a+1,{},value.b+2};}
extern AlignedGap aeth_aligned_gap(AlignedGap value);
Merge c_merge(Merge value){return (Merge){.bits=value.bits^0x123456789abcdef0ULL};}
extern Merge aeth_merge(Merge value);
Pair c_spill5(i64 a,i64 b,i64 c,i64 d,i64 e,Pair value,i64 last){return (Pair){value.a+a+2*b+3*c+4*d+5*e,value.b+last};}
extern Pair aeth_spill5(i64 a,i64 b,i64 c,i64 d,i64 e,Pair value,i64 last);
Mixed c_spill_sse(double a,double b,double c,double d,double e,double f,double g,double h,Mixed value,i64 last){return (Mixed){value.a+a+b+c+d+e+f+g+h,value.b+last};}
extern Mixed aeth_spill_sse(double a,double b,double c,double d,double e,double f,double g,double h,Mixed value,i64 last);
Mixed c_spill_gp(i64 a,i64 b,i64 c,i64 d,i64 e,i64 f,Mixed value,double last){return (Mixed){value.a+last,value.b+a+b+c+d+e+f};}
extern Mixed aeth_spill_gp(i64 a,i64 b,i64 c,i64 d,i64 e,i64 f,Mixed value,double last);
Large c_result_spill(i64 a,i64 b,i64 c,i64 d,i64 e,Pair value,i64 last){return (Large){value.a+a+b+c+d+e,value.b+last,77};}
extern Large aeth_result_spill(i64 a,i64 b,i64 c,i64 d,i64 e,Pair value,i64 last);
int sysv_abi_check(void) {
 Mixed m=aeth_mixed((Mixed){2.5,100}); if(m.a!=4.0||m.b!=107)return 11;
 Gap g=aeth_gap((Gap){3,6.5}); if(g.a!=5||g.b!=10.0)return 12;
 AlignedGap ag=aeth_aligned_gap((AlignedGap){3,{},4}); if(ag.a!=4||ag.b!=6)return 13;
 Merge u=aeth_merge((Merge){.bits=0x123456789abcdef5ULL}); if(u.bits!=5)return 14;
 Pair p=aeth_spill5(1,2,3,4,5,(Pair){10,20},99); if(p.a!=65||p.b!=119)return 15;
 Mixed s=aeth_spill_sse(1,2,3,4,5,6,7,8,(Mixed){10,20},99); if(s.a!=46||s.b!=119)return 16;
 Mixed q=aeth_spill_gp(1,2,3,4,5,6,(Mixed){10,20},99.5); if(q.a!=109.5||q.b!=41)return 17;
 Large l=aeth_result_spill(1,2,3,4,5,(Pair){10,20},99); if(l.a!=25||l.b!=119||l.c!=77)return 18;
 return 0;
}
