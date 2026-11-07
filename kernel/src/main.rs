//! XILI — noyau x86_64 `no_std`, écrit de zéro en Rust.
//!
//! Architecture du noyau (vue d'ensemble, jalons marquéés dans la séquence) :
//!
//!   1. `boot.s` / `isr.s` — BIOS → 32 bits → 64 bits long mode, GDT,
//!      IDT, TSS, PIC, PIT ; stubs ASM `entry_ring3` / `syscall_entry`.
//!   2. `mm/` — allocation mémoire : phys bitmap/buddy, slab, paging 4 niveaux.
//!   3. `task/` — planificateur à priorités avec aging.
//!   4. `irq.rs` / `idt.rs` — interruptions matérielles et exceptions.
//!   5. `console/` / `serial.rs` / `vga.rs` — canaux d'émission (UART + VGA).
//!   6. `vfs/` + `initrd/` + `elf/` + `syscall.rs` — J3 : VFS, initrd cpio,
//!      loader ELF, stubs syscalls, transition noyau → tâche ring 3.
//!   7. `run_initrd_elf()` — lancement du programme utilisateur embarqué.
//!
//! Le noyau est volontairement single-threadé : le scheduler tourne sur
//! timer IRQ (hlt), aucune exécution parallèle n'existe encore.

#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;

mod console;
mod elf;
mod idt;
mod initrd;
mod io;
mod irq;
mod mbi;
mod mm;
mod net;
mod storage;
mod pic;
mod runtime;
mod serial;
mod syscall;
mod task;
mod tss;
mod vga;
mod vfs;

global_asm!(include_str!("boot.s"), options(att_syntax));
global_asm!(include_str!("isr.s"), options(att_syntax));

extern "C" {
    fn entry_ring3(rip: u64, rsp: u64) -> !;
}

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

/// Point d'arrivee en R0 apres le test syscall (J3.2).
#[no_mangle]
extern "C" fn ring0_after_test() -> ! {
    crate::console::print(format_args!("J3.2: retour R0 via iretq apres syscall - OK\n"));
    loop { unsafe { asm!("hlt") } }
}

/// Point d'arrivee apres un page-fault volontaire (J3.2).
#[no_mangle]
extern "C" fn pf_test() -> ! {
    crate::console::print(format_args!("J3.2: page-fault volontaire - OK\n"));
    loop { unsafe { asm!("hlt") } }
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

    // --- J2.4 : allocateur physique bitmap --------------------------------
    //
    // Le bitmap est construit à partir de la carte MBI, puis testé
    // en boucle : 100 Mio de pages sont allouées puis libérées ; le
    // compteur de pages libres doit revenir exactement à la valeur
    // initiale.
    //
    // L'allocateur n'est pas appelé depuis les IRQ (AI-WORKFLOW §6) : il
    // n'y a pas encore d'interruptions, et l'allocation reste
    // sédentaire.
    //
    // IMPORTANT : phys::init DOIT précéder paging::build car build()
    // alloue maintenant les PT de l'identity map en 4 Kio via alloc_page.
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


    //
    // Après l'allocateur physique bitmap (J2.4), on initialise le slab
    // qui fournit kmalloc/kfree. Le test alloue/libère des blocs de
    // tailles variées, vérifie l'alignement et la non-contamination.
    unsafe { mm::slab::kmalloc_init() };
    console::print(format_args!(
        "allocateur noyau : slab (kmalloc/kfree, {} pages)\n",
        mm::slab::pages()
    ));
    let kmalloc_ok = unsafe { mm::slab::test() };
    console::print(format_args!(
        "J2.5c: allocateur noyau slab kmalloc/kfree - {} (pages {})\n",
        if kmalloc_ok { "OK" } else { "ECHEC" },
        mm::slab::pages()
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
    console::print(format_args!("J3.2: configuration MSR + GDT segments R3...\n"));
    unsafe { syscall::init() };
    console::print(format_args!(
        "J3.2: configuration MSR (EFER/STAR/LSTART/FMASK) + GDT R0/R3 OK\n"
    ));

    // --- J3.5 : INITRD (archive cpio newc) -----------------------------
    static INITRD_ARCHIVE: &[u8] = include_bytes!("../../assets/initrd.cpio");
    crate::initrd::init(INITRD_ARCHIVE);

    #[repr(C, align(16))]
    struct AlignedStack([u8; 8192]);
    static mut RING3_STACK: AlignedStack = AlignedStack([0; 8192]);
    let user_stack = core::ptr::addr_of!(RING3_STACK) as u64;
    let user_stack_top = user_stack + 8192;
    console::print(format_args!(
        "  user stack = {:#018x} (top = {:#018x})\n",
        user_stack as u64, user_stack_top as u64
    ));
    console::print(format_args!("  entree R3: iretq -> CS=0x2B SS=0x33\n"));

    console::print(format_args!("J3.4: VFS + /dev pret\n"));
    unsafe { crate::vfs::init() };
    unsafe {
        if let Some(tty) = crate::vfs::lookup(b"tty") {
            crate::vfs::open(tty, Some(0));
            crate::vfs::open(tty, Some(1));
            crate::vfs::open(tty, Some(2));
        }
    }

    // J5 : initialisation du sous-système réseau (virtio-net).
    console::print(format_args!("J5.0: net pret\n"));
    let _net_ok = unsafe { crate::net::virtio_net::init() };
    if _net_ok {
        console::print(format_args!("J5.1: virtio-net OK\n"));
        unsafe { crate::net::virtio_net::rx_init_buffers() };
        // Ping ICMP : echo request vers 10.0.2.2 (passerelle QEMU user networking).
        let ok = unsafe { crate::net::virtio_net::ping([10, 0, 2, 2], 200) };
        if ok {
            console::print(format_args!("J5.3: ping OK\n"));
        } else {
            console::print(format_args!("J5.3: ping echec\n"));
        }
    } else {
        console::print(format_args!("J5.1: virtio-net absent (QEMU sans -net ?)\n"));
    }

    // J6 : stockage IDE legacy + ext2 (lecture seule + création test).
    console::print(format_args!("J6.0: storage pret\n"));
    let _storage_ok = unsafe { crate::storage::ide::init() };
    if _storage_ok {
        console::print(format_args!("J6.1: IDE OK\n"));
        let ext2_ok = unsafe { crate::storage::ext2::init() };
        if ext2_ok {
            console::print(format_args!("J6.2: ext2 OK\n"));
            unsafe { crate::storage::ext2::list_dir(2) };
            let mut fbuf = [0u8; 64];
            let n = unsafe { crate::storage::ext2::read_file(12, &mut fbuf) };
            crate::console::print(format_args!("ext2: hello.txt ({} octets): ", n));
            for i in 0..n {
                crate::serial::write_byte(fbuf[i]);
            }
            crate::serial::write_byte(b'\n');
        } else {
            console::print(format_args!("J6.2: ext2 echec\n"));
        }
    } else {
        console::print(format_args!("J6.1: IDE absent\n"));
    }

    unsafe { crate::syscall::set_current_exe(b"shell.elf") };
    // Lance shell.elf depuis l'initrd (J3 final : premier shell utilisateur).
    run_initrd_elf("shell.elf", user_stack_top)
}

fn run_initrd_elf(name: &str, user_stack_top: u64) -> ! {
    let name_bytes = name.as_bytes();
    let elf_data = match crate::initrd::find(name_bytes) {
        Some((data, size)) => unsafe { core::slice::from_raw_parts(data, size) },
        None => {
            console::print(format_args!(
                "J3.5: ECHEC — '{}' introuvable dans l'initrd\n",
                name
            ));
            loop { unsafe { asm!("hlt") } }
        }
    };

    console::print(format_args!("J3.5: chargement de '{}' depuis l'initrd...\n", name));
    let elf_info = match unsafe { elf::load_elf(elf_data) } {
        Some(info) => info,
        None => {
            console::print(format_args!("J3.5: ECHEC — ELF invalide\n"));
            loop { unsafe { asm!("hlt") } }
        }
    };

    // Interpréteur (ld-musl) : on démarre sur son entrée, pas sur celle
    // du programme. AT_BASE pointe sur la base de l'interpréteur.
    let (entry, stack_top) = if elf_info.interp_name_len > 0 {
        let interp_name = &elf_info.interp_name[..elf_info.interp_name_len];
        let interp_str = core::str::from_utf8(interp_name).unwrap_or("<inv>");
        console::print(format_args!(
            "  interpréteur : '{}' (PT_INTERP)\n",
            interp_str
        ));

        let (idata, isize) = match crate::initrd::find(interp_name) {
            Some(v) => v,
            None => {
                console::print(format_args!(
                    "J3.5: ECHEC — interpréteur '{}' introuvable\n",
                    interp_str
                ));
                loop { unsafe { asm!("hlt") } }
            }
        };
        let idata_slice = unsafe { core::slice::from_raw_parts(idata, isize) };
        let interp_info = match unsafe { elf::load_elf(idata_slice) } {
            Some(info) => info,
            None => {
                console::print(format_args!("J3.5: ECHEC — interpréteur ELF invalide\n"));
                loop { unsafe { asm!("hlt") } }
            }
        };
        let interp_base = interp_info.load_base;
        let sp = unsafe { elf::build_stack(user_stack_top, 4, name, &elf_info, interp_base) };
        (interp_info.entry, sp)
    } else {
        let sp = unsafe { elf::build_stack(user_stack_top, 4, name, &elf_info, 0) };
        (elf_info.entry, sp)
    };

    console::print(format_args!(
        "  ELF entre  = {:#018x} ({} octets)\n",
        entry, elf_data.len()
    ));
    console::print(format_args!("  execution en R3 via iretq...\n"));

    // J4-C : enregistrer le processus comme tâche du scheduler, puis
    // passer en ring 3 via entry_ring3. Le contexte du processus est
    // sauvegardé dans sa tâche à chaque syscall (handle_syscall).
    unsafe {
        let task_id = crate::task::create_task(crate::syscall::user_task_entry, 1);
        if task_id < crate::task::MAX_TASKS {
            crate::syscall::register_user_task(task_id, entry, stack_top);
            crate::syscall::USER_TASK_ID = task_id;
        }
    }

    unsafe { entry_ring3(entry, stack_top) };
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    console::print(format_args!("\n*** PANIC ***\n{}\n", info));
    loop { unsafe { asm!("hlt") } }
}
