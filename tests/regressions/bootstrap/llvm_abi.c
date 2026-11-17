#include <stdbool.h>
#include <stdint.h>
#include <stdarg.h>
#include <string.h>
int8_t abi_i8(int8_t x) { return x + 1; }
uint8_t abi_u8(uint8_t x) { return x + 1; }
int16_t abi_i16(int16_t x) { return x + 1; }
uint16_t abi_u16(uint16_t x) { return x + 1; }
bool abi_bool(bool x) { return !x; }
float abi_f32(float x) { return x + 0.5f; }
int abi_variadic(int tag, ...) {
    va_list ap; va_start(ap, tag);
    int a = va_arg(ap, int), b = va_arg(ap, int), c = va_arg(ap, int);
    double d = va_arg(ap, double); const char *text = va_arg(ap, const char *);
    va_end(ap);
    return tag != 7 || a != -100 || b != 60000 || c != 1 || d != 1.25 || strcmp(text, "ok");
}
