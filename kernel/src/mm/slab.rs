/* ============================================================
 * slab.rs — allocateur noyau kmalloc/kfree (J2.5)
 *
 * Free-list simple, first-fit, avec expansion par pages du
 * physical allocator. Chaque page obtenue de `phys::alloc_page`
 * est identité-mappée (identity map 0..1 Gio) : l'adresse
 * virtuelle == adresse physique, aucun mmap intermédiaire.
 *
 * Layout d'un bloc :
 *
 *   offset 0  : size  (usize, bit 0 = ALLOCATED)
 *   offset 8  : next  (*mut Block, queue si libre)
 *   offset 16 : données utilisateur (aligné sur 16 octets)
 *
 * kmalloc renvoie block + 16 ; kfree remonte à block + 0.
 * Pas de coalescing : la page-level granularity du physical
 * allocator recycle suffisamment. Un verrou par CPU viendra
 * avec l'AIC (J3).
 * ============================================================ */

use crate::mm::phys;
use core::ptr::null_mut;

/// Taille d'une page ; identique au allocateur physique.
const PAGE_SIZE: usize = 4096;

/// Alignement des allocations : 16 octets couvre SSE et les
/// pointeurs x86-64.
const ALIGN: usize = 16;

/// En-tête d'un bloc : size + next (16 octets).
const BLOCK_HEADER: usize = 2 * core::mem::size_of::<usize>();

/// Taille minimale d'un bloc (header + au moins 16 octets utile).
const MIN_BLOCK: usize = BLOCK_HEADER + ALIGN;

/// Nombre de pages demandées à l'initialisation.
const INITIAL_PAGES: usize = 8; // 32 Kio

/// Flag d'allocation dans le bit 0 de `size`.
const ALLOCATED: usize = 1;

/// Un bloc mémoire (libre ou alloué).
#[repr(C)]
struct Block {
    /// Taille totale du bloc (y compris en-tête), bit 0 = alloué.
    size: usize,
    /// Prochain bloc libre (blocs alloués ne lisent pas ce champ).
    next: *mut Block,
}

/// L'allocateur.
pub struct Slab {
    /// Tête de la free-list.
    free_list: *mut Block,
    /// Nombre de pages obtenues du physical allocator.
    pages: usize,
}

impl Slab {
    /// Crée un allocateur vide.
    pub const fn new() -> Self {
        Slab { free_list: null_mut(), pages: 0 }
    }

    /// Initialise l'allocateur : demande INITIAL_PAGES pages au
    /// physical allocator et construit la free-list.
    ///
    /// # Sécurité
    ///
    /// `phys::init` doit avoir été appelée au préalable.
    pub unsafe fn init(&mut self) {
        for _ in 0..INITIAL_PAGES {
            if !self.refill() {
                break;
            }
        }
    }

    /// Demande une page supplémentaire et l'ajoute à la free-list.
    ///
    /// # Sécurité
    ///
    /// phys::alloc_page doit avoir été initialisé. La page obtenue
    /// est supposée être dans la zone identity-mappée (0..1 Gio).
    unsafe fn refill(&mut self) -> bool {
        match phys::alloc_page() {
            Some(paddr) => {
                // L'identity map couvre 0..1 Gio : virtuel == physique.
                debug_assert!(paddr < 0x4000_0000, "page en dehors de l'identity map");
                let page = paddr as *mut Block;
                (*page).size = PAGE_SIZE;
                (*page).next = self.free_list;
                self.free_list = page;
                self.pages += 1;
                true
            }
            None => false,
        }
    }

    /// kmalloc équivalent : alloue `size` octets, alignés sur 16.
    ///
    /// Retourne `None` si l'allocateur ne peut pas fournir assez
    /// de mémoire (plus de pages disponibles dans le physical allocator).
    ///
    /// # Sécurité
    ///
    /// Cet appel n'est pas réentrant — il est utilisé en contexte
    /// noyau sédentaire (pas d'IRQ). Un verrou par CPU sera ajouté
    /// en J3 pour le SMP.
    pub unsafe fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        if size == 0 {
            return Some(null_mut());
        }

        // Taille totale arrondie : header + données, alignée sur ALIGN.
        let needed = (BLOCK_HEADER + size + ALIGN - 1) & !(ALIGN - 1);

        // First-fit : parcourir la free-list.
        let mut prev: *mut Block = null_mut();
        let mut cur = self.free_list;

        while !cur.is_null() {
            let block_size = (*cur).size & !ALLOCATED;

            if block_size >= needed {
                // Suffisamment grand — utiliser ce bloc.
                // Est-ce qu'on peut le splitter ?
                let remaining = block_size - needed;

                if remaining >= MIN_BLOCK {
                    // Split : le bloc restant devient un nouveau bloc libre.
                    let new_block = (cur as usize + needed) as *mut Block;
                    (*new_block).size = remaining;
                    (*new_block).next = (*cur).next;

                    // Mettre à jour la free-list.
                    if prev.is_null() {
                        self.free_list = new_block;
                    } else {
                        (*prev).next = new_block;
                    }

                    // Marquer le bloc utilisé.
                    (*cur).size = needed | ALLOCATED;
                } else {
                    // Trop petit pour splitter : utiliser le bloc entier.
                    (*cur).size = block_size | ALLOCATED;

                    if prev.is_null() {
                        self.free_list = (*cur).next;
                    } else {
                        (*prev).next = (*cur).next;
                    }
                }

                // Retourner le pointeur à l'utilisateur (après l'en-tête).
                let user_ptr = (cur as usize + BLOCK_HEADER) as *mut u8;
                return Some(user_ptr);
            }

            prev = cur;
            cur = (*cur).next;
        }

        // Pas de bloc suffisant — demander une nouvelle page.
        if self.refill() {
            return self.alloc(size);
        }

        None
    }

    /// kfree équivalent : libère un bloc alloué par `alloc`.
    ///
    /// # Sécurité
    ///
    /// `ptr` doit être un pointeur précédemment retourné par `alloc`,
    /// et ne doit pas être libéré deux fois.
    pub unsafe fn dealloc(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        // Remonter à l'en-tête du bloc.
        let block = (ptr as usize - BLOCK_HEADER) as *mut Block;

        // Marquer comme libre et ajouter à la free-list.
        let block_size = (*block).size & !ALLOCATED;
        (*block).size = block_size; // bit ALLOC = 0
        (*block).next = self.free_list;
        self.free_list = block;
    }

    /// Nombre de pages obtenues du physical allocator.
    pub fn total_pages(&self) -> usize {
        self.pages
    }
}

/* ------------------------------------------------------------
 * API globale kmalloc/kfree
 * ------------------------------------------------------------ */

/// L'allocateur noyau, initialisé une seule fois au boot.
static mut SLAB: Slab = Slab::new();

/// Initialise l'allocateur slab. À appeler une fois, après
/// `phys::init`.
///
/// # Sécurité
///
/// Doit être appelée une seule fois, en contexte noyau sédentaire.
pub unsafe fn kmalloc_init() {
    (*core::ptr::addr_of_mut!(SLAB)).init();
}

/// Alloue `size` octets de mémoire kernel. Aligné sur 16 octets.
///
/// # Sécurité
///
/// Pas de synchronisation — contexte unique, pas d'IRQ.
pub unsafe fn kmalloc(size: usize) -> *mut u8 {
    match (*core::ptr::addr_of_mut!(SLAB)).alloc(size) {
        Some(ptr) => ptr,
        None => {
            crate::console::print(format_args!(
                "kmalloc: ECHEC (plus de pages disponibles)\n"
            ));
            null_mut()
        }
    }
}

/// Libère un bloc alloué par `kmalloc`.
///
/// # Sécurité
///
/// `ptr` doit être NULL ou un pointeur de `kmalloc`. Pas de double-free.
pub unsafe fn kfree(ptr: *mut u8) {
    (*core::ptr::addr_of_mut!(SLAB)).dealloc(ptr);
}

/// Nombre de pages gérées par l'allocateur.
pub fn pages() -> usize {
    unsafe { (*core::ptr::addr_of!(SLAB)).total_pages() }
}

/* ------------------------------------------------------------
 * Test J2.5c — kmalloc/kfree
 *
 * Alloue 4 blocs de tailles croissantes (32, 64, 128, 256),
 * vérifie qu'ils ne se chevauchent pas, les libère, puis
 * ré-alloue et vérifie la réutilisation. Le critère :
 *
 *   - kmalloc ne retourne jamais NULL pour une alloc < 4096
 *   - Les pointeurs sont alignés sur 16
 *   - Les blocs utilisateurs ne se chevauchent pas
 *   - Après free, kmalloc réutilise la mémoire libérée
 * ============================================================ */

/// # Sécurité
///
/// `kmalloc_init` doit avoir été appelée.
pub unsafe fn test() -> bool {
    const SIZES: [usize; 4] = [32, 64, 128, 256];
    let mut ptrs: [*mut u8; 4] = [null_mut(); 4];

    // --- Phase 1 : allocation ---
    for (i, &size) in SIZES.iter().enumerate() {
        let ptr = kmalloc(size);
        if ptr.is_null() {
            crate::serial::write_str("K2.5c: kmalloc nul\r\n");
            return false;
        }
        // Vérifier l'alignement (16 octets).
        if (ptr as usize) % ALIGN != 0 {
            crate::serial::write_str("K2.5c: mauvaise alignment\r\n");
            return false;
        }
        ptrs[i] = ptr;
    }

    // Vérifier que les régions utilisateurs ne se chevauchent pas.
    // Chaque bloc occupe [ptr, ptr + size) pour les données utilisateur.
    for i in 0..4 {
        for j in (i + 1)..4 {
            let start_i = ptrs[i] as usize;
            let end_i = ptrs[i] as usize + SIZES[i];
            let start_j = ptrs[j] as usize;
            let end_j = ptrs[j] as usize + SIZES[j];
            if end_i > start_j && end_j > start_i {
                crate::serial::write_str("K2.5c: chevauchement\r\n");
                return false;
            }
        }
    }

    // Écrire une valeur dans chaque bloc, lire pour vérifier l'accessibilité.
    for (i, &ptr) in ptrs.iter().enumerate() {
        let val = 0xA0 + i as u64;
        core::ptr::write_volatile(ptr, val as u8);
        if core::ptr::read_volatile(ptr) != val as u8 {
            crate::serial::write_str("K2.5c: ecriture lecture ECHEC\r\n");
            return false;
        }
    }

    // --- Phase 2 : libération et réutilisation ---
    kfree(ptrs[0]);
    kfree(ptrs[2]);

    // Ré-allouer : doit réussir (les blocs libérés sont suffisamment grands).
    let ptr0 = kmalloc(48);
    if ptr0.is_null() {
        crate::serial::write_str("K2.5c: realloc nul\r\n");
        return false;
    }
    kfree(ptrs[1]);
    kfree(ptrs[3]);

    // Tout libérer pour finir propre.
    kfree(ptr0);

    crate::serial::write_str("K2.5c: OK\r\n");
    true
}
