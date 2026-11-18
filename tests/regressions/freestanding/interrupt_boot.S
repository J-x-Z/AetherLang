// CPU-side ABI oracle. Aether supplies both handlers.
#include "../../../examples/kernel/multiboot64.inc"
boot_main64:
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
idt_pointer:
    .word 16 * 256 - 1
    .quad idt

.section .bss
.balign 4096
idt:                    .skip 4096
