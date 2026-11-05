/* ============================================================
 * syscall.rs — appels système x86-64 (J3.2)
 *
 * `syscall`/`sysret` utilisent les MSRs suivants :
 *
  *   IA32_STAR   (0xC000_0081) : sélecteurs CS/SS noyau et utilisateur
  *   IA32_LSTART  (0xC000_0082) : adresse de l'entrée syscall
  *   IA32_FMASK   (0xC000_0084) : masque de bits RFLAGS à effacer (IF|TF)
  *   IA32_EFER    (0xC000_0080) : bit SCE (System Call Extensions)
  *
  * NOTE: QEMU 8.x TCG utilise des indices MSR internes (0xC000_0081-0xC000_0084)
 /// au lieu des valeurs standard x86 (0xC000_0008-0xC000_000A). Le helper_wrmsr
 /// de QEMU compare ECX contre 0xC0000081/0x82/0x84 — les valeurs standard
 /// (0xC0000008/0x09/0x0A) sont silencieusement ignorées (pas de case match).
 *
 * Convention d'appel (Linux x86-64) :
 *
 *   rax = numéro de syscall
 *   rdi, rsi, rdx, r10, r8, r9 = arguments
 *   rax = valeur de retour (ou -errno)
 *
 * Le CPU sauvegarde RIP dans RCX et RFLAGS dans R11 avant d'entrer
 * dans le handler. `sysret` utilise RCX comme RIP de retour et R11
 * comme RFLAGS de retour.
 *
 * Le handler `syscall_entry` (isr.s) :
 *   1. Sauvegarde les 15 GPRs sur la pile noyau (même layout que irq_common)
 *   2. Appelle handle_syscall(gpr_ptr) — lit/modifie le bloc
 *   3. Restore les GPRs (RCX, R11)
 *   4. Si SYSCALL_TEST_DONE=0 : `sysret` → retour en anneau 3
 *      Si SYSCALL_TEST_DONE=1 : `iretq` → retour en anneau 0 (J3.2 test done)
 * ============================================================ */

use core::arch::asm;

use crate::task;

/* ------------------------------------------------------------
 * MSRs
 * ------------------------------------------------------------ */

/// IA32_EFER — Extended Feature Enable Register.
const MSR_EFER: u32 = 0xC000_0080;
/// IA32_STAR — kernel/user CS/SS selectors.
/// QEMU 8.x TCG internal MSR index (NOT standard 0xC0000008).
const MSR_STAR: u32 = 0xC000_0081;
/// IA32_LSTAR — syscall entry point.
/// QEMU 8.x TCG internal MSR index (NOT standard 0xC0000009).
const MSR_LSTAR: u32 = 0xC000_0082;
/// IA32_FMASK — RFLAGS bits cleared by syscall.
/// QEMU 8.x TCG internal MSR index (NOT standard 0xC000000A).
const MSR_FMASK: u32 = 0xC000_0084;

/// Bit SCE dans EFER (System Call Extensions).
const EFR_SCE: u64 = 1 << 0;

/* ------------------------------------------------------------
 * Sélecteurs pour MSR_STAR
 * ------------------------------------------------------------ */

/// Code segment 64-bit (L=1) pour syscall — STAR[63:48].
/// Utilise KERNEL_CS64 (0x38) et non KERNEL_CS (0x08) : le noyau ne peut
/// pas faire de far-jump pour recharger CS, donc 0x08 garde L=0 (hérité du
/// bootloader). Mais `syscall` entre en R0 via ce sélecteur → il faut L=1
/// pour que le handler exécute en 64-bit mode.
const STAR_KERNEL_CS: u64 = crate::tss::KERNEL_CS64 as u64;
/// Kernel SS (0x10) → STAR[47:32], chargé par `syscall`.
const STAR_KERNEL_SS: u64 = crate::tss::KERNEL_DS as u64;
/// User CS (0x28) → STAR[31:16], chargé par `sysret`.
const STAR_USER_CS: u64 = crate::tss::USER_CS as u64;
/// User SS (0x30) → STAR[15:0], chargé par `sysret`.
const STAR_USER_SS: u64 = crate::tss::USER_DS as u64;

/// RFLAGS masqués par `syscall` : IF (bit 9) + TF (bit 8).
const FMASK: u64 = 0x202;

/* ------------------------------------------------------------
 * Numéros de syscall (Linux x86-64)
 * ------------------------------------------------------------ */

const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_EXIT: u64 = 60;
const SYS_EXIT_GROUP: u64 = 231;
const SYS_GETPID: u64 = 39;
const SYS_GETTID: u64 = 186;
const SYS_BRK: u64 = 12;
const SYS_MMAP: u64 = 9;
const SYS_OPENAT: u64 = 257;
const SYS_NEWFSTATAT: u64 = 262;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_WAIT4: u64 = 61;
const SYS_PIPE: u64 = 22;
const SYS_GETCWD: u64 = 79;
const SYS_CHDIR: u64 = 80;
const SYS_DUP: u64 = 32;
const SYS_DUP2: u64 = 33;
const SYS_LSEEK: u64 = 8;
const SYS_MPROTECT: u64 = 10;
const SYS_MUNMAP: u64 = 11;
const SYS_UNAME: u64 = 63;
const SYS_GETDENTS64: u64 = 217;
const SYS_FUTEX: u64 = 202;
const SYS_IOCTL: u64 = 16;
const SYS_RT_SIGACTION: u64 = 13;
const SYS_RT_SIGPROCMASK: u64 = 14;
const SYS_KILL: u64 = 62;
const SYS_TGKILL: u64 = 234;
const SYS_GETRANDOM: u64 = 318;
const SYS_MEMFD_CREATE: u64 = 319;

/// Descripteurs fichiers — 3 emplacements réservés (stdin/stdout/stderr),
/// fd 3+ réservés aux fichiers du VFS. La table vive est celle du VFS
/// (`crate::vfs::OPEN_FILES`) : les syscalls l'utilisent en source de
/// vérité (CR-1).
const MAX_FD: usize = 8;

/// Fournit `bcmp` pour le linker. LLVM génère des appels à `bcmp` pour
/// comparer des tranches `[u8]` ; dans un kernel no_std, l'implémentation
/// libc est absente. Implémentation manuelle.
#[no_mangle]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        if unsafe { *a.add(i) } != unsafe { *b.add(i) } {
            return 1;
        }
    }
    0
}

/// Syscall 0x1FF — "test done": déclenche le retour en anneau 0 via iretq.
/// Le handler modifie RCX (→ ring0_after_test) et R11 (→ RFLAGS),
/// puis syscall_entry utilise iretq au lieu de sysret.
const SYS_TEST_DONE: u64 = 0x1FF;

/* ------------------------------------------------------------ *
 * États de processus (J4)
 * ------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Running,
    Zombie,
}

#[derive(Clone, Copy)]
pub struct Process {
    pub ppid: u64,
    pub state: ProcState,
    pub exit_code: i64,
    pub task_id: usize,
}

static mut PROCESSES: [Option<Process>; 16] = [None; 16];
#[link_section = ".data"]
static mut NEXT_PID: u64 = 1;
static mut CURRENT_TASK_ID: usize = 0;
#[link_section = ".data"]
static mut CURRENT_PID: u64 = 1;
static mut CURRENT_CWD: [u8; 64] = [0; 64];

/// Tâche du processus userland courant (0 = noyau principal).
///
/// Mis à jour par `run_initrd_elf` à l'enregistrement du processus, et
/// par `sys_fork` pour l'enfant. Sert à sauvegarder le contexte avant
/// un `fork`.
pub static mut USER_TASK_ID: usize = 0;

/* ------------------------------------------------------------ *
 * fork / execve / wait4 / pipe / mmap (J4)
 * ------------------------------------------------------------ */

/// sys_fork() — crée un processus enfant (copie du contexte courant).
///
/// L'enfant reprend exactement où le parent était : même bloc GPR, même
/// pile noyau, même interrupt frame. Seul RAX diffère (0 chez l'enfant,
/// PID chez le parent). L'enfant est inséré dans le scheduler et
/// reprendra sur sa propre pile lors du prochain tick.
fn sys_fork(gpr_ptr: u64) -> i64 {
    // 1. Créer une nouvelle tâche dans le scheduler.
    let child_task_id = unsafe { task::create_task(fork_child_entry, 2) };
    if child_task_id >= task::MAX_TASKS {
        return -12; // -ENOMEM
    }

    // 2. Allouer un PID.
    let pid = unsafe { alloc_pid() };
    if pid == 0 {
        return -12;
    }

    // 3. Enregistrer le processus.
    unsafe {
        PROCESSES[child_task_id] = Some(Process {
            ppid: CURRENT_PID,
            state: ProcState::Running,
            exit_code: 0,
            task_id: child_task_id,
        });
    }

    // 4. Copier le contexte userland du parent dans l'enfant.
    //    Le parent est un userland : sa tâche porte un frame iretq
    //    (40 octets) au sommet de sa pile. On copie ce frame, puis on
    //    ajuste RAX à 0 pour l'enfant.
    unsafe {
        let current_task_id = if USER_TASK_ID != 0 {
            USER_TASK_ID
        } else {
            task::CURRENT
        };
        let current_task = &task::TASKS[current_task_id];
        let child_task = &mut task::TASKS[child_task_id];

        let current_rsp = current_task.saved_rsp;
        let current_stack_top =
            current_task.stack.as_ptr() as u64 + task::TASK_STACK_SIZE as u64;

        // Copier le frame iretq (40 octets) au sommet de la pile enfant.
        let child_stack_top =
            child_task.stack.as_ptr() as u64 + task::TASK_STACK_SIZE as u64;
        let child_rsp = child_stack_top - 40;

        core::ptr::copy_nonoverlapping(
            current_rsp as *const u8,
            child_rsp as *mut u8,
            40,
        );

        child_task.saved_rsp = child_rsp;
        child_task.priority = 2;
        child_task.age = 0;
        child_task.is_user = true;

        // Dans l'enfant, RAX = 0 (retour fork). RAX est le premier mot
        // du bloc GPR… mais ici le frame est un frame iretq, pas un bloc
        // GPR. Le retour fork est porté par le stub de reprise : on
        // place 0 dans le registre RAX via un champ dédié du frame.
        // Le plus simple : le frame iretq ne porte pas RAX. On utilise
        // donc une convention : le stub lit RAX dans le bloc GPR qui
        // suit le frame. On écrit 0 à cet emplacement.
        let child_gpr = child_rsp - task::GPR_SAVE_SIZE as u64;
        core::ptr::write_volatile(child_gpr as *mut u64, 0u64); // RAX = 0

        // Dans le parent, RAX = pid de l'enfant.
        set_gpr(gpr_ptr, SYS_RAX, pid as u64);
    }

    // 5. Retourner le PID enfant au parent.
    pid as i64
}

/// Point d'entrée de la tâche userland : démare le programme en ring 3.
///
/// Le frame iretq a été préparé par `register_user_task` au sommet de
/// la pile de la tâche. On saute au stub de reprise, qui fait `iretq`.
pub extern "C" fn user_task_entry() -> ! {
    unsafe {
        let task_id = task::CURRENT;
        let rsp = task::TASKS[task_id].saved_rsp;

        core::arch::asm!(
            "mov rsp, {0}",
            "jmp resume_ring3",
            in(reg) rsp,
            options(noreturn)
        );
    }
}

/// Point d'entrée de la tâche enfant après fork.
///
/// Le frame iretq a été copié par `sys_fork` depuis la tâche du
/// parent. On saute au stub de reprise.
pub extern "C" fn fork_child_entry() -> ! {
    unsafe {
        let child_task_id = task::CURRENT;
        let rsp = task::TASKS[child_task_id].saved_rsp;

        core::arch::asm!(
            "mov rsp, {0}",
            "jmp resume_ring3",
            in(reg) rsp,
            options(noreturn)
        );
    }
}

/* ------------------------------------------------------------
 * Enregistrement du processus courant dans le scheduler (J4-C)
 *
 * Les processus userland tournent sur la pile du boot, pas dans une
 * tâche du scheduler. Pour que `fork` puisse copier un contexte, il
 * faut que le processus courant ait sa propre pile de tâche et son
 * propre frame d'interruption.
 * ------------------------------------------------------------ */

/// Enregistre le contexte du processus courant dans sa tâche.
///
/// Appelé depuis `handle_syscall` au moment où un `syscall` entre en
/// ring 0 : le bloc GPR du processus est alors disponible dans
/// `gpr_ptr`, et l'interrupt frame (qui contient le RIP et le RSP
/// utilisateur) se trouve juste au-dessus. On copie le tout dans la
/// pile de la tâche, afin que `fork` puisse cloner un contexte complet.
///
/// `task_id` doit être l'identifiant de la tâche du processus courant.
pub unsafe fn save_user_context(task_id: usize, gpr_ptr: u64) {
    if task_id >= task::MAX_TASKS {
        return;
    }

    // Le RIP utilisateur est dans RCX (offset 16), le RSP dans R15
    // (offset 112) — sauvegardés par `syscall` dans le bloc GPR.
    let user_rip = core::ptr::read_volatile((gpr_ptr + 16) as *const u64);
    let user_rsp = core::ptr::read_volatile((gpr_ptr + 112) as *const u64);

    let task = &mut task::TASKS[task_id];
    let stack_top = task.stack.as_ptr() as u64 + task::TASK_STACK_SIZE as u64;

    // Frame iretq en haut de pile : [rip][cs][rflags][rsp][ss].
    let rsp = stack_top - 40;
    let frame = rsp as *mut u64;
    core::ptr::write_volatile(frame.add(0), user_rip);   // rip
    core::ptr::write_volatile(frame.add(1), 0x2B);       // cs = USER_CS|RPL3
    core::ptr::write_volatile(frame.add(2), 0x202);      // rflags = IF
    core::ptr::write_volatile(frame.add(3), user_rsp);   // rsp
    core::ptr::write_volatile(frame.add(4), 0x33);       // ss = USER_DS|RPL3

    task.saved_rsp = rsp;
    task.is_user = true;
}

/// Enregistre le processus userland courant comme tâche du scheduler.
///
/// Construit un frame d'interruption factice (rip = adresse de reprise,
/// cs = USER_CS, rflags = IF) sur la pile de la tâche, puis la rend
/// courante. Le `syscall` qui a déclenché l'enregistrement reprendra
/// sur ce frame.
pub unsafe fn register_user_task(task_id: usize, user_rip: u64, user_rsp: u64) {
    let task = &mut task::TASKS[task_id];
    let stack_top = task.stack.as_ptr() as u64 + task::TASK_STACK_SIZE as u64;

    // Frame iretq en haut de pile : [rip][cs][rflags][rsp][ss].
    let rsp = stack_top - 40;
    let frame = rsp as *mut u64;
    core::ptr::write_volatile(frame.add(0), user_rip);   // rip
    core::ptr::write_volatile(frame.add(1), 0x2B);       // cs = USER_CS|RPL3
    core::ptr::write_volatile(frame.add(2), 0x202);      // rflags = IF
    core::ptr::write_volatile(frame.add(3), user_rsp);   // rsp
    core::ptr::write_volatile(frame.add(4), 0x33);       // ss = USER_DS|RPL3

    task.saved_rsp = rsp;
    task.priority = 1;
    task.age = 0;
    task.is_user = true;
}

/// Charge un ELF depuis l'initrd et retourne (entry, stack_top).
///
/// Si le binaire a un `PT_INTERP`, l'interpréteur est chargé à son tour
/// et c'est son point d'entrée qui est retourné (avec `AT_BASE` dans
/// l'auxv). Sinon, c'est le point d'entrée du binaire lui-même.
unsafe fn load_from_initrd(name: &[u8], stack_top: u64) -> Option<(u64, u64)> {
    let (data, size) = crate::initrd::find(name)?;
    let elf_data = core::slice::from_raw_parts(data, size);
    let elf_info = crate::elf::load_elf(elf_data)?;

    // Interpréteur : charger ld-musl et démarrer sur son entrée.
    if elf_info.interp_name_len > 0 {
        let interp_name = &elf_info.interp_name[..elf_info.interp_name_len];
        let (idata, isize) = crate::initrd::find(interp_name)?;
        let idata_slice = core::slice::from_raw_parts(idata, isize);
        let interp_info = crate::elf::load_elf(idata_slice)?;

        // Base de l'interpréteur = premier segment LOAD.
        let interp_base = interp_info.load_base;
        let sp = crate::elf::build_stack(stack_top, 4, name_str(name), &elf_info, interp_base);
        return Some((interp_info.entry, sp));
    }

    // Binaire statique : on démarre directement sur son entrée.
    let sp = crate::elf::build_stack(stack_top, 4, name_str(name), &elf_info, 0);
    Some((elf_info.entry, sp))
}

/// Convertit un nom d'initrd en `&str` (NUL-terminé ou non).
fn name_str(name: &[u8]) -> &str {
    core::str::from_utf8(name).unwrap_or("<inv>")
}

/// sys_execve(filename, argv, envp) — remplace le processus courant
/// par un nouveau programme ELF.
fn sys_execve(filename: u64, _argv: u64, _envp: u64) -> i64 {
    // 1. Lire le nom de fichier depuis l'espace utilisateur.
    let name = unsafe { read_user_str(filename) };
    let len = name.iter().position(|&b| b == 0).unwrap_or(64);
    let name_slice = &name[..len];
    crate::console::print(format_args!(
        "sys_execve: name={:?} len={}\n",
        core::str::from_utf8(name_slice).unwrap_or("<inv>"),
        len
    ));

    // 2. Charger le programme (et son interpréteur le cas échéant).
    let (entry, stack_top) = match unsafe { load_from_initrd(name_slice, 0x100_0000) } {
        Some(v) => v,
        None => return -8, // -ENOEXEC
    };

    // 3. Sauter vers le point d'entrée (ne retourne pas).
    unsafe { crate::entry_ring3(entry, stack_top) };
}

/// sys_wait4(pid, wstatus, options) — attend la fin d'un enfant.
///
/// `pid = -1` : n'importe quel enfant. `pid > 0` : un enfant précis.
/// `wstatus` peut être NULL. Retourne le PID de l'enfant terminé, ou
/// -errno (-ECHILD si aucun enfant zombie).
fn sys_wait4(pid: i64, wstatus: u64, _options: u64) -> i64 {
    let current_pid = unsafe { CURRENT_PID };

    // Sélectionner l'enfant à attendre.
    let target: Option<usize> = unsafe {
        let mut found = None;
        for i in 0..PROCESSES.len() {
            if let Some(proc) = PROCESSES[i] {
                if proc.ppid == current_pid && proc.state == ProcState::Zombie {
                    if pid == -1 || proc.task_id as i64 == pid {
                        found = Some(i);
                        break;
                    }
                }
            }
        }
        found
    };

    match target {
        Some(i) => {
            let proc = unsafe { PROCESSES[i] }.unwrap();
            // Écrire le statut de sortie si le buffer est fourni.
            if wstatus != 0 && user_buf_ok(wstatus, 4) {
                unsafe {
                    let code = (proc.exit_code as i32) << 8;
                    core::ptr::write_volatile(wstatus as *mut i32, code);
                }
            }
            // Nettoyer le processus.
            unsafe {
                PROCESSES[i] = None;
            }
            proc.task_id as i64
        }
        None => -10, // -ECHILD
    }
}

/// sys_pipe(fildes) — crée un pipe anonyme.
/// fildes pointe vers un tableau de 2 entiers (read_fd, write_fd).
fn sys_pipe(fildes: u64) -> i64 {
    match unsafe { crate::vfs::pipe() } {
        Some((read_fd, write_fd)) => {
            unsafe {
                core::ptr::write_volatile(fildes as *mut u32, read_fd as u32);
                core::ptr::write_volatile((fildes + 4) as *mut u32, write_fd as u32);
            }
            0
        }
        None => -28, // -ENOSPC
    }
}

/// sys_mmap(addr, length, prot, flags, fd, offset) — projette de la mémoire.
///
/// J4-A : gère `MAP_ANONYMOUS` (fd=-1) et `MAP_PRIVATE`. Si `addr` est 0,
/// choisit une adresse libre dans l'espace utilisateur (à partir de
/// `MMAP_BASE`). Sinon mappe à l'adresse demandée (sans garantie
/// `MAP_FIXED` : on vérifie que la page est libre).
///
/// Protéctions : `PROT_READ`/`PROT_WRITE`/`PROT_EXEC` → drapeaux de page.
fn sys_mmap(addr: u64, length: u64, prot: u64, flags: u64, fd: i64, offset: u64) -> i64 {
    if length == 0 {
        return -22; // -EINVAL
    }

    // MAP_ANONYMOUS = 0x20 ; MAP_PRIVATE = 0x02.
    const MAP_PRIVATE: u64 = 0x02;
    const MAP_ANONYMOUS: u64 = 0x20;
    const MAP_FIXED: u64 = 0x10;

    let _ = MAP_PRIVATE; // accepté, pas de COW à ce stade

    // Adresse de base de l'espace mmap utilisateur (au-dessus du tas).
    const MMAP_BASE: u64 = 0x0000_2000_0000_0000;

    let num_pages = ((length + 0xFFF) & !0xFFF) / 0x1000;

    // Choisir l'adresse de départ.
    let base = if addr != 0 {
        // MAP_FIXED : l'appelant exige cette adresse.
        if flags & MAP_FIXED == 0 {
            // Sans MAP_FIXED, on mappe à l'adresse demandée seulement si
            // elle est libre ; sinon on choisit une adresse au-dessus.
            let mut free = true;
            for i in 0..num_pages {
                if unsafe { crate::mm::paging::translate(addr + i * 0x1000) }.is_some() {
                    free = false;
                    break;
                }
            }
            if free { addr } else { MMAP_BASE }
        } else {
            addr
        }
    } else {
        MMAP_BASE
    };

    // Allouer et mapper chaque page.
    let mut flags_page = crate::mm::paging::PRESENT;
    if prot & 0x2 != 0 {
        flags_page |= crate::mm::paging::WRITABLE;
    }
    if prot & 0x1 == 0 {
        flags_page |= crate::mm::paging::NOEXEC;
    }
    // L'espace utilisateur doit être accessible en ring 3.
    flags_page |= crate::mm::paging::USER;

    let mut paddrs: [u64; 64] = [0; 64];
    if num_pages > paddrs.len() as u64 {
        return -12; // -ENOMEM : zone trop grande pour ce stade
    }

    for i in 0..num_pages {
        let paddr = match crate::mm::phys::alloc_page() {
            Some(p) => p as u64,
            None => {
                // Libérer les pages déjà allouées.
                for j in 0..i {
                    unsafe { crate::mm::phys::free_page(paddrs[j as usize] as usize) };
                }
                return -12; // -ENOMEM
            }
        };
        paddrs[i as usize] = paddr;
    }

    for i in 0..num_pages {
        let vaddr = base + i * 0x1000;
        unsafe {
            crate::mm::paging::map_page(vaddr, paddrs[i as usize], flags_page);
        }
    }

    // Si une fd est fournie (sans MAP_ANONYMOUS), copier le contenu du
    // fichier à l'offset demandé. À ce stade, seuls les fichiers initrd
    // sont lisibles : on copie depuis l'inode.
    if fd >= 0 && flags & MAP_ANONYMOUS == 0 {
        if let Some((data, size)) = unsafe { crate::vfs::read_fd_all(fd as usize) } {
            let src_off = offset as usize;
            if src_off < size {
                let n = core::cmp::min(size - src_off, (num_pages * 0x1000) as usize);
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.add(src_off),
                        base as *mut u8,
                        n,
                    );
                }
            }
        }
    }

    base as i64
}

/* ------------------------------------------------------------ *
 * Helpers processus
 * ------------------------------------------------------------ */

unsafe fn alloc_pid() -> u64 {
    let pid = NEXT_PID;
    NEXT_PID += 1;
    pid
}

/* ------------------------------------------------------------
 * MSR read/write primitives (wrmsr/rdmsr)
 * ------------------------------------------------------------ */

/// Écrit `value` dans le MSR `msr`.
///
/// # Sécurité
///
/// Appel privilégié (CPL 0). Le code doit savoir que le MSR est valide
/// pour le CPU cible.
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nostack, preserves_flags)
    );
}

/// Lit la valeur du MSR `msr`.
///
/// # Sécurité
///
/// Appel privilégié (CPL 0).
#[allow(dead_code)]
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nostack, preserves_flags)
    );
    (low as u64) | ((high as u64) << 32)
}

/* ------------------------------------------------------------
 * Initialisation des MSRs — appelée une fois depuis kernel_main.
 * ------------------------------------------------------------ */

/// Initialise les MSRs pour `syscall`/`sysret` :
///   - IA32_EFER.SCE  : active l'instruction `syscall`
///   - IA32_STAR      : encode CS/SS noyau (0x08/0x10) et utilisateur (0x28/0x30)
///   - IA32_LSTAR     : pointe sur `syscall_entry` (isr.s)
///   - IA32_FMASK     : masque IF|TF afin que le handler s'exécute avec
///                     interruptions désactivées
///
/// # Sécurité
///
/// MSR write privileged — CPL 0 obligatoire.
pub unsafe fn init() {
    // On QEMU 8.x TCG, writing IA32_EFER (even the same value) invalidates
    // the QEMU-internal cache of the syscall MSRs (STAR/LSTART/FMASK).
    // Writing EFER.SCE FIRST, then STAR/LSTART/FMASK AFTER, ensures the
    // cache is populated after invalidation so both rdmsr reads and the
    // `syscall` instruction see correct values.
    let efer = rdmsr(MSR_EFER);

    // IA32_EFER : activer SCE (bit 0) — BEFORE les autres MSRs.
    wrmsr(MSR_EFER, efer | EFR_SCE);

    // IA32_STAR : [63:48]=kernel SS, [47:32]=kernel CS,
    //             [31:16]=user CS (SYSRET +2), [15:0]=user SS
    let star: u64 = (STAR_KERNEL_SS << 48)
        | (STAR_KERNEL_CS << 32)
        | (STAR_USER_CS << 16)
        | (STAR_USER_SS);
    wrmsr(MSR_STAR, star);

    // IA32_LSTAR : entry point du handler syscall.
    let lstar: u64 = syscall_entry as *const () as u64;
    wrmsr(MSR_LSTAR, lstar);

    // IA32_FMASK : bits RFLAGS à effacer par syscall (IF|TF).
    wrmsr(MSR_FMASK, FMASK);
}

// Pointeur vers le stub d'entrée en assembleur (isr.s).

extern "C" {
    fn syscall_entry();
}

// Drapeau global defini dans isr.s -- mis a 1 par handle_syscall
// pour que syscall_entry utilise iretq au lieu de sysret (retour R0).
extern "C" {
    static mut SYSCALL_TEST_DONE: u8;
    static mut EXIT_REQUESTED: u8;
}

// Fonction entree en anneau 0 apres le test syscall (J3.2).
// Definie dans main.rs, appelee via iretq deparis syscall_entry.
extern "C" {
    fn ring0_after_test();
    fn pf_test();
}

/* ------------------------------------------------------------
 * Layout du bloc GPR pour le handler syscall
 *
 * Le bloc est identique à celui de irq_common (15 × u64, 120 octets),
 * poussé par syscall_entry dans le même ordre : r15 → rax.
 *
 *   offset 0  : rax  (syscall number / return value)
 *   offset 8  : rbx
 *   offset 16 : rcx  (user RIP, saved by syscall)
 *   offset 24 : rdx  (arg3)
 *   offset 32 : rsi  (arg2)
 *   offset 40 : rdi  (arg1)
 *   offset 48 : rbp
 *   offset 56 : r8   (arg5)
 *   offset 64 : r9   (arg6)
 *   offset 72 : r10  (arg4)
 *   offset 80 : r11  (user RFLAGS, saved by syscall)
 *   offset 88 : r12
 *   offset 96 : r13
 *   offset 104: r14
 *   offset 112: r15
 * ------------------------------------------------------------ */

/// Lit un registre sauvegardé dans le bloc GPR.
unsafe fn gpr(gpr_ptr: u64, offset: usize) -> u64 {
    core::ptr::read_volatile((gpr_ptr + offset as u64) as *const u64)
}

/// Écrit une valeur dans un registre sauvegardé.
unsafe fn set_gpr(gpr_ptr: u64, offset: usize, value: u64) {
    core::ptr::write_volatile((gpr_ptr + offset as u64) as *mut u64, value);
}

/// Offsets dans le bloc GPR (doivent correspondre à l'ordre de push en ASM).
const SYS_RAX: usize = 0;
#[allow(dead_code)]
const SYS_RCX: usize = 16;  // user RIP (saved by syscall)
const SYS_RDX: usize = 24;  // arg3
const SYS_RSI: usize = 32;  // arg2
const SYS_RDI: usize = 40;  // arg1
#[allow(dead_code)]
const SYS_R8: usize = 56;   // arg5
#[allow(dead_code)]
const SYS_R9: usize = 64;   // arg6
#[allow(dead_code)]
const SYS_R10: usize = 72;  // arg4
#[allow(dead_code)]
const SYS_R11: usize = 80;  // user RFLAGS (saved by syscall)

/* ------------------------------------------------------------
 * Syscalls
 * ------------------------------------------------------------ */

/* ------------------------------------------------------------
 * Helpers user <-> kernel
 * ------------------------------------------------------------ */

/// Limite supérieure des buffers utilisateurs acceptés par les
/// syscalls (espace utilisateur "utilisé" du noyau : programmes ring3
/// en dessous de 4 GiB). Tout buffer dépassant cette frontière est
/// considéré comme invalide (-EFAULT) — CR-3.
const USER_BUF_LIMIT: u64 = 0x1_0000_0000;

/// Limite haute de l'espace utilisateur (au-delà : noyau).
///
/// L'espace utilisateur s'étend de 0 à `USER_SPACE_TOP` ; au-delà, les
/// adresses appartiennent au noyau (higher-half). Les buffers
/// utilisateurs doivent rester sous cette limite.
const USER_SPACE_TOP: u64 = 0x0000_8000_0000_0000;

/// Vérifie qu'un buffer `[addr, addr + len)` est entièrement dans
/// l'espace utilisateur. Renvoie false si le pointeur est invalide ou
/// si l'addition overflow (checked_add).
fn user_buf_ok(addr: u64, len: u64) -> bool {
    addr.checked_add(len).map_or(false, |end| end <= USER_SPACE_TOP)
}

/// sys_write(fd, buf, count) — écrit sur le port série pour fd=1.
///
/// # Sécurité
///
/// `buf` provient de l'anneau 3 : on vérifie qu'il est dans l'espace
/// utilisateur avant toute lecture noyau. Le descripteur est résolu via
/// le VFS (source de vérité des fds — CR-1) : fd=1 pointe sur /dev/tty,
/// qui écrit sur le port série.
fn sys_write(fd: u64, buf: u64, count: u64) -> i64 {
    if fd as usize >= MAX_FD {
        return -9; // -EBADF
    }
    if !user_buf_ok(buf, count) {
        return -14; // -EFAULT : buffer hors de l'espace utilisateur
    }
    unsafe {
        let n = crate::vfs::write(fd as usize, buf as *const u8, count as usize);
        n.map(|v| v as i64).unwrap_or(-9)
    }
}

/// sys_read(fd, buf, count) — lit depuis un descripteur ouvert.
///
/// Délègue à `vfs::read`, qui appelle le pilote approprié selon le
/// descripteur. Pour fd=0 (TTY), `dev_tty_read` lit sur le port série
/// et bloque tant qu'aucun octet n'est disponible.
fn sys_read(fd: u64, buf: u64, count: u64) -> i64 {
    let idx = fd as usize;
    if idx >= MAX_FD {
        return -9; // -EBADF
    }
    if !user_buf_ok(buf, count) {
        return -14; // -EFAULT : buffer hors de l'espace utilisateur
    }
    unsafe {
        let n = crate::vfs::read(idx, buf as *mut u8, count as usize);
        // W5 : propager le errno plutot que de tout replier en -EBADF.
        match n {
            Ok(v) => v as i64,
            Err(e) => e,
        }
    }
}

/// sys_close(fd) — ferme un descripteur fichier.
/// Pour fd < 3 (stdin/stdout/stderr), renvoie -EBADF.
/// Sinon ferme via le VFS (source de vérité des fds — CR-1).
fn sys_close(fd: u64) -> i64 {
    if fd < 3 {
        -9 // -EBADF
    } else if fd as usize >= MAX_FD {
        -9 // -EBADF
    } else {
        unsafe { crate::vfs::close(fd as usize) };
        0
    }
}

/// sys_brk(addr) — définit ou interroge le point d'ancre du tas (heap).
///
/// addr=0 : renvoie l'adresse courante (query).
/// addr!=0 : accepte le nouvel addr (pas de vraie gestion mémoire pour l'instant).
fn sys_brk(addr: u64) -> i64 {
    if addr == 0 {
        0x6000_0000  // adresse de base du tas utilisateur (user-space, pas noyau)
    } else {
        addr as i64
    }
}

/// sys_lseek(fd, offset, whence) — repositionne le curseur d'un fd.
fn sys_lseek(fd: u64, offset: i64, whence: u32) -> i64 {
    if fd as usize >= MAX_FD {
        return -9; // -EBADF
    }
    match unsafe { crate::vfs::lseek(fd as usize, offset, whence) } {
        Ok(pos) => pos,
        Err(e) => e,
    }
}

/// sys_mprotect(addr, len, prot) — change les protections d'une zone.
///
/// Version J4-A : applique les drapeaux `PRESENT`/`WRITABLE`/`NOEXEC`
/// sur les pages de la zone. `PROT_READ` seul retire `WRITABLE`.
fn sys_mprotect(addr: u64, len: u64, prot: u64) -> i64 {
    if addr == 0 || len == 0 {
        return -22; // -EINVAL
    }
    if addr % 0x1000 != 0 {
        return -22; // -EINVAL : addr doit être aligné page
    }
    if !user_buf_ok(addr, len) {
        return -14; // -EFAULT
    }

    let num_pages = ((len + 0xFFF) & !0xFFF) / 0x1000;
    let mut flags = crate::mm::paging::PRESENT;
    if prot & 0x2 != 0 {
        flags |= crate::mm::paging::WRITABLE;
    }
    if prot & 0x1 == 0 {
        flags |= crate::mm::paging::NOEXEC;
    }

    for i in 0..num_pages {
        let vaddr = addr + i * 0x1000;
        // On ne peut pas changer les drapeaux d'une page sans connaître
        // son adresse physique : on lit l'entrée existante.
        let paddr = match unsafe { crate::mm::paging::translate(vaddr) } {
            Some(p) => p,
            None => return -12, // -ENOMEM : page absente
        };
        unsafe { crate::mm::paging::map_page(vaddr, paddr, flags) };
    }
    0
}

/// sys_munmap(addr, len) — libère une zone mappée.
fn sys_munmap(addr: u64, len: u64) -> i64 {
    if addr == 0 || len == 0 || addr % 0x1000 != 0 {
        return -22; // -EINVAL
    }
    if !user_buf_ok(addr, len) {
        return -14; // -EFAULT
    }

    let num_pages = ((len + 0xFFF) & !0xFFF) / 0x1000;
    for i in 0..num_pages {
        let vaddr = addr + i * 0x1000;
        if let Some(paddr) = unsafe { crate::mm::paging::translate(vaddr) } {
            unsafe {
                crate::mm::paging::unmap_page(vaddr);
                crate::mm::phys::free_page(paddr as usize);
            }
        }
    }
    0
}

/// sys_uname(buf) — remplit une structure `struct utsname` (6 × 65 octets).
///
/// Layout Linux x86-64 : sysname, nodename, release, version, machine,
/// domainname — chacun 65 octets, NUL-terminés.
fn sys_uname(buf: u64) -> i64 {
    if !user_buf_ok(buf, 390) {
        return -14; // -EFAULT
    }

    let fields: [&[u8]; 6] = [
        b"XILI",          // sysname
        b"xili",          // nodename
        b"0.1.0",         // release
        b"#1",            // version
        b"x86_64",        // machine
        b"(none)",        // domainname
    ];

    unsafe {
        core::ptr::write_bytes(buf as *mut u8, 0, 390);
        for (i, field) in fields.iter().enumerate() {
            let dst = (buf as *mut u8).add(i * 65);
            core::ptr::copy_nonoverlapping(field.as_ptr(), dst, field.len().min(64));
        }
    }
    0
}

/// sys_getdents64(fd, buf, count) — lit des entrées de répertoire.
fn sys_getdents64(fd: u64, buf: u64, count: u64) -> i64 {
    if fd as usize >= MAX_FD {
        return -9; // -EBADF
    }
    if !user_buf_ok(buf, count) {
        return -14; // -EFAULT
    }
    match unsafe { crate::vfs::getdents64(fd as usize, buf as *mut u8, count as usize) } {
        Ok(n) => n as i64,
        Err(e) => e,
    }
}

/* ------------------------------------------------------------
 * futex — verrou userspace (J4-B)
 *
 * Opérations minimales pour musl :
 *   FUTEX_WAIT (0)      : dort si *uaddr == val
 *   FUTEX_WAKE (1)      : réveille jusqu'à val attentes
 *   FUTEX_WAIT_BITSET (9)  : idem WAIT avec masque de bits
 *   FUTEX_WAKE_BITSET (10) : idem WAKE avec masque de bits
 *
 * Le noyau est mono-tâche : un WAIT ne peut pas bloquer le CPU. On
 * implémente une attente coopérative : si la valeur ne correspond pas,
 * on rend la main immédiatement avec -EAGAIN (musl gère ce cas en
 * réessayant). Le WAKE réveille les attentes enregistrées.
 * ------------------------------------------------------------ */

const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;

const FUTEX_PRIVATE_FLAG: u32 = 128;
const FUTEX_CLOCK_REALTIME: u32 = 256;

/// Nombre maximal d'attentes futex enregistrées.
const FUTEX_MAX_WAITERS: usize = 16;

/// Une attente futex enregistrée.
#[derive(Clone, Copy)]
struct FutexWaiter {
    /// Adresse du mot de 32 bits surveillé.
    uaddr: u64,
    /// Valeur attendue.
    val: u32,
    /// Tâche (PID) en attente, 0 = slot libre.
    pid: u64,
}

static mut FUTEX_WAITERS: [FutexWaiter; FUTEX_MAX_WAITERS] = [
    FutexWaiter { uaddr: 0, val: 0, pid: 0 };
    FUTEX_MAX_WAITERS
];

/// sys_futex(uaddr, op, val, timeout, uaddr2, val3) — opérations futex.
fn sys_futex(uaddr: u64, op: u32, val: u32, _timeout: u64, _uaddr2: u64, val3: u32) -> i64 {
    if uaddr == 0 || uaddr % 4 != 0 {
        return -22; // -EINVAL
    }
    if !user_buf_ok(uaddr, 4) {
        return -14; // -EFAULT
    }

    // Les drapeaux privés/real-time sont acceptés mais sans effet.
    let op_base = op & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

    match op_base {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            // Lire la valeur courante du mot.
            let cur = unsafe { core::ptr::read_volatile(uaddr as *const u32) };

            // Attendu : FUTEX_WAIT_BITSET compare sur 32 bits avec le
            // masque val3 ; FUTEX_WAIT compare l'égalité simple.
            let matches = if op_base == FUTEX_WAIT_BITSET {
                // BITSET : le masque sélectionne les bits à comparer.
                (cur ^ val) & val3 == 0
            } else {
                cur == val
            };

            if !matches {
                // La valeur a déjà changé : pas besoin d'attendre.
                return 0;
            }

            // Enregistrer l'attente (le noyau est mono-tâche : on ne
            // dort pas vraiment, on rend la main et musl réessaie).
            let pid = unsafe { CURRENT_PID };
            unsafe {
                for w in FUTEX_WAITERS.iter_mut() {
                    if w.pid == 0 {
                        w.uaddr = uaddr;
                        w.val = val;
                        w.pid = pid;
                        break;
                    }
                }
            }
            // -EAGAIN : musl réessaie immédiatement (spin).
            -11
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => {
            // Réveiller jusqu'à `val` attentes sur cette adresse.
            let mut woken = 0u32;
            unsafe {
                for w in FUTEX_WAITERS.iter_mut() {
                    if w.pid != 0 && w.uaddr == uaddr && woken < val {
                        w.pid = 0;
                        w.uaddr = 0;
                        woken += 1;
                    }
                }
            }
            woken as i64
        }
        _ => -38, // -ENOSYS : opération non implémentée
    }
}

/* ------------------------------------------------------------
 * ioctl — contrôle des descripteurs (J4-B)
 *
 * Seuls les ioctl TTY sont implémentés à ce stade :
 *   TCGETS (0x5401)  : lit les attributs du terminal (termios)
 *   TCSETS (0x5402)  : écrit les attributs
 *   TIOCGWINSZ (0x5413) : lit la taille de la fenêtre (winsize)
 * ------------------------------------------------------------ */

const TCGETS: u64 = 0x5401;
const TCSETS: u64 = 0x5402;
const TIOCGWINSZ: u64 = 0x5413;

/// `struct termios` Linux x86-64 (60 octets).
#[repr(C)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 19],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

/// `struct winsize` (8 octets).
#[repr(C)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// Valeurs termios par défaut (équivalent `stty sane` simplifié).
const DEFAULT_TERMIOS: Termios = Termios {
    c_iflag: 0x0000_250B, // IGNBRK|BRKINT|ISTRIP|ICRNL|IXON
    c_oflag: 0x0000_0005, // OPOST|ONLCR
    c_cflag: 0x0000_00BF, // CS8|CREAD
    c_lflag: 0x0000_850B, // ISIG|ICANON|ECHO|ECHOE|ECHOK|IEXTEN
    c_line: 0,
    c_cc: [
        0x03, 0x1C, 0x7F, 0x15, 0x04, 0x00, 0x01, 0xFF,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00,
    ],
    c_ispeed: 0x0000_000F, // B38400
    c_ospeed: 0x0000_000F,
};

/// sys_ioctl(fd, request, arg) — contrôle d'un descripteur.
fn sys_ioctl(fd: u64, request: u64, arg: u64) -> i64 {
    if fd as usize >= MAX_FD {
        return -9; // -EBADF
    }

    match request {
        TCGETS => {
            if !user_buf_ok(arg, core::mem::size_of::<Termios>() as u64) {
                return -14; // -EFAULT
            }
            unsafe {
                core::ptr::write_volatile(arg as *mut Termios, DEFAULT_TERMIOS);
            }
            0
        }
        TCSETS => {
            // Accepté sans effet : le terminal est en mode canonique.
            0
        }
        TIOCGWINSZ => {
            if !user_buf_ok(arg, core::mem::size_of::<Winsize>() as u64) {
                return -14; // -EFAULT
            }
            // 80 × 24 : la console VGA.
            let ws = Winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
            unsafe {
                core::ptr::write_volatile(arg as *mut Winsize, ws);
            }
            0
        }
        _ => -25, // -ENOTTY : ioctl inconnu
    }
}

/* ------------------------------------------------------------
 * signaux (J4-B)
 *
 * Seul SIGKILL (9) est traité : il termine le processus. Les autres
 * signaux sont acceptés mais sans effet (pas de livraison réelle).
 * ------------------------------------------------------------ */

const SIGKILL: u64 = 9;
const SIGTERM: u64 = 15;
const SIGINT: u64 = 2;
const SIGCHLD: u64 = 17;

/// sys_rt_sigaction(signum, act, oldact) — installe un gestionnaire.
///
/// Version minimale : accepte l'appel et mémorise le gestionnaire pour
/// SIGCHLD. Pas de livraison réelle (le noyau ne lève pas de signaux).
fn sys_rt_sigaction(signum: u64, act: u64, oldact: u64) -> i64 {
    if signum == 0 || signum > 64 {
        return -22; // -EINVAL
    }
    if oldact != 0 && !user_buf_ok(oldact, 24) {
        return -14; // -EFAULT
    }
    if oldact != 0 {
        // Ancien gestionnaire : SIG_DFL (0).
        unsafe { core::ptr::write_bytes(oldact as *mut u8, 0, 24) };
    }
    if act != 0 && !user_buf_ok(act, 24) {
        return -14; // -EFAULT
    }
    let _ = signum;
    0
}

/// sys_rt_sigprocmask(how, set, oldset) — masque de signaux.
fn sys_rt_sigprocmask(_how: u64, set: u64, oldset: u64) -> i64 {
    if oldset != 0 {
        if !user_buf_ok(oldset, 8) {
            return -14; // -EFAULT
        }
        unsafe { core::ptr::write_volatile(oldset as *mut u64, 0) };
    }
    if set != 0 && !user_buf_ok(set, 8) {
        return -14; // -EFAULT
    }
    0
}

/// sys_kill(pid, sig) — envoie un signal à un processus.
fn sys_kill(pid: u64, sig: u64) -> i64 {
    if pid == 0 {
        return -22; // -EINVAL
    }
    // Seul SIGKILL est réellement traité : le processus visé devient
    // zombie s'il est connu.
    if sig == SIGKILL || sig == SIGTERM || sig == SIGINT {
        unsafe {
            for i in 0..PROCESSES.len() {
                if let Some(proc) = PROCESSES[i] {
                    if proc.task_id as u64 == pid && proc.state == ProcState::Running {
                        PROCESSES[i] = Some(Process {
                            state: ProcState::Zombie,
                            exit_code: 0,
                            ..proc
                        });
                        return 0;
                    }
                }
            }
        }
        return -3; // -ESRCH
    }
    // SIGCHLD et les autres : acceptés sans effet.
    0
}

/// sys_tgkill(tgid, tid, sig) — kill ciblé par thread.
fn sys_tgkill(tgid: u64, _tid: u64, sig: u64) -> i64 {
    sys_kill(tgid, sig)
}

/// sys_getrandom(buf, count, flags) — générateur aléatoire.
///
/// Remplit avec des octets dérivés du compteur de ticks et du PIT.
fn sys_getrandom(buf: u64, count: u64, _flags: u64) -> i64 {
    if !user_buf_ok(buf, count) {
        return -14; // -EFAULT
    }
    unsafe {
        let ticks = crate::irq::ticks();
        for i in 0..count as usize {
            let b = crate::io::inb(0x40);
            let mixed = b ^ ((ticks >> (i % 8)) as u8);
            core::ptr::write_volatile((buf as *mut u8).add(i), mixed);
        }
    }
    count as i64
}

/// sys_memfd_create(name, flags) — fichier anonyme en mémoire.
///
/// Version minimale : renvoie un descripteur factice (pas de fichier
/// réel). Accepté pour que musl ne échoue pas.
fn sys_memfd_create(name: u64, _flags: u64) -> i64 {
    if name != 0 && !user_buf_ok(name, 64) {
        return -14; // -EFAULT
    }
    // Ouvre un inode /dev/zero comme substitut.
    match unsafe { crate::vfs::lookup(b"zero") } {
        Some(inode) => unsafe {
            match crate::vfs::open(inode, None) {
                Some(fd) => fd as i64,
                None => -28, // -ENOSPC
            }
        },
        None => -2, // -ENOENT
    }
}

/// sys_exit(code) — termine le processus utilisateur ELF (J3.3).
///
/// Le handler fixe EXIT_REQUESTED (1) et RCX (-> pf_test) pour que
/// syscall_entry transfère le contrôle en R0 via jmp *%rcx (même
/// mécanisme que SYS_TEST_DONE, mais déclenché par sys_exit au lieu
/// de 0x1FF). RAX = 0 (valeur de retour inutilisée — sysret n'est
/// pas utilisé, le contrôle passe directement vers pf_test).
fn sys_exit(gpr_ptr: u64, code: u64) -> i64 {
    crate::console::print(format_args!(
        "\nJ3.3: processus ELF termine (exit code {})...\n",
        code
    ));
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(EXIT_REQUESTED), 1u8);
        set_gpr(gpr_ptr, SYS_RCX, pf_test as *const () as u64);
        // W3 : RFLAGS utilisateur = 0 (IF=0) — le bit 1 (TF) n'est pas un
        // masque d'interruption.
        set_gpr(gpr_ptr, SYS_R11, 0x0);
    }
    0
}

/// sys_getpid() — retourne le PID courant.
fn sys_getpid() -> i64 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CURRENT_PID)) as i64 }
}

/// sys_getcwd() — retourne le répertoire courant.
pub fn sys_getcwd(buf: u64, size: u64) -> i64 {
    if !user_buf_ok(buf, size) {
        return -14; // -EFAULT : buffer user invalide
    }
    unsafe {
        let cwd = core::ptr::addr_of!(CURRENT_CWD) as *const u8;
        let len = core::ptr::read_volatile(core::ptr::addr_of!(CURRENT_CWD))
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(0);
        let copy_len = core::cmp::min(len as u64, size);
        core::ptr::copy_nonoverlapping(cwd, buf as *mut u8, copy_len as usize);
        // écrire le NUL terminal si pas déjà présent
        if copy_len < size {
            *((buf + copy_len) as *mut u8) = 0;
        }
        (copy_len + 1) as i64
    }
}

/// sys_chdir() — change le répertoire courant.
pub fn sys_chdir(path: u64) -> i64 {
    if !user_buf_ok(path, 64) {
        return -14; // -EFAULT : chemin user invalide
    }
    unsafe {
        let mut buf = [0u8; 64];
        for i in 0..63 {
            let byte = core::ptr::read_volatile((path + i as u64) as *const u8);
            if byte == 0 { break; }
            buf[i] = byte;
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(0);
        if len == 0 || len >= 64 { return -34; }  // -ENAMETOOLONG
        let cwd = core::ptr::addr_of_mut!(CURRENT_CWD) as *mut u8;
        core::ptr::copy_nonoverlapping(buf.as_ptr(), cwd, len + 1);
    }
    0
}

/// sys_dup() — duplique un descripteur de fichier vers le premier fd
/// libre >= 3 (implémentation via le VFS — CR-1/CR-4).
pub fn sys_dup(oldfd: u64) -> i64 {
    match unsafe { crate::vfs::dup(oldfd as usize) } {
        Ok(fd) => fd as i64,
        Err(e) => e,
    }
}

/// sys_dup2() — duplique oldfd vers newfd.
/// Ferme `newfd` s'il était ouvert (implémentation via le VFS — CR-1/CR-4).
pub fn sys_dup2(oldfd: u64, newfd: u64) -> i64 {
    match unsafe { crate::vfs::dup2(oldfd as usize, newfd as usize) } {
        Ok(_) => newfd as i64,
        Err(e) => e,
    }
}

/// sys_gettid() — retourne le TID courant (= PID en l'absence de threads).
fn sys_gettid() -> i64 {
    sys_getpid()
}

/// Lit une chaîne NUL-terminée depuis l'espace utilisateur.
/// `ptr` doit être validé (user_buf_ok) avant l'appel — CR-3/W4.
unsafe fn read_user_str(ptr: u64) -> [u8; 64] {
    let mut buf = [0u8; 64];
    for i in 0..63 {
        let byte = core::ptr::read_volatile((ptr + i as u64) as *const u8);
        if byte == 0 {
            break;
        }
        buf[i] = byte;
    }
    buf
}

/// sys_openat(dirfd, pathname, flags, mode) — ouvre un fichier.
///
/// Résout d'abord dans le VFS (fichiers /dev, /proc, répertoires), puis
/// dans l'initrd. Retourne le descripteur, ou -errno.
fn sys_openat(_dirfd: i64, pathname: u64, _flags: u64, _mode: u64) -> i64 {
    let name = unsafe { read_user_str(pathname) };
    let len = name.iter().position(|&b| b == 0).unwrap_or(64);
    let name_slice = &name[..len];

    // 1. VFS : /dev, /proc, répertoires.
    if let Some(inode) = unsafe { crate::vfs::lookup(name_slice) } {
        return unsafe {
            match crate::vfs::open(inode, None) {
                Some(fd) => fd as i64,
                None => -28, // -ENOSPC
            }
        };
    }

    // 2. Initrd : fichiers réguliers.
    match crate::initrd::find(name_slice) {
        Some((data, size)) => {
            let inode = crate::vfs::Inode {
                file_type: crate::vfs::FileType::Regular,
                minor: 0,
                data: data as *const u8,
                size,
            };
            unsafe {
                if let Some(fd) = crate::vfs::open(inode, None) {
                    fd as i64
                } else {
                    -28 // -ENOSPC
                }
            }
        }
        None => -2, // -ENOENT
    }
}

/// sys_newfstatat(dirfd, pathname, statbuf, flags) — stat via le VFS.
fn sys_newfstatat(_dirfd: i64, pathname: u64, statbuf: u64, _flags: u64) -> i64 {
    let name = unsafe { read_user_str(pathname) };
    let len = name.iter().position(|&b| b == 0).unwrap_or(64);
    let name_slice = &name[..len];

    match unsafe { crate::vfs::lookup(name_slice) } {
        Some(inode) => {
            // CR-3 : le buffer stat user est valide avant écriture noyau.
            if !user_buf_ok(statbuf, 144) {
                return -14; // -EFAULT
            }
            unsafe {
                core::ptr::write_bytes(statbuf as *mut u8, 0, 144);
                let mode = match inode.file_type {
                    crate::vfs::FileType::Regular => 0x81A4,   // S_IFREG | 0644
                    crate::vfs::FileType::CharDev => 0x21A4,   // S_IFCHR | 0644
                    crate::vfs::FileType::Directory => 0x41ED, // S_IFDIR | 0755
                    crate::vfs::FileType::Pipe => 0x81A4,      // S_IFIFO | 0644
                    crate::vfs::FileType::ProcFile => 0x81A4,  // S_IFIFO | 0644
                };
                core::ptr::write_volatile((statbuf + 16) as *mut u32, mode);
                core::ptr::write_volatile((statbuf + 40) as *mut u64, inode.size as u64);
            }
            0
        }
        None => -2, // -ENOENT
    }
}

/* ------------------------------------------------------------
 * Handler principal — appelé par syscall_entry (isr.s)
 * ------------------------------------------------------------ */

/// Point d'entrée du handler syscall.
///
/// `gpr_ptr` pointe vers le bloc GPR de 15 registres (120 octets)
/// sauvegardé par `syscall_entry`. Le handler :
///   1. Lit le numéro de syscall (RAX à offset 0)
///   2. Lit les arguments (RDI/RSI/RDX/R10/R8/R9)
///   3. Appelle le syscall correspondant
///   4. Écrit la valeur de retour dans RAX (offset 0)
///
/// Pour `exit`/`exit_group` (J3.3), le handler fixe EXIT_REQUESTED + RCX
/// (-> pf_test) pour un retour en R0 via jmp *%rcx. RAX = 0 (inutilisé).
/// Pour `SYS_TEST_DONE` (J3.2), le handler modifie RCX (-> ring0_after_test) et
/// fixe le drapeau SYSCALL_TEST_DONE pour un retour en anneau 0 via
/// `jmp *%rcx` (iretq non utilisable sous QEMU TCG 8.2 : CS chargé de
/// STAR[47:32] = KERNEL_DS 0x10 au lieu de STAR[63:48] = KERNEL_CS 0x38).
/// Pour les autres, `sysret` restaure RCX (user RIP) et R11 (user RFLAGS),
/// puis retourne en anneau 3.
#[no_mangle]
pub extern "C" fn handle_syscall(gpr_ptr: u64) {
    let nr = unsafe { gpr(gpr_ptr, SYS_RAX) } as u64;
    let arg1 = unsafe { gpr(gpr_ptr, SYS_RDI) } as u64;
    let arg2 = unsafe { gpr(gpr_ptr, SYS_RSI) } as u64;
    let arg3 = unsafe { gpr(gpr_ptr, SYS_RDX) } as u64;
    let arg4 = unsafe { gpr(gpr_ptr, SYS_R10) };
    let arg5 = unsafe { gpr(gpr_ptr, SYS_R8) };
    // W6 : l'offset de mmap est le 6e argument (registre R9 pour la
    // convention syscall), pas le 5e (R8).
    let arg6 = unsafe { gpr(gpr_ptr, SYS_R9) };

    // J4-C : sauvegarder le contexte du processus dans sa tâche, pour
    // que `fork` puisse le cloner. Seulement si un processus userland
    // est enregistré (USER_TASK_ID != 0).
    unsafe {
        let user_task = USER_TASK_ID;
        if user_task != 0 {
            save_user_context(user_task, gpr_ptr);
        }
    }

    let ret: i64 = match nr {
        SYS_READ => sys_read(arg1, arg2, arg3),
        SYS_WRITE => sys_write(arg1, arg2, arg3),
        SYS_OPEN => -38,  // -ENOSY (deprecated, use SYS_OPENAT)
        SYS_OPENAT => sys_openat(arg1 as i64, arg2, arg3, arg4),
        SYS_CLOSE => sys_close(arg1),
        SYS_BRK => sys_brk(arg1),
        SYS_GETPID => sys_getpid(),
        SYS_GETTID => sys_gettid(),
        SYS_MMAP => sys_mmap(arg1, arg2, arg3, arg4, arg5 as i64, arg6),
        SYS_NEWFSTATAT => sys_newfstatat(arg1 as i64, arg2, arg3, arg4),
        SYS_FORK => sys_fork(gpr_ptr),
        SYS_EXECVE => sys_execve(arg1, arg2, arg3),
        SYS_WAIT4 => sys_wait4(arg1 as i64, arg2, arg3),
        SYS_GETCWD => sys_getcwd(arg1 as u64, arg2 as u64),
        SYS_CHDIR => sys_chdir(arg1 as u64),
        SYS_DUP => sys_dup(arg1 as u64),
        SYS_DUP2 => sys_dup2(arg1 as u64, arg2 as u64),
        SYS_PIPE => sys_pipe(arg1),
        SYS_LSEEK => sys_lseek(arg1, arg2 as i64, arg3 as u32),
        SYS_MPROTECT => sys_mprotect(arg1, arg2, arg3),
        SYS_MUNMAP => sys_munmap(arg1, arg2),
        SYS_UNAME => sys_uname(arg1),
        SYS_GETDENTS64 => sys_getdents64(arg1, arg2, arg3),
        SYS_FUTEX => sys_futex(arg1, arg2 as u32, arg3 as u32, arg4, arg5, arg6 as u32),
        SYS_IOCTL => sys_ioctl(arg1, arg2, arg3),
        SYS_RT_SIGACTION => sys_rt_sigaction(arg1, arg2, arg3),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(arg1, arg2, arg3),
        SYS_KILL => sys_kill(arg1, arg2),
        SYS_TGKILL => sys_tgkill(arg1, arg2, arg3),
        SYS_GETRANDOM => sys_getrandom(arg1, arg2, arg3),
        SYS_MEMFD_CREATE => sys_memfd_create(arg1, arg2),
        SYS_EXIT | SYS_EXIT_GROUP => {
            sys_exit(gpr_ptr, arg1)
        }
        SYS_TEST_DONE => {
            // Le handler imprime le marqueur J3.2, modifie RCX (-> ring0_after_test)
            // et R11 (-> RFLAGS), puis fixe le drapeau SYSCALL_TEST_DONE.
            // syscall_entry utilise iretq au lieu de sysret pour revenir en R0.
            crate::console::print(format_args!(
                "J3.2: syscall/sysret + MSRs (LSTAR/STAR/SFMASK/EFER) + GDT R0/R3 - OK\n"
            ));
            unsafe {
                core::ptr::write_volatile(core::ptr::addr_of_mut!(SYSCALL_TEST_DONE), 1u8);
                set_gpr(gpr_ptr, SYS_RCX, ring0_after_test as *const () as u64);
                // W3 : RFLAGS utilisateur = 0 (IF=0).
                set_gpr(gpr_ptr, SYS_R11, 0x0);
            }
            0
        }
        _ => -38, // -ENOSYS
    };

    // Écrire la valeur de retour dans le slot RAX.
    unsafe { set_gpr(gpr_ptr, SYS_RAX, ret as u64) };
}
