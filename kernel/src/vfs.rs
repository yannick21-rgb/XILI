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
use crate::mm::paging::PAGE_SIZE;
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
}

#[derive(Clone, Copy)]
pub struct Inode {
    pub file_type: FileType,
    pub major: u32,
    pub minor: u32,
    pub data: *const u8,
    pub size: usize,
    pub pipe: Option<Pipe>,
}

impl Inode {
    pub const fn empty() -> Self {
        Inode {
            file_type: FileType::Regular,
            major: 0,
            minor: 0,
            data: core::ptr::null(),
            size: 0,
            pipe: None,
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
            major: 0,
            minor: 0,
            data: core::ptr::null(),
            size: 0,
            pipe: None,
        });
    }

    // 1. Chercher dans l'initrd.
    if let Some((data, size)) = initrd::find(path) {
        return Some(Inode {
            file_type: FileType::Regular,
            major: 0,
            minor: 0,
            data,
            size,
            pipe: None,
        });
    }

    // 2. /dev : inodes devices directs (pas de table CharDev/FileOps).
    match path {
        b"null" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                major: 1,
                minor: 3,
                data: core::ptr::null(),
                size: 0,
                pipe: None,
            });
        }
        b"zero" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                major: 1,
                minor: 5,
                data: core::ptr::null(),
                size: 0,
                pipe: None,
            });
        }
        b"random" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                major: 1,
                minor: 8,
                data: core::ptr::null(),
                size: 0,
                pipe: None,
            });
        }
        b"tty" => {
            return Some(Inode {
                file_type: FileType::CharDev,
                major: 1,
                minor: 4,
                data: core::ptr::null(),
                size: 0,
                pipe: None,
            });
        }
        _ => {}
    }

    // 3. /proc minimal (J4).
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
        major: 0,
        minor: idx as u32,
        data: core::ptr::null(),
        size: 0,
        pipe: None,
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
    let line =
        b"0x10000000-0x1000b000 r-xp 00000000 00:00 0 /busybox\n\
          0x1000b000-0x1000c000 rw-p 0000a000 00:00 0\n";
    let n = min(count, line.len());
    for i in 0..n {
        unsafe { core::ptr::write_volatile((_buf as *mut u8).add(i), line[i]) };
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

impl Pipe {
    pub const fn empty() -> Self {
        Pipe {
            buf: core::ptr::null_mut(),
            len: 0,
            cap: 0,
            rpos: 0,
            wpos: 0,
            readers: 0,
            writers: 0,
        }
    }
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
        major: 0,
        minor: pipe_idx as u32,
        data: core::ptr::null(),
        size: 0,
        pipe: PIPES[pipe_idx],
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
        FileType::ProcFile => Err(-30),
    }
}
