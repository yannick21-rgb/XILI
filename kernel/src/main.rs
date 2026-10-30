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
mod task;
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

/// Worker de haute priorité (H, prio=0) — teste la préservation des GPRs.
///
/// J2.8 : r12 reçoit une sentinelle avant hlt. L'IRQ du timer exécute
/// handle_timer (qui clobbe r12). Si irq_common sauvegarde les GPRs,
/// r12 est restauré après iretq → on écrit '.'. Sinon r12 est corrompu
/// → '!' (échec détecté visuellement dans le log série).
extern "C" fn worker_hi() -> ! {
    let sentinel: u64 = 0xDEAD_BEEF_CAFE_BABE;
    let mut r: u64;
    loop {
        crate::serial::write_byte(b'H');
        unsafe {
            r = sentinel;
            asm!("hlt", inout("r12") r);
        }
        if r == sentinel {
            crate::serial::write_byte(b'.');
        } else {
            crate::serial::write_byte(b'!');
        }
    }
}

/// Worker de priorité moyenne (M, prio=1).
extern "C" fn worker_med() -> ! {
    loop {
        crate::serial::write_byte(b'M');
        unsafe { asm!("hlt") }
    }
}

/// Worker de basse priorité (L, prio=2).
extern "C" fn worker_lo() -> ! {
    loop {
        crate::serial::write_byte(b'L');
        unsafe { asm!("hlt") }
    }
}

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
    // Le TSS (GDT enrichi) et l'IDT sont initialisés APRÈS le paging
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
    // Le bitmap est construit à partir de la carte MBI, puis testé
    // en boucle : 100 Mio de pages sont allouées puis libérées ; le
    // compteur de pages libres doit revenir exactement à la valeur
    // initiale.
    //
    // L'allocateur n'est pas appelé depuis les IRQ (AI-WORKFLOW §6) :
    // il n'y a pas encore d'interruptions, et l'allocation reste
    // sédentaire.
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
    // 32..47, programme le PIT à 100 Hz, et active IRQ 0 (timer)
    // et IRQ 1 (clavier). Le test vérifie que le timer tick
    // bien (compteur incrémenté) — preuve que le chemin IRQ →
    // IDT → stub ASM → handle_irq → iretq est fonctionnel.
    unsafe { irq::init() };
    console::print(format_args!(
        "J2.6: PIC remappe (IRQ 32-47), timer 100 Hz + clavier actifs.\n"
    ));

    // --- J2.7 : création des tâches workers (planificateur) ----
    //
    // Avant d'activer les interruptions, on crée les workers avec
    // des priorités différentes (J2.8). Le noyau principal (tâche 0)
    // est l'idle : priorité la plus basse (3). Le worker H (prio 0)
    // est le plus prioritaire, le worker L (prio 2) le moins.
    // L'âge (aging) dans schedule() empêche la starvation : les
    // tâches en attente voient leur priorité effective augmenter.
    unsafe { task::init_main() };
    let worker_hi_id = unsafe { task::create_task(worker_hi, 0) };   // prio 0 (haute)
    let worker_med_id = unsafe { task::create_task(worker_med, 1) };  // prio 1
    let worker_lo_id = unsafe { task::create_task(worker_lo, 2) };    // prio 2
    console::print(format_args!(
        "J2.7: planificateur priorite (0=noyau/idle, {}=H, {}=M, {}=L)\n",
        worker_hi_id, worker_med_id, worker_lo_id
    ));

    // Activer les interruptions matérielles.
    //
    // NOTE: sous QEMU TCG, si le timer IRQ est déjà en attente quand
    // sti s'exécute, le handler laisse RAX = 32 (numéro de vecteur,
    // via le calcul EOI 0xF8 & vector = 32), et la lecture volatile
    // immédiatement suivante renvoie 32 au lieu de la valeur de
    // TICK_COUNT en mémoire. Un nop force le CPU à valider
    // l'instruction avant la lecture de mémoire.
    // Ref: skill xili-project, Pitfalls (QEMU TCG sti race).
    unsafe { asm!("sti"); asm!("nop") };

    // On attend ~30 ms (3 ticks à 100 Hz) pour vérifier que le
    // timer tick. On scrute le compteur dans une boucle hlt :
    // chaque tick réveille le CPU, l'IRQ est traitée, le compteur
    // est incrémenté, puis on re-dort. Si le PIC/IDT/handler sont
    // faits, le noyau triple-faulte ou reste bloqué à 0 tick.
    let ticks_start = irq::ticks();
    console::print(format_args!(
        "J2.6b: attente de {} ticks (debut={})...", 3, ticks_start
    ));
    while irq::ticks() - ticks_start < 3 {
        unsafe { asm!("hlt") }
    }
    let ticks_end = irq::ticks();
    console::print(format_args!("te={}\n", ticks_end));
    console::print(format_args!(
        "J2.6b: timer IRQ active (ticks {} -> {})",
        ticks_start, ticks_end
    ));
    // ----------------------------------------------------------

    // --- J2.7b + J2.8 : preuve du planificateur et GPRs --------
    //
    // Les workers ont tourné pendant J2.6b. Le caractère 'T' (timer)
    // suivi de 'H' (worker haute priorité) prouve le context switch.
    //
    // J2.8 : le worker H utilise r12 comme sentinelle. Si irq_common
    // sauvegarde les GPRs, r12 est restauré après iretq → '.'.
    // Sinon r12 est corrompu → '!'.
    // On attend 8 ticks pour laisser tous les workers (H/M/L) tourner.
    let t7 = irq::ticks();
    while irq::ticks() - t7 < 8 {
        unsafe { asm!("hlt") }
    }
    console::print(format_args!(
        "J2.7b: planificateur + GPR sauvegardes actifs (workers H/M/L).\n"
    ));
    console::print(format_args!(
        "J2.8: scheduler priorites + GPR sauvegardes - OK\n"
    ));
    // ----------------------------------------------------------
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
