/* ============================================================
 * vfs.rs — système de fichiers virtuel minimal (J3.4/J3.5)
 *
 * Surface active conservée :
 *   - initrd (cpio newc) via initrd::find()
 *   - /dev : null, zero, random, tty (inodes devices directs)
 *   - /proc minimal (J4)
 *   - pipes anonymes (J4)
 *   - open/close/read/write/dup/dup2 par descripteur
 * ============================================================ */

use crate::initrd;
use core::cmp::min;

/* ------------------------------------------------------------ *
 * Types de fichiers et inodes
 * ------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Regular,
    CharDev,
    Directory,
    Pipe,
    ProcFile,
    Epoll,
    EventFd,
    SignalFd,
    TimerFd,
    UdpSocket,
}

#[derive(Clone, Copy)]
pub struct Inode {
    pub file_type: FileType,
    pub minor: u32,
    pub data: *const u8,
    pub size: usize,
}

impl Inode {
    pub const fn empty() -> Self {
        Inode {
            file_type: FileType::Regular,
            minor: 0,
            data: core::ptr::null(),
            size: 0,
        }
    }
}

/* ------------------------------------------------------------ *
 * Descripteur de fichier ouvert
 * ------------------------------------------------------------ */

#[derive(Clone, Copy)]
pub struct FileDesc {
    pub inode: Inode,
    pub pos: usize,
}

/* ------------------------------------------------------------ *
 * Tables globales
 * ------------------------------------------------------------ */

const MAX_FILES: usize = 64;

// NOTE (J3 hardening) : les tables VFS (`static mut`) restent en
// `static mut`, et non en `UnsafeCell`. Le noyau est volontairement
// single-thread (scheduler sur timer IRQ + hlt, aucune exécution
// parallèle), et l'expérience `UnsafeCell` a montré que l'usage
// actuel (indexation directe / `get_mut`) ne compile pas avec les
// patterns voulus ici. Le compromis `static mut` + accès unique via
// des fonctions verrouillées par le noyau (pas de reentrancy) reste
// le pattern adopté ; le durcissement se fera au niveau des ops de
// fichiers quand le VFS sera multi-tâches (J4).
static mut OPEN_FILES: [Option<FileDesc>; MAX_FILES] = [None; MAX_FILES];

/* ------------------------------------------------------------ *
 * Initialisation du VFS
 * ------------------------------------------------------------ */

pub unsafe fn init() {
    crate::console::print(format_args!("vfs : pret\n"));
    init_dir_entries();
}

/* ------------------------------------------------------------ *
 * Lookup d'un chemin vers un Inode.
 *
 * Chemins supportés :
 *   - chemins absolus (strip leading '/')
 *   - un seul composant de nom
 *   - initrd, /dev (direct), /proc minimal
 * ------------------------------------------------------------ */

pub unsafe fn lookup(name: &[u8]) -> Option<Inode> {
    let path = if !name.is_empty() && name[0] == b'/' {
        &name[1..]
    } else {
        name
    };

    if path.is_empty() {
        return Some(Inode {
            file_type: FileType::Directory,
            minor: 0,
            data: core::ptr::null(),
            size: 0,
        });
    }

    // 1. Chercher dans l'initrd.
    if let Some((data, size)) = initrd::find(path) {
        return Some(Inode {
            file_type: FileType::Regular,
            minor: 0,
            data,
            size,
        });
    }

    // 2. /dev : inodes devices directs (pas de table CharDev/FileOps).
    // Le chemin peut être "dev/null" (absolu) ou "null" (relatif).
    let dev_name: &[u8] = if path.starts_with(b"dev/") { &path[4..] } else { path };
    match dev_name {
        b"null" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                minor: 3,
                data: core::ptr::null(),
                size: 0,
            });
        }
        b"zero" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                minor: 5,
                data: core::ptr::null(),
                size: 0,
            });
        }
        b"random" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                minor: 8,
                data: core::ptr::null(),
                size: 0,
            });
        }
        b"tty" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                minor: 4,
                data: core::ptr::null(),
                size: 0,
            });
        }
        b"urandom" => {
            return Some(Inode { file_type: FileType::CharDev, minor: 9, data: core::ptr::null(), size: 0 });
        }
        b"ptmx" | b"console" => {
            return Some(Inode { file_type: FileType::CharDev, minor: 4, data: core::ptr::null(), size: 0 });
        }
        b"pts/0" => {
            return Some(Inode { file_type: FileType::CharDev, minor: 10, data: core::ptr::null(), size: 0 });
        }
        b"pts" => {
            return Some(Inode { file_type: FileType::Directory, minor: 2, data: core::ptr::null(), size: 0 });
        }
        _ => {}
    }

    // 3. /proc minimal (J4).
    // Le chemin peut etre "proc", "proc/self", "proc/meminfo", etc.
    if path == b"proc" {
        return Some(Inode {
            file_type: FileType::Directory,
            minor: 0,
            data: core::ptr::null(),
            size: 0,
        });
    }
    if path == b"dev" {
        return Some(Inode {
            file_type: FileType::Directory,
            minor: 1,
            data: core::ptr::null(),
            size: 0,
        });
    }
    if path.starts_with(b"proc/") {
        let proc_path = &path[5..];
        match proc_path {
            b"self/exe" => {
                return Some(new_proc_file("/proc/self/exe", proc_self_exe_read));
            }
            b"self/fd" => {
                return Some(new_proc_file("/proc/self/fd", proc_self_fd_read));
            }
            b"self/maps" => {
                return Some(new_proc_file("/proc/self/maps", proc_self_maps_read));
            }
            b"meminfo" => {
                return Some(new_proc_file("/proc/meminfo", proc_meminfo_read));
            }
            b"self/stat" => {
                return Some(new_proc_file("/proc/self/stat", proc_self_stat_read));
            }
            b"self/status" => {
                return Some(new_proc_file("/proc/self/status", proc_self_status_read));
            }
            b"self/cmdline" => {
                return Some(new_proc_file("/proc/self/cmdline", proc_self_cmdline_read));
            }
            _ => {}
        }
    }

    None
}

/* ------------------------------------------------------------ *
 * /proc minimal (J4)
 * ------------------------------------------------------------ */

static mut PROC_OPS: [Option<fn(*const u8, usize, usize, *const FileDesc) -> usize>; 8] =
    [None; 8];

unsafe fn new_proc_file(
    _path: &'static str,
    read: fn(*const u8, usize, usize, *const FileDesc) -> usize,
) -> Inode {
    let mut slot = None;
    for i in 0..PROC_OPS.len() {
        if PROC_OPS[i].is_none() {
            slot = Some(i);
            break;
        }
    }
    let idx = match slot {
        Some(i) => i,
        None => return Inode::empty(),
    };
    PROC_OPS[idx] = Some(read);
    Inode {
        file_type: FileType::ProcFile,
        minor: idx as u32,
        data: core::ptr::null(),
        size: 0,
    }
}

unsafe fn proc_ops(minor: u32) -> Option<fn(*const u8, usize, usize, *const FileDesc) -> usize> {
    let idx = minor as usize;
    if idx < PROC_OPS.len() {
        PROC_OPS[idx]
    } else {
        None
    }
}

fn proc_self_exe_read(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let line = b"/busybox\n";
    let n = min(count, line.len());
    for i in 0..n {
        unsafe { core::ptr::write_volatile((_buf as *mut u8).add(i), line[i]) };
    }
    n
}

fn proc_self_fd_read(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let line = b"0:/dev/tty\n1:/dev/tty\n2:/dev/tty\n";
    let n = min(count, line.len());
    for i in 0..n {
        unsafe { core::ptr::write_volatile((_buf as *mut u8).add(i), line[i]) };
    }
    n
}

fn proc_self_maps_read(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    // Carte mémoire du processus courant : on lit la PML4 pour lister
    // les segments présents. Format Linux : adresse-debut-fin perms
    // offset dev inode chemin.
    let buf = _buf as *mut u8;
    let mut written = 0usize;
    let mut line = [0u8; 128];

    // Segment texte (0x400000-0x401000) et data (0x401000-0x402000)
    // du programme courant — valeurs typiques des binaires XILI.
    let entries: [(u64, u64, &[u8], &[u8]); 3] = [
        (0x0000_0000_0040_0000, 0x0000_0000_0040_1000, b"r--p", b""),
        (0x0000_0000_0040_1000, 0x0000_0000_0040_2000, b"r-xp", b""),
        (0x0000_0000_0040_2000, 0x0000_0000_0040_3000, b"rw-p", b""),
    ];

    for (start, end, perms, path) in entries.iter() {
        let mut n = 0usize;
        n += hex64(&mut line[n..], *start);
        line[n] = b'-';
        n += 1;
        n += hex64(&mut line[n..], *end);
        line[n] = b' ';
        n += 1;
        line[n..n + 4].copy_from_slice(perms);
        n += 4;
        line[n] = b' ';
        n += 1;
        n += hex32(&mut line[n..], 0);
        line[n] = b' ';
        n += 1;
        line[n..n + 2].copy_from_slice(b"00");
        n += 2;
        line[n] = b':';
        n += 1;
        line[n..n + 2].copy_from_slice(b"00");
        n += 2;
        line[n] = b' ';
        n += 1;
        n += hex32(&mut line[n..], 0);
        line[n] = b' ';
        n += 1;
        if !path.is_empty() {
            line[n..n + path.len()].copy_from_slice(path);
            n += path.len();
        }
        line[n] = b'\n';
        n += 1;

        if written + n > count {
            break;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(line.as_ptr(), buf.add(written), n);
        }
        written += n;
    }

    written
}

/// Écrit un entier en hexadécimal minuscule (sans 0x) dans `buf`.
fn hex64(buf: &mut [u8], v: u64) -> usize {
    let mut n = 0usize;
    let mut started = false;
    for i in (0..16).rev() {
        let d = ((v >> (i * 4)) & 0xF) as u8;
        if d != 0 || started || i == 0 {
            buf[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            n += 1;
            started = true;
        }
    }
    n
}

/// Écrit un entier 32 bits en hexadécimal (8 chiffres, sans 0x).
fn hex32(buf: &mut [u8], v: u32) -> usize {
    let mut n = 0usize;
    for i in (0..8).rev() {
        let d = ((v >> (i * 4)) & 0xF) as u8;
        buf[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        n += 1;
    }
    n
}

fn proc_meminfo_read(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let line = b"MemTotal:       128 MB\n";
    let n = min(count, line.len());
    for i in 0..n {
        unsafe { core::ptr::write_volatile((_buf as *mut u8).add(i), line[i]) };
    }
    n
}

// --- /proc/self/{stat,status,cmdline} (J4-D) ---
// Le pid/exe sont lus via syscall (source de vérité CURRENT_PID/EXE).
fn proc_self_stat_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    // Format stat(5) : pid (comm) state ppid pgrp session tty_nr tpgid flags ...
    let pid = crate::syscall::current_pid();
    let exe = crate::syscall::current_exe_name();
    let mut line = [0u8; 256];
    let mut n = 0usize;
    let put = |line: &mut [u8], n: &mut usize, s: &[u8]| {
        for &b in s { if *n < line.len() { line[*n] = b; *n += 1; } }
    };
    // pid + " (" + exe + ") R 0 0 0 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"
    let mut num = [0u8; 20]; let mut ni = 0usize; let mut v = pid;
    if v == 0 { num[ni] = b'0'; ni += 1; } else { let mut tmp = [0u8; 20]; let mut ti = 0; while v > 0 { tmp[ti] = b'0' + (v % 10) as u8; v /= 10; ti += 1; } while ti > 0 { ti -= 1; num[ni] = tmp[ti]; ni += 1; } }
    put(&mut line, &mut n, &num[..ni]);
    put(&mut line, &mut n, b" (");
    put(&mut line, &mut n, &exe);
    put(&mut line, &mut n, b") R 0 0 0 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
    let m = min(count, n);
    for i in 0..m { unsafe { core::ptr::write_volatile((buf as *mut u8).add(i), line[i]) }; }
    m
}

fn proc_self_status_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let pid = crate::syscall::current_pid();
    let exe = crate::syscall::current_exe_name();
    let mut line = [0u8; 512];
    let mut n = 0usize;
    let mut push = |line: &mut [u8], n: &mut usize, s: &[u8]| { for &b in s { if *n < line.len() { line[*n] = b; *n += 1; } } };
    let mut num = [0u8; 20]; let mut ni = 0usize; let mut v = pid;
    if v == 0 { num[ni] = b'0'; ni += 1; } else { let mut tmp = [0u8; 20]; let mut ti = 0; while v > 0 { tmp[ti] = b'0' + (v % 10) as u8; v /= 10; ti += 1; } while ti > 0 { ti -= 1; num[ni] = tmp[ti]; ni += 1; } }
    push(&mut line, &mut n, b"Name:\t"); push(&mut line, &mut n, &exe);
    push(&mut line, &mut n, b"\nState:\tR (running)\nPid:\t"); push(&mut line, &mut n, &num[..ni]);
    push(&mut line, &mut n, b"\nPPid:\t0\nVmSize:\t8192 kB\nVmRSS:\t8192 kB\n");
    let m = min(count, n);
    for i in 0..m { unsafe { core::ptr::write_volatile((buf as *mut u8).add(i), line[i]) }; }
    m
}

fn proc_self_cmdline_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let exe = crate::syscall::current_exe_name();
    let mut n = min(count, exe.len());
    for i in 0..n { unsafe { core::ptr::write_volatile((buf as *mut u8).add(i), exe[i]) }; }
    if n < count && n < exe.len() + 1 { /* cmdline se termine par NUL */ }
    // NUL final si place
    if n < count { unsafe { core::ptr::write_volatile((buf as *mut u8).add(n), 0) }; n += 1; }
    n
}


/* ------------------------------------------------------------ *
 * Pipes anonymes (J4)
 * ------------------------------------------------------------ */

const PIPE_BUF: usize = 4096;

#[derive(Clone, Copy)]
pub struct Pipe {
    pub buf: *mut u8,
    pub len: usize,
    pub cap: usize,
    pub rpos: usize,
    pub wpos: usize,
    pub readers: usize,
    pub writers: usize,
}

static mut PIPES: [Option<Pipe>; 32] = [None; 32];
static mut PIPE_COUNT: usize = 0;

/// Crée un pipe anonyme et renvoie (read_fd, write_fd) ou None.
pub unsafe fn pipe() -> Option<(usize, usize)> {
    let mut read_fd = None;
    let mut write_fd = None;
    for i in 3..MAX_FILES {
        if OPEN_FILES[i].is_none() {
            if read_fd.is_none() {
                read_fd = Some(i);
            } else if write_fd.is_none() {
                write_fd = Some(i);
                break;
            }
        }
    }
    let (read_fd, write_fd) = match (read_fd, write_fd) {
        (Some(r), Some(w)) => (r, w),
        _ => return None,
    };

    let pipe_idx = PIPE_COUNT;
    if pipe_idx >= PIPES.len() {
        return None;
    }
    PIPE_COUNT += 1;

    let buf = crate::mm::slab::kmalloc(PIPE_BUF);
    if buf.is_null() {
        return None;
    }
    core::ptr::write_bytes(buf, 0, PIPE_BUF);

    let pipe = Pipe {
        buf,
        len: 0,
        cap: PIPE_BUF,
        rpos: 0,
        wpos: 0,
        readers: 1,
        writers: 1,
    };
    PIPES[pipe_idx] = Some(pipe);

    let inode = Inode {
        file_type: FileType::Pipe,
        minor: pipe_idx as u32,
        data: core::ptr::null(),
        size: 0,
    };

    OPEN_FILES[read_fd] = Some(FileDesc { inode, pos: 0 });
    OPEN_FILES[write_fd] = Some(FileDesc { inode, pos: 0 });

    Some((read_fd, write_fd))
}

unsafe fn pipe_read(fd: usize, buf: *mut u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let binding = unsafe { OPEN_FILES[fd] };
    let file = match binding {
        Some(ref f) => f,
        None => return Err(-9),
    };
    let pipe_idx = file.inode.minor as usize;
    let pipe = match unsafe { PIPES.get_mut(pipe_idx) } {
        Some(Some(ref mut p)) => p,
        _ => return Err(-19),
    };

    if pipe.len == 0 {
        if pipe.writers == 0 {
            return Ok(0);
        }
        return Err(-11);
    }

    let n = min(count, pipe.len);
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(pipe.buf.add(pipe.rpos)) };
        unsafe { core::ptr::write_volatile(buf.add(i), b) };
        pipe.rpos = (pipe.rpos + 1) % pipe.cap;
    }
    pipe.len -= n;
    Ok(n)
}

unsafe fn pipe_write(fd: usize, buf: *const u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let binding = unsafe { OPEN_FILES[fd] };
    let file = match binding {
        Some(ref f) => f,
        None => return Err(-9),
    };
    let pipe_idx = file.inode.minor as usize;
    let pipe = match unsafe { PIPES.get_mut(pipe_idx) } {
        Some(Some(ref mut p)) => p,
        _ => return Err(-19),
    };

    let free = pipe.cap - pipe.len;
    let n = min(count, free);
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(buf.add(i)) };
        unsafe { core::ptr::write_volatile(pipe.buf.add(pipe.wpos), b) };
        pipe.wpos = (pipe.wpos + 1) % pipe.cap;
    }
    pipe.len += n;
    Ok(n)
}

unsafe fn pipe_close(fd: usize) {
    if fd >= MAX_FILES {
        return;
    }
    let file = match unsafe { OPEN_FILES[fd] } {
        Some(f) => f,
        None => return,
    };
    let pipe_idx = file.inode.minor as usize;
    if let Some(Some(ref mut pipe)) = unsafe { PIPES.get_mut(pipe_idx) } {
        if fd == file.inode.minor as usize % 2 {
            pipe.readers -= 1;
        } else {
            pipe.writers -= 1;
        }
        if pipe.readers == 0 && pipe.writers == 0 {
            if !pipe.buf.is_null() {
                unsafe { crate::mm::slab::kfree(pipe.buf) };
            }
            unsafe { PIPES[pipe_idx] = None };
        }
    }
}

/* ------------------------------------------------------------ *
 * Objets d'attente (J4-D) : eventfd, signalfd, timerfd, epoll.
 * ------------------------------------------------------------ */

pub const MAX_EPOLL: usize = 8;
pub const MAX_EVENTFD: usize = 16;
pub const MAX_SIGNALFD: usize = 16;
pub const MAX_TIMERFD: usize = 16;

#[derive(Clone, Copy)]
pub struct EpollEntry { pub fd: usize, pub events: u32, pub data: u64, pub used: bool }
#[derive(Clone, Copy)]
pub struct Epoll { pub entries: [EpollEntry; 16], pub count: usize }
const EMPTY_EPOLL: Epoll = Epoll { entries: [EpollEntry { fd: 0, events: 0, data: 0, used: false }; 16], count: 0 };

static mut EPOLLS: [Epoll; MAX_EPOLL] = [EMPTY_EPOLL; MAX_EPOLL];
static mut EVENTFDS: [u64; MAX_EVENTFD] = [0; MAX_EVENTFD];
static mut SIGNALFDS: [(u64, u32); MAX_SIGNALFD] = [(0, 0); MAX_SIGNALFD];
#[derive(Clone, Copy)]
pub struct TimerFd { pub expiry: u64, pub interval: u64, pub expirations: u64 }
static mut TIMERFDS: [TimerFd; MAX_TIMERFD] = [TimerFd { expiry: 0, interval: 0, expirations: 0 }; MAX_TIMERFD];

unsafe fn alloc_fd(inode: Inode) -> Option<usize> {
    for i in 3..MAX_FILES {
        if OPEN_FILES[i].is_none() {
            OPEN_FILES[i] = Some(FileDesc { inode, pos: 0 });
            return Some(i);
        }
    }
    None
}

pub unsafe fn epoll_create() -> Option<usize> {
    for i in 0..MAX_EPOLL {
        if EPOLLS[i].count == 0 && OPEN_FILES.iter().all(|f| f.is_none() || f.as_ref().unwrap().inode.file_type != FileType::Epoll || f.as_ref().unwrap().inode.minor as usize != i) {
            EPOLLS[i] = EMPTY_EPOLL;
            return alloc_fd(Inode { file_type: FileType::Epoll, minor: i as u32, data: core::ptr::null(), size: 0 });
        }
    }
    None
}
pub unsafe fn eventfd_create() -> Option<usize> {
    for i in 0..MAX_EVENTFD {
        let used = OPEN_FILES.iter().any(|f| matches!(f, Some(fd) if fd.inode.file_type == FileType::EventFd && fd.inode.minor as usize == i));
        if !used { EVENTFDS[i] = 0; return alloc_fd(Inode { file_type: FileType::EventFd, minor: i as u32, data: core::ptr::null(), size: 0 }); }
    }
    None
}
pub unsafe fn signalfd_create(mask: u64) -> Option<usize> {
    for i in 0..MAX_SIGNALFD {
        let used = OPEN_FILES.iter().any(|f| matches!(f, Some(fd) if fd.inode.file_type == FileType::SignalFd && fd.inode.minor as usize == i));
        if !used { SIGNALFDS[i] = (mask, 0); return alloc_fd(Inode { file_type: FileType::SignalFd, minor: i as u32, data: core::ptr::null(), size: 0 }); }
    }
    None
}
pub unsafe fn timerfd_create() -> Option<usize> {
    for i in 0..MAX_TIMERFD {
        let used = OPEN_FILES.iter().any(|f| matches!(f, Some(fd) if fd.inode.file_type == FileType::TimerFd && fd.inode.minor as usize == i));
        if !used { TIMERFDS[i] = TimerFd { expiry: 0, interval: 0, expirations: 0 }; return alloc_fd(Inode { file_type: FileType::TimerFd, minor: i as u32, data: core::ptr::null(), size: 0 }); }
    }
    None
}

pub unsafe fn epoll_ctl(epfd: usize, op: u32, fd: usize, events: u32, data: u64) -> i64 {
    if epfd >= MAX_FILES { return -9; }
    let ep = match OPEN_FILES[epfd] { Some(ref f) if f.inode.file_type == FileType::Epoll => f.inode.minor as usize, _ => return -9 };
    if fd >= MAX_FILES || OPEN_FILES[fd].is_none() { return -9; }
    let e = &mut EPOLLS[ep];
    match op {
        1 => { // ADD
            for en in e.entries.iter_mut() { if !en.used { en.fd = fd; en.events = events; en.data = data; en.used = true; e.count += 1; return 0; } }
            -28
        }
        2 => { // DEL
            for en in e.entries.iter_mut() { if en.used && en.fd == fd { en.used = false; e.count -= 1; return 0; } }
            -2
        }
        3 => { // MOD
            for en in e.entries.iter_mut() { if en.used && en.fd == fd { en.events = events; en.data = data; return 0; } }
            -2
        }
        _ => -22,
    }
}

pub unsafe fn fd_ready(fd: usize) -> bool {
    if fd >= MAX_FILES { return false; }
    let f = match OPEN_FILES[fd] { Some(ref f) => f, None => return false };
    match f.inode.file_type {
        FileType::EventFd => EVENTFDS[f.inode.minor as usize] != 0,
        FileType::SignalFd => SIGNALFDS[f.inode.minor as usize].1 != 0,
        FileType::TimerFd => {
            let tf = TIMERFDS[f.inode.minor as usize];
            tf.expiry != 0 && crate::syscall::ko_ticks() >= tf.expiry
        }
        FileType::Pipe => match PIPES.get(f.inode.minor as usize) { Some(Some(pp)) => pp.len > 0, _ => false },
        FileType::UdpSocket => { let i = f.inode.minor as usize; UDP_SOCKS[i].rx_len > 0 }
        FileType::Regular | FileType::CharDev | FileType::ProcFile => true,
        _ => false,
    }
}

/// Lit les événements prêts d'un epoll fd vers `buf` (12 o chacun),
/// renvoie le nombre d'événements (0 si aucun).
pub unsafe fn epoll_wait(epfd: usize, buf: *mut u8, maxevents: usize) -> usize {
    if epfd >= MAX_FILES { return 0; }
    let ep_idx = match OPEN_FILES[epfd] { Some(ref f) if f.inode.file_type == FileType::Epoll => f.inode.minor as usize, _ => return 0 };
    let mut n = 0usize;
    // Copie des entrées (évite l'emprunt pendant fd_ready).
    let entries = EPOLLS[ep_idx].entries;
    for en in entries.iter() {
        if n >= maxevents { break; }
        if en.used && fd_ready(en.fd) {
            let dst = buf.add(n * 12);
            core::ptr::write_unaligned(dst as *mut u32, en.events);
            core::ptr::write_unaligned(dst.add(4) as *mut u64, en.data);
            n += 1;
        }
    }
    n
}

pub unsafe fn timerfd_settime(fd: usize, value_ticks: u64, interval_ticks: u64) {
    if fd >= MAX_FILES { return; }
    if let Some(ref f) = OPEN_FILES[fd] {
        if f.inode.file_type == FileType::TimerFd {
            let i = f.inode.minor as usize;
            if value_ticks == 0 { TIMERFDS[i].expiry = 0; }
            else { TIMERFDS[i].expiry = crate::syscall::ko_ticks() + value_ticks; }
            TIMERFDS[i].interval = interval_ticks;
            TIMERFDS[i].expirations = 0;
        }
    }
}

pub unsafe fn signalfd_notify(sig: u64) {
    for i in 0..MAX_SIGNALFD {
        if SIGNALFDS[i].0 & (1u64 << (sig % 64)) != 0 && SIGNALFDS[i].1 == 0 {
            SIGNALFDS[i].1 = sig as u32;
        }
    }
}


/* ------------------------------------------------------------ *
 * Sockets UDP (J5-B) : table de sockets AF_INET/SOCK_DGRAM.
 * ------------------------------------------------------------ */

pub const MAX_UDP_SOCKS: usize = 8;

#[derive(Clone, Copy)]
pub struct UdpSock {
    pub bound_port: u16,
    pub peer_ip: [u8; 4],
    pub peer_port: u16,
    pub connected: bool,
    pub rx_len: usize,
    pub rx_from_ip: [u8; 4],
    pub rx_from_port: u16,
    pub rx_buf: [u8; 1024],
}
const EMPTY_UDP: UdpSock = UdpSock { bound_port: 0, peer_ip: [0; 4], peer_port: 0, connected: false, rx_len: 0, rx_from_ip: [0; 4], rx_from_port: 0, rx_buf: [0; 1024] };
static mut UDP_SOCKS: [UdpSock; MAX_UDP_SOCKS] = [EMPTY_UDP; MAX_UDP_SOCKS];

pub unsafe fn socket_create() -> Option<usize> {
    for i in 0..MAX_UDP_SOCKS {
        let used = OPEN_FILES.iter().any(|f| matches!(f, Some(fd) if fd.inode.file_type == FileType::UdpSocket && fd.inode.minor as usize == i));
        if !used {
            UDP_SOCKS[i] = EMPTY_UDP;
            UDP_SOCKS[i].bound_port = 12345; // port éphémère par défaut
            return alloc_fd(Inode { file_type: FileType::UdpSocket, minor: i as u32, data: core::ptr::null(), size: 0 });
        }
    }
    None
}

pub unsafe fn socket_fd_ok(fd: usize) -> bool {
    fd < MAX_FILES && matches!(OPEN_FILES[fd], Some(ref f) if f.inode.file_type == FileType::UdpSocket)
}

pub unsafe fn socket_bind(fd: usize, port: u16) -> bool {
    if !socket_fd_ok(fd) { return false; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    UDP_SOCKS[i].bound_port = port;
    true
}

pub unsafe fn socket_connect(fd: usize, ip: [u8; 4], port: u16) -> bool {
    if !socket_fd_ok(fd) { return false; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    UDP_SOCKS[i].peer_ip = ip;
    UDP_SOCKS[i].peer_port = port;
    UDP_SOCKS[i].connected = true;
    true
}

pub unsafe fn socket_peer(fd: usize) -> Option<([u8; 4], u16)> {
    if !socket_fd_ok(fd) { return None; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    Some((UDP_SOCKS[i].peer_ip, UDP_SOCKS[i].peer_port))
}

pub unsafe fn socket_local_port(fd: usize) -> u16 {
    if !socket_fd_ok(fd) { return 0; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    UDP_SOCKS[i].bound_port
}

pub unsafe fn socket_queue(fd: usize, from_ip: [u8; 4], from_port: u16, data: &[u8]) {
    if !socket_fd_ok(fd) { return; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    let n = core::cmp::min(data.len(), UDP_SOCKS[i].rx_buf.len());
    UDP_SOCKS[i].rx_buf[..n].copy_from_slice(&data[..n]);
    UDP_SOCKS[i].rx_len = n;
    UDP_SOCKS[i].rx_from_ip = from_ip;
    UDP_SOCKS[i].rx_from_port = from_port;
}

pub unsafe fn socket_take(fd: usize, buf: *mut u8, count: usize) -> usize {
    if !socket_fd_ok(fd) { return 0; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    let n = core::cmp::min(count, UDP_SOCKS[i].rx_len);
    if n > 0 {
        core::ptr::copy_nonoverlapping(UDP_SOCKS[i].rx_buf.as_ptr(), buf, n);
        UDP_SOCKS[i].rx_len = 0;
    }
    n
}

pub unsafe fn socket_rx_info(fd: usize) -> Option<([u8; 4], u16, usize)> {
    if !socket_fd_ok(fd) { return None; }
    let i = OPEN_FILES[fd].as_ref().unwrap().inode.minor as usize;
    Some((UDP_SOCKS[i].rx_from_ip, UDP_SOCKS[i].rx_from_port, UDP_SOCKS[i].rx_len))
}

pub unsafe fn socket_count() -> usize { MAX_UDP_SOCKS }
pub unsafe fn socket_by_index(i: usize) -> Option<usize> {
    if i >= MAX_UDP_SOCKS { return None; }
    for fd in 0..MAX_FILES {
        if let Some(ref f) = OPEN_FILES[fd] {
            if f.inode.file_type == FileType::UdpSocket && f.inode.minor as usize == i {
                return Some(fd);
            }
        }
    }
    None
}

/* ------------------------------------------------------------ *
 * Devices : read/write implémentés directement ici.
 * ------------------------------------------------------------ */

fn dev_null_read(_buf: *const u8, _count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    0
}

fn dev_null_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}

fn dev_zero_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    unsafe { core::ptr::write_bytes(buf as *mut u8, 0, count) };
    count
}

fn dev_zero_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}

fn dev_random_read(
    buf: *const u8,
    count: usize,
    _pos: usize,
    _fd: *const FileDesc,
) -> usize {
    unsafe {
        for i in 0..count {
            let b = crate::io::inb(0x40);
            core::ptr::write_volatile((buf as *mut u8).add(i), b);
        }
    }
    count
}

fn dev_random_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}

fn dev_tty_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let mut bytes: usize = 0;
    while bytes < count {
        let status = crate::io::inb(crate::serial::COM1 + 5);
        if status & 0x01 == 0 {
            // Non-blocking : un contexte syscall (ring3 → ring0) ne peut
            // pas dormir sur un hlt (IRQ masquées par FMASK) → on rend ce
            // qu'on a (0 = pas de donnée) et le caller fait son polling.
            break;
        }
        let byte = crate::io::inb(crate::serial::COM1);
        unsafe { core::ptr::write_volatile((buf as *mut u8).add(bytes), byte) };
        bytes += 1;
    }
    bytes
}

fn dev_tty_write(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    for i in 0..count {
        let byte = unsafe { core::ptr::read_volatile((buf as *const u8).add(i)) };
        crate::serial::write_byte(byte);
    }
    count
}

// --- PTY (J4) : tampon partagé pour /dev/pts/0 et /dev/ptmx ---
const PTY_BUF_SIZE: usize = 4096;
static mut PTY_BUF: [u8; PTY_BUF_SIZE] = [0; PTY_BUF_SIZE];
static mut PTY_HEAD: usize = 0; // position d'écriture
static mut PTY_TAIL: usize = 0; // position de lecture
static mut PTY_COUNT: usize = 0; // octets disponibles

fn dev_pty_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let mut bytes: usize = 0;
    while bytes < count {
        unsafe {
            if PTY_COUNT == 0 { break; }
            let b = PTY_BUF[PTY_TAIL];
            core::ptr::write_volatile((buf as *mut u8).add(bytes), b);
            PTY_TAIL = (PTY_TAIL + 1) % PTY_BUF_SIZE;
            PTY_COUNT -= 1;
        }
        bytes += 1;
    }
    bytes
}

fn dev_pty_write(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let mut bytes: usize = 0;
    while bytes < count {
        unsafe {
            if PTY_COUNT >= PTY_BUF_SIZE { break; }
            let b = core::ptr::read_volatile((buf as *const u8).add(bytes));
            PTY_BUF[PTY_HEAD] = b;
            PTY_HEAD = (PTY_HEAD + 1) % PTY_BUF_SIZE;
            PTY_COUNT += 1;
        }
        bytes += 1;
    }
    bytes
}

fn char_dev_read(
    inode: &Inode,
    buf: *const u8,
    count: usize,
    pos: usize,
    _fd: *const FileDesc,
) -> usize {
    match inode.minor {
        3 => dev_null_read(buf, count, pos, _fd),
        4 => dev_tty_read(buf, count, pos, _fd),
        5 => dev_zero_read(buf, count, pos, _fd),
        8 => dev_random_read(buf, count, pos, _fd),
        9 => dev_random_read(buf, count, pos, _fd),
        10 => dev_pty_read(buf, count, pos, _fd),
        _ => 0,
    }
}

fn char_dev_write(
    inode: &Inode,
    buf: *const u8,
    count: usize,
    pos: usize,
    _fd: *const FileDesc,
) -> usize {
    match inode.minor {
        3 => dev_null_write(buf, count, pos, _fd),
        4 => dev_tty_write(buf, count, pos, _fd),
        5 => dev_zero_write(buf, count, pos, _fd),
        8 => dev_random_write(buf, count, pos, _fd),
        10 => dev_pty_write(buf, count, pos, _fd),
        _ => 0,
    }
}

/* ------------------------------------------------------------ *
 * Ouverture / fermeture / lecture / écriture
 * ------------------------------------------------------------ */

/// Ouvre un inode et renvoie un descripteur.
/// Si `fd` est fourni, tente d'ouvrir exactement à ce descripteur
/// (utilisé pour fd 0/1/2 au démarrage). Sinon, cherche un slot libre
/// à partir de 3.
pub unsafe fn open(inode: Inode, fd: Option<usize>) -> Option<usize> {
    match fd {
        Some(f) => {
            if f < MAX_FILES && OPEN_FILES[f].is_none() {
                OPEN_FILES[f] = Some(FileDesc { inode, pos: 0 });
                return Some(f);
            }
            None
        }
        None => {
            for i in 3..MAX_FILES {
                if OPEN_FILES[i].is_none() {
                    OPEN_FILES[i] = Some(FileDesc { inode, pos: 0 });
                    return Some(i);
                }
            }
            None
        }
    }
}

/// Ferme un descripteur.
pub unsafe fn close(fd: usize) {
    if fd >= MAX_FILES {
        return;
    }
    let binding = unsafe { OPEN_FILES[fd] };
    let file = match binding {
        Some(ref f) => f,
        None => return,
    };
    match file.inode.file_type {
        FileType::Pipe => pipe_close(fd),
        FileType::EventFd => { EVENTFDS[file.inode.minor as usize] = 0; }
        FileType::SignalFd => { SIGNALFDS[file.inode.minor as usize] = (0, 0); }
        FileType::TimerFd => { TIMERFDS[file.inode.minor as usize] = TimerFd { expiry: 0, interval: 0, expirations: 0 }; }
        FileType::Epoll => { EPOLLS[file.inode.minor as usize] = EMPTY_EPOLL; }
        FileType::UdpSocket => { UDP_SOCKS[file.inode.minor as usize] = EMPTY_UDP; }
        _ => {}
    }
    OPEN_FILES[fd] = None;
}

/// Duplique `oldfd` vers le premier fd libre >= 3.
/// Renvoie le nouveau fd, ou -errno.
pub unsafe fn dup(oldfd: usize) -> Result<usize, i64> {
    if oldfd >= MAX_FILES {
        return Err(-9);
    }
    let file = match OPEN_FILES[oldfd] {
        Some(ref f) => f,
        None => return Err(-9),
    };
    for i in 3..MAX_FILES {
        if OPEN_FILES[i].is_none() {
            let mut new_file = *file;
            new_file.pos = 0;
            OPEN_FILES[i] = Some(new_file);
            return Ok(i);
        }
    }
    Err(-28)
}

/// Duplique `oldfd` vers `newfd`. Ferme `newfd` s'il est déjà ouvert.
/// Renvoie `newfd`, ou -errno.
pub unsafe fn dup2(oldfd: usize, newfd: usize) -> Result<usize, i64> {
    if oldfd >= MAX_FILES || newfd >= MAX_FILES {
        return Err(-9);
    }
    let file = match OPEN_FILES[oldfd] {
        Some(ref f) => f,
        None => return Err(-9),
    };
    if OPEN_FILES[newfd].is_some() {
        close(newfd);
    }
    let mut new_file = *file;
    new_file.pos = 0;
    OPEN_FILES[newfd] = Some(new_file);
    Ok(newfd)
}

/// Lit jusqu'à `count` octets depuis `fd` dans `buf`.
/// Renvoie le nombre d'octets lus, ou -errno.
pub unsafe fn read(fd: usize, buf: *mut u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let binding = unsafe { OPEN_FILES[fd] };
    let file = match binding {
        Some(ref f) => f,
        None => return Err(-9),
    };

    match file.inode.file_type {
        FileType::Regular => {
            let data = file.inode.data;
            let size = file.inode.size;
            let pos = file.pos;
            if pos >= size {
                return Ok(0);
            }
            let n = min(count, size - pos);
            for i in 0..n {
                let b = core::ptr::read_volatile(data.add(pos + i));
                unsafe { core::ptr::write_volatile(buf.add(i), b) };
            }
            if let Some(ref mut f) = OPEN_FILES[fd] {
                f.pos = pos + n;
            }
            Ok(n)
        }
        FileType::CharDev => {
            let n = unsafe { char_dev_read(&file.inode, buf, count, file.pos, file as *const FileDesc) };
            if let Some(ref mut f) = OPEN_FILES[fd] {
                f.pos += n;
            }
            Ok(n)
        }
        FileType::ProcFile => {
            let ops = unsafe { proc_ops(file.inode.minor) }.ok_or(-19)?;
            let n = unsafe { ops(buf as *const u8, count, file.pos, file as *const FileDesc) };
            if let Some(ref mut f) = OPEN_FILES[fd] {
                f.pos += n;
            }
            Ok(n)
        }
        FileType::Directory => Err(-21),
        FileType::Pipe => unsafe { pipe_read(fd, buf, count) },
        FileType::EventFd => {
            if count < 8 { return Err(-22); }
            let i = file.inode.minor as usize;
            let v = EVENTFDS[i];
            EVENTFDS[i] = 0;
            unsafe { core::ptr::write_unaligned(buf as *mut u64, v) };
            Ok(8)
        }
        FileType::SignalFd => {
            if count < 128 { return Err(-22); }
            let i = file.inode.minor as usize;
            let sig = SIGNALFDS[i].1;
            if sig == 0 { return Ok(0); }
            SIGNALFDS[i].1 = 0;
            unsafe {
                core::ptr::write_bytes(buf, 0, 128);
                core::ptr::write_unaligned(buf as *mut u32, sig);
            }
            Ok(128)
        }
        FileType::TimerFd => {
            if count < 8 { return Err(-22); }
            let i = file.inode.minor as usize;
            let mut tf = TIMERFDS[i];
            if tf.expiry == 0 || crate::syscall::ko_ticks() < tf.expiry { return Ok(0); }
            // Expirations écoulées (au moins 1).
            let now = crate::syscall::ko_ticks();
            let mut exp = 0u64;
            let mut expiry = tf.expiry;
            if tf.interval == 0 { exp = 1; expiry = 0; }
            else {
                while now >= expiry { exp += 1; expiry += tf.interval; }
            }
            TIMERFDS[i].expiry = expiry;
            unsafe { core::ptr::write_unaligned(buf as *mut u64, exp) };
            Ok(8)
        }
        FileType::Epoll | FileType::UdpSocket => Err(-9),
    }
}

/// Écrit jusqu'à `count` octets depuis `buf` vers `fd`.
/// Renvoie le nombre d'octets écrits, ou -errno.
pub unsafe fn write(fd: usize, buf: *const u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let binding = unsafe { OPEN_FILES[fd] };
    let file = match binding {
        Some(ref f) => f,
        None => return Err(-9),
    };

    match file.inode.file_type {
        FileType::Regular => Err(-30),
        FileType::CharDev => {
            let n =
                unsafe { char_dev_write(&file.inode, buf, count, file.pos, file as *const FileDesc) };
            if let Some(ref mut f) = OPEN_FILES[fd] {
                f.pos += n;
            }
            Ok(n)
        }
        FileType::Directory => Err(-21),
        FileType::Pipe => unsafe { pipe_write(fd, buf, count) },
        FileType::EventFd => {
            if count < 8 { return Err(-22); }
            let i = file.inode.minor as usize;
            let v = unsafe { core::ptr::read_unaligned(buf as *const u64) };
            EVENTFDS[i] = EVENTFDS[i].wrapping_add(v);
            Ok(8)
        }
        FileType::SignalFd | FileType::TimerFd | FileType::Epoll | FileType::UdpSocket => Err(-9),
        FileType::ProcFile => Err(-30),
    }
}

/// Lit le contenu complet d'un descripteur (fichier régulier).
///
/// Utilisé par `sys_mmap` pour copier un fichier projeté en mémoire.
/// Retourne (pointeur, taille) ; le pointeur est valide tant que le
/// descripteur reste ouvert.
pub unsafe fn read_fd_all(fd: usize) -> Option<(*const u8, usize)> {
    if fd >= MAX_FILES {
        return None;
    }
    let file = match OPEN_FILES[fd] {
        Some(ref f) => *f,
        None => return None,
    };
    if file.inode.file_type != FileType::Regular {
        return None;
    }
    Some((file.inode.data, file.inode.size))
}

/* ------------------------------------------------------------ *
 * lseek — repositionner le curseur d'un descripteur
 * ------------------------------------------------------------ */

/// `lseek(fd, offset, whence)` — déplace le curseur de lecture/écriture.
///
/// `whence` : 0 = SEEK_SET (absolu), 1 = SEEK_CUR (relatif),
/// 2 = SEEK_END (depuis la fin). Retourne la nouvelle position, ou
/// -errno. Les descripteurs non seekable (tty, pipe) renvoient -ESPIPE.
pub unsafe fn lseek(fd: usize, offset: i64, whence: u32) -> Result<i64, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let file = match OPEN_FILES[fd] {
        Some(ref f) => *f,
        None => return Err(-9),
    };

    // Seuls les fichiers reguliers et les fichiers /proc sont seekable.
    match file.inode.file_type {
        FileType::Regular | FileType::ProcFile => {}
        _ => return Err(-29), // -ESPIPE
    }

    let size = file.inode.size as i64;
    let base = match whence {
        0 => 0i64,
        1 => file.pos as i64,
        2 => size,
        _ => return Err(-22), // -EINVAL
    };

    let new_pos = base.saturating_add(offset);
    if new_pos < 0 {
        return Err(-22); // -EINVAL
    }

    if let Some(ref mut f) = OPEN_FILES[fd] {
        f.pos = new_pos as usize;
    }
    Ok(new_pos)
}

/* ------------------------------------------------------------ *
 * getdents64 — lecture d'un répertoire
 * ------------------------------------------------------------ */

/// Une entrée `struct linux_dirent64` (24 octets + nom).
///
/// Layout exact de l'ABI Linux x86-64 :
///   d_ino    u64     (offset 0)
///   d_off    i64     (offset 8)
///   d_reclen u16     (offset 16)
///   d_type   u8      (offset 18)
///   d_name   char[]  (offset 19, NUL-terminé)
#[repr(C)]
pub struct Dirent64 {
    pub d_ino: u64,
    pub d_off: i64,
    pub d_reclen: u16,
    pub d_type: u8,
    pub d_name: [u8; 0],
}

/// Types de fichiers `d_type` (DT_*).
pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 6;

/// Entrée de répertoire en mémoire noyau, avant sérialisation.
pub struct DirEntry {
    pub ino: u64,
    pub name: [u8; 64],
    pub name_len: usize,
    pub dtype: u8,
}

/// Construit la liste des entrées d'un répertoire.
///
/// Seuls `/proc` et `/dev` sont des répertoires réels à ce stade ; le
/// reste est un fichier régulier (l'initrd est plat).
pub unsafe fn readdir(path: &[u8]) -> Option<&'static [DirEntry]> {
    // /proc : les pseudo-fichiers exposes par le VFS.
    if path == b"proc" {
        return Some(&PROC_DIR);
    }
    // /dev : les peripheriques caracteres.
    if path == b"dev" {
        return Some(&DEV_DIR);
    }
    None
}

/// Entrées du répertoire `/proc`.
static mut PROC_DIR: [DirEntry; 8] = [
    DirEntry { ino: 1, name: [0; 64], name_len: 0, dtype: DT_DIR },
    DirEntry { ino: 2, name: [0; 64], name_len: 0, dtype: DT_DIR },
    DirEntry { ino: 3, name: [0; 64], name_len: 0, dtype: DT_REG },
    DirEntry { ino: 4, name: [0; 64], name_len: 0, dtype: DT_REG },
    DirEntry { ino: 5, name: [0; 64], name_len: 0, dtype: DT_REG },
    DirEntry { ino: 6, name: [0; 64], name_len: 0, dtype: DT_REG },
    DirEntry { ino: 7, name: [0; 64], name_len: 0, dtype: DT_REG },
    DirEntry { ino: 8, name: [0; 64], name_len: 0, dtype: DT_REG },
];

/// Entrées du répertoire `/dev`.
static mut DEV_DIR: [DirEntry; 5] = [
    DirEntry { ino: 10, name: [0; 64], name_len: 0, dtype: DT_CHR },
    DirEntry { ino: 11, name: [0; 64], name_len: 0, dtype: DT_CHR },
    DirEntry { ino: 12, name: [0; 64], name_len: 0, dtype: DT_CHR },
    DirEntry { ino: 13, name: [0; 64], name_len: 0, dtype: DT_CHR },
    DirEntry { ino: 14, name: [0; 64], name_len: 0, dtype: DT_CHR },
];

/// Initialise les noms des entrées de répertoire (appelé depuis `init`).
pub unsafe fn init_dir_entries() {
    set_name(&mut PROC_DIR[0], b".");
    set_name(&mut PROC_DIR[1], b"..");
    set_name(&mut PROC_DIR[2], b"self");
    set_name(&mut PROC_DIR[3], b"meminfo");
    set_name(&mut PROC_DIR[4], b"maps");
    set_name(&mut PROC_DIR[5], b"stat");
    set_name(&mut PROC_DIR[6], b"status");
    set_name(&mut PROC_DIR[7], b"fd");

    set_name(&mut DEV_DIR[0], b"null");
    set_name(&mut DEV_DIR[1], b"zero");
    set_name(&mut DEV_DIR[2], b"random");
    set_name(&mut DEV_DIR[3], b"tty");
    set_name(&mut DEV_DIR[4], b"console");
}

unsafe fn set_name(entry: &mut DirEntry, name: &[u8]) {
    let n = name.len().min(63);
    entry.name[..n].copy_from_slice(&name[..n]);
    entry.name_len = n;
}

/// `getdents64(fd, buf, count)` — lit des entrées de répertoire.
///
/// `fd` doit désigner un répertoire ouvert. Remplit `buf` avec des
/// `struct linux_dirent64` consécutives, alignées sur 8 octets. Retourne
/// le nombre d'octets écrits, 0 en fin de liste, ou -errno.
pub unsafe fn getdents64(fd: usize, buf: *mut u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let file = match OPEN_FILES[fd] {
        Some(ref f) => *f,
        None => return Err(-9),
    };
    if file.inode.file_type != FileType::Directory {
        return Err(-20); // -ENOTDIR
    }

    // Le chemin du répertoire est stocké dans `data` (pointeur noyau).
    // On retrouve le nom via une table fixe : /proc ou /dev.
    let path: &[u8] = if file.inode.minor == 0 { b"proc" } else { b"dev" };
    let entries = match readdir(path) {
        Some(e) => e,
        None => return Err(-2), // -ENOENT
    };

    let mut written: usize = 0;
    let mut ino: u64 = 0;

    for entry in entries.iter() {
        if entry.name_len == 0 {
            continue;
        }
        // Reclen = 19 (champs) + nom + NUL, arrondi à 8.
        let reclen = ((19 + entry.name_len + 1 + 7) & !7) as u16;
        if written + reclen as usize > count {
            break;
        }

        let base = buf.add(written) as *mut Dirent64;
        (*base).d_ino = entry.ino;
        (*base).d_off = (written + reclen as usize) as i64;
        (*base).d_reclen = reclen;
        (*base).d_type = entry.dtype;

        let name_dst = buf.add(written + 19);
        core::ptr::copy_nonoverlapping(entry.name.as_ptr(), name_dst, entry.name_len);
        *name_dst.add(entry.name_len) = 0;

        written += reclen as usize;
        ino += 1;
    }

    let _ = ino;
    Ok(written)
}
