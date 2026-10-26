/* ============================================================
 * boot.s — entrée multiboot2, passage en mode long (x86-64)
 *
 * GRUB nous donne la main en mode protégé 32 bits, paging OFF.
 * On construit des tables de pages identity-mappées sur 1 Gio
 * (pages de 2 Mio), on active PAE + LME + PG, puis on saute en
 * 64 bits et on appelle kernel_main(magic, mbi).
 * ============================================================ */

.section .multiboot, "a"
.align 8
mb_start:
    .long 0xE85250D6                                /* magic multiboot2 */
    .long 0                                         /* architecture i386 */
    .long mb_end - mb_start                         /* longueur */
    .long -(0xE85250D6 + 0 + (mb_end - mb_start))   /* checksum */
    .align 8
    .short 0                                        /* tag fin */
    .short 0
    .long 8
mb_end:

/* ---------------------------------------------------------- */
.section .bss
.align 16
stack_bottom:
    .skip 16384
stack_top:

.align 4096
pml4:
    .skip 4096
pdpt:
    .skip 4096
pd:
    .skip 4096

/* ---------------------------------------------------------- */
.section .rodata
.align 8
gdt64:
    .quad 0
    .quad 0x00AF9A000000FFFF   /* 0x08 : code 64 bits */
    .quad 0x00AF92000000FFFF   /* 0x10 : données 64 bits */
gdt64_end:
gdt64_ptr:
    .word gdt64_end - gdt64 - 1
    .long gdt64

/* ---------------------------------------------------------- */
.section .text
.global _start
.code32
_start:
    cli
    mov $stack_top, %esp

    /* GRUB : eax = magic multiboot2, ebx = adresse de la mbi */
    mov %eax, %edi
    mov %ebx, %esi

    /* --- zéroïser les 3 tables de pages (12 Kio) --- */
    mov $pml4, %eax
    xor %ecx, %ecx
1:
    movl $0, (%eax,%ecx,4)
    inc %ecx
    cmp $3072, %ecx
    jne 1b

    /* --- PML4[0] -> PDPT, PDPT[0] -> PD --- */
    mov $pdpt, %eax
    or $0x3, %eax
    mov %eax, pml4

    mov $pd, %eax
    or $0x3, %eax
    mov %eax, pdpt

    /* --- PD : 512 pages de 2 Mio = 1 Gio identity-mappé --- */
    mov $pd, %eax
    xor %ecx, %ecx
2:
    mov %ecx, %edx
    shl $21, %edx
    or $0x83, %edx              /* present | writable | huge */
    mov %edx, (%eax,%ecx,8)
    inc %ecx
    cmp $512, %ecx
    jne 2b

    /* --- charger CR3 avec la PML4 --- */
    mov $pml4, %eax
    mov %eax, %cr3

    /* --- activer PAE --- */
    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4

    /* --- activer LME (IA32_EFER bit 8) et NXE (bit 11) --- */
    mov $0xC0000080, %ecx
    rdmsr
    or $0x900, %eax             /* LME | NXE */
    wrmsr

    /* --- activer paging --- */
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0

    /* --- GDT 64 bits + saut lointain --- */
    lgdt gdt64_ptr
    ljmp $0x08, $long_mode_start

/* ---------------------------------------------------------- */
.code64
long_mode_start:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    mov $stack_top, %rsp

    /* --- activer SSE (sinon #UD sur movaps) --- */
    mov %cr0, %rax
    and $-5, %rax               /* clear EM (bit 2) */
    or $0x2, %rax               /* set MP */
    mov %rax, %cr0

    mov %cr4, %rax
    or $0x600, %rax             /* OSFXSR | OSXMMEXCPT */
    mov %rax, %cr4

    fninit

    /* rdi = magic, rsi = mbi (déjà placés en 32 bits) */
    call kernel_main

3:
    hlt
    jmp 3b
