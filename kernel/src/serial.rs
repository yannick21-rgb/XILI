use core::arch::asm;

const COM1: u16 = 0x3F8;

#[inline]
fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline]
fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

pub fn init() {
    outb(COM1 + 1, 0x00); // désactiver les interruptions
    outb(COM1 + 3, 0x80); // DLAB
    outb(COM1 + 0, 0x01); // diviseur bas : 115200 bauds
    outb(COM1 + 1, 0x00); // diviseur haut
    outb(COM1 + 3, 0x03); // 8 bits, pas de parité, 1 stop
    outb(COM1 + 2, 0xC7); // FIFO
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
