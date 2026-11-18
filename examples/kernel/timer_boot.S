#define AETHER_BOOT_SSE2
#include "multiboot64.inc"

boot_main64:
    lea timer_irq(%rip), %rax
    lea idt+16*0x20(%rip), %rdi
    movw %ax, (%rdi)
    movw $8, 2(%rdi)
    movb $0x8e, 5(%rdi)
    shr $16, %rax
    movw %ax, 6(%rdi)
    shr $16, %rax
    movl %eax, 8(%rdi)
    lidt idt_pointer(%rip)

    // Map IRQ0 to vector 0x20. Automatic EOI keeps the handler free of port I/O.
    mov $0x11, %al
    outb %al, $0x20
    outb %al, $0xa0
    mov $0x20, %al
    outb %al, $0x21
    mov $0x28, %al
    outb %al, $0xa1
    mov $4, %al
    outb %al, $0x21
    mov $2, %al
    outb %al, $0xa1
    mov $3, %al                       // 8086 mode and automatic EOI.
    outb %al, $0x21
    outb %al, $0xa1
    mov $0xff, %al
    outb %al, $0xa1
    mov $0xfe, %al                    // Only the PIT is unmasked.
    outb %al, $0x21

    mov $0x34, %al                    // PIT channel 0, low/high divisor, rate generator.
    outb %al, $0x43
    mov $11932, %ax                   // 1193182 Hz / 11932 is approximately 100 Hz.
    outb %al, $0x40
    mov %ah, %al
    outb %al, $0x40
    sti
    call timer_work
    cli
    mov %eax, %r12d
    call timer_report
    mov $0x10, %eax
    add %r12d, %eax
    outl %eax, $0xf4
    hlt

.globl timer_putc
timer_putc:
    mov %dil, %al
    outb %al, $0xe9
    ret

.section .rodata
idt_pointer:
    .word 16*256-1
    .quad idt
.section .bss
.balign 16
idt: .skip 16*256
