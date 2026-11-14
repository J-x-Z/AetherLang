#include <stdint.h>
#include <stddef.h>
#include <string.h>
typedef union { uint32_t bits; float number; uint8_t bytes[4]; } Word;
typedef struct { uint8_t tag; Word word; int32_t data[3]; } Record;
int64_t native_counter=40;
Word native_word={.number=4};
extern int64_t __aeth_m0_COUNTER;
extern struct {int32_t data[3];} __aeth_m0_BUFFER;
extern Record __aeth_m0_RECORD;
extern Record __aeth_m0_ZERO;
extern int32_t *const __aeth_m0_ELEMENT;
extern Word *const __aeth_m0_WORD_POINTER;
extern uint8_t *const __aeth_m0_LABEL;
Word word_flip(Word value) { value.bits ^= UINT32_C(0x80000000); return value; }
int32_t globals_check(void) {
    if(native_counter!=42||native_word.number!=5) return 21;
    if(__aeth_m0_COUNTER!=9||__aeth_m0_BUFFER.data[1]!=22) return 22;
    if(__aeth_m0_RECORD.word.number!=2||__aeth_m0_RECORD.data[2]!=7) return 23;
    if(__aeth_m0_ELEMENT!=&__aeth_m0_BUFFER.data[1]) return 24;
    if(__aeth_m0_WORD_POINTER!=&__aeth_m0_RECORD.word) return 25;
    if(strcmp((char*)__aeth_m0_LABEL,"static bytes")) return 26;
    for(size_t i=0;i<sizeof(__aeth_m0_ZERO);i++) if(((unsigned char*)&__aeth_m0_ZERO)[i]) return 27;
    return 0;
}
