#include <stdint.h>
typedef float f32x4 __attribute__((vector_size(16)));
typedef double f64x2 __attribute__((vector_size(16)));
typedef int32_t i32x4 __attribute__((vector_size(16)));
extern f32x4 aether_f32(f32x4);
extern f64x2 aether_f64(f64x2);
extern i32x4 aether_i32(i32x4);
f32x4 native_f32(f32x4 value) {return value*(f32x4){2,2,2,2};}
f64x2 native_f64(f64x2 value) {return value-(f64x2){3,3};}
i32x4 native_i32(i32x4 value) {return value+(i32x4){5,5,5,5};}
int32_t vector_callback_check(void) {
    f32x4 f=aether_f32((f32x4){4,5,6,7});
    f64x2 d=aether_f64((f64x2){5,7});
    i32x4 i=aether_i32((i32x4){1,3,5,7});
    return f[0]!=5 || f[3]!=8 || d[0]!=10 || d[1]!=14 || i[0]!=4 || i[3]!=10;
}
