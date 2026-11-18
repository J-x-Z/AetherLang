#include <stdint.h>
#include <string.h>
typedef union {float a,b;} Scalar;
typedef struct {int32_t index; double value;} Coordinates;
typedef struct __attribute__((packed)) {uint8_t tag; Coordinates point; uint8_t tail;} Wire;
typedef union {int32_t *address; uint64_t bits;} Address;
typedef struct __attribute__((packed)) {uint8_t tag; Address payload;} Slot;
extern Wire roundtrip(Wire);
extern const Slot POINTERS[];
Wire apply(Wire value) {
    value.tail += 2;
    return roundtrip(value);
}
Scalar flip(Scalar value) {return (Scalar){.b=value.a+1.0f};}
int32_t inspect(void) {
    int32_t *first,*second;
    memcpy(&first,&POINTERS[0].payload,sizeof first);
    memcpy(&second,&POINTERS[1].payload,sizeof second);
    return POINTERS[0].tag==6 && POINTERS[1].tag==7 && first+1==second && *first==17 && *second==29 ? 0 : 7;
}
