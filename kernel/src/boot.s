/* ============================================================
 * boot.s — entrée multiboot2, passage en mode long (x86-64)
 *
 * GRUB donne la main en mode protégé 32 bits, pagination OFF, à
 * partir de 1 MiB (adresse physique < 4 GiB, imposée par le saut
 * 32 bits de GRUB/multiboot2).
 *
 * Le bootstrap (sections .multiboot + .boot*) est lié à une adresse
 * physique : c'est là que GRUB saute, et la pagination n'est pas
 * encore allumée. Il construit des tables qui couvrent les deux :
 *
 *   - identity  0 .. 1 Gio              → bootstrap lui-même
 *                                          (tables, pile, GDT)
 *   - higher    HIGHER_HALF .. +1 Gio    → noyau lié en higher-half
 *
 * On entre en mode long sur l'identity map, puis on appelle
 * kernel_main — liée en higher-half — par un appel indirect
 * (movabs + call *%rax) : un `call rel32` ne peut pas traverser
 * les 2 GiB entre le bootstrap (bas) et le noyau (haut). Une fois
 * les tables Rust de mm::paging actives, le bootstrap est débranché.
 * ============================================================ */

.set HIGHER_HALF, 0xFFFFFFFF80000000

/* Indices calculés depuis HIGHER_HALF, jamais écrits en dur :
 * c'est bien l'index 510 qui correspond au higher-half (et non 511),
 * car le bit 30 vaut 0 dans 0xFF..FF80..0 — voir references/page-tables.md
 * §1. Le code Rust (paging.rs) calcule de la même façon. */
.set HH_PML4_IDX,  (HIGHER_HALF >> 39) & 0x1FF
.set HH_PDPT_IDX,  (HIGHER_HALF >> 30) & 0x1FF

/* ============================================================ */
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

/* ============================================================ */
/* BSS du bootstrap : pile + tables de pages.                    */
/* @nobits : non incluses dans le fichier mais dans l'espace     */
/* d'adressage (mise à zéro par GRUB + par la boucle ci-dessous) */
/* pour les tables).                                           */
.section .bootbss, "a", @nobits
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

/* ============================================================ */
.section .bootrodata, "a"
.align 8
gdt64:
    .quad 0
    .quad 0x00AF9A000000FFFF   /* 0x08 : code 64 bits */
    .quad 0x00AF92000000FFFF   /* 0x10 : données 64 bits */
gdt64_end:
gdt64_ptr:
    .word gdt64_end - gdt64 - 1
    .long gdt64                 /* base 32 bits : adresse physique < 4 GiB */

/* ============================================================ */
.section .boottext, "ax"
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
1:  movl $0, (%eax,%ecx,4)
    inc %ecx
    cmp $3072, %ecx
    jne 1b

    /* --- PML4[0] -> PDPT, PDPT[0] -> PD (identity map 0..1 Gio) --- */
    mov $pdpt, %eax
    or $0x3, %eax
    mov %eax, pml4

    mov $pd, %eax
    or $0x3, %eax
    mov %eax, pdpt

    /* --- PD : 512 grandes pages de 2 Mio = 1 Gio identity-mappé --- */
    mov $pd, %eax
    xor %ecx, %ecx
2:  mov %ecx, %edx
    shl $21, %edx
    or $0x83, %edx              /* present | writable | huge */
    mov %edx, (%eax,%ecx,8)
    inc %ecx
    cmp $512, %ecx
    jne 2b

    /* --- higher-half : même 1 Gio mappé en haut pour atteindre le noyau
     * PML4[HH_PML4_IDX] -> PDPT ; PDPT[HH_PDPT_IDX] -> PD (identique,
     * les 512 grandes pages couvrent 0..1 Gio à la fois en identity et
     * en higher-half). --- */
    mov $pdpt, %eax
    or $0x3, %eax
    mov %eax, pml4 + HH_PML4_IDX*8

    mov $pd, %eax
    or $0x3, %eax
    mov %eax, pdpt + HH_PDPT_IDX*8

    /* --- charger CR3 avec la PML4 --- */
    mov $pml4, %eax
    mov %eax, %cr3

    /* --- activer PAE --- */
    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4

    /* --- activer LME (IA32_EFER bit 8) et NXE (bit 11) ---
     * NXE est obligatoire : sans lui, le bit 63 d'un descripteur
     * est un bit réservé et déclenche une faute RSV au premier accès. */
    mov $0xC0000080, %ecx
    rdmsr
    or $0x900, %eax             /* LME | NXE */
    wrmsr

    /* --- activer paging --- */
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0

    /* --- GDT 64 bits + saut lointain en mode long --- */
    lgdt gdt64_ptr
    ljmp $0x08, $long_mode_start

/* ---------------------------------------------------------- */
.code64
long_mode_start:
    /* Entrée en mode long sur l'identity map (adresse physique < 4 GiB). */
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

    /* --- appeler kernel_main, liée en higher-half (> 4 GiB) ---
     * Un `call rel32` ne peut pas couvrir l'écart entre le bootstrap
     * (bas, < 4 GiB) et le noyau (higher-half, > 4 GiB) : on charge
     * l'adresse complète dans rax puis on appelle indirect. Le mapping
     * higher-half (construit ci-dessus) rend l'adresse valide. */
    movabs $kernel_main, %rax
    call *%rax

    /* kernel_main ne doit pas retourner ; si ça arrive, on halt. */
3:  hlt
    jmp 3b
