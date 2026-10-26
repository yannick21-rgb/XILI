use core::sync::atomic::{AtomicUsize, Ordering};

const VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const ATTR: u8 = 0x07; // gris clair sur noir

static CURSOR: AtomicUsize = AtomicUsize::new(0);

pub fn clear() {
    for cell in 0..WIDTH * HEIGHT {
        unsafe {
            *VGA_BUFFER.add(cell * 2) = b' ';
            *VGA_BUFFER.add(cell * 2 + 1) = ATTR;
        }
    }
    CURSOR.store(0, Ordering::Relaxed);
}

pub fn write_str(s: &str) {
    for byte in s.bytes() {
        write_byte(byte);
    }
}

fn write_byte(byte: u8) {
    if byte == b'\n' {
        newline();
        return;
    }

    let mut pos = CURSOR.load(Ordering::Relaxed);
    if pos >= WIDTH * HEIGHT {
        scroll();
        pos = CURSOR.load(Ordering::Relaxed);
    }

    unsafe {
        *VGA_BUFFER.add(pos * 2) = byte;
        *VGA_BUFFER.add(pos * 2 + 1) = ATTR;
    }
    CURSOR.store(pos + 1, Ordering::Relaxed);
}

fn newline() {
    let pos = CURSOR.load(Ordering::Relaxed);
    let line_end = (pos / WIDTH + 1) * WIDTH;
    if line_end >= WIDTH * HEIGHT {
        scroll();
    } else {
        CURSOR.store(line_end, Ordering::Relaxed);
    }
}

fn scroll() {
    for cell in WIDTH..WIDTH * HEIGHT {
        unsafe {
            let src = *VGA_BUFFER.add(cell * 2);
            let attr = *VGA_BUFFER.add(cell * 2 + 1);
            *VGA_BUFFER.add((cell - WIDTH) * 2) = src;
            *VGA_BUFFER.add((cell - WIDTH) * 2 + 1) = attr;
        }
    }
    for cell in (WIDTH * HEIGHT - WIDTH)..(WIDTH * HEIGHT) {
        unsafe {
            *VGA_BUFFER.add(cell * 2) = b' ';
            *VGA_BUFFER.add(cell * 2 + 1) = ATTR;
        }
    }
    CURSOR.store(WIDTH * (HEIGHT - 1), Ordering::Relaxed);
}
