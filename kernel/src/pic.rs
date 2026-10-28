/* ============================================================
 * pic.rs — remaillage du PIC 8259 (maître + esclave)
 *
 * J2.6 : le PIC 8259A initialise par défaut les vecteurs 0x08..0x0F,
 * ce qui entre en conflit avec les exceptions (#DF = 8, #GP = 13...).
 * On le remaille vers 0x20..0x2F (32..47) :
 *
 *   maître  (ports 0x20/0x21) → vecteurs 32..39  (IRQ 0..7)
 *   esclave (ports 0xA0/0xA1) → vecteurs 40..47  (IRQ 8..15)
 *
 * Séquence ICW (Intel 8259A) :
 *   1. ICW1  : 0x11 — edge, cascade, ICW4 attendu
 *   2. ICW2  : offset de vecteur (0x20 ou 0x28)
 *   3. ICW3  : master=0x04 (IR2↔esclave), slave=0x02 (cascade identity)
 *   4. ICW4  : 0x01 — mode 8086, EOI manuel
 *   5. OCW1  : mask = 0 (activer toutes les IRQ — on masquera ensuite)
 * ============================================================ */

use crate::io::outb;

// --- Ports du PIC maître ---
const PIC0_CMD: u16 = 0x20;
const PIC0_DATA: u16 = 0x21;

// --- Ports du PIC esclave ---
const PIC1_CMD: u16 = 0xA0;
const PIC1_DATA: u16 = 0xA1;

// --- Offsets de vecteur ---
const MASTER_OFFSET: u8 = 32; // IRQ 0..7  → vecteurs 32..39
const SLAVE_OFFSET: u8 = 40;  // IRQ 8..15 → vecteurs 40..47

/// Commande EOI (End Of Interrupt) au PIC.
const PIC_EOI: u8 = 0x20;

/// Remappe le PIC maître et esclave vers les vecteurs 32..47.
///
/// Masque ensuite toutes les IRQ sauf le timer (IRQ 0) et le clavier
/// (IRQ 1). Les autres restent masquées jusqu'au J2.6c.
///
/// # Sécurité
///
/// Doit être appelé une seule fois, avant `sti`. Après l'appel,
/// les IRQ hardware utilisent les vecteurs 32..47 de l'IDT.
pub unsafe fn init() {
    // ICW1 : initialisation, ICW4 attendu.
    outb(PIC0_CMD, 0x11);
    outb(PIC1_CMD, 0x11);

    // ICW2 : offset de vecteur.
    outb(PIC0_DATA, MASTER_OFFSET);
    outb(PIC1_DATA, SLAVE_OFFSET);

    // ICW3 : maître IR2 → esclave, esclave ID = 2.
    outb(PIC0_DATA, 0x04);
    outb(PIC1_DATA, 0x02);

    // ICW4 : mode 8086, EOI manuel.
    outb(PIC0_DATA, 0x01);
    outb(PIC1_DATA, 0x01);

    // OCW1 : config du masque. On active IRQ 0 (timer) et IRQ 1 (clavier).
    // IRQ 0 = bit 0, IRQ 1 = bit 1. Masque = 0xFC (active 0 et 1, masque le reste).
    outb(PIC0_DATA, 0xFC);

    // OCW1 esclave : masque tout pour l'instant.
    outb(PIC1_DATA, 0xFF);

    // Petit délai I/O pour laisser le PIC traiter.
    // (pas de outb de synchronization, le PIC est asynchrone)
    outb(PIC0_CMD, PIC_EOI); // flush du registrICW au PIC maître
    outb(PIC1_CMD, PIC_EOI); // flush au PIC esclave
}

/// Envoie un EOI (End Of Interrupt) au(s) PIC(s) concerné(s).
///
/// # Arguments
///
/// * `irq` — numéro d'IRQ (0..15), PAS le vecteur. IRQ 8..15 passent
///   par l'esclave → EOI sur les deux PICs (cascade).
pub fn eoi(irq: u8) {
    if irq >= 8 {
        // Esclave puis maître (cascade).
        outb(PIC1_CMD, PIC_EOI);
        outb(PIC0_CMD, PIC_EOI);
    } else {
        outb(PIC0_CMD, PIC_EOI);
    }
}
