//! console.rs — console double : UART 16550 (COM1) + VGA texte (80×25).
//!
//! Implémente `core::fmt::Write` pour que les macros no_std du noyau écrivent
//! partout à la fois (série puis VGA, dans cet ordre). Le writer no_std exige
//! une implémentation infallible côté noyau : l'échec est ignoré (`let _`).

use core::fmt::{self, Write};

use crate::{serial, vga};

pub struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        serial::write_str(s);
        vga::write_str(s);
        Ok(())
    }
}

/// Imprime sur la console (série + VGA) de façon **atomique**.
///
/// Le handler du timer écrit un « T » nu à chaque tick (100 Hz) et les
/// workers écrivent « H. »/« L »/« M » : ces caractères s'intercalaient en
/// milieu de ligne sur la voie série et découpaient les marqueurs du boot
/// (« J3T.HTLTMT.HT.5: chargement… »), ce qui faisait échouer les gates du
/// Makefile au hasard. On masque les IRQ pendant l'impression pour obtenir
/// une ligne entière, puis on restaure RFLAGS tel quel (l'appelant reste
/// maître de son état d'interruption).
pub fn print(args: fmt::Arguments) {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem));
        core::arch::asm!("cli", options(nostack, nomem));
        let _ = Console.write_fmt(args);
        core::arch::asm!(
            "push {0}",
            "popfq",
            in(reg) rflags,
            options(nostack, nomem)
        );
    }
}
