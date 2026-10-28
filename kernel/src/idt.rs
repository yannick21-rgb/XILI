#![allow(function_casts_as_integer)]

/* ============================================================
 * idt.rs — Table d'interruptions (IDT) + gestion des exceptions
 *
 * J2.5 : 256 entrées. Les vecteurs 0..31 sont des exceptions ;
 * 32..255 sont des IRQ (restent à NULL tant qu'il n'y a pas de
 * PIC/APIC). Les fautes critiques sont gérées explicitement :
 *
 *   #DF (8)  Double Fault  → IST1 (pile de secours)
 *   #GP (13) General Prot → code d'erreur + RIP
 *   #PF (14) Page Fault   → CR2 + code d'erreur + RIP
 *
 * Les stubs d'assemblage uniformisent la pile : chaque stub pousse
 *   [vector][error_code][rip][cs][rflags]
 * puis `call handle_exception`. Le CPU pousse error_code/RIP/CS/RFLAGS ;
 * le stub ajoute un 0 factice pour les exceptions sans code d'erreur.
 * ============================================================ */

use core::arch::asm;

use crate::console;

/* ------------------------------------------------------------
 * Cadre d'exception.
 *
 * Layout construit par les stubs :
 *   [vector][error_code][rip][cs][rflags]
 * ------------------------------------------------------------ */

/// Cadre d'une exception, lu par [`handle_exception`].
#[repr(C)]
pub struct ExceptionFrame {
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

/// Nombre d'entrées de l'IDT.
const IDT_LEN: usize = 256;

/// Descripteur d'interruption (16 octets).
///
/// Layout Intel SDM vol. 3 §6.14.1 :
///
/// | bits    | champ                |
/// |---------|----------------------|
/// | 0..15   | offset [15:0]        |
/// | 16..31  | selector (0x08=code) |
/// | 32..34  | IST (0 ou 1..7)      |
/// | 35..39  | 0 (réservé)          |
/// | 40..43  | type (0xE interr./0xF trap) |
/// | 44..46  | DPL                  |
/// | 47      | présent              |
/// | 48..63  | offset [31:16]       |
/// | 64..95  | offset [63:32]       |
/// | 96..127 | 0 (réservé)          |
#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    gate_type: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// Sélecteur de code noyau.
    const CODE_SELECTOR: u16 = 0x08;

    /// Construit une entrée pointant vers `handler`.
    ///
    /// `ist` = 0 (pile courante) ou 1..7 (pile IST).
    /// `gate_type` = 0x8E (interrupt gate) ou 0x8F (trap gate).
    const fn new(handler: usize, ist: u8, gate_type: u8) -> Self {
        IdtEntry {
            offset_low: (handler & 0xFFFF) as u16,
            selector: Self::CODE_SELECTOR,
            ist: ist & 0x07,
            gate_type,
            offset_mid: ((handler >> 16) & 0xFFFF) as u16,
            offset_high: ((handler >> 32) & 0xFFFF_FFFF) as u32,
            reserved: 0,
        }
    }

    /// Entrée absente (NULL) : déclenche #GP si utilisée.
    const fn null() -> Self {
        IdtEntry {
            offset_low: 0,
            selector: 0,
            ist: 0,
            gate_type: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
}

/// L'IDT — 256 entrées, statique, 4096 octets (une page).
static mut IDT: [IdtEntry; IDT_LEN] = [IdtEntry::null(); IDT_LEN];

/// Pointeur IDTR (16 octets en mode long).
#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

/* ------------------------------------------------------------
 * Stubs d'assemblage — 32 vecteurs d'exceptions.
 *
 * Exceptions SANS code d'erreur (poussent un 0 factice) :
 *   0,1,2,3,4,5,6,7,8,9,16,18,19,20,22,23,24,25,26,27,28,31
 *
 * Exceptions AVEC code d'erreur (le CPU l'a déjà poussé) :
 *   10,11,12,13,14,17,21,29,30
 * ------------------------------------------------------------ */

// Symboles des stubs — déclarés extern "C" pour être adressables
// depuis Rust. Leur définition se trouve dans le global_asm! ci-dessous.
extern "C" {
    fn isr_stub_0();
    fn isr_stub_1();
    fn isr_stub_2();
    fn isr_stub_3();
    fn isr_stub_4();
    fn isr_stub_5();
    fn isr_stub_6();
    fn isr_stub_7();
    fn isr_stub_8();
    fn isr_stub_9();
    fn isr_stub_10();
    fn isr_stub_11();
    fn isr_stub_12();
    fn isr_stub_13();
    fn isr_stub_14();
    fn isr_stub_16();
    fn isr_stub_17();
    fn isr_stub_18();
    fn isr_stub_19();
    fn isr_stub_20();
    fn isr_stub_21();
    fn isr_stub_22();
    fn isr_stub_23();
    fn isr_stub_24();
    fn isr_stub_25();
    fn isr_stub_26();
    fn isr_stub_27();
    fn isr_stub_28();
    fn isr_stub_29();
    fn isr_stub_30();
    fn isr_stub_31();
}

// Les stubs `isr_stub_N` et `isr_common` sont définis dans isr.s,
// inclus depuis main.rs par `global_asm!(include_str!("isr.s"), ...)`.

// Stubs d'interruptions matérielles (vecteurs 32..47).
extern "C" {
    fn irq_stub_32();
    fn irq_stub_33();
    fn irq_stub_34();
    fn irq_stub_35();
    fn irq_stub_36();
    fn irq_stub_37();
    fn irq_stub_38();
    #[allow(dead_code)]
    fn irq_stub_39();
    fn irq_stub_40();
    fn irq_stub_41();
    fn irq_stub_42();
    fn irq_stub_43();
    fn irq_stub_44();
    fn irq_stub_45();
    fn irq_stub_46();
    fn irq_stub_47();
}

/* ------------------------------------------------------------
 * Lectures de registres matériels.
 * ------------------------------------------------------------ */

/// Lit CR2 — l'adresse virtuelle ayant déclenché le #PF.
fn read_cr2() -> u64 {
    let cr2: u64;
    unsafe {
        asm!("mov {0}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
    }
    cr2
}

/* ------------------------------------------------------------
 * Gestionnaire d'exceptions.
 * ------------------------------------------------------------ */

/// Nom lisible d'un vecteur d'exception.
fn vector_name(vector: u64) -> &'static str {
    match vector {
        0 => "#DE Divide Error",
        1 => "#DB Debug",
        2 => "NMI",
        3 => "#BP Breakpoint",
        4 => "#OF Overflow",
        5 => "#BR BOUND Range",
        6 => "#UD Invalid Opcode",
        7 => "#NM Device Not Available",
        8 => "#DF Double Fault",
        9 => "FS Error",
        10 => "#TS Invalid TSS",
        11 => "#NP Segment Not Present",
        12 => "#SS Stack-Segment Fault",
        13 => "#GP General Protection",
        14 => "#PF Page Fault",
        15 => "reserved",
        16 => "#MF x87 FPU",
        17 => "#AC Alignment Check",
        18 => "#MC Machine Check",
        19 => "#XF SIMD FP Exception",
        20 => "#VE Virtualization",
        21 => "#VC Virtual Callable",
        22 => "#NPF Nested Page Fault",
        23..=31 => "reserved",
        _ => "unknown",
    }
}

/// Gestionnaire commun appelé par tous les stubs.
///
/// # Argument
///
/// `frame` pointe sur la pile telle que construite par le stub :
///   [vector][error_code][rip][cs][rflags]
///
/// # Comportement
///
/// Détaille la faute (vecteur, nom, code d'erreur, RIP, CR2 pour
/// #PF) sur la console, puis bloque dans une boucle `hlt`. Ce n'est
/// PAS un `panic!` : il n'y a pas de débordement de pile à ce stade,
/// et le handler tient déjà dans la pile IST pour #DF.
#[no_mangle]
pub extern "C" fn handle_exception(frame: *const ExceptionFrame) -> ! {
    let f = unsafe { &*frame };

    console::print(format_args!("\n--- EXCEPTION ---\n"));
    console::print(format_args!(
        "vecteur {} ({})\n", f.vector, vector_name(f.vector)
    ));
    console::print(format_args!("rip      : {:#018x}\n", f.rip));
    console::print(format_args!("code     : {:#x}\n", f.error_code));

    // #PF : CR2 donne l'adresse fautive.
    if f.vector == 14 {
        let cr2 = read_cr2();
        let present = f.error_code & 0x1 != 0;
        let write = f.error_code & 0x2 != 0;
        let user = f.error_code & 0x4 != 0;
        let reserved = f.error_code & 0x8 != 0;
        let instr = f.error_code & 0x10 != 0;
        console::print(format_args!("cr2      : {:#018x}\n", cr2));
        console::print(format_args!(
            "         {} {}, {} mode, {}\n",
            if present { "protection violation" } else { "page non presente" },
            if write { "ecriture" } else { "lecture" },
            if user { "user" } else { "kernel" },
            if instr { "fetch instruction" } else { "" },
        ));
        if reserved {
            console::print(format_args!("         RSV (reserved bit violation)\n"));
        }
    }

    // #GP : décoder l'origine du code d'erreur.
    if f.vector == 13 {
        let ext = f.error_code & 0x1 != 0;
        let idt = f.error_code & 0x2 != 0;
        let ti = f.error_code & 0x4 != 0;
        let index = f.error_code >> 3;
        console::print(format_args!(
            "         {}{}{}, index {}\n",
            if ext { "externe" } else { "interne" },
            if idt { " [IDT]" } else { " [GDT/LDT]" },
            if ti { " (TSS)" } else { "" },
            index,
        ));
    }

    // #DF : code d'erreur toujours 0, pile IST1.
    if f.vector == 8 {
        console::print(format_args!("         (double fault — pile IST1)\n"));
    }

    console::print(format_args!("--- blocage ---\n"));
    loop {
        unsafe { asm!("hlt") }
    }
}

/* ------------------------------------------------------------
 * Initialisation.
 *
 * Remplit les 32 premières entrées (exceptions) et les 16 suivantes
 * (IRQ 32..47 via le PIC remappé). Les IRQ restantes (48..255)
 * restent NULL.
 * ------------------------------------------------------------ */

pub unsafe fn init() {
    // --- Exceptions (vecteurs 0..31) ---
    let stubs: [usize; 32] = [
        isr_stub_0 as usize,
        isr_stub_1 as usize,
        isr_stub_2 as usize,
        isr_stub_3 as usize,
        isr_stub_4 as usize,
        isr_stub_5 as usize,
        isr_stub_6 as usize,
        isr_stub_7 as usize,
        isr_stub_8 as usize,
        isr_stub_9 as usize,
        isr_stub_10 as usize,
        isr_stub_11 as usize,
        isr_stub_12 as usize,
        isr_stub_13 as usize,
        isr_stub_14 as usize,
        0, // 15 réservé
        isr_stub_16 as usize,
        isr_stub_17 as usize,
        isr_stub_18 as usize,
        isr_stub_19 as usize,
        isr_stub_20 as usize,
        isr_stub_21 as usize,
        isr_stub_22 as usize,
        isr_stub_23 as usize,
        isr_stub_24 as usize,
        isr_stub_25 as usize,
        isr_stub_26 as usize,
        isr_stub_27 as usize,
        isr_stub_28 as usize,
        isr_stub_29 as usize,
        isr_stub_30 as usize,
        isr_stub_31 as usize,
    ];

    for vec in 0..32usize {
        if vec == 15 {
            continue; // vecteur 15 = réservé, reste NULL
        }

        // #DF (8) : IST1 + trap gate. Toutes les autres : interrupt gate.
        let (ist, gate) = if vec == 8 {
            (1u8, 0x8Fu8) // IST1, trap gate
        } else {
            (0u8, 0x8Eu8) // pile courante, interrupt gate
        };

        IDT[vec] = IdtEntry::new(stubs[vec], ist, gate);
    }

    // --- IRQ hardware (vecteurs 32..47, remappés par le PIC) ---
    // Interrupt gates, pile courante (IST=0), DPL=0.
    let irq_stubs: [usize; 16] = [
        irq_stub_32 as usize,
        irq_stub_33 as usize,
        irq_stub_34 as usize,
        irq_stub_35 as usize,
        irq_stub_36 as usize,
        irq_stub_37 as usize,
        irq_stub_38 as usize,
        irq_stub_39 as usize,
        irq_stub_40 as usize,
        irq_stub_41 as usize,
        irq_stub_42 as usize,
        irq_stub_43 as usize,
        irq_stub_44 as usize,
        irq_stub_45 as usize,
        irq_stub_46 as usize,
        irq_stub_47 as usize,
    ];

    for (i, &handler) in irq_stubs.iter().enumerate() {
        IDT[32 + i] = IdtEntry::new(handler, 0, 0x8E);
    }

    // Charger l'IDTR.
    let idtr = Idtr {
        limit: ((core::mem::size_of::<IdtEntry>() * IDT_LEN - 1) & 0xFFFF) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };
    asm!(
        "lidt [{0}]",
        in(reg) core::ptr::addr_of!(idtr) as u64,
        options(nostack, preserves_flags)
    );
}
