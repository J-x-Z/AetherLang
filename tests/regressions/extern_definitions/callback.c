#include <stdint.h>

extern int32_t native_callback(int32_t value);
extern void native_notice(void);
static int32_t notices;

int32_t call_from_c(int32_t value) { return native_callback(value); }
void record_notice(void) { notices += 1; }
int32_t call_notice_from_c(void) {
    native_notice();
    return notices;
}
