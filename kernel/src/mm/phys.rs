/* ============================================================
 * phys.rs — allocateur physique par bitmap (J2.4)
 *
 * Un bit par page de 4 Kio — 0 = libre, 1 = utilisée. C'est le
 * premier allocateur de frames physiques ; le buddy viendra le
 * remplacer plus tard pour réduire la fragmentation (ARCHITECTURE.md
 * §6 : « bitmap puis buddy »).
 *
 * Le bitmap couvre [0, MAX_PHYS) — un plafond fixé en compilation assez
 * pour la RAM visible sur la cible (QEMU -m 512M et l'E820 BIOS). Tout
 * ce qui est au-dessus (régions UEFI > 4 GiB) est traité comme utilisé
 * et jamais rendu.
 *
 * `init` marque d'abord TOUTES les pages utilisées, libère ensuite
 * uniquement les régions AVAILABLE de la carte Multiboot2, puis réserve
 * les pages qui appartiennent au noyau (bootstrap + image + BSS — qui
 * contient déjà le bitmap, les tables de pages et la MemMap) et la MBI
 * elle-même. Rien de ce qui est au noyau n'est donc jamais rendu.
 *
 * Pas d'allocation dans les IRQ (AI-WORKFLOW §6) : l'allocateur est
 * utilisé en contexte noyau sédentaire, avant l'activation des
 * interruptions. Un verrou par CPU sera ajouté quand les IRQ arriveront.
 * ============================================================ */

use crate::mbi;
use crate::mm::paging::HIGHER_HALF;

/// Taille d'une page.
const PAGE_SIZE: usize = 4096;

/// Adresse physique plafond du bitmap : 4 GiB = 2^20 pages.
const MAX_PHYS: u64 = 4 * 1024 * 1024 * 1024;

/// Nombre de pages gérées.
const MAX_PAGES: usize = (MAX_PHYS / PAGE_SIZE as u64) as usize;

/// Nombre de mots de 64 bits (64 pages par mot).
const WORDS: usize = MAX_PAGES / 64;

// Symbole posé par le linker : fin du noyau (image + BSS), virtuel en
// higher-half. Sa valeur physique (_kend - HIGHER_HALF) donne la borne
// supérieure des pages à réserver au noyau.
extern "C" {
    static _kend: u8;
}

/// Un bit par page. 0 = libre, 1 = utilisée.
///
/// Accédé par `addr_of_mut!` : construire une référence sur un `static mut`
/// est un avertissement (demain une erreur), l'usage du pointeur l'évite
/// sans rien céder sur la sûreté — tout reste dans un bloc `unsafe`.
static mut BITMAP: [u64; WORDS] = [0; WORDS];

/// Comptes de diagnostic : pages libres / utilisées.
static mut FREE: u32 = 0;
static mut USED: u32 = 0;

/// Mot de départ pour la recherche du prochain bloc libre (optimisation
/// amortie : on ne repart pas forcément de 0).
static mut NEXT_WORD: usize = 0;

/// Page du noyau : le bootstrap + l'image sont chargés par GRUB à 1 MiB.
const KERNEL_PA: u64 = 0x0010_0000;

/// Indice de mot contenant la page `page`.
const fn word_of(page: usize) -> usize {
    page / 64
}

/// Position du bit dans son mot.
const fn bit_of(page: usize) -> usize {
    page % 64
}

/// Numéro de page d'une adresse physique.
const fn page_of(paddr: u64) -> usize {
    (paddr / PAGE_SIZE as u64) as usize
}

/// Masque du bit `page` dans son mot.
const fn mask_of(page: usize) -> u64 {
    1u64 << bit_of(page)
}

/// Marque les pages de [paddr, paddr + len) comme utilisées.
///
/// Idempotente : ne double-pas les comptes — une page déjà utilisée reste
/// utilisée.
///
/// # Sécurité
///
/// `paddr`+`len` peut dépasser `MAX_PHYS` : tout ce qui est au-dessus du
/// plafond est ignoré (déjà traité comme utilisé par l'initialisation).
unsafe fn reserve(paddr: u64, len: u64) {
    if len == 0 {
        return;
    }
    let first = page_of(paddr).min(MAX_PAGES);
    let last = page_of(paddr + len - 1).min(MAX_PAGES - 1);
    if first > last {
        return;
    }
    let bits = &mut *core::ptr::addr_of_mut!(BITMAP);
    for page in first..=last {
        let w = word_of(page);
        let m = mask_of(page);
        if bits[w] & m == 0 {
            bits[w] |= m;
            USED += 1;
            FREE = FREE.wrapping_sub(1);
        }
    }
}

/// Libère les pages de [paddr, paddr + len) : les marque libres.
///
/// Idempotente : ne libère qu'une page réellement utilisée.
///
/// # Sécurité
///
/// Même bornage que [`reserve`].
unsafe fn release(paddr: u64, len: u64) {
    if len == 0 {
        return;
    }
    let first = page_of(paddr).min(MAX_PAGES);
    let last = page_of(paddr + len - 1).min(MAX_PAGES - 1);
    if first > last {
        return;
    }
    let bits = &mut *core::ptr::addr_of_mut!(BITMAP);
    for page in first..=last {
        let w = word_of(page);
        let m = mask_of(page);
        if bits[w] & m != 0 {
            bits[w] &= !m;
            FREE += 1;
            USED = USED.wrapping_sub(1);
            if w < NEXT_WORD {
                NEXT_WORD = w;
            }
        }
    }
}

/// Construit le bitmap à partir de la carte Multiboot2.
///
/// # Sécurité
///
/// `mbi` doit être l'adresse physique de la MBI, fournie par GRUB dans EBX.
/// Elle reste valide tant que le noyau n'a pas réalloué les pages qu'elle
/// décrit ; ici on ne fait que lire.
pub unsafe fn init(mbi: u32) {
    // 1. Tout marqué utilisé.
    let bits = &mut *core::ptr::addr_of_mut!(BITMAP);
    for w in bits.iter_mut() {
        *w = u64::MAX;
    }
    USED = MAX_PAGES as u32;
    FREE = 0;
    NEXT_WORD = 0;

    // 2. Libérer les régions AVAILABLE de la carte — tout le reste
    //    (RESERVED, ACPI_NVS, BAD, > 4 GiB) reste utilisé.
    mbi::for_each_tag(mbi, |typ, off, size| {
        if typ != mbi::tag::MEMORY_MAP || size < 16 {
            return;
        }
        let entry_size = unsafe { mbi::load_u32(mbi, off + 8) } as usize;
        if entry_size < core::mem::size_of::<mbi::MMapEntry>() {
            return;
        }
        let last = off + size;
        let mut at = off + 16;
        while at + entry_size <= last {
            let base = unsafe { mbi::load_u64(mbi, at) };
            let len = unsafe { mbi::load_u64(mbi, at + 8) };
            let typ2 = unsafe { mbi::load_u32(mbi, at + 16) };
            if typ2 == mbi::region::AVAILABLE {
                // #Sécurité : lecture d'une région décrite par la MBI.
                unsafe { release(base, len) }
            }
            at += entry_size;
        }
    });

    // 3. Réserver la mémoire du noyau : 1 MiB de bas (IVT, BDA, VGA…)
    //    puis le bloc image + BSS. Le BSS contient le bitmap et les tables
    //    de pages, donc un seul trait couvre tout ce qui appartient au noyau.
    let kend = core::ptr::addr_of!(_kend) as u64 - HIGHER_HALF;
    reserve(0, KERNEL_PA);                       // 1 Mio de bas : jamais alloué
    reserve(KERNEL_PA, kend - KERNEL_PA);       // image du noyau (bootstrap+BSS)

    // 4. La MBI peut se trouver en dehors de l'image : la réserver évite de
    //    la libérer — et de l'écraser — à un appel d'allocation.
    reserve(mbi as u64, unsafe { mbi::total_size(mbi) } as u64);

    // Invariant : USED + FREE == MAX_PAGES — chaque page est soit libre,
    // soit utilisée.
}

/// Alloue une page, renvoie son adresse physique.
///
/// Recherche circulaire depuis `NEXT_WORD` : le bitmap libre apparaît vite
/// sans repartir de zéro à chaque appel — amorti O(1) sur une zone contiguë.
pub fn alloc_page() -> Option<usize> {
    unsafe {
        let bits = &mut *core::ptr::addr_of_mut!(BITMAP);
        let mut w = NEXT_WORD;
        for _ in 0..WORDS {
            if bits[w] != u64::MAX {
                // Bit libre = 0. Inverser pour que `trailing_zeros`
                // renvoie la position du plus bas bit libre (sinon on
                // tournerait autour du plus bas bit utilisé).
                let b = (!bits[w]).trailing_zeros() as usize;
                let page = w * 64 + b;
                bits[w] |= 1u64 << b;
                if bits[w] == u64::MAX {
                    NEXT_WORD = (w + 1) % WORDS; // le mot est épuelé
                } else {
                    NEXT_WORD = w;               // peut en contenir d'autres
                }
                FREE = FREE.wrapping_sub(1);
                USED = USED.wrapping_add(1);
                return Some(page * PAGE_SIZE);
            }
            w = (w + 1) % WORDS;
        }
        None
    }
}

/// Libère une page allouée par [`alloc_page`].
///
/// Idempotente : libérer deux fois la même page ne change rien.
pub fn free_page(paddr: usize) {
    unsafe {
        let page = page_of(paddr as u64);
        if page >= MAX_PAGES {
            return;
        }
        let w = word_of(page);
        let m = mask_of(page);
        let bits = &mut *core::ptr::addr_of_mut!(BITMAP);
        if bits[w] & m != 0 {
            bits[w] &= !m;
            FREE = FREE.wrapping_add(1);
            USED = USED.wrapping_sub(1);
            if w < NEXT_WORD {
                NEXT_WORD = w;
            }
        }
    }
}

/// Nombre de pages libres.
pub fn free_pages() -> u32 {
    unsafe { FREE }
}

/// Nombre de pages utilisées (noyau + réservées).
pub fn used_pages() -> u32 {
    unsafe { USED }
}

/// Nombre total de pages gérées par le bitmap.
pub fn total_pages() -> u32 {
    MAX_PAGES as u32
}

/// J2.4 — test de l'allocateur physique bitmap.
///
/// Alloue 100 Mio de pages (25600 x 4 Kio), puis les libère, en vérifiant
/// que le compteur de pages libres revient exactement à la valeur
/// initiale. Un réservoir circulaire de 256 entrées (2 Kio) contient les
/// adresses en cours — assez pour ne pas déborder la pile du noyau
/// (16 Kio), assez petit pour forcer des libérations intermédiaires et
/// exercer le bitmap boucle sur boucle.
///
/// # Sécurité
///
/// Requiert que [`init`] ait été appelée au préalable (bitmap construit).
pub unsafe fn test() -> bool {
    const MOIB: usize = 100 * 1024 * 1024;
    const NPAGES: usize = MOIB / PAGE_SIZE;
    const POOL: usize = 256;

    let mut cache: [usize; POOL] = [0; POOL];
    let mut head: usize = 0;
    let mut got: usize = 0;

    let before = free_pages();

    loop {
        match alloc_page() {
            Some(paddr) => {
                cache[head] = paddr;
                head += 1;
                got += 1;
                if got >= NPAGES {
                    break;
                }
                if head == POOL {
                    for p in cache.iter() {
                        free_page(*p);
                    }
                    head = 0;
                }
            }
            None => break,
        }
    }
    for p in cache.iter().take(head) {
        free_page(*p);
    }

    got == NPAGES && free_pages() == before
}
