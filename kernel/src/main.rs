#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod console;
mod mbi;
mod mm;
mod runtime;
mod serial;
mod vga;

global_asm!(include_str!("boot.s"), options(att_syntax));

const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d7_6289;

/// Marqueur du jalon J1. `make test` et `make test-uefi` le cherchent dans
/// le journal série : ne pas le renommer sans mettre les cibles à jour.
const J1_MARKER: &str = "J1: noyau charge, mode long actif, console OK.";

/// Marqueur du jalon J2.3 : les tables de pages 4 niveaux sont actives.
///
/// C'est lui qui prouve que la bascule de CR3 a réussi : un descripteur
/// fautif fait triple-fauter le noyau avant qu'il ne l'imprime.
const J23_MARKER: &str = "J2.3: tables de pages 4 niveaux, identity + higher-half.";

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

    // --- J2.2 : carte mémoire -------------------------------------------
    //
    // La carte mémoire Multiboot2 est parfois appelée « E820 », du nom de
    // l'interruption BIOS qui la fournissait avant l'UEFI. Le format n'est
    // pas identique, mais la structure et l'intention le sont.

    let info = unsafe { mbi::MemInfo::read(mbi) };
    match info {
        Some(info) => {
            console::print(format_args!(
                "tag 1 mem_lower : {} Kio (sous 1 Mio)\n",
                info.mem_lower
            ));
            console::print(format_args!(
                "tag 1 mem_upper : {} Kio (au-dessus de 1 Mio)\n",
                info.mem_upper
            ));
        }
        None => {
            // GRUB 2.12 émet un tag MEMORY_INFO de 9 octets au lieu des 16
            // de la spec : les deux champs sont absents. On le dit plutôt
            // que d'afficher du remplissage.
            console::print(format_args!(
                "tag 1 mem_lower : absent (taille de tag non conforme)\n"
            ));
        }
    }

    console::print(format_args!(
        "taille de la mbi : {} octets, somme de controle {:#x}\n",
        unsafe { mbi::total_size(mbi) },
        unsafe { mm::checksum(mbi) }
    ));

    unsafe { mm::report_all(mbi) };
    // --------------------------------------------------------------------

    // --- J2.3 : tables de pages 4 niveaux --------------------------------
    //
    // On construit les tables en Rust et on les active. L'identity map
    // est conservé : le noyau n'est pas encore relié en higher-half, son
    // code et sa pile vivent à 1 Mio et doivent rester accessibles.
    //
    // Si les tables sont fautes, la bascule de CR3 triple-faulte — QEMU
    // redémarre et le marqueur J2.3 n'apparait pas.

    let tables = mm::paging::build();
    mm::paging::report(&tables);
    unsafe { mm::paging::load(tables.pml4()) };

    console::print(format_args!(
        "\nCR3 charge : {:#018x}\n",
        tables.pml4()
    ));
    mm::paging::verify();
    console::print(format_args!("{}\n", J23_MARKER));
    // --------------------------------------------------------------------

    console::print(format_args!("\n{}\n", J1_MARKER));
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
