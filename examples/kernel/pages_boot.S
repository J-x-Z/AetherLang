#include "multiboot64.inc"

boot_main64:
#ifdef AETHER_BOOT_SSE2
    // The x86-64 default code generators may use XMM registers for integer
    // data. This single-CPU guest enables SSE before calling their output.
    mov %cr0, %rax
    and $-13, %rax                    // Clear EM and TS.
    or $2, %rax                       // Set MP.
    mov %rax, %cr0
    mov %cr4, %rax
    or $0x600, %rax                   // OSFXSR and OSXMMEXCPT.
    mov %rax, %cr4
#endif
    // First verify that we really run the Aether failure path.
    lea pages(%rip), %rdi
    mov $3, %esi
    call kernel_main
    cmp $1, %eax
    jne .Lbad_contract
    lea pages(%rip), %rdi
    mov $4, %esi
    call kernel_main
    // The QEMU debug-exit port reports (value << 1) | 1.
    test %eax, %eax
    jnz .Lcheck_failed
    mov $0x10, %eax
    jmp .Lexit
.Lcheck_failed:
    add $0x20, %eax
    jmp .Lexit
.Lbad_contract:
    mov $0x30, %eax
.Lexit:
    outl %eax, $0xf4
    hlt

.section .bss
.balign 4096
pages: .skip 4 * 4096
