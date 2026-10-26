/* ============================================================
 * mm — gestion de la mémoire
 *
 * J2.2 : lecture et affichage de la carte mémoire.
 *
 * On se contente de constater ce que le bootloader a relevé, sans
 * encore allouer ni modifier la pagination : l'allocateur physique
 * (bitmap, puis buddy) arrive en J2.4 et le paging higher-half en J2.3.
 * ============================================================ */

use crate::console;
use crate::mbi::{self, region, tag};
use core::mem::size_of;

pub mod paging;

/// Index des tranches dans [`MemMap::summary`].
pub const IDX_AVAILABLE: usize = 0;
pub const IDX_RESERVED: usize = 1;
pub const IDX_OTHER: usize = 2;

/// Nombre de tranches conservées.
const SUMMARY_LEN: usize = 3;

/// Tranche de la carte mémoire à laquelle appartient un type de région.
fn region_index(typ: u32) -> usize {
    match typ {
        region::AVAILABLE => IDX_AVAILABLE,
        region::RESERVED => IDX_RESERVED,
        _ => IDX_OTHER,
    }
}

/// Nom lisible d'un type de région.
fn region_label(typ: u32) -> &'static str {
    match typ {
        region::AVAILABLE => "disponible",
        region::RESERVED => "reserve",
        region::ACPI_RECLAIMABLE => "acpi-recl",
        region::ACPI_NVS => "acpi-nvs",
        region::BAD => "defectueux",
        _ => "autre",
    }
}

/// Carte mémoire telle que relevée par le bootloader.
///
/// On conserve les agrégats et la liste des régions ; la MBI elle-même
/// reste lisible, inutile d'en faire une copie.
pub struct MemMap {
    /// Nombre de régions par tranche : disponible, réservée, autre.
    summary: [u32; SUMMARY_LEN],
    /// Nombre total de descripteurs lus.
    count: u32,
    /// Somme des régions disponibles, en Mio.
    available_mib: u64,
    /// Somme des régions réservées, en Mio.
    reserved_mib: u64,
    /// Somme des autres régions, en Mio.
    other_mib: u64,
    /// Adresse de fin de la dernière région disponible, 0 si aucune.
    highest_available: u64,
}

impl MemMap {
    /// Parcourt la MBI et construit la carte.
    ///
    /// # Sécurité
    ///
    /// `mbi_addr` doit être l'adresse physique fournie par GRUB. Elle reste
    /// valide tant que le noyau n'a pas réalloué les pages qu'elle décrit ;
    /// ici on ne fait que lire.
    pub unsafe fn scan(mbi_addr: u32) -> Self {
        let mut map = MemMap {
            summary: [0; SUMMARY_LEN],
            count: 0,
            available_mib: 0,
            reserved_mib: 0,
            other_mib: 0,
            highest_available: 0,
        };

        mbi::for_each_tag(mbi_addr, |typ, off, size| {
            if typ != tag::MEMORY_MAP || size < 16 {
                return;
            }

            // entry_size est juste après l'en-tête du tag (8 octets).
            let entry_size = mbi::load_u32(mbi_addr, off + 8) as usize;

            if entry_size < size_of::<mbi::MMapEntry>() {
                return;
            }

            let first = off + 16; // en-tête du tag + entry_size + version
            let last = off + size;

            let mut at = first;
            while at + entry_size <= last {
                map.push(at, mbi_addr);
                at += entry_size;
            }
        });

        map
    }

    /// Enregistre un descripteur situé à l'offset `at` de la MBI.
    ///
    /// # Sécurité
    ///
    /// `at` doit désigner un descripteur complet dans la MBI.
    unsafe fn push(&mut self, at: usize, mbi_addr: u32) {
        let base = mbi::load_u64(mbi_addr, at);
        let len = mbi::load_u64(mbi_addr, at + 8);
        let typ = mbi::load_u32(mbi_addr, at + 16);

        self.count = self.count.saturating_add(1);
        let idx = region_index(typ);
        self.summary[idx] = self.summary[idx].saturating_add(1);

        let mib = len >> 20; // 1 Mio = 2^20 octets
        match typ {
            region::AVAILABLE => {
                self.available_mib += mib;
                self.highest_available = self.highest_available.max(base + len);
            }
            region::RESERVED => self.reserved_mib += mib,
            _ => self.other_mib += mib,
        }
    }

    /// Nombre de descripteurs lus.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Somme des régions disponibles, en Mio.
    pub fn available_mib(&self) -> u64 {
        self.available_mib
    }

    /// Adresse de fin de la dernière région disponible, 0 si aucune.
    pub fn highest_available(&self) -> u64 {
        self.highest_available
    }
}

/// Affiche la synthèse de la carte mémoire.
pub fn report(map: &MemMap) {
    console::print(format_args!("carte memoire : {} region(s)\n", map.count()));
    console::print(format_args!(
        "  disponible : {:>3} region(s)  {:>6} Mio\n",
        map.summary[IDX_AVAILABLE], map.available_mib
    ));
    console::print(format_args!(
        "  reserve    : {:>3} region(s)  {:>6} Mio\n",
        map.summary[IDX_RESERVED], map.reserved_mib
    ));
    console::print(format_args!(
        "  autre      : {:>3} region(s)  {:>6} Mio\n",
        map.summary[IDX_OTHER], map.other_mib
    ));
    console::print(format_args!(
        "  RAM disponible : {} Mio, jusqu'a {:#x}\n",
        map.available_mib(),
        map.highest_available()
    ));
}

/// Taille lisible d'une région, en octets, Kio ou Mio selon l'échelle.
///
/// Une région de 640 Kio ne doit pas s'afficher « 0 Mio » : on
/// descend d'échelle tant que la valeur serait nulle.
fn human_size(bytes: u64) -> (u64, &'static str) {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;

    if bytes >= GIB {
        (bytes / GIB, "Gio")
    } else if bytes >= MIB {
        (bytes / MIB, "Mio")
    } else if bytes >= KIB {
        (bytes / KIB, "Kio")
    } else {
        (bytes, "o")
    }
}

/// Affiche le détail de chaque région.
///
/// # Sécurité
///
/// Comme [`MemMap::scan`].
pub unsafe fn report_entries(mbi_addr: u32) {
    mbi::for_each_tag(mbi_addr, |typ, off, size| {
        if typ != tag::MEMORY_MAP || size < 16 {
            return;
        }

        let entry_size = mbi::load_u32(mbi_addr, off + 8) as usize;
        if entry_size < size_of::<mbi::MMapEntry>() {
            return;
        }

        let mut at = off + 16;
        let last = off + size;

        while at + entry_size <= last {
            let base = mbi::load_u64(mbi_addr, at);
            let len = mbi::load_u64(mbi_addr, at + 8);
            let typ = mbi::load_u32(mbi_addr, at + 16);
            let (size, unit) = human_size(len);

            console::print(format_args!(
                "  {:#018x} {:<11} {:>8} {}\n",
                base,
                region_label(typ),
                size,
                unit
            ));

            at += entry_size;
        }
    });
}

/// Affiche la carte mémoire complète : synthèse puis détail.
///
/// # Sécurité
///
/// Comme [`MemMap::scan`].
pub unsafe fn report_all(mbi_addr: u32) {
    let map = MemMap::scan(mbi_addr);
    report(&map);

    if map.summary[IDX_AVAILABLE] > 0 {
        console::print(format_args!("\nregions :\n"));
        report_entries(mbi_addr);
    }
}

/// Somme de contrôle de la MBI, pour détecter une corruption.
///
/// Ce n'est pas un vrai contrôle : on additionne les mots de 32 bits. Une
/// valeur identique d'un boot à l'autre sur la même machine est le signe
/// que la structure est intacte et que le bootloader a bien rempli les tags.
///
/// # Sécurité
///
/// `mbi_addr` doit être l'adresse physique de la MBI.
pub unsafe fn checksum(mbi_addr: u32) -> u32 {
    let end = mbi::total_size(mbi_addr) as usize;

    let mut acc: u32 = 0;
    let mut off = 0usize;

    while off + 4 <= end {
        acc = acc.wrapping_add(mbi::load_u32(mbi_addr, off));
        off += 4;
    }

    acc
}
