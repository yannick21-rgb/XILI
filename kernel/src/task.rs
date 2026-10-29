/* ============================================================
 * task.rs — planificateur round-robin (J2.7)
 *
 * Chaque tâche possède sa propre pile noyau (16 Kio). Quand le
 * timer (IRQ timer, vecteur 32) se déclenche, le CPU pousse le
 * cadre d'interruption [vector][error_code][rip][cs][rflags] sur
 * la pile de la tâche courante.
 *
 * Le handler d'IRQ (handle_irq, irq.rs) appelle schedule() qui :
 *   1. Sauvegarde le RSP courant (= pointeur vers le cadre) dans
 *      la structure de la tâche courante.
 *   2. Passe à la tâche suivante (round-robin, 1 tâche par tick).
 *   3. Retourne le RSP de la prochaine tâche → handle_irq le
 *      renvoie dans RAX, irq_common le charge, add $16 + iretq
 *      restaure le prochain contexte.
 *
 * Pour une tâche nouvelle, la pile est initialisée à la main avec
 * le même [vector][error_code][rip][cs][rflags] que produirait un
 * IRQ — iretq saute donc directement à l'entry point.
 * ============================================================ */

/// Segment sélecteur CS noyau (défini par la GDT, entrée 1).
const KERNEL_CS: u64 = 0x08;

/// Flags initial : IF=1 (interruptions activées).
const EFLAGS_IF: u64 = 0x202;

/// Vecteur timer utilisé pour l'initialisation manuelle des piles
/// de tâches (doit correspondre au timer IRQ vector dans l'IDT).
const TIMER_VECTOR: u64 = 32;

/// La pile de chaque tâche : 16 Kio, alignée à 16 octets.
const TASK_STACK_SIZE: usize = 16_384;

/// Nombre maximum de tâches (y compris le noyau principal).
const MAX_TASKS: usize = 8;

/// Description d'une tâche.
#[derive(Clone, Copy)]
pub struct Task {
    /// Pile noyau de la tâche.
    #[allow(dead_code)]
    pub stack: [u8; TASK_STACK_SIZE],
    /// RSP à restaurer : pointe sur [vector][error_code][rip][cs][rflags]
    /// de cette tâche, soit sur la pile si elle a été interrompue, soit
    /// sur une initialisation manuelle pour une tâche nouvelle.
    pub saved_rsp: u64,
}

/// Tableau des tâches. L'entrée 0 est toujours le noyau principal.
static mut TASKS: [Task; MAX_TASKS] = [
    Task {
        stack: [0; TASK_STACK_SIZE],
        saved_rsp: 0,
    };
    MAX_TASKS
];

/// Indice de la tâche courante dans TASKS.
static mut CURRENT: usize = 0;

/// Nombre de tâches créées (prochain ID disponible).
static mut NUM_TASKS: usize = 1; // 0 = noyau principal

/// Compteur de ticks du worker (pour diagnostic futur).
#[allow(dead_code)]
static mut WORKER_ITER: u64 = 0;

/// Initialise le noyau principal.
///
/// Le noyau principal (tâche 0) est déjà en cours d'exécution sur sa pile
/// de boot. `saved_rsp` sera initialisé paresseusement par `schedule()`
/// au premier tick du timer : `schedule()` stocke toujours `current_rsp`
/// dans `TASKS[CURRENT].saved_rsp` AVANT de lire la prochaine tâche, donc
/// la valeur 0 initiale est immédiatement écrasée — aucune lecture de
/// `saved_rsp` pour la tâche 0 n'a lieu avant cela.
pub unsafe fn init_main() {
    TASKS[0].saved_rsp = 0;
}

/// Crée une nouvelle tâche et retourne son indice.
///
/// La pile est initialisée avec un cadre d'interruption factice
/// ([vector=32][0][rip=entry][cs=0x08][rflags=0x202]) afin que
/// iretq dans irq_common saute directement à entry.
pub unsafe fn create_task(entry: extern "C" fn() -> !) -> usize {
    let id = NUM_TASKS;
    NUM_TASKS += 1;

    let task = &mut TASKS[id];
    let stack_top = task.stack.as_mut_ptr() as u64 + TASK_STACK_SIZE as u64;

    // Cadre d'interruption au sommet de la pile (pile croît vers le bas).
    let rsp = stack_top - 40; // 5 * 8 octets

    // [vector][error_code][rip][cs][rflags]
    *(rsp as *mut u64) = TIMER_VECTOR;               // vector
    *((rsp + 8) as *mut u64) = 0;                    // error_code
    *((rsp + 16) as *mut u64) = entry as u64;        // rip (point d'entrée)
    *((rsp + 24) as *mut u64) = KERNEL_CS;           // cs
    *((rsp + 32) as *mut u64) = EFLAGS_IF;           // rflags (IF=1)

    task.saved_rsp = rsp;
    id
}

/// Pointe la tâche suivante en round-robin.
#[inline]
fn next_task(current: usize) -> usize {
    let num_tasks = unsafe { NUM_TASKS };
    let mut next = current + 1;
    if next >= num_tasks {
        next = 0; // retour au noyau principal
    }
    next
}

/// Planifie la prochaine tâche.
///
/// # Arguments
///
/// `current_rsp` : RSP du cadre d'interruption courant (pointe vers
/// [vector][error_code][rip][cs][rflags] sur la pile de la tâche
/// courante).
///
/// # Retourne
///
/// Le RSP de la tâche suivante (même format de cadre). irq_common
/// charge ce RSP, retire vector+error_code, et fait iretq.
pub unsafe fn schedule(current_rsp: u64) -> u64 {
    // Sauvegarde du RSP courant.
    // Pour le noyau principal (id 0), la première fois c'est une
    // initialisation paresseuse : saved_rsp vaut 0.
    TASKS[CURRENT].saved_rsp = current_rsp;

    // Round-robin : passe à la tâche suivante.
    CURRENT = next_task(CURRENT);

    TASKS[CURRENT].saved_rsp
}
