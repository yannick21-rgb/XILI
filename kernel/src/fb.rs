//! J7 — Framebuffer VBE/GOP + curseur logiciel.

use crate::console;
use crate::mbi;
use crate::mm::paging;

const FB_VIRT_BASE: u64 = paging::HIGHER_HALF + 0x2000_0000;

static mut FB_READY: bool = false;
static mut FB_VIRT: u64 = 0;
static mut FB_WIDTH: u32 = 0;
static mut FB_HEIGHT: u32 = 0;
static mut FB_PITCH: u32 = 0;
static mut FB_BPP: u8 = 0;

const CURSOR_W: usize = 12;
const CURSOR_H: usize = 19;
static mut CURSOR_BG: [u32; CURSOR_W * CURSOR_H] = [0; CURSOR_W * CURSOR_H];
static mut CURSOR_X: i32 = 0;
static mut CURSOR_Y: i32 = 0;
static mut CURSOR_DRAWN: bool = false;
static mut CURSOR_READY: bool = false;

const CURSOR_PATTERN: [u16; CURSOR_H] = [
    0b100000000000, 0b110000000000, 0b111000000000, 0b111100000000,
    0b111110000000, 0b111111000000, 0b111111100000, 0b111111110000,
    0b111111111000, 0b111111100000, 0b111110000000, 0b111011000000,
    0b110001100000, 0b100000110000, 0b000000011000, 0b000000001100,
    0b000000000110, 0b000000000011, 0b000000000001,
];

pub unsafe fn init(mbi: u32) -> bool {
    let info = match mbi::FramebufferInfo::read(mbi) {
        Some(i) => i,
        None => {
            console::print(format_args!("J7.1: framebuffer absent (pas de tag 8)\n"));
            return false;
        }
    };

    console::print(format_args!(
        "J7.1: framebuffer {}x{} bpp={} pitch={} addr={:#x} type={}\n",
        info.width, info.height, info.bpp, info.pitch, info.addr, info.typ
    ));

    if info.bpp != 32 || info.typ != 1 {
        if info.typ == 2 {
            console::print(format_args!("J7.1: mode texte (BIOS sans graphique) — skip\n"));
        } else {
            console::print(format_args!(
                "J7.1: format non supporte (bpp={}, type={})\n",
                info.bpp, info.typ
            ));
        }
        return false;
    }
    console::print(format_args!("J7.1: would map (not doing)\n"));
    false
}

pub unsafe fn is_ready() -> bool { FB_READY }
pub unsafe fn width() -> u32 { FB_WIDTH }
pub unsafe fn height() -> u32 { FB_HEIGHT }
pub unsafe fn put_pixel(_x: u32, _y: u32, _color: u32) {}
pub unsafe fn fill_gradient() {}
pub unsafe fn fill_color(_color: u32) {}
pub unsafe fn cursor_init() {}
pub unsafe fn cursor_move(_new_x: i32, _new_y: i32) {}
pub unsafe fn cursor_ready() -> bool { false }
