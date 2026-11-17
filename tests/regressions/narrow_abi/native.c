#include <stdint.h>
#include <stdbool.h>
extern int8_t exported_i8(void);
extern int16_t exported_i16(void);
extern uint8_t exported_u8(void);
extern uint16_t exported_u16(void);
extern bool exported_bool(void);
int32_t c_signed(int8_t x, int16_t y) { return (int32_t)x + y; }
uint32_t c_unsigned(uint8_t x, uint16_t y, bool b) { return (uint32_t)x + y + b; }
int32_t check_exports(void) {
    return exported_i8() != -128 || exported_i16() != -32768 ||
           exported_u8() != 255 || exported_u16() != 65535 || !exported_bool();
}
