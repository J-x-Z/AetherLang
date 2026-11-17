// Multiboot v1 entry and CPU-side ABI oracle. Aether supplies both handlers.
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
    lea idt+16*0x31(%rip), %rdi
    lea tick(%rip), %rax
    call .Linstall_gate
    lea idt+16*13(%rip), %rdi
    lea exception(%rip), %rax
    call .Linstall_gate
    lidt idt_pointer(%rip)
    mov %rsp, %r12
    mov $0x11223344, %r10d
    int $0x31
    hlt                               // tick must skip this byte in saved RIP.
    cmp %r12, %rsp
    jne .Lfailed
    cmpl $0x11223344, %r10d
    jne .Lfailed
    cmpq $1, ticks_seen(%rip)
    jne .Lfailed

    mov $0x28, %eax                   // Outside this three-entry GDT.
.Lfault:
    mov %ax, %ds                      // Hardware pushes a #GP error code.
    .fill 0x28 - (. - .Lfault), 1, 0xf4
    cmp %r12, %rsp                    // Handler must pop the error word.
    jne .Lfailed
    cmpl $0x11223344, %r10d
    jne .Lfailed
    cmpq $0x28, error_seen(%rip)
    jne .Lfailed
    lea .Lfault(%rip), %rcx
    cmp %rcx, fault_ip(%rip)
    jne .Lfailed
    mov $0x10, %eax
    jmp .Lexit
.Lfailed:
    mov $0x20, %eax
.Lexit:
    outl %eax, $0xf4                  // QEMU isa-debug-exit encodes (val<<1)|1.
    hlt

.Linstall_gate:                        // Zeroed IDT, DPL 0 interrupt gate.
    movw %ax, (%rdi)
    movw $8, 2(%rdi)
    movb $0x8e, 5(%rdi)
    shr $16, %rax
    movw %ax, 6(%rdi)
    ret

.section .rodata
.balign 8
gdt:
    .quad 0, 0x00af9a000000ffff, 0x00cf92000000ffff
gdt_pointer:
    .word gdt_pointer - gdt - 1
    .long gdt
idt_pointer:
    .word 16 * 256 - 1
    .quad idt

.section .bss
.balign 4096
page_map:               .skip 4096
page_directory_pointer: .skip 4096
page_directory:         .skip 4096
idt:                    .skip 4096
.balign 16
stack:                  .skip 16384
stack_top:
