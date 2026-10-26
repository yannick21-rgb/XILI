#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod console;
mod serial;
mod vga;

global_asm!(include_str!("boot.s"), options(att_syntax));

const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d7_6289;

#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, mbi: u32) -> ! {
    serial::init();
    vga::clear();

    console::print(format_args!("XILI - noyau x86-64 (Rust)\n"));
    console::print(format_args!("--------------------------------------\n"));
    console::print(format_args!("magic multiboot2 : {:#010x}\n", magic));
    console::print(format_args!("adresse mbi      : {:#010x}\n", mbi));

    if magic == MULTIBOOT2_BOOTLOADER_MAGIC {
        console::print(format_args!("Boot multiboot2  : OK\n"));
    } else {
        console::print(format_args!("Boot multiboot2  : ECHEC (magic invalide)\n"));
    }

    console::print(format_args!("\nJ1: noyau charge, mode long actif, console OK.\n"));
    console::print(format_args!("Le systeme est en halt. Ctrl+Alt+Suppr pour quitter QEMU.\n"));

    loop {
        unsafe { asm!("hlt") }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    console::print(format_args!("\n*** PANIC ***\n{}\n", info));
    loop {
        unsafe { asm!("hlt") }
    }
}
