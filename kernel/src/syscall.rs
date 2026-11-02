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

/// Descripteurs fichiers — 3 emplacements pour les fichiers initrd.
/// fd 0/1/2 = stdin/stdout/stderr (réservés), fd 3+ = fichiers cpio.
const MAX_FD: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
struct FileDesc {
    data: *const u8,
    size: usize,
    pos: usize,
}

static mut FD_TABLE: [FileDesc; MAX_FD] = [FileDesc {
    data: core::ptr::null(),
    size: 0,
    pos: 0,
}; MAX_FD];

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

/* ------------------------------------------------------------
 * Compteur de PID statique.
 * ------------------------------------------------------------ */

static mut CURRENT_PID: u64 = 1;

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

    // IA32_STAR : [63:48]=kernel CS, [47:32]=kernel SS,
    //             [31:16]=user CS,  [15:0]=user SS
    let star: u64 = (STAR_KERNEL_CS << 48)
        | (STAR_KERNEL_SS << 32)
        | (STAR_USER_CS << 16)
        | (STAR_USER_SS);
    wrmsr(MSR_STAR, star);

    // IA32_LSTAR : entry point du handler syscall.
    let lstar: u64 = syscall_entry as *const () as u64;
    wrmsr(MSR_LSTAR, lstar);

    // IA32_FMASK : bits RFLAGS à effacer par syscall (IF|TF).
    wrmsr(MSR_FMASK, FMASK);

    // Debug: read back MSRs and print to verify
    let star_read = rdmsr(MSR_STAR);
    let lstar_read = rdmsr(MSR_LSTAR);
    let fmask_read = rdmsr(MSR_FMASK);
    let efer_read = rdmsr(MSR_EFER);
    crate::console::print(format_args!(
        "J3.2 DEBUG: STAR={:#018x} LSTAR={:#018x} FMASK={:#018x} EFER={:#018x}\n",
        star_read, lstar_read, fmask_read, efer_read
    ));
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

/// sys_write(fd, buf, count) — écrit sur le port série pour fd=1.
///
/// # Sécurité
///
/// `buf` provient de l'anneau 3. Ici, le noyau et l'utilisateur partagent
/// la même identity map (0..1 Gio), donc l'accès est sûr tant que
/// l'adresse est dans la RAM disponible.
fn sys_write(fd: u64, buf: u64, count: u64) -> i64 {
    match fd {
        1 => {
            for i in 0..count {
                let byte =
                    unsafe { core::ptr::read_volatile((buf + i) as *const u8) };
                crate::serial::write_byte(byte);
            }
            count as i64
        }
        _ => -9, // -EBADF
    }
}

/// sys_read(fd, buf, count) — lit depuis l'entrée standard (fd=0) ou
/// depuis un fichier initrd (fd 3+).
///
/// Pour fd=0, tente de lire depuis le port série (COM1) de façon
/// non bloquante. Si aucune données disponible, renvoie 0 (EOF).
/// Pour fd >= 3, lit depuis le fichier identifié dans FD_TABLE.
/// Pour fd < 3 (autre que stdin), renvoie -EBADF.
fn sys_read(fd: u64, buf: u64, count: u64) -> i64 {
    match fd {
        0 => {
            let mut bytes: u64 = 0;
            for i in 0..count {
                let status = crate::io::inb(crate::serial::COM1 + 5);
                if status & 0x01 == 0 {
                    break;  // pas de données disponibles
                }
                let byte = crate::io::inb(crate::serial::COM1);
                unsafe {
                    core::ptr::write_volatile((buf + i) as *mut u8, byte);
                }
                bytes += 1;
            }
            bytes as i64
        }
        fd => {
            // fd >= 3 : lecture depuis un fichier initrd (J3.5)
            let idx = fd as usize;
            if idx >= MAX_FD {
                -9 // -EBADF
            } else {
                unsafe {
                    if FD_TABLE[idx].data.is_null() {
                        -9 // -EBADF — fd non ouvert
                    } else {
                        let data = FD_TABLE[idx].data;
                        let size = FD_TABLE[idx].size;
                        let pos = FD_TABLE[idx].pos;
                        if pos >= size {
                            0 // EOF
                        } else {
                            let n = core::cmp::min(
                                count as usize,
                                size - pos,
                            );
                            if n == 0 {
                                0
                            } else {
                                for i in 0..n {
                                    let byte = core::ptr::read_volatile(
                                        data.wrapping_add(pos + i),
                                    );
                                    core::ptr::write_volatile(
                                        (buf + i as u64) as *mut u8,
                                        byte,
                                    );
                                }
                                FD_TABLE[idx].pos = pos + n;
                                n as i64
                            }
                        }
                    }
                }
            }
        }
    }
}

/// sys_close(fd) — ferme un descripteur fichier.
/// Pour fd < 3 (stdin/stdout/stderr), renvoie -EBADF.
/// Sinon renvoie 0 (aucun descripteur réel à fermer).
fn sys_close(fd: u64) -> i64 {
    if fd < 3 {
        -9 // -EBADF
    } else if fd as usize >= MAX_FD {
        -9 // -EBADF
    } else {
        unsafe {
            FD_TABLE[fd as usize] = FileDesc {
                data: core::ptr::null(),
                size: 0,
                pos: 0,
            };
        }
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
        set_gpr(gpr_ptr, SYS_R11, 0x2);
    }
    0
}

/// sys_getpid() — retourne le PID courant.
fn sys_getpid() -> i64 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(CURRENT_PID)) as i64 }
}

/// sys_gettid() — retourne le TID courant (= PID en l'absence de threads).
fn sys_gettid() -> i64 {
    sys_getpid()
}

/// Lit une chaîne NUL-terminée depuis l'espace utilisateur.
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

/// sys_openat(dirfd, pathname, flags, mode) — ouvre un fichier initrd.
fn sys_openat(_dirfd: i64, pathname: u64, _flags: u64, _mode: u64) -> i64 {
    let name = unsafe { read_user_str(pathname) };
    let len = name.iter().position(|&b| b == 0).unwrap_or(64);
    let name_slice = &name[..len];

    match crate::initrd::find(name_slice) {
        Some((data, size)) => {
            let idx = unsafe {
                let mut fd: Option<usize> = None;
                for i in 3..MAX_FD {
                    if FD_TABLE[i].data.is_null() {
                        fd = Some(i);
                        break;
                    }
                }
                fd
            };
            match idx {
                Some(i) => {
                    unsafe {
                        FD_TABLE[i] = FileDesc { data, size, pos: 0 };
                    }
                    i as i64
                }
                None => -28, // -ENOSPC
            }
        }
        None => -2, // -ENOENT
    }
}

/// sys_newfstatat(dirfd, pathname, statbuf, flags) — stat sur un fichier.
fn sys_newfstatat(_dirfd: i64, pathname: u64, statbuf: u64, _flags: u64) -> i64 {
    let name = unsafe { read_user_str(pathname) };
    let len = name.iter().position(|&b| b == 0).unwrap_or(64);
    let name_slice = &name[..len];

    match crate::initrd::find(name_slice) {
        Some((_, size)) => {
            unsafe {
                // Stat buffer x86-64 glibc : 144 octets
                core::ptr::write_bytes(statbuf as *mut u8, 0, 144);
                // st_mode @ offset 16 : S_IFREG | 0644 = 0x81A4
                core::ptr::write_volatile((statbuf + 16) as *mut u32, 0x81A4);
                // st_size @ offset 40
                core::ptr::write_volatile((statbuf + 40) as *mut u64, size as u64);
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
    let nr = unsafe { gpr(gpr_ptr, SYS_RAX) };
    let arg1 = unsafe { gpr(gpr_ptr, SYS_RDI) };
    let arg2 = unsafe { gpr(gpr_ptr, SYS_RSI) };
    let arg3 = unsafe { gpr(gpr_ptr, SYS_RDX) };
    let arg4 = unsafe { gpr(gpr_ptr, SYS_R10) };

    let ret: i64 = match nr {
        SYS_READ => sys_read(arg1, arg2, arg3),
        SYS_WRITE => sys_write(arg1, arg2, arg3),
        SYS_OPEN => -38,  // -ENOSY (deprecated, use SYS_OPENAT)
        SYS_OPENAT => sys_openat(arg1 as i64, arg2, arg3, arg4),
        SYS_CLOSE => sys_close(arg1),
        SYS_BRK => sys_brk(arg1),
        SYS_GETPID => sys_getpid(),
        SYS_GETTID => sys_gettid(),
        SYS_MMAP => -38,  // -ENOSY (not implemented)
        SYS_NEWFSTATAT => sys_newfstatat(arg1 as i64, arg2, arg3, arg4),
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
                set_gpr(gpr_ptr, SYS_R11, 0x2);
            }
            0
        }
        _ => -38, // -ENOSYS
    };

    // Écrire la valeur de retour dans le slot RAX.
    unsafe { set_gpr(gpr_ptr, SYS_RAX, ret as u64) };
}
