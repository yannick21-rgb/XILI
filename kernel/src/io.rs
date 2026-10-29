/* ============================================================
 * io.rs — accès aux ports d'E/S (inb / outb)
 *
 * Le noyau est no_std et n'a pas de libstd. Les primitives de
 * port inb/outb sont donc définies ici, en assembleur inline,
 * et utilisées par le PIC, le clavier, le timer, etc.
 *
 * IMPORTANT : `nomem` est INTENTIONNELLEMENT ABSENT des options.
 * Le `out`/`in` touche des ports d'E/S, pas de la mémoire, mais
 * le compilateur ne le sait pas forcément et `nomem` lui dit que
 * l'asm n'accède pas à la mémoire — ce qui permet de réordonder
 * les read_volatile/write_volatile autour de l'appel. Or les
 * séquences de programmation du PIC et du PIT doivent rester
 * strictement ordonnées par rapport aux accès mémoire volatile
 * (ex: tick counter, IDT). Sans `nomem`, l'asm est un bug
 * mémoire invisible pour l'optimiseur, ce qui a corrompu
 * TICK_COUNT en J2.6. `nostack` et `preserves_flags` suffisent
 * et correctement, car l'asm touche DX/AL et rien d'autre.
 * ============================================================ */

use core::arch::asm;

/// Écrit un octet sur un port d'E/S.
#[inline]
pub fn outb(port: u16, value: u8) {
    // SAFETY: l'accès au port est un appel d'E/S privilégié ( CPL 0 ).
    // Seuls les pilotes matériels noyau appellent ceci, en mode noyau.
    // Pas de `nomem` : l'asm doit rester une barrière mémoire pour
    // empêcher la réordonnance des read/write_volatile autour du PIC/PIT.
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nostack, preserves_flags)
        );
    }
}

/// Lit un octet depuis un port d'E/S.
#[inline]
pub fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nostack, preserves_flags)
        );
    }
    value
}

/// Écrit un mot (16 bits) sur un port d'E/S.
#[allow(dead_code)]
#[inline]
pub fn outw(port: u16, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nostack, preserves_flags)
        );
    }
}

/// Lit un mot (16 bits) depuis un port d'E/S.
#[allow(dead_code)]
#[inline]
pub fn inw(port: u16) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in ax, dx",
            out("ax") value,
            in("dx") port,
            options(nostack, preserves_flags)
        );
    }
    value
}
