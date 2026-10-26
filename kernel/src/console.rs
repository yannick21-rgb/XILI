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
