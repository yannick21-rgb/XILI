/* ============================================================
 * paging.rs — tables de pages 4 niveaux (x86-64)
 *
 * J2.3 : on remplace les tables construites en assembleur dans
 * `boot.s` par des tables bâties en Rust, qui couvrent deux
 * espaces en même temps :
 *
 *   - l'identity map   0 .. 1 Gio
 *   - le higher-half   0xFFFFFFFF80000000 .. +1 Gio
 *
 * L'identity map est conservé volontairement : le noyau n'est pas
 * encore relié en higher-half, son code et sa pile vivent à 1 Mio.
 * Tant qu'il tourne là, il faut que 1 Mio reste accessible. On le
 * supprimera quand le noyau saura sauter en higher-half.
 *
 * Une page de 4 Kio est mappée à part, dans un espace qui n'est
 * couvert par aucun grand descripteur, pour prouver que le parcours
 * à 4 niveaux fonctionne aussi en 4 Kio — pas seulement en 2 Mio.
 *
 * Adressage x86-64 :
 *
 *   virtuel 63..48   extension du bit 47 (canonical)
 *   virtuel 47..39   index PML4      (9 bits)
 *   virtuel 38..30   index PDPT      (9 bits)
 *   virtuel 29..21   index PD        (9 bits)
 *   virtuel 20..12   index PT        (9 bits)
 *   virtuel 11..0    décalage dans la page
 * ============================================================ */

use core::sync::atomic::{fence, Ordering};

use crate::console;

/// Taille d'une page ordinaire.
pub const PAGE_SIZE: usize = 4096;
/// Taille d'une grande page (descripteur PD avec le bit Huge).
pub const PAGE_2M: usize = 2 * 1024 * 1024;

/// Début de l'espace higher-half.
///
/// 0xFFFFFFFF80000000 est la base « moitié haute » classique : 256
/// entrées de la PML4 pointent vers le noyau, les 256 autres vers
/// l'espace utilisateur.
pub const HIGHER_HALF: u64 = 0xFFFF_FFFF_8000_0000;

/// Adresse virtuelle du descripteur de test 4 Kio.
///
/// Choisie dans un slot de PML4 inutilisé pour ne recouvrir ni
/// l'identity map (index 0) ni le higher-half (index 511) : un
/// descripteur posé au mauvais index écrase le précédent, et le
/// symptôme — une translation qui renvoie « absent » — ne dit rien
/// de la cause.
pub const SCRATCH_VADDR: u64 = 0x0000_0080_0000_0000;

/* ------------------------------------------------------------
 * Bits d'un descripteur
 *
 * Seuls les bits réellement posés sont définis ici. Les autres
 * (accessible utilisateur, politique de cache, marqueurs posés par
 * le matériel) viendront avec les jalons qui en ont besoin : un
 * bit non utilisé ne doit pas survivre dans le code.
 * ------------------------------------------------------------ */

/// Descripteur présent.
const PRESENT: u64 = 1 << 0;
/// Écriture autorisée.
const WRITABLE: u64 = 1 << 1;
/// Grande page : le descripteur PD pointe directement la page.
const HUGE: u64 = 1 << 7;
/// Page globale, non invalidée par un changement de CR3.
const GLOBAL: u64 = 1 << 8;
/// Exécution interdite.
const NOEXEC: u64 = 1 << 63;

/// Masque des bits d'adresse physique (12..51).
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
/// Masque des bits que [`Entry::new`] conserve.
const FLAG_MASK: u64 = 0x8000_0000_0000_01FF;

/// Nombre de descripteurs par table.
pub const ENTRIES: usize = 512;

/// Drapeaux d'une page du noyau : présente, inscriptible, globale.
///
/// Globale : le descripteur survit à un changement de CR3. Les pages
/// du noyau restent mappées en permanence, inutile de les recharger
/// dans le TLB à chaque bascule.
///
/// Valable pour une page mappée — grande page incluse. **Pas** pour un
/// descripteur qui pointe vers une table : voir [`LINK`].
const KERNEL_PAGE: u64 = PRESENT | WRITABLE | GLOBAL;

/// Drapeaux d'un descripteur de table (PML4E, PDPTE, PDE vers une PT).
///
/// Le bit Global est volontairement absent : il n'a de sens qu'au
/// dernier niveau du parcours. Sur un descripteur de table, les bits
/// 8 à 11 sont réservés à zéro — les poser déclenche une faute de page
/// « reserved bit violation » pendant le parcours, signalée par le bit
/// RSV du code d'erreur (0x0a : absent, écriture, RSV).
const LINK: u64 = PRESENT | WRITABLE;

/// Drapeaux d'une page de données : en plus, exécution interdite.
///
/// Le noyau ne s'exécute jamais depuis ses pages de données ; retirer
/// le droit d'exécution y enlève un moyen d'exploitation.
const DATA_PAGE: u64 = PRESENT | WRITABLE | GLOBAL | NOEXEC;

/* ------------------------------------------------------------
 * Descripteur
 * ------------------------------------------------------------ */

/// Une entrée de table de pages.
///
/// Les bits sont manipulés un par un plutôt qu'avec un champ de
/// bits : la disposition est celle du matériel, et un `struct` avec
/// des `bool` ne la reproduirait pas exactement.
#[derive(Clone, Copy)]
pub struct Entry(u64);

impl Entry {
    /// Descripteur vide.
    pub const fn empty() -> Self {
        Entry(0)
    }

    /// Construit un descripteur vers `addr`, avec les drapeaux `flags`.
    ///
    /// `addr` est un multiple de 4 Kio ; les 12 bits de poids faible
    /// sont ignorés, c'est le décalage dans la page.
    pub const fn new(addr: u64, flags: u64) -> Self {
        Entry((addr & ADDR_MASK) | (flags & FLAG_MASK))
    }

    /// Valeur brute du descripteur.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Adresse physique pointée.
    pub const fn addr(self) -> u64 {
        self.0 & ADDR_MASK
    }

    /// Le matériel peut-il utiliser ce descripteur ?
    pub const fn is_present(self) -> bool {
        self.0 & PRESENT != 0
    }

    /// Grande page (bit Huge) : uniquement pertinent en PD.
    pub const fn is_huge(self) -> bool {
        self.0 & HUGE != 0
    }

    /// Drapeaux du descripteur.
    pub const fn flags(self) -> u64 {
        self.0 & FLAG_MASK
    }
}

/* ------------------------------------------------------------
 * Table de pages
 * ------------------------------------------------------------ */

/// Une table de pages : 512 descripteurs, alignée sur 4 Kio.
///
/// L'alignement n'est pas un détail : un descripteur de table ne
/// garde que les bits 12..51 de l'adresse, donc une table mal alignée
/// verrait son adresse tronquée.
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [Entry; ENTRIES],
}

impl PageTable {
    /// Table vide : aucun descripteur présent.
    pub const fn new() -> Self {
        PageTable {
            entries: [Entry::empty(); ENTRIES],
        }
    }

    /// Remet tous les descripteurs à zéro.
    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = Entry::empty();
        }
    }

    /// Descripteur à l'index `idx`.
    pub fn entry(&self, idx: usize) -> Entry {
        self.entries[idx]
    }

    /// Écrit le descripteur à l'index `idx`.
    pub fn set(&mut self, idx: usize, entry: Entry) {
        self.entries[idx] = entry;
    }

    /// Nombre de descripteurs présents.
    pub fn count_present(&self) -> u32 {
        self.entries
            .iter()
            .filter(|e| e.is_present())
            .count() as u32
    }
}

/* ------------------------------------------------------------
 * Les tables du noyau
 *
 * Statiques et accédées par adresse : construire une référence vers
 * un `static mut` est un avertissement (et bientôt une erreur), et
 * `addr_of_mut!` l'évite sans rien céder sur la sûreté — l'accès
 * reste dans un bloc `unsafe`.
 * ------------------------------------------------------------ */

/// Table de niveau 4, racine.
static mut PML4: PageTable = PageTable::new();
/// PDPT de l'identity map.
static mut PDPT_LOW: PageTable = PageTable::new();
/// PD de l'identity map : 512 grandes pages de 2 Mio.
static mut PD_LOW: PageTable = PageTable::new();
/// PDPT du higher-half.
static mut PDPT_HIGH: PageTable = PageTable::new();
/// PD du higher-half.
static mut PD_HIGH: PageTable = PageTable::new();

/// PDPT du descripteur de test 4 Kio.
static mut PDPT_SCRATCH: PageTable = PageTable::new();
/// PD du descripteur de test.
static mut PD_SCRATCH: PageTable = PageTable::new();
/// PT du descripteur de test : 512 pages de 4 Kio.
static mut PT_SCRATCH: PageTable = PageTable::new();

/// Page physique de 4 Kio, alignée.
///
/// Un `repr(align)` ne s'applique qu'à un type : la page est donc un
/// `struct` d'un seul tableau, pas un tableau statique nu.
#[repr(C, align(4096))]
pub struct Page([u64; PAGE_SIZE / 8]);

impl Page {
    /// Page remplie de zéros.
    pub const fn zeroed() -> Self {
        Page([0; PAGE_SIZE / 8])
    }
}

/// Page physique servant de cible au descripteur de test.
///
/// Le test écrit par l'adresse virtuelle de test et relit par
/// l'adresse physique : si les deux donnent la même valeur, le
/// parcours à 4 niveaux a bien résolu l'adresse.
static mut SCRATCH_PAGE: Page = Page::zeroed();

/* ------------------------------------------------------------
 * Adresses physiques
 *
 * Le noyau est lié en higher-half : `addr_of!` renvoie une adresse
 * virtuelle en higher-half. Les descripteurs de table conservent
 * l'adresse PHYSIQUE (exigée par le matériel), d'où la soustraction
 * de HIGHER_HALF. L'identity map (0..1 Gio) mappant les mêmes pages
 * physiques rend ces adresses physiques accessibles en lecture et
 * écriture.
 * ------------------------------------------------------------ */

/// Adresse physique d'une table statique.
unsafe fn phys<T>(table: *const T) -> u64 {
    table as u64 - HIGHER_HALF
}

/// Index de niveau 4 d'une adresse virtuelle.
const fn pml4_index(vaddr: u64) -> usize {
    ((vaddr >> 39) & 0x1FF) as usize
}

/// Index de niveau 3 (PDPT) d'une adresse virtuelle.
const fn pdpt_index(vaddr: u64) -> usize {
    ((vaddr >> 30) & 0x1FF) as usize
}

/// Index de niveau 2 (PD) d'une adresse virtuelle.
const fn pd_index(vaddr: u64) -> usize {
    ((vaddr >> 21) & 0x1FF) as usize
}

/// Index de niveau 1 (PT) d'une adresse virtuelle.
const fn pt_index(vaddr: u64) -> usize {
    ((vaddr >> 12) & 0x1FF) as usize
}

/* ------------------------------------------------------------
 * Construction
 * ------------------------------------------------------------ */

/// Adresse physique de la PML4 après construction.
pub struct KernelTables {
    /// Adresse physique de la racine.
    pml4: u64,
    /// Nombre de descripteurs posés dans la PML4.
    pml4_entries: u32,
    /// Descripteurs de l'identity map (grandes pages).
    identity_entries: u32,
    /// Descripteurs du higher-half (grandes pages).
    higher_half_entries: u32,
    /// Descripteurs 4 Kio posés.
    page4k_entries: u32,
}

impl KernelTables {
    /// Adresse physique de la racine — à charger dans CR3.
    pub fn pml4(&self) -> u64 {
        self.pml4
    }

    /// Résout une adresse virtuelle en adresse physique.
    ///
    /// Parcourt les quatre niveaux et renvoie `None` si le chemin
    /// contient un descripteur absent. C'est ce qui permet de
    /// vérifier les tables **avant** de les activer : un descripteur
    /// mal indexé se voit tout de suite, sans déclencher de faute.
    pub fn translate(&self, vaddr: u64) -> Option<u64> {
        let pml4 = unsafe { &*core::ptr::addr_of!(PML4) };
        let e = pml4.entry(pml4_index(vaddr));
        if !e.is_present() {
            return None;
        }

        let pdpt = unsafe { &*(e.addr() as *const PageTable) };
        let e = pdpt.entry(pdpt_index(vaddr));
        if !e.is_present() {
            return None;
        }

        let pd = unsafe { &*(e.addr() as *const PageTable) };
        let e = pd.entry(pd_index(vaddr));
        if !e.is_present() {
            return None;
        }
        if e.is_huge() {
            // Grande page : les 21 bits de poids faible sont le décalage.
            return Some(e.addr() + (vaddr & 0x1F_FFFF));
        }

        let pt = unsafe { &*(e.addr() as *const PageTable) };
        let e = pt.entry(pt_index(vaddr));
        if !e.is_present() {
            return None;
        }
        Some(e.addr() + (vaddr & 0xFFF))
    }
}

/// Construit les tables du noyau.
///
/// Couvre l'identity map et le higher-half en pages de 2 Mio, plus
/// une page de 4 Kio mappée dans un espace dédié. Renvoie le résumé
/// de ce qui a été posé.
pub fn build() -> KernelTables {
    unsafe {
        let pml4 = &mut *core::ptr::addr_of_mut!(PML4);
        let pdpt_low = &mut *core::ptr::addr_of_mut!(PDPT_LOW);
        let pd_low = &mut *core::ptr::addr_of_mut!(PD_LOW);
        let pdpt_high = &mut *core::ptr::addr_of_mut!(PDPT_HIGH);
        let pd_high = &mut *core::ptr::addr_of_mut!(PD_HIGH);
        let pdpt_scratch = &mut *core::ptr::addr_of_mut!(PDPT_SCRATCH);
        let pd_scratch = &mut *core::ptr::addr_of_mut!(PD_SCRATCH);
        let pt_scratch = &mut *core::ptr::addr_of_mut!(PT_SCRATCH);

        pml4.clear();
        pdpt_low.clear();
        pd_low.clear();
        pdpt_high.clear();
        pd_high.clear();
        pdpt_scratch.clear();
        pd_scratch.clear();
        pt_scratch.clear();

        // --- identity map : 0 .. 1 Gio, pages de 2 Mio ---
        // PD_LOW : 512 grandes pages de 2 Mio, de 0 à 1 Gio.
        for i in 0..ENTRIES {
            pd_low.set(
                i,
                Entry::new((i as u64) * PAGE_2M as u64, KERNEL_PAGE | HUGE),
            );
        }

        // PML4[0] -> PDPT_LOW -> PD_LOW.
        // Les indices sont calculés depuis l'adresse, jamais écrits en
        // dur : une adresse dont les bits 38..30 sont tous à 1 (le
        // higher-half) a un index de PDPT de 511, pas de 0.
        pdpt_low.set(
            pdpt_index(0),
            Entry::new(phys(core::ptr::addr_of!(PD_LOW)), LINK),
        );
        pml4.set(
            pml4_index(0),
            Entry::new(phys(core::ptr::addr_of!(PDPT_LOW)), LINK),
        );

        // --- higher-half : 0xFFFFFFFF80000000 .. +1 Gio ---
        // PML4[511] : les 256 slots hauts de la table sont réservés au
        // noyau, et cette adresse occupe le dernier.
        for i in 0..ENTRIES {
            pd_high.set(
                i,
                Entry::new((i as u64) * PAGE_2M as u64, KERNEL_PAGE | HUGE),
            );
        }
        pdpt_high.set(
            pdpt_index(HIGHER_HALF),
            Entry::new(phys(core::ptr::addr_of!(PD_HIGH)), LINK),
        );
        pml4.set(
            pml4_index(HIGHER_HALF),
            Entry::new(phys(core::ptr::addr_of!(PDPT_HIGH)), LINK),
        );

        // --- page de test 4 Kio ---
        // Chaque niveau est indexé depuis SCRATCH_VADDR : la chaîne
        // PML4 -> PDPT_SCRATCH -> PD_SCRATCH -> PT_SCRATCH -> page.
        pd_scratch.set(
            pd_index(SCRATCH_VADDR),
            Entry::new(phys(core::ptr::addr_of!(PT_SCRATCH)), LINK),
        );
        pdpt_scratch.set(
            pdpt_index(SCRATCH_VADDR),
            Entry::new(phys(core::ptr::addr_of!(PD_SCRATCH)), LINK),
        );
        pml4.set(
            pml4_index(SCRATCH_VADDR),
            Entry::new(phys(core::ptr::addr_of!(PDPT_SCRATCH)), LINK),
        );

        pt_scratch.set(
            pt_index(SCRATCH_VADDR),
            Entry::new(phys(core::ptr::addr_of!(SCRATCH_PAGE)), DATA_PAGE),
        );

        // Les écritures dans les tables doivent être visibles avant que
        // le matériel ne les utilise.
        fence(Ordering::SeqCst);

        KernelTables {
            pml4: phys(core::ptr::addr_of!(PML4)),
            pml4_entries: pml4.count_present(),
            identity_entries: pd_low.count_present(),
            higher_half_entries: pd_high.count_present(),
            page4k_entries: pt_scratch.count_present(),
        }
    }
}

/// Charge la PML4 dans CR3.
///
/// Un changement de CR3 vide les TLB non globaux : c'est le bon
/// moment de basculer.
///
/// # Sécurité
///
/// `pml4` doit être l'adresse d'une PML4 valide dont les descripteurs
/// couvrent le code en cours d'exécution. Sinon, triple faute
/// immédiate.
pub unsafe fn load(pml4: u64) {
    core::arch::asm!("mov cr3, {}", in(reg) pml4, options(nomem, nostack));
}

/* ------------------------------------------------------------
 * Rapport
 * ------------------------------------------------------------ */

/// Affiche le contenu des tables et vérifie la résolution d'adresses.
pub fn report(tables: &KernelTables) {
    console::print(format_args!(
        "paging : 4 niveaux\n  pml4 physique      : {:#018x}\n",
        tables.pml4
    ));
    console::print(format_args!(
        "  descripteurs pml4  : {} (identity, higher-half, test 4 Kio)\n",
        tables.pml4_entries
    ));
    console::print(format_args!(
        "  identity 0..1 Gio  : {} grandes pages de 2 Mio\n",
        tables.identity_entries
    ));
    console::print(format_args!(
        "  higher-half        : {} grandes pages de 2 Mio\n",
        tables.higher_half_entries
    ));
    console::print(format_args!(
        "  page de test 4 Kio : {} page(s)\n",
        tables.page4k_entries
    ));

    // Résolution avant activation : un index erroné se voit ici,
    // sans déclencher de faute de page.
    let kernel_phys = HIGHER_HALF.wrapping_add(0x100000);
    console::print(format_args!("\nresolution :\n"));
    console::print(format_args!(
        "  identity  {:#018x} -> {:#x}\n",
        0x100000u64,
        tables.translate(0x100000).unwrap_or(0)
    ));
    console::print(format_args!(
        "  haute     {:#018x} -> {:#x}\n",
        kernel_phys,
        tables.translate(kernel_phys).unwrap_or(0)
    ));
    console::print(format_args!(
        "  test 4K   {:#018x} -> {:#x}\n",
        SCRATCH_VADDR,
        tables.translate(SCRATCH_VADDR).unwrap_or(0)
    ));

    // Descripteurs réellement posés, pour contrôle : les drapeaux sont
    // ceux que le noyau a demandés, le matériel n'a encore rien marqué.
    unsafe {
        let pml4 = &*core::ptr::addr_of!(PML4);
        let pdpt_low = &*core::ptr::addr_of!(PDPT_LOW);
        let pd_low = &*core::ptr::addr_of!(PD_LOW);
        let pt_scratch = &*core::ptr::addr_of!(PT_SCRATCH);

        console::print(format_args!("\ndescripteurs :\n"));
        console::print(format_args!(
            "  pml4[0]        {:#018x} flags {:#x}\n",
            pml4.entry(0).raw(),
            pml4.entry(0).flags()
        ));
        console::print(format_args!(
            "  pdpt_low[0]    {:#018x} flags {:#x}\n",
            pdpt_low.entry(pdpt_index(0)).raw(),
            pdpt_low.entry(pdpt_index(0)).flags()
        ));
        console::print(format_args!(
            "  pd_low[0]      {:#018x} flags {:#x}\n",
            pd_low.entry(pd_index(0x100000)).raw(),
            pd_low.entry(pd_index(0x100000)).flags()
        ));
        console::print(format_args!(
            "  pt[0] 4 Kio    {:#018x} flags {:#x}\n",
            pt_scratch.entry(pt_index(SCRATCH_VADDR)).raw(),
            pt_scratch.entry(pt_index(SCRATCH_VADDR)).flags()
        ));
    }
}

/// Vérifie le mapping à l'exécution, **après** le chargement de CR3.
///
/// Écrit par l'adresse virtuelle de test et relit par l'adresse
/// physique : le parcours à 4 niveaux n'est prouvé que si les deux
/// voies désignent la même page.
///
/// À appeler après [`load`] : avant, l'adresse de test n'est pas
/// encore mappée et l'écriture déclenche une faute de page.
pub fn verify() {
    unsafe {
        let scratch_va = SCRATCH_VADDR as *mut u64;
        let scratch_pa = phys(core::ptr::addr_of!(SCRATCH_PAGE)) as *mut u64;

        *scratch_pa = 0;
        *scratch_va = 0x1234_5678_9ABC_DEF0;

        let par_virt = *scratch_va;
        let par_phys = *scratch_pa;

        console::print(format_args!("\nactivation :\n"));
        console::print(format_args!(
            "  ecriture par virtuel : {:#x}\n  lecture par physique : {:#x}\n",
            par_virt, par_phys
        ));

        if par_virt == par_phys {
            console::print(format_args!("  page 4 Kio : OK\n"));
        } else {
            console::print(format_args!("  page 4 Kio : ECHEC (deux pages differentes)\n"));
        }
    }
}
