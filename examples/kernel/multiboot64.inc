// Multiboot v1: enter long mode with 2 MiB mapped and a 16-byte-aligned stack.
// The including image provides boot_main64 with interrupts disabled.
.section .text.boot,"ax",@progbits
.code32
.globl _start
.Lheader:
    .long 0x1badb002, 0x10000, -(0x1badb002 + 0x10000)
    .long .Lheader, .Lheader, 0, __bss_end, _start
_start:
    cli
    cld
    lgdt gdt_pointer
    movl $(page_directory + 3), page_directory_pointer
    movl $(page_directory_pointer + 3), page_map
    movl $0x83, page_directory          // Identity-map the first 2 MiB.
    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4
    mov $page_map, %eax
    mov %eax, %cr3
    mov $0xc0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0
    ljmp $8, $.Llong_mode

.code64
.Llong_mode:
    mov $16, %ax
    mov %ax, %ss
    mov %ax, %ds
    mov %ax, %es
    lea stack_top(%rip), %rsp
    jmp boot_main64

.section .rodata
.balign 8
gdt:
    .quad 0, 0x00af9a000000ffff, 0x00cf92000000ffff
gdt_pointer:
    .word gdt_pointer - gdt - 1
    .long gdt
.section .bss
.balign 4096
page_map:               .skip 4096
page_directory_pointer: .skip 4096
page_directory:         .skip 4096
.balign 16
stack:                  .skip 16384
stack_top:

.section .text.boot,"ax",@progbits
.code64
