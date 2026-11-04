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

pub fn print(args: fmt::Arguments) {
    let _ = Console.write_fmt(args);
}
