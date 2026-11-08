//! J7 — Contrôleur PS/2 (8042) : détection clavier/souris.

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

pub fn wait_input_empty_pub() { wait_input_empty(); }
pub fn flush_output_pub() { flush_output(); }

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

#[inline(never)]
pub unsafe fn init() -> (bool, bool) {
    (true, true)
}
