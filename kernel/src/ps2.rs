//! J7 — Contrôleur PS/2 (8042) : détection clavier/souris.
//!
//! NOTE (2026-11-08) : `init()` est de nouveau complet. Il avait été mis en
//! no-op (commits `76172ca` → `29b79c0`) car il « cassait le boot de
//! shell.elf », root cause inconnue. Elle est désormais identifiée : ce
//! n'était **pas** le PS/2. Le chargeur ELF (`elf.rs`) débordait lors de la
//! mise à zéro du BSS et écrasait la table de pages issue du split de la
//! grande page de 2 Mio (phys 0x404000) : l'entrée du shell devenait non
//! présente → #PF en R3 → #DF → triple fault. Les E/S du 8042 n'étaient
//! qu'un déclencheur de calendrier. Corrigé dans `elf.rs`.

use crate::console;
use crate::io::{inb, outb};

static mut MOUSE_BUF: [u8; 3] = [0; 3];
static mut MOUSE_IDX: usize = 0;
static mut MOUSE_X: i32 = 0;
static mut MOUSE_Y: i32 = 0;
static mut MOUSE_HAVE_ORIGIN: bool = false;

fn wait_input_empty() {
    for _ in 0..100_000 {
        if inb(0x64) & 0x02 == 0 {
            return;
        }
    }
}

fn wait_output_full() {
    for _ in 0..100_000 {
        if inb(0x64) & 0x01 != 0 {
            return;
        }
    }
}

fn flush_output() {
    for _ in 0..1000 {
        if inb(0x64) & 0x01 != 0 {
            let _ = inb(0x60);
        } else {
            break;
        }
    }
}

pub unsafe fn mouse_byte(b: u8) {
    MOUSE_BUF[MOUSE_IDX] = b;
    MOUSE_IDX += 1;
    if MOUSE_IDX < 3 {
        return;
    }
    MOUSE_IDX = 0;
    let dx = MOUSE_BUF[1] as i8 as i32;
    let dy = MOUSE_BUF[2] as i8 as i32;

    if !MOUSE_HAVE_ORIGIN {
        if crate::fb::is_ready() {
            MOUSE_X = (crate::fb::width() as i32) / 2;
            MOUSE_Y = (crate::fb::height() as i32) / 2;
        } else {
            MOUSE_X = 0;
            MOUSE_Y = 0;
        }
        MOUSE_HAVE_ORIGIN = true;
    }

    MOUSE_X += dx;
    MOUSE_Y -= dy;

    if crate::fb::is_ready() {
        let w = crate::fb::width() as i32;
        let h = crate::fb::height() as i32;
        if MOUSE_X < 0 {
            MOUSE_X = 0;
        }
        if MOUSE_Y < 0 {
            MOUSE_Y = 0;
        }
        if MOUSE_X >= w {
            MOUSE_X = w - 1;
        }
        if MOUSE_Y >= h {
            MOUSE_Y = h - 1;
        }
    }

    if crate::fb::is_ready() && crate::fb::cursor_ready() {
        crate::fb::cursor_move(MOUSE_X, MOUSE_Y);
    }
}

/// Imprime un marqueur J7 (redondant avec `console::print`, désormais
/// atomique — voir `console.rs`). Conservé pour la lisibilité de `init()`.
pub unsafe fn marker(args: core::fmt::Arguments) {
    console::print(args);
}

pub unsafe fn init() -> (bool, bool) {
    marker(format_args!("J7.2: PS/2 init begin\n"));

    wait_input_empty();
    // Désactiver les deux ports.
    outb(0x64, 0xAD);
    wait_input_empty();
    outb(0x64, 0xA7);
    wait_input_empty();
    flush_output();

    // Self-test contrôleur.
    outb(0x64, 0xAA);
    wait_output_full();
    let self_test = inb(0x60);
    let ctrl_ok = self_test == 0x55;
    marker(format_args!(
        "J7.2: controller self-test = {:#x} ({})\n",
        self_test,
        if ctrl_ok { "OK" } else { "ECHEC" }
    ));

    // Test premier port (clavier).
    outb(0x64, 0xAB);
    wait_output_full();
    let port1 = inb(0x60);
    let kb_ok = ctrl_ok && port1 == 0x00;

    // Test second port (souris).
    outb(0x64, 0xA9);
    wait_output_full();
    let port2 = inb(0x60);
    let mouse_ok = ctrl_ok && port2 == 0x00;

    if kb_ok {
        marker(format_args!("J7.2: PS/2 clavier detecte\n"));
    } else {
        marker(format_args!(
            "J7.2: clavier absent ou echec (code={:#x})\n",
            port1
        ));
    }
    if mouse_ok {
        // Activer le reporting souris.
        wait_input_empty();
        outb(0x64, 0xD4);
        wait_input_empty();
        outb(0x60, 0xF4);
        wait_output_full();
        let _ack = inb(0x60);

        // Démasquer IRQ12 (+ cascade).
        crate::pic::unmask(12);

        marker(format_args!("J7.3: PS/2 souris detectee\n"));
    } else {
        marker(format_args!(
            "J7.3: souris absente ou echec (code={:#x})\n",
            port2
        ));
    }

    // Réactiver les ports détectés.
    if kb_ok {
        outb(0x64, 0xAE);
        wait_input_empty();
    }
    if mouse_ok {
        outb(0x64, 0xA8);
        wait_input_empty();
    }

    // Configurer les IRQ (lire config, set bits 0 et 1).
    outb(0x64, 0x20);
    wait_output_full();
    let mut cfg = inb(0x60);
    marker(format_args!("J7.2: config byte = {:#x}\n", cfg));
    if kb_ok {
        cfg |= 0x01;
    }
    if mouse_ok {
        cfg |= 0x02;
    }
    // On conserve la config existante pour ne pas casser le clavier.
    wait_input_empty();
    outb(0x64, 0x60);
    wait_input_empty();
    outb(0x60, cfg);
    wait_input_empty();

    flush_output();

    marker(format_args!("J7.2: PS/2 init end\n"));
    (kb_ok, mouse_ok)
}
