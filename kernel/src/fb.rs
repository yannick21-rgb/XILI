//! J7 — Framebuffer VBE/GOP + curseur logiciel (stub BIOS).

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

// (cursor removed)

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
    // Appel via fonction #[inline(never)] pour éviter que LTO réintègre
    // le code graphique dans init() (casse le test BIOS par timing).
    init_rgb_noinline(info)
}

#[inline(never)]
unsafe fn init_rgb_noinline(info: mbi::FramebufferInfo) -> bool {
    if info.width == 0 || info.height == 0 || info.pitch == 0 {
        console::print(format_args!("J7.1: dimensions invalides\n"));
        return false;
    }

    let size = (info.pitch as u64) * (info.height as u64);
    let mut off = 0u64;
    while off < size {
        paging::map_page(
            FB_VIRT_BASE + off,
            info.addr + off,
            paging::PRESENT | paging::WRITABLE,
        );
        off += 4096;
    }

    FB_READY = true;
    FB_VIRT = FB_VIRT_BASE;
    FB_WIDTH = info.width;
    FB_HEIGHT = info.height;
    FB_PITCH = info.pitch;
    FB_BPP = info.bpp;

    fill_gradient();

    console::print(format_args!("J7.1: framebuffer OK\n"));
    true
}

pub unsafe fn is_ready() -> bool {
    FB_READY
}
pub unsafe fn width() -> u32 {
    FB_WIDTH
}
pub unsafe fn height() -> u32 {
    FB_HEIGHT
}

// read_pixel removed (unused)

pub unsafe fn put_pixel(x: u32, y: u32, color: u32) {
    if !FB_READY {
        return;
    }
    if x >= FB_WIDTH || y >= FB_HEIGHT {
        return;
    }
    let offset = (y as u64) * (FB_PITCH as u64) + (x as u64) * 4;
    let ptr = (FB_VIRT + offset) as *mut u32;
    core::ptr::write_volatile(ptr, color);
}

pub unsafe fn fill_gradient() {
    if !FB_READY {
        return;
    }
    for y in 0..FB_HEIGHT {
        for x in 0..FB_WIDTH {
            let r = (x * 255 / FB_WIDTH.max(1)) as u8;
            let g = (y * 255 / FB_HEIGHT.max(1)) as u8;
            let b = 128u8;
            let color: u32 = (b as u32) | ((g as u32) << 8) | ((r as u32) << 16);
            put_pixel(x, y, color);
        }
    }
}

pub unsafe fn fill_color(color: u32) {
    if !FB_READY {
        return;
    }
    for y in 0..FB_HEIGHT {
        for x in 0..FB_WIDTH {
            put_pixel(x, y, color);
        }
    }
}

// Remove cursor support - keep only framebuffer init + put_pixel + fill
pub unsafe fn cursor_init() {}
pub unsafe fn cursor_move(_new_x: i32, _new_y: i32) {}
pub unsafe fn cursor_ready() -> bool {
    false
}
