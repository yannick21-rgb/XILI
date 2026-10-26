/* ============================================================
 * mbi.rs — structure de la Multiboot2 Information (MBI)
 *
 * GRUB laisse en EBX l'adresse physique d'une structure en deux
 * niveaux : un en-tête fixe, puis une suite de tags alignés sur
 * 8 octets.
 *
 *   mbi :  +0   u32  taille totale (en-tête compris)
 *         +4   u32  réservé, toujours 0
 *         +8   tag …
 *   tag :  +0   u16  type
 *         +2   u16  flags
 *         +4   u32  taille totale du tag (en-tête compris)
 *         +8   charge utile
 *
 * Référence : Multiboot2 specification, section 3.4.
 *
 * Les structures ci-dessous décrivent la disposition des données. On
 * ne les déréférence **jamais** : leurs champs ne sont pas garantis
 * alignés, et construire une référence vers un champ d'une structure
 * `#[repr(C, packed)]` est un comportement indéfini. Toute lecture
 * passe par les fonctions `load_*`, qui utilisent
 * `ptr::read_unaligned`.
 * ============================================================ */

/// En-tête de la Multiboot2 Information.
#[repr(C, packed)]
pub struct Mbi {
    /// Taille totale, en-tête compris.
    pub total_size: u32,
    /// Réservé, toujours 0.
    pub reserved: u32,
    /* Les tags suivent immédiatement, alignés sur 8 octets. */
}

/// En-tête commun de chaque tag Multiboot2.
#[repr(C, packed)]
pub struct TagHeader {
    /// Type du tag.
    pub typ: u16,
    /// Drapeaux.
    pub flags: u16,
    /// Taille totale du tag, en-tête compris.
    pub size: u32,
}

/// Tag 1 : informations de base de la mémoire.
///
/// Deux `u32` consécutifs : l'alignement est naturel, pas besoin de
/// `packed`, et les champs restent lisibles depuis l'appelant.
#[repr(C)]
pub struct MemInfo {
    /// Mémoire disponible, en Kio.
    pub mem_lower: u32,
    /// Mémoire disponible au-dessus de 1 Mio, en Kio.
    pub mem_upper: u32,
}

/// Taille que la spécification Multiboot2 donne au tag MEMORY_INFO.
///
/// GRUB 2.12 ne la respecte pas : il émet un tag de 9 octets, dont les
/// deux derniers sont du remplissage. Lire `mem_lower` et `mem_upper` à
/// `off + 8` et `off + 12` renvoie alors des octets de bourrage — d'où
/// l'affichage de valeurs absurdes (des centaines de Mio de « RAM sous
/// 1 Mio »). On n'accepte donc que la taille exacte de la spec.
pub const MEMORY_INFO_SIZE: usize = 16;

/// Tag 6 : carte mémoire détaillée.
#[repr(C, packed)]
pub struct MemMapTag {
    /// En-tête du tag.
    pub header: TagHeader,
    /// Taille d'une entrée de la carte.
    pub entry_size: u32,
    /// Version du descripteur, 0 chez GRUB.
    pub entry_version: u32,
    /* Les entrées suivent, alignées sur 8 octets. */
}

/// Descripteur d'une région de la carte mémoire.
///
/// `size` est la taille de la structure, **pas** la taille de la région :
/// le champ `pad` absorbe la différence pour que le format puisse évoluer
/// sans casser les bootloaders.
#[repr(C, packed)]
pub struct MMapEntry {
    /// Adresse physique de début de la région.
    pub base: u64,
    /// Taille de la région, en octets.
    pub len: u64,
    /// Type de la région.
    pub typ: u32,
    /// Toujours 0.
    pub pad: u32,
}

/// Types de région de la carte mémoire Multiboot2.
pub mod region {
    /// RAM disponible.
    pub const AVAILABLE: u32 = 1;
    /// Réservée : ne pas utiliser.
    pub const RESERVED: u32 = 2;
    /// Réutilisable par l'OS après avoir lu les informations ACPI.
    pub const ACPI_RECLAIMABLE: u32 = 3;
    /// À préserver pendant la mise en veille.
    pub const ACPI_NVS: u32 = 4;
    /// Défectueuse.
    pub const BAD: u32 = 5;
}

/// Types de tags Multiboot2 que le noyau lit.
pub mod tag {
    /// Fin de la liste des tags.
    pub const END: u16 = 0;
    /// Informations de base de la mémoire.
    pub const MEMORY_INFO: u16 = 1;
    /// Carte mémoire détaillée.
    pub const MEMORY_MAP: u16 = 6;
}

/* ------------------------------------------------------------
 * Le format Multiboot2 est un contrat
 *
 * Les structures ci-dessus ne sont jamais construites : elles
 * décrivent la disposition des octets et servent d'assertion de
 * taille. Si l'une d'elles change, la lecture des tags est fausse
 * silencieusement — d'où les `const _` ci-dessous, qui font échouer
 * la compilation plutôt que le boot.
 * ------------------------------------------------------------ */

use core::mem::size_of;

const _: () = assert!(size_of::<Mbi>() == 8);
const _: () = assert!(size_of::<TagHeader>() == 8);
const _: () = assert!(size_of::<MemInfo>() == 8);
const _: () = assert!(size_of::<MemMapTag>() == 16);
const _: () = assert!(size_of::<MMapEntry>() == 24);

/* ------------------------------------------------------------
 * Lectures non alignées
 * ------------------------------------------------------------ */

/// Aligne une taille sur 8 octets, sans dépendre d'une méthode récente.
pub const fn align8(n: usize) -> usize {
    (n + 7) & !7
}

/// Lit un `u16` à l'offset `off` de la MBI.
///
/// # Sécurité
///
/// `mbi` doit être l'adresse physique de la MBI et `mbi + off + 2` dans
/// la plage décrite par son en-tête.
pub unsafe fn load_u16(mbi: u32, off: usize) -> u16 {
    core::ptr::read_unaligned((mbi as *const u8).add(off) as *const u16)
}

/// Lit un `u32` à l'offset `off` de la MBI.
///
/// # Sécurité
///
/// Comme [`load_u16`].
pub unsafe fn load_u32(mbi: u32, off: usize) -> u32 {
    core::ptr::read_unaligned((mbi as *const u8).add(off) as *const u32)
}

/// Lit un `u64` à l'offset `off` de la MBI.
///
/// # Sécurité
///
/// Comme [`load_u16`].
pub unsafe fn load_u64(mbi: u32, off: usize) -> u64 {
    core::ptr::read_unaligned((mbi as *const u8).add(off) as *const u64)
}

/// Taille totale de la MBI, en octets.
///
/// # Sécurité
///
/// `mbi` doit être l'adresse physique de la MBI.
pub unsafe fn total_size(mbi: u32) -> u32 {
    load_u32(mbi, 0)
}

/// Type et taille d'un tag situé à `off`.
///
/// `(0, 8)` marque la fin de la liste.
///
/// # Sécurité
///
/// Comme [`load_u16`].
pub unsafe fn tag_at(mbi: u32, off: usize) -> (u16, usize) {
    let typ = load_u16(mbi, off);
    let size = load_u32(mbi, off + 4) as usize;
    (typ, size)
}

/// Avance à l'offset du tag suivant.
pub fn next_tag(off: usize, size: usize) -> usize {
    off + align8(size).max(8)
}

/// Itère sur les tags de la MBI.
///
/// `f` reçoit `(type, offset, taille)`. Le parcours s'arrête au tag de fin
/// ou dès que l'en-tête du tag suivant dépasserait la structure.
///
/// # Sécurité
///
/// `mbi` doit être l'adresse physique de la MBI.
pub unsafe fn for_each_tag<F: FnMut(u16, usize, usize)>(mbi: u32, mut f: F) {
    let end = total_size(mbi) as usize;
    let mut off = 8usize; // l'en-tête de la MBI fait 8 octets

    while off + 8 <= end {
        let (typ, size) = tag_at(mbi, off);

        if typ == tag::END && size == 8 {
            break;
        }

        f(typ, off, size);
        off = next_tag(off, size);
    }
}

impl MemInfo {
    /// Lit le tag 1 (MEMORY_INFO) de la MBI.
    ///
    /// Retourne `None` si le tag est absent **ou** si sa taille n'est pas
    /// celle de la spécification : un tag tronqué ne contient pas les deux
    /// `u32`, et les lire au-delà renverrait du remplissage comme si c'était
    /// de la mémoire. Le tag 6 (carte mémoire) porte la même information de
    /// façon plus détaillée et est celui qu'on exploite réellement.
    ///
    /// # Sécurité
    ///
    /// `mbi` doit être l'adresse physique de la MBI.
    pub unsafe fn read(mbi: u32) -> Option<Self> {
        let mut found: Option<Self> = None;

        for_each_tag(mbi, |typ, off, size| {
            if typ == tag::MEMORY_INFO && size == MEMORY_INFO_SIZE {
                found = Some(MemInfo {
                    mem_lower: load_u32(mbi, off + 8),
                    mem_upper: load_u32(mbi, off + 12),
                });
            }
        });

        found
    }
}
