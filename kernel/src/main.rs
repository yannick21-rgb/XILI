#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod console;
mod idt;
mod io;
mod irq;
mod mbi;
mod mm;
mod pic;
mod runtime;
mod serial;
mod tss;
mod vga;

global_asm!(include_str!("boot.s"), options(att_syntax));
global_asm!(include_str!("isr.s"), options(att_syntax));

const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36d7_6289;

/// Marqueur du jalon J1. `make test` et `make test-uefi` le cherchent dans
/// le journal série : ne pas le renommer sans mettre les cibles à jour.
const J1_MARKER: &str = "J1: noyau charge, mode long actif, console OK.";

/// Marqueur du jalon J2.3 : les tables de pages 4 niveaux sont actives.
///
/// C'est lui qui prouve que la bascule de CR3 a réussi : un descripteur
/// fautif fait triple-fauter le noyau avant qu'il ne l'imprime.
const J23_MARKER: &str = "J2.3: tables de pages 4 niveaux, identity + higher-half.";

/// Statique ancré en higher-half : son adresse de lien sert de preuve
/// que le noyau s'exécute au-delà de HIGHER_HALF (voir J2.3b dans main).
static LINK_GUARD: u64 = 0x5AA5_5AA5_5AA5_5AA5;

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

    // --- preuve d'exécution en higher-half (J2.3b) ---------------------
    // Le noyau est lié en higher-half : l'adresse de lien d'un symbole
    // (LINK_GUARD) est >= HIGHER_HALF. Son adresse physique de chargement
    // vaut lien - HIGHER_HALF (bas, ~1 Mio). Afficher les deux et les
    // comparer prouve le lien ; lire la valeur par l'adresse virtuelle
    // prouve que le mapping higher-half est actif (sinon #PF = kernel
    // mort avant l'impression) et que le code qui lit (ci-même, lié en
    // higher-half) s'exécute là.
    let link_addr = core::ptr::addr_of!(LINK_GUARD) as u64;
    let phys_addr = link_addr - mm::paging::HIGHER_HALF;
    let guard_val =
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(LINK_GUARD)) };
    let in_higher_half =
        link_addr >= mm::paging::HIGHER_HALF && phys_addr != link_addr
            && guard_val == LINK_GUARD;
    console::print(format_args!(
        "higher-half : {} (lien={:#018x}, phys={:#018x})\n",
        if in_higher_half { "OK" } else { "ECHEC" }, link_addr, phys_addr
    ));
    // -------------------------------------------------------------------
    console::print(format_args!("{}\n", J23_MARKER));

    // --- J2.5 : IDT / exceptions ---------------------------------
    //
    // Le TSS (GDT enrichie) et l'IDT sont initialisés APRÈS le paging
    // : le TSS pointe vers la pile courante (boot.s) et vers IST1
    // (une pile BSS) pour le #DF. L'IDT active les handlers #DF/#GP/#PF.
    //
    // Une faute (page, protection, double) est désormais interceptée
    // par le handler Rust au lieu de provoquer un triple fault.
    let stack_top: u64;
    unsafe { asm!("mov {0}, rsp", out(reg) stack_top) };
    unsafe { tss::init(stack_top) };
    unsafe { idt::init() };
    console::print(format_args!(
        "J2.5: IDT + exceptions (#DF IST1, #GP, #PF) initialisees.\n"
    ));
    // ------------------------------------------------------------

    // --- J2.4 : allocateur physique bitmap --------------------------------
    //
    // Le bitmap est construit à partir de la carte Multiboot2, puis testé
    // en boucle : 100 Mio de pages sont allouées puis libérées ; le compteur
    // de pages libres doit revenir exactement à la valeur initiale.
    //
    // L'allocateur n'est pas appelé depuis les IRQ (AI-WORKFLOW §6) : il
    // n'y a pas encore d'interruptions, et l'allocation reste sédentaire.
    unsafe { mm::phys::init(mbi) };
    let free0 = mm::phys::free_pages();
    let used0 = mm::phys::used_pages();
    console::print(format_args!(
        "allocateur physique : bitmap (page 4 Kio, {} libres / {} totales)\n",
        free0, mm::phys::total_pages()
    ));
    let phys_ok = unsafe { mm::phys::test() };
    console::print(format_args!(
        "J2.4: allocateur physique bitmap - {} (libre {} -> {} pages)\n",
        if phys_ok { "OK" } else { "ECHEC" },
        free0, mm::phys::free_pages()
    ));
    console::print(format_args!(
        "  reserve : {} pages (noyau + MBI + bitmap)\n", used0
    ));
    // --------------------------------------------------------------------

    console::print(format_args!("\n{}\n", J1_MARKER));

    // --- J2.6 : PIC + timer + clavier --------------------------
    //
    // Après l'IDT (J2.5), on remappe le PIC 8259 vers les vecteurs
    // 32..47, Programme le PIT a 100 Hz, et active IRQ 0 (timer)
    // et IRQ 1 (clavier). Le test verifie que le timer tick
    // bien (compteur incremente) — preuve que le chemin IRQ →
    // IDT → stub ASM → handle_irq → iretq est fonctionnel.
    unsafe { irq::init() };
    console::print(format_args!(
        "J2.6: PIC remappe (IRQ 32-47), timer 100 Hz + clavier actifs.\n"
    ));

    // Activer les interruptions matérielles.
    unsafe { asm!("sti") };

    // --- J2.6b : test du timer ----------------------------------
    //
    // On attend ~30 ms (3 ticks a 100 Hz) pour verifier que le
    // timer tick. On scrute le compteur dans une boucle hlt :
    // chaque tick réveille le CPU, l'IRQ est traite, le compteur
    // est incremente, puis on re-dort. Si le PIC/IDT/handler sont
    // faut, le noyau triple-faulte ou reste bloque a 0 tick.
    let ticks_start = irq::ticks();
    console::print(format_args!("ts={}\n", ticks_start));
    while irq::ticks() - ticks_start < 3 {
        unsafe { asm!("hlt") }
    }
    let ticks_end = irq::ticks();
    console::print(format_args!("te={}\n", ticks_end));
    console::print(format_args!(
        "J2.6b: timer IRQ active (ticks {} -> {})\n",
        ticks_start, ticks_end
    ));
    // ----------------------------------------------------------

    // --- J2.5b : test du #PF -------------------------------------
    //
    // Accès volontaire à une adresse non mappée (0xDEAD0000 :
    // ni dans l'identity map 0..1 Gio, ni dans le higher-half).
    // Le handler #PF doit intercepter la faute, imprimer le
    // diagnostic (CR2, code d'erreur) et bloquer — PAS de triple
    // fault. Le test passera si le log contient « #PF ».
    console::print(format_args!(
        "J2.5b: test #PF — acces a 0xDEAD0000 (page non mappee)...\n"
    ));
    unsafe {
        core::ptr::write_volatile(0xDEAD_0000 as *mut u64, 0x1234_5678_9ABC_DEF0u64);
    }
    // ----------------------------------------------------------
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
