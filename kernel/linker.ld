/* ================================================================
 * linker.ld — layout du noyau XILI
 *
 * J2.3b (higher-half) : le noyau est lié en higher-half mais
 * chargé physiquement juste après le bootstrap.
 *
 * GRUB (multiboot2) charge chaque segment PT_LOAD à son
 * `p_paddr` (adresse physique) : les adresses physiques doivent
 * rester dans la mémoire basse (< 4 GiB). Le noyau est lié à
 * HIGHER_HALF + vaddr, mais son LMA (adresse de chargement) vaut
 * `vaddr - HIGHER_HALF`.
 *
 *   .text   : vaddr = HIGHER_HALF + _boot_end, paddr = _boot_end
 *   .rodata : vaddr = ... + text,             paddr = _boot_end + text
 *   ...
 *
 * Le point d'entrée (_start) reste à 1 MiB : c'est la limite
 * imposée par le saut 32 bits de GRUB/multiboot2. Le bootstrap
 * (.multiboot + .boot*) est lié en identity (vaddr == paddr), le
 * noyau (kernel_main et les modules Rust) en higher-half.
 *
 * `AT(. - HIGHER_HALF)` utilise le compteur de localisation
 * courant (`.`) — et non `ADDR(.section)` — car lld ne résout pas
 * toujours le auto-référence d'une section dans son propre AT().
 * Le compteur `.` est virtuel (high) ; `. - HIGHER_HALF` est donc
 * l'adresse physique, toujours en phase (vaddr - HV == paddr).
 * ================================================================ */

ENTRY(_start)

KERNEL_PA    = 0x00100000;                 /* 1 MiB — chargé par GRUB */
HIGHER_HALF  = 0xFFFFFFFF80000000;         /* base du higher-half */

SECTIONS
{
    /* --- Bootstrap : physique, identity-mappé, < 4 GiB --- */
    . = KERNEL_PA;

    /* `KEEP` : le header multiboot2 n'est pas référencé par le code
     * (GRUB le lit dans le fichier, pas via un symbole), donc --gc-sections
     * le supprimerait. On le force à rester. */
    .multiboot : {
        KEEP(*(.multiboot))
    }

    .boottext : {
        *(.boottext)
    }

    .bootrodata : {
        KEEP(*(.bootrodata))
    }

    .bootbss (NOLOAD) : {
        *(.bootbss)
    }

    _boot_end = .;

    /* --- Noyau : higher-half. LMA physique = vaddr - HIGHER_HALF. --- */
    . = HIGHER_HALF + _boot_end;

    .text : AT(. - HIGHER_HALF) {
        *(.text .text.*)
    }

    .rodata : AT(. - HIGHER_HALF) {
        *(.rodata .rodata.*)
    }

    .data : AT(. - HIGHER_HALF) {
        *(.data .data.*)
    }

    /* BSS — zero-fillé par le bootstrap (boot.s), PAS par GRUB.
     * GRUB ne zère pas toujours le BSS des segments higher-half
     * (p_vaddr != p_paddr) : on ne s'y fie pas. */
    .bss (NOLOAD) : AT(. - HIGHER_HALF) {
        _bss_start = .;
        *(COMMON)
        *(.bss .bss.*)
        _bss_end = .;
        _kend = .;         /* fin du noyau (image + BSS) : virtuel, en higher-half.
                             * L'allocateur physique le convertit en adresse
                             * physique de fin pour savoir quoi réserver. */
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.comment)
        *(.note*)
    }
}
