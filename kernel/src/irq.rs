/* ============================================================
 * irq.rs — gestion des interruptions matérielles (IRQ)
 *
 * J2.6 : après le remappage du PIC (pic.rs), les IRQ 0..15
 * arrivent sur les vecteurs 32..47. Ce module :
 *
 *   - programme le PIT pour un tick de 100 Hz (IRQ 0 → 32)
 *   - fournit `handle_irq`, point d'entrée commun des stubs ASM
 *   - gestionnaire timer : incrémente un tick counter
 *   - gestionnaire clavier : lit le scancode sur le port 0x60
 *
 * Le handler envoie un EOI au PIC avant de retourner ; le stub
 * ASM fait ensuite `iretq` pour reprendre l'exécution interrompue.
 * ============================================================ */

use crate::io::{inb, outb};
use crate::pic;

use crate::idt::ExceptionFrame;

/* ------------------------------------------------------------
 * PIT (Programmable Interval Timer 8254)
 * ------------------------------------------------------------ */

const PIT_CHANNEL0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;

const PIT_FREQUENCY: u32 = 1_193_180; // Hz d'origine du PIT
const TIMER_HZ: u32 = 100;             // tick cible

/* ------------------------------------------------------------
 * Compteurs — volatile statics, protégés par les interrupt gates
 * (IF=0 pendant le handler → pas de concurrence réelle).
 * ------------------------------------------------------------ */

/// Nombre de ticks depuis le démarrage du timer.
static mut TICK_COUNT: u64 = 0;

/// Dernier scancode clavier reçu.
#[allow(dead_code)]
static mut LAST_SCANCODE: u8 = 0;

/// Nombre total de scancodes reçus.
#[allow(dead_code)]
static mut KEY_COUNT: u64 = 0;

/// Incrémente le tick counter. Appelé depuis l'handler timer.
#[inline]
fn tick() {
    unsafe {
        let v = core::ptr::read_volatile(core::ptr::addr_of!(TICK_COUNT));
        core::ptr::write_volatile(core::ptr::addr_of_mut!(TICK_COUNT), v + 1);
    }
}

/* ------------------------------------------------------------
 * PIT programming
 * ------------------------------------------------------------ */

/// Programme le PIT pour des interruptions à `TIMER_HZ` Hz.
///
/// # Sécurité
///
/// Appelé une seule fois, avant `sti`. Mode 2 (rate generator),
/// accès lobyte/hibyte, canal 0, binaire.
unsafe fn init_timer() {
    let divisor = (PIT_FREQUENCY / TIMER_HZ) as u16;

    outb(PIT_COMMAND, 0x36); // commande : canal 0, lobyte+hibyte, mode 2, binaire
    outb(PIT_CHANNEL0, (divisor & 0xFF) as u8); // diviseur bas
    outb(PIT_CHANNEL0, (divisor >> 8) as u8);  // diviseur haut
}

/* ------------------------------------------------------------
 * Handlers IRQ
 * ------------------------------------------------------------ */

/// Handler timer (IRQ 0, vecteur 32).
///
/// Incrémente le tick counter. Le PIT tourne à ~100 Hz.
fn handle_timer() {
    crate::serial::write_byte(b'T');
    tick();
}

/// Handler clavier (IRQ 1, vecteur 33).
///
/// Lit le scancode sur le port 0x60. On stocke le dernier reçu.
fn handle_keyboard() {
    let sc = inb(0x60);
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(LAST_SCANCODE), sc);
        let k = core::ptr::read_volatile(core::ptr::addr_of!(KEY_COUNT));
        core::ptr::write_volatile(core::ptr::addr_of_mut!(KEY_COUNT), k + 1);
    }
}

/// Point d'entrée commun — appelé par `irq_common` en ASM.
///
/// J2.8 : `gpr_ptr` pointe vers le bloc GPR sauvé par irq_common
/// (15 registres, 120 octets). L'interrupt frame [vector][error_code]
/// [rip][cs][rflags] se trouve juste au-dessus, à gpr_ptr + GPR_SAVE_SIZE.
///
/// # Comportement
///
/// Dépile le vecteur, appelle le handler approprié, envoie un EOI
/// au PIC, puis retourne. Le stub ASM fait `iretq` pour reprendre
/// l'exécution interrompue. Sur le timer (vecteur 32), appelle
/// schedule() qui retourne le RSP de la tâche suivante.
///
/// # Argument
///
/// `gpr_ptr` : RSP après le push des GPRs par irq_common. Pointe
/// vers le bloc GPR au sommet de la pile.
///
/// # Retourne
///
/// Le GPR pointer (RSP) de la tâche suivante — soit `gpr_ptr` lui-même
/// (même tâche, IRQ non-timer), soit le résultat de schedule() (timer).
#[no_mangle]
pub extern "C" fn handle_irq(gpr_ptr: u64) -> u64 {
    // L'interrupt frame est au-dessus du bloc GPR.
    let frame_ptr =
        (gpr_ptr + crate::task::GPR_SAVE_SIZE as u64) as *const ExceptionFrame;
    let f = unsafe { &*frame_ptr };

    match f.vector {
        32 => handle_timer(),
        33 => handle_keyboard(),
        _ => {
            crate::serial::write_byte(b'?');
        }
    }

    // EOI : le IRQ est vector - 32 (remappage PIC).
    pic::eoi((f.vector - 32) as u8);

    // J2.7 : sur le timer (vecteur 32), on planifie la prochaine tâche.
    // handle_irq retourne le GPR ptr de la tâche suivante — irq_common
    // le charge, pop les GPRs, et iretq, restaurant le contexte suivant.
    // Pour les autres IRQ, on renvoie gpr_ptr (pas de switch).
    if f.vector == 32 {
        return unsafe { crate::task::schedule(gpr_ptr) };
    }

    gpr_ptr
}

/* ------------------------------------------------------------
 * Compteurs publics — pour le test et le diagnostic.
 * ------------------------------------------------------------ */

/// Nombre de ticks de timer depuis le démarrage.
pub fn ticks() -> u64 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(TICK_COUNT)) }
}

/// Dernier scancode clavier reçu.
#[allow(dead_code)]
pub fn last_scancode() -> u8 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(LAST_SCANCODE)) }
}

/// Nombre total de scancodes reçus.
#[allow(dead_code)]
pub fn key_count() -> u64 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(KEY_COUNT)) }
}

/* ------------------------------------------------------------
 * Initialisation.
 *
 * Remappe le PIC, programme le PIT, puis active IRQ 0 (timer)
 * et IRQ 1 (clavier). Appelée APRÈS idt::init() (qui charge l'IDT).
 * ============================================================= */

pub unsafe fn init() {
    pic::init();
    init_timer();
}
