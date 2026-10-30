/* ============================================================
 * task.rs — planificateur à priorités avec aging (J2.8)
 *
 * Chaque tâche possède sa propre pile noyau (32 Kio), un champ
 * `priority` (0 = plus haute) et un champ `age` (ticks d'inactivité).
 * Le scheduler (timer IRQ) :
 *   1. Sauvegarde le RSP courant (pointeur vers le bloc GPR) dans
 *      la structure de la tâche courante.
 *   2. Incrémente l'âge de toutes les tâches sauf la courante.
 *   3. Calcule la priorité effective = priority - age (saturation
 *      à 0 pour éviter l'underflow). Une tâche en attente voit sa
 *      priorité effective augmenter → fin par être sélectionnée
 *      (anti-starvation / aging).
 *   4. Sélectionne la tâche avec la priorité effective minimale ;
 *      en cas d'égalité, round-robin depuis current+1.
 *   5. Retourne le RSP de la tâche suivante → irq_common le
 *      charge, restaure les 15 GPRs, et fait iretq.
 *
 * Layout de pile d'une tâche (push décrémental, adresses croissantes) :
 *
 *   saved_rsp → [rax]                    ← GPR bloc (15 × 8 = 120 octets)
 *              [rbx]
 *              [rcx]
 *              [rdx]
 *              [rsi]
 *              [rdi]
 *              [rbp]
 *              [r8]
 *              [r9]
 *              [r10]
 *              [r11]
 *              [r12]
 *              [r13]
 *              [r14]
 *              [r15]
 *              [vector=32]              ← interrupt frame (5 × 8 = 40 octets)
 *              [error_code=0]
 *              [rip=entry]
 *              [cs=0x08]
 *              [rflags=0x202]
 *              ...                      ← repos de la pile inutilisé
 *
 * irq_common pousse les GPRs, appelle handle_irq → schedule(), qui
 * échange saved_rsp, puis irq_common pop les GPRs de la nouvelle tâche
 * et fait iretq sur son interrupt frame.
 * ============================================================ */

/// Segment sélecteur CS noyau (défini par la GDT, entrée 1).
const KERNEL_CS: u64 = 0x08;

/// Flags initial : IF=1 (interruptions activées).
const EFLAGS_IF: u64 = 0x202;

/// Vecteur timer utilisé pour l'initialisation manuelle des piles
/// de tâches (doit correspondre au timer IRQ vector dans l'IDT).
const TIMER_VECTOR: u64 = 32;

/// Nombre de registres généraux sauvegardés par irq_common (15 :
/// tous sauf RSP, qui est déduit du bloc sauvé lui-même).
pub const NGPRS: usize = 15;

/// Taille du bloc GPR sauvé (15 × 8 = 120 octets).
/// Exportée pour que handle_irq puisse retrouver l'interrupt frame
/// située juste au-dessus du bloc GPR.
pub const GPR_SAVE_SIZE: usize = NGPRS * 8;

/// Taille de l'interrupt frame [vector][error_code][rip][cs][rflags].
const FRAME_SIZE: usize = 5 * 8; // 40

/// Taille totale du bloc sauvé : GPRs + interrupt frame.
const TASK_FRAME_SIZE: usize = GPR_SAVE_SIZE + FRAME_SIZE; // 160

/// La pile de chaque tâche : 32 Kio, alignée à 16 octets.
const TASK_STACK_SIZE: usize = 32_384;

/// Nombre maximum de tâches (y compris le noyau principal).
const MAX_TASKS: usize = 8;

/// Priorité du noyau principal : la plus basse.
/// Le noyau est la tâche d'inactivité — il ne tourne que lorsqu'aucune
/// tâche utilisateur n'est prête (ou via l'aging).
const KERNEL_PRIORITY: u8 = 3;

/// Description d'une tâche.
#[derive(Clone, Copy)]
pub struct Task {
    /// Pile noyau de la tâche.
    #[allow(dead_code)]
    pub stack: [u8; TASK_STACK_SIZE],
    /// RSP à restaurer : pointe sur le bloc GPR sauvé.
    pub saved_rsp: u64,
    /// Priorité de la tâche (0 = plus haute).
    pub priority: u8,
    /// Nombre de ticks depuis la dernière exécution (pour l'âge).
    pub age: u64,
}

/// Tableau des tâches. L'entrée 0 est toujours le noyau principal.
static mut TASKS: [Task; MAX_TASKS] = [
    Task {
        stack: [0; TASK_STACK_SIZE],
        saved_rsp: 0,
        priority: KERNEL_PRIORITY,
        age: 0,
    };
    MAX_TASKS
];

/// Indice de la tâche courante dans TASKS.
static mut CURRENT: usize = 0;

/// Nombre de tâches créées (prochain ID disponible).
static mut NUM_TASKS: usize = 1; // 0 = noyau principal

/// Initialise le noyau principal.
///
/// Le noyau principal (tâche 0) est déjà en cours d'exécution sur sa
/// pile de boot. `saved_rsp` sera initialisé paresseusement par
/// `schedule()` au premier tick du timer : `schedule()` stocke toujours
/// `current_rsp` dans `TASKS[CURRENT].saved_rsp` AVANT de lire la
/// prochaine tâche, donc la valeur 0 initiale est immédiatement
/// écrasée — aucune lecture de `saved_rsp` pour la tâche 0 n'a lieu
/// avant cela.
pub unsafe fn init_main() {
    TASKS[0].saved_rsp = 0;
    TASKS[0].priority = KERNEL_PRIORITY;
    TASKS[0].age = 0;
}

/// Crée une nouvelle tâche et retourne son indice.
///
/// La pile est initialisée avec un bloc GPR (tous à 0) suivi d'un
/// interrupt frame factice ([vector=32][error_code=0][rip=entry][cs=0x08][rflags=0x202])
/// afin que iretq dans irq_common saute directement à entry après
/// le pop des GPRs.
///
/// # Sécurité
///
/// Appelé une seule fois par tâche, avant `sti`. `entry` ne doit pas
/// retourner (boucle infinie ou panic).
pub unsafe fn create_task(entry: extern "C" fn() -> !, priority: u8) -> usize {
    let id = NUM_TASKS;
    NUM_TASKS += 1;

    let task = &mut TASKS[id];

    // Le bloc GPR + interrupt frame (= TASK_FRAME_SIZE) est placé
    // au sommet de la pile. saved_rsp pointe sur le bloc GPR.
    let stack_top = task.stack.as_mut_ptr() as u64 + TASK_STACK_SIZE as u64;
    let rsp = stack_top - TASK_FRAME_SIZE as u64;

    // --- Zone GPR : 15 registres à 0 ---
    // (Un worker écrivant dans un registre trouvera la valeur préservée
    //  uniquement si irq_common sauvegarde/restaure correctement les GPRs.)
    let gpr_base = rsp as *mut u64;
    for i in 0..NGPRS {
        core::ptr::write_volatile(gpr_base.add(i), 0u64);
    }

    // --- Interrupt frame — immédiatement au-dessus du bloc GPR ---
    // saved_rsp + GPR_SAVE_SIZE → [vector][error_code][rip][cs][rflags]
    let frame_base = (rsp + GPR_SAVE_SIZE as u64) as *mut u64;
    core::ptr::write_volatile(frame_base.add(0), TIMER_VECTOR);   // vector
    core::ptr::write_volatile(frame_base.add(1), 0);              // error_code
    core::ptr::write_volatile(frame_base.add(2), entry as u64);   // rip
    core::ptr::write_volatile(frame_base.add(3), KERNEL_CS);      // cs
    core::ptr::write_volatile(frame_base.add(4), EFLAGS_IF);     // rflags

    task.saved_rsp = rsp;
    task.priority = priority;
    task.age = 0;
    id
}

/// Calcule la priorité effective d'une tâche.
///
/// effective = priority - age  (arithmétique signée i64)
///
/// Plus l'âge est grand, plus la priorité effective est basse (meilleure).
/// L'arithmétique signée évite la saturation : une tâche prioritaire 3
/// qui a attendu 5 ticks a une priorité effective de -2, battant ainsi
/// la tâche prioritaire 0 qui vient de s'exécuter (effective = 0).
#[inline]
fn effective_priority(priority: u8, age: u64) -> i64 {
    priority as i64 - age as i64
}

/// Planifie la prochaine tâche — priorité avec aging (anti-starvation).
///
/// # Algorithme
///
/// 1. Incrémente l'âge de toutes les tâches sauf la courante.
///    Réinitialise l'âge de la courante à 0.
/// 2. Calcule la priorité effective = priority.saturating_sub(age)
///    pour chaque tâche. Une tâche en attente voit sa priorité
///    effective augmenter jusqu'à devenir la plus haute.
/// 3. Sélectionne la tâche avec la priorité effective minimale.
///    En cas d'égalité, round-robin (le premier trouvé après CURRENT
///    gagne) → part égale de CPU entre les tâches de même niveau.
///
/// # Arguments
///
/// `current_rsp` : RSP du bloc GPR de la tâche courante (mis en place
/// par irq_common avant l'appel).
///
/// # Retourne
///
/// Le RSP du bloc GPR de la tâche suivante. irq_common le charge,
/// pop les GPRs, et fait iretq.
pub unsafe fn schedule(current_rsp: u64) -> u64 {
    // Sauvegarde du RSP courant (pointeur vers le bloc GPR de la
    // tâche courante, posé par irq_common).
    TASKS[CURRENT].saved_rsp = current_rsp;

    let num_tasks = NUM_TASKS;
    if num_tasks <= 1 {
        return current_rsp; // rien à planifier
    }

    // 1. Met à jour l'âge : incrémente les autres, remet la courante à 0.
    for i in 0..num_tasks {
        if i != CURRENT {
            TASKS[i].age += 1;
        }
    }
    TASKS[CURRENT].age = 0;

    // 2. Trouve la tâche avec la priorité effective minimale.
    //    Round-robin : on scanne depuis CURRENT+1, le premier trouvé
    //    à égalité gagne → part égale entre tâches de même niveau.
    let mut best = CURRENT;
    let mut best_eff: i64 = effective_priority(
        TASKS[CURRENT].priority, TASKS[CURRENT].age
    );

    for i in 1..=num_tasks {
        let idx = (CURRENT + i) % num_tasks;
        let eff = effective_priority(TASKS[idx].priority, TASKS[idx].age);
        if eff < best_eff {
            best_eff = eff;
            best = idx;
        }
    }

    CURRENT = best;
    TASKS[CURRENT].saved_rsp
}
