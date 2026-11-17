#include <stdint.h>
#include <stddef.h>
#include <string.h>
typedef union { uint32_t bits; float number; uint8_t bytes[4]; } Word;
typedef struct { uint8_t tag; Word word; int32_t data[3]; } Record;
int64_t native_counter=40;
Word native_word={.number=4};
extern int64_t COUNTER;
extern struct {int32_t data[3];} BUFFER;
extern Record RECORD;
extern Record ZERO;
extern int32_t *const ELEMENT;
extern Word *const WORD_POINTER;
extern uint8_t *const LABEL;
Word word_flip(Word value) { value.bits ^= UINT32_C(0x80000000); return value; }
int32_t globals_check(void) {
    if(native_counter!=42||native_word.number!=5) return 21;
    if(COUNTER!=9||BUFFER.data[1]!=22) return 22;
    if(RECORD.word.number!=2||RECORD.data[2]!=7) return 23;
    if(ELEMENT!=&BUFFER.data[1]) return 24;
    if(WORD_POINTER!=&RECORD.word) return 25;
    if(strcmp((char*)LABEL,"static bytes")) return 26;
    for(size_t i=0;i<sizeof(ZERO);i++) if(((unsigned char*)&ZERO)[i]) return 27;
    return 0;
}
