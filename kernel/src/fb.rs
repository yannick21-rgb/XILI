//! J7 — Framebuffer VBE/GOP + curseur logiciel (stub BIOS).

use crate::console;
use crate::mbi;

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

pub unsafe fn is_ready() -> bool {
    false
}
pub unsafe fn width() -> u32 {
    0
}
pub unsafe fn height() -> u32 {
    0
}
pub unsafe fn put_pixel(_x: u32, _y: u32, _color: u32) {}
pub unsafe fn fill_gradient() {}
pub unsafe fn fill_color(_color: u32) {}
pub unsafe fn cursor_init() {}
pub unsafe fn cursor_move(_new_x: i32, _new_y: i32) {}
pub unsafe fn cursor_ready() -> bool {
    false
}
