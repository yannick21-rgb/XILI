/* ============================================================
 * serial.rs — console UART 16550 (COM1)
 *
 * Le port série est le canal de sortie principal du noyau. Sur
 * QEMU (`-serial stdio`) ou une machine physique, il est lu par
 * un terminal. GRUB le configure, mais on le ré-initialise ici
 * pour garantir 115200 8N1 indépendamment du bootloader.
 * ============================================================ */

use crate::io::outb;
use crate::io::inb;

const COM1: u16 = 0x3F8;

pub fn init() {
    outb(COM1 + 1, 0x00); // désactiver les interruptions
    outb(COM1 + 3, 0x80); // DLAB
    outb(COM1 + 0, 0x01); // diviseur bas : 115200 bauds
    outb(COM1 + 1, 0x00); // diviseur haut
    outb(COM1 + 3, 0x03); // 8 bits, pas de parité, 1 stop
    outb(COM1 + 2, 0xC7); // FIFO à 14 octets
    outb(COM1 + 4, 0x0B); // DTR | RTS | OUT2
}

pub fn write_byte(byte: u8) {
    while inb(COM1 + 5) & 0x20 == 0 {}
    outb(COM1, byte);
}

pub fn write_str(s: &str) {
    for byte in s.bytes() {
        write_byte(byte);
    }
}
