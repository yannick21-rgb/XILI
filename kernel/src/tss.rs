/* ============================================================
 * tss.rs — TSS 64 bits + GDT noyau + IST1 pour #DF
 *
 * J2.5 : le TSS 64 bits n'est plus qu'une table de piles. Les champs
 * qui nous concernent :
 *
 *   - RSP0     : pile du noyau (anneau 0 → 0, pas utilisé directement
 *                à ce stade — les IRQ partent de l'anneau 0).
 *
 *   - IST[0]   : pile IST1 — une pile connue dans la BSS, pointée
 *                par le descripteur #DF de l'IDT. Une double faute
 *                survient souvent avec une pile corrompue ou un
 *                débordement : repartir sur la pile courante la
 *                ferait planter à nouveau. IST1 évite le triple fault.
 *
 *   - IOMAP    : absente (I/O map base = 0xFFFF, au-delà de la TSS).
 *
 * Le descripteur TSS est un descripteur de segment système (S=0,
 * type=0x9 = TSS disponible) : il occupe DEUX entrées de GDT —
 * 0x18 et 0x20.
 *
 * La GDT complète (null / code / données / TSS) remplace celle de
 * boot.s après l'entrée en mode long. Les sélecteurs 0x08 (code) et
 * 0x10 (données) sont inchangés — les `mov ax, 0x10` existants
 * restent valides.
 * ============================================================ */

use core::arch::asm;

/// TSS 64 bits (104 octets).
///
/// Layout conforme à l'Intel SDM vol. 3, tableau 7-13 :
///
/// | offset | champ          |
/// |--------|----------------|
/// | 0      | reserved (4)   |
/// | 4      | reserved (4)   |
/// | 8      | RSP0           |
/// | 16     | RSP1           |
/// | 24     | RSP2           |
/// | 32     | reserved       |
/// | 40-47  | IST1           |
/// | 48-87  | IST2..IST7     |
/// | 96     | reserved (4)   |
/// | 100    | I/O map base   |
/// | 102    | reserved (2)   |
#[repr(C)]
struct Tss {
    _rsv1: u32,       // 0
    _rsv2: u32,       // 4
    rsp0: u64,        // 8
    _rsp1: u64,       // 16
    _rsp2: u64,       // 24
    _rsv3: u64,       // 32
    ist1: u64,        // 40
    _ist2: u64,       // 48
    _ist3: u64,       // 56
    _ist4: u64,       // 64
    _ist5: u64,       // 72
    _ist6: u64,       // 80
    _ist7: u64,       // 88
    _rsv4: u32,       // 96
    iomap_base: u16,  // 100
    _rsv5: u16,       // 102
}

/// Vérifié à la compilation : 104 octets, comme l'exige le matériel.
const _: () = assert!(core::mem::size_of::<Tss>() == 104);

/// Pile IST1 : 8 KiB dans la BSS, alignée sur 16.
///
/// Le sommet (adresse la plus haute) est chargé dans IST1 par `init`.
#[repr(align(16))]
#[allow(dead_code)]
struct IstStack {
    bytes: [u8; 8192],
}

#[link_section = ".data"]
static mut TSS: Tss = Tss {
    _rsv1: 0,
    _rsv2: 0,
    rsp0: 0,
    _rsp1: 0,
    _rsp2: 0,
    _rsv3: 0,
    ist1: 0,
    _ist2: 0,
    _ist3: 0,
    _ist4: 0,
    _ist5: 0,
    _ist6: 0,
    _ist7: 0,
    _rsv4: 0,
    iomap_base: 0xFFFF,
    _rsv5: 0,
};

#[link_section = ".data"]
static mut IST1_STACK: IstStack = IstStack { bytes: [0; 8192] };

/// --- Sélecteurs de segment (GDT) ---
///
///   [0] null      0x00
///   [1] code R0   0x08  0x00AF9A000000FFFF (L=0 — CS non rechargée après lgdt, cf. ci-dessous)
///   [2] données R0 0x10  0x00AF92000000FFFF
///   [3] TSS low   0x18  (mis à jour par init)
///   [4] TSS high  0x20  (mis à jour par init)
///   [5] code R3   0x28  0x00BF_FA00_0000_FFFF (DPL=3, L=1, G=1)
///   [6] données R3 0x30  0x00AF_F200_0000_FFFF (DPL=3)
///   [7] code R0 64 0x38  0x00BF_9A00_0000_FFFF (DPL=0, L=1, G=1)
pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
/// Code segment 64-bit (L=1) pour syscall/iretq — sélecteur 0x38.
/// Distinct de KERNEL_CS (0x08, L=0) : le noyau ne peut pas faire de
/// far-jump pour recharger CS (higher-half), donc CS reste celui du
/// bootloader. Les IRQ matériels continuent d'utiliser 0x08. Mais
/// `syscall` (R0→R3) et iretq (R3→R0) doivent utiliser un segment
/// 64-bit valide, d'où cette entrée supplémentaire [7].
#[allow(dead_code)]
pub const KERNEL_CS64: u16 = 0x38;
#[allow(dead_code)]
pub const TSS_SELECTOR: u16 = 0x18;
pub const USER_CS: u16 = 0x28;
pub const USER_DS: u16 = 0x30;

/// GDT complète du noyau — 8 entrées de 8 octets (64 octets, alignés sur 8).
///
/// Les entrées [5] et [6] sont les segments anneau 3 (code/data DPL=3),
/// utilisés par `sysret` pour revenir en mode utilisateur. Leurs sélecteurs
/// (0x28 / 0x30) sont chargés par `sysret` depuis MSR_STAR.
#[repr(C, align(8))]
struct Gdt {
    entries: [u64; 8],
}

#[link_section = ".data"]
static mut GDT: Gdt = Gdt {
    entries: [
        0x0000_0000_0000_0000, // [0] null
        0x00AF_9A00_0000_FFFF, // [1] code R0  0x08
        0x00AF_9200_0000_FFFF, // [2] données R0 0x10
        0,                     // [3] TSS low   0x18
        0,                     // [4] TSS high  0x20
        0x00BF_FA00_0000_FFFF, // [5] code R3   0x28
        0x00AF_F200_0000_FFFF, // [6] donnees R3 0x30
        0x00BF_9A00_0000_FFFF, // [7] code R0 64 0x38
    ],
};

/// GDTR (16 octets en mode long : limite 16 bits + base 64 bits).
#[repr(C, packed)]
struct Gdtr {
    limit: u16,
    base: u64,
}

/// Encode le descripteur TSS (8 octets × 2) depuis l'adresse et la limite.
///
///   low  : limite[15:0] | base[15:0] | base[23:16] | type(0x9) | S=0 | DPL=0 | P=1 | limite[19:16] | base[31:24]
///   high : base[63:32]
fn tss_descriptor(base: u64, limit: u64) -> (u64, u64) {
    let low = (limit & 0xFFFF) as u64
        | ((base & 0xFFFF) as u64) << 16
        | (((base >> 16) & 0xFF) as u64) << 32
        | (0x9u64) << 40               // type = TSS available
        | (1u64) << 47                 // présent
        | (((limit >> 16) & 0xF) as u64) << 48
        | (((base >> 24) & 0xFF) as u64) << 56;
    let high = ((base >> 32) & 0xFF) as u64;
    (low, high)
}

/// Initialise le TSS (RSP0 + IST1), écrit le descripteur TSS dans la
/// GDT, charge le GDTR, puis charge le TR (`ltr 0x18`).
///
/// # Sécurité
///
/// Appelée une seule fois, en mode long, avant `idt::init`.
/// `stack_top` est le sommet de la pile du noyau (celui de boot.s).
pub unsafe fn init(stack_top: u64) {
    // --- TSS ---
    let tss = &mut *core::ptr::addr_of_mut!(TSS);
    tss.rsp0 = stack_top;
    let ist_top = (core::ptr::addr_of!(IST1_STACK) as usize + 8192) as u64;
    tss.ist1 = ist_top;

    // --- Descripteur TSS dans la GDT ---
    let base = core::ptr::addr_of!(TSS) as u64;
    let limit = core::mem::size_of::<Tss>() as u64 - 1;
    let (low, high) = tss_descriptor(base, limit);

    (*core::ptr::addr_of_mut!(GDT)).entries[3] = low;
    (*core::ptr::addr_of_mut!(GDT)).entries[4] = high;

    // --- lgdt + ltr ---
    let gdtr = Gdtr {
        limit: (core::mem::size_of::<Gdt>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    // LGDT needs the gdtr address; le registre contenant l'adresse
    // de gdtr est lu depuis la pile. Pas de `nomem` : lgdt/ltr
    // lisent effectivement de la mémoire (la GDT via le GDTR).
    asm!(
        "lgdt [{0}]",
        in(reg) core::ptr::addr_of!(gdtr) as u64,
        options(nostack, preserves_flags)
    );
    // `ltr` prend r/m16 ; le sélecteur 0x18 → TSS (entrée 3 de la GDT).
    // `{0:x}` force le format 16 bits (ex. `cx` et non `rax`).
    asm!(
        "ltr {0:x}",
        in(reg) 0x18u16,
        options(nostack, preserves_flags)
    );
}
