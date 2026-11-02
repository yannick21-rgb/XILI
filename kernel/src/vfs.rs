/* ============================================================
 * vfs.rs — système de fichiers virtuel minimal (J3.4/J3.5)
 *
 * Fournit :
 *   - un VFS à inodes et dentries,
 *   - un /dev (devtmpfs) avec null, zero, random, tty,
 *   - un lookup de chemin,
 *   - des opérations read/write/close/open par descripteur.
 *
 * Ce module ne connaît que l'initrd (cpio newc) et /dev.
 * Un vrai montage, des pipes et des sockets viendront plus tard.
 * ============================================================ */

use crate::initrd;
use crate::mm::paging::PAGE_SIZE;
use core::cmp::min;

/* ------------------------------------------------------------ *
 * Constantes de périphériques /dev
 * ------------------------------------------------------------ */

const DEV_NULL_MINOR: u32 = 3;
const DEV_ZERO_MINOR: u32 = 5;
const DEV_RANDOM_MINOR: u32 = 8;
const DEV_TTY_MINOR: u32 = 5; /* tty émulé */

/* ------------------------------------------------------------ *
 * Types de fichiers et inodes
 * ------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileType {
    Regular,
    CharDev,
    Directory,
    Pipe,
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
 * Dentry : entrée dans un répertoire
 * ------------------------------------------------------------ */

#[derive(Clone, Copy)]
pub struct Dentry {
    pub name: [u8; 64],
    pub name_len: usize,
    pub inode: Inode,
}

impl Dentry {
    pub const fn empty() -> Self {
        Dentry {
            name: [0; 64],
            name_len: 0,
            inode: Inode::empty(),
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
const MAX_DENTRIES: usize = 64;

static mut OPEN_FILES: [Option<FileDesc>; MAX_FILES] = [None; MAX_FILES];
static mut ROOT_DENTRIES: [Dentry; MAX_DENTRIES] = [Dentry::empty(); MAX_DENTRIES];
static mut ROOT_COUNT: usize = 0;

/* ------------------------------------------------------------ *
 * Helpers
 * ------------------------------------------------------------ */

unsafe fn str_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a == b
}

/* ------------------------------------------------------------ *
 * Initialisation du VFS : racine = initrd, puis /dev
 * ------------------------------------------------------------ */

pub unsafe fn init() {
    // L'initrd est déjà initialisé par main.rs avant cette fonction.
    // Ce module ne fait que confirmer que le VFS est prêt.
    crate::console::print(format_args!("vfs : pret\n"));
}

/* ------------------------------------------------------------ *
 * Lookup d'un chemin vers un Inode.
 *
 * Chemins supportés pour l'instant :
 *   - chemins absolus starts with '/' (strip leading slash)
 *   - un seul composant de nom (pas de récursion /a/b/c)
 *   - d'abord dans l'initrd, puis dans /dev
 * ------------------------------------------------------------ */

pub unsafe fn lookup(name: &[u8]) -> Option<Inode> {
    // Strip leading '/'.
    let path = if !name.is_empty() && name[0] == b'/' {
        &name[1..]
    } else {
        name
    };

    if path.is_empty() {
        // Racine : inode factice directory.
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

    // 2. Chercher dans /dev.
    if path == b"null" {
        return Some(new_char_dev(DEV_NULL_MINOR, &DEV_NULL_OPS));
    }
    if path == b"zero" {
        return Some(new_char_dev(DEV_ZERO_MINOR, &DEV_ZERO_OPS));
    }
    if path == b"random" {
        return Some(new_char_dev(DEV_RANDOM_MINOR, &DEV_RANDOM_OPS));
    }
    if path == b"tty" {
        return Some(new_char_dev(DEV_TTY_MINOR, &DEV_TTY_OPS));
    }

    None
}

/* ------------------------------------------------------------ *
 * Opérations sur les fichiers
 * ------------------------------------------------------------ */

#[derive(Clone, Copy)]
pub enum Op {
    Read,
    Write,
}

pub struct FileOps {
    pub read: Option<fn(*const u8, usize, usize, *const FileDesc) -> usize>,
    pub write: Option<fn(*const u8, usize, usize, *const FileDesc) -> usize>,
}

/* null : read EOF, write drop */
fn dev_null_read(_buf: *const u8, _count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    0
}
fn dev_null_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}
static DEV_NULL_OPS: FileOps = FileOps {
    read: Some(dev_null_read),
    write: Some(dev_null_write),
};

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
    // Chercher deux slots libres.
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

    // Chercher un slot pipe libre.
    let pipe_idx = PIPE_COUNT;
    if pipe_idx >= PIPES.len() {
        return None;
    }
    PIPE_COUNT += 1;

    // Allouer le buffer.
    let buf = unsafe { crate::mm::slab::kmalloc(PIPE_BUF) };
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

    // Créer les inodes pipe.
    let inode = Inode {
        file_type: FileType::Pipe,
        major: 0,
        minor: pipe_idx as u32,
        data: core::ptr::null(),
        size: 0,
        pipe: PIPES[pipe_idx],
    };

    // read fd
    OPEN_FILES[read_fd] = Some(FileDesc { inode, pos: 0 });
    // write fd
    OPEN_FILES[write_fd] = Some(FileDesc { inode, pos: 0 });

    Some((read_fd, write_fd))
}

fn pipe_read(fd: usize, buf: *mut u8, count: usize) -> Result<usize, i64> {
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
        _ => return Err(-19), // -ENODEV
    };

    if pipe.len == 0 {
        if pipe.writers == 0 {
            return Ok(0); // EOF
        }
        return Err(-11); // -EAGAIN
    }

    let n = core::cmp::min(count, pipe.len);
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(pipe.buf.add(pipe.rpos)) };
        unsafe { core::ptr::write_volatile(buf.add(i), b) };
        pipe.rpos = (pipe.rpos + 1) % pipe.cap;
    }
    pipe.len -= n;
    Ok(n)
}

fn pipe_write(fd: usize, buf: *const u8, count: usize) -> Result<usize, i64> {
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
    let n = core::cmp::min(count, free);
    for i in 0..n {
        let b = unsafe { core::ptr::read_volatile(buf.add(i)) };
        unsafe { core::ptr::write_volatile(pipe.buf.add(pipe.wpos), b) };
        pipe.wpos = (pipe.wpos + 1) % pipe.cap;
    }
    pipe.len += n;
    Ok(n)
}

fn pipe_close(fd: usize) {
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

/* zero : read zeros, write drop */
fn dev_zero_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    unsafe { core::ptr::write_bytes(buf as *mut u8, 0, count) };
    count
}
fn dev_zero_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}
static DEV_ZERO_OPS: FileOps = FileOps {
    read: Some(dev_zero_read),
    write: Some(dev_zero_write),
};

/* random : lecture depuis le port 0x40 (PIT counter, assez pour un début) */
fn dev_random_read(
    buf: *const u8,
    count: usize,
    _pos: usize,
    _fd: *const FileDesc,
) -> usize {
    unsafe {
        for i in 0..count {
            // Lecture d'un octet sur le port timer 0.
            // Ce n'est pas cryptographiquement sûr, mais suffit pour busybox.
            let b = crate::io::inb(0x40);
            core::ptr::write_volatile((buf as *mut u8).add(i), b);
        }
    }
    count
}
fn dev_random_write(_buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    count
}
static DEV_RANDOM_OPS: FileOps = FileOps {
    read: Some(dev_random_read),
    write: Some(dev_random_write),
};

/* tty : écriture vers le port série, lecture non bloquante depuis le port série */
fn dev_tty_read(buf: *const u8, count: usize, _pos: usize, _fd: *const FileDesc) -> usize {
    let mut bytes: usize = 0;
    for i in 0..count {
        let status = crate::io::inb(crate::serial::COM1 + 5);
        if status & 0x01 == 0 {
            break;
        }
        let byte = crate::io::inb(crate::serial::COM1);
        unsafe { core::ptr::write_volatile((buf as *mut u8).add(i), byte) };
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
static DEV_TTY_OPS: FileOps = FileOps {
    read: Some(dev_tty_read),
    write: Some(dev_tty_write),
};

/* Pointeur vers les ops d'un périphérique caractère. */
#[derive(Clone, Copy)]
struct CharDev {
    minor: u32,
    ops: &'static FileOps,
}

static mut CHARDEVS: [Option<CharDev>; 64] = [None; 64];

unsafe fn register_char_dev(minor: u32, ops: &'static FileOps) {
    let idx = minor as usize;
    if idx < CHARDEVS.len() {
        CHARDEVS[idx] = Some(CharDev { minor, ops });
    }
}

unsafe fn char_dev_ops(minor: u32) -> Option<&'static FileOps> {
    let idx = minor as usize;
    if idx < CHARDEVS.len() {
        CHARDEVS[idx].map(|d| d.ops)
    } else {
        None
    }
}

unsafe fn new_char_dev(minor: u32, ops: &'static FileOps) -> Inode {
    register_char_dev(minor, ops);
    Inode {
        file_type: FileType::CharDev,
        major: 1, /* major 1 = char */
        minor,
        data: core::ptr::null(),
        size: 0,
        pipe: None,
    }
}

/* ------------------------------------------------------------ *
 * Ouverture / fermeture / lecture / écriture
 * ------------------------------------------------------------ */

/// Ouvre un inode et renvoie un descripteur.
///
/// Si `fd` est fourni, tente d'ouvrir exactement à ce descripteur
/// (utilisé pour fd 0/1/2 au démarrage). Sinon, cherche un slot libre
/// à partir de 3.
pub unsafe fn open(inode: Inode, fd: Option<usize>) -> Option<usize> {
    match fd {
        Some(f) => {
            if f < MAX_FILES && unsafe { OPEN_FILES[f] }.is_none() {
                unsafe { OPEN_FILES[f] = Some(FileDesc { inode, pos: 0 }) };
                return Some(f);
            }
            None
        }
        None => {
            for i in 3..MAX_FILES {
                if unsafe { OPEN_FILES[i] }.is_none() {
                    unsafe { OPEN_FILES[i] = Some(FileDesc { inode, pos: 0 }) };
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
    unsafe { OPEN_FILES[fd] = None };
}

/// Lit jusqu'à `count` octets depuis `fd` dans `buf`.
/// Renvoie le nombre d'octets lus, ou -errno.
pub unsafe fn read(fd: usize, buf: *mut u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9); // -EBADF
    }
    let file = match unsafe { &mut OPEN_FILES[fd] } {
        Some(f) => f,
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
                core::ptr::write_volatile(buf.add(i), b);
            }
            if let Some(ref mut f) = unsafe { OPEN_FILES[fd] } {
                f.pos = pos + n;
            }
            Ok(n)
        }
        FileType::CharDev => {
            let ops = char_dev_ops(file.inode.minor)
                .ok_or(-19)?;
            let read_fn = ops.read.ok_or(-38)?;
            let n = read_fn(buf as *const u8, count, file.pos, file as *const FileDesc);
            if let Some(ref mut f) = unsafe { OPEN_FILES[fd] } {
                f.pos += n;
            }
            Ok(n)
        }
        FileType::Directory => Err(-21),
        FileType::Pipe => pipe_read(fd, buf, count),
    }
}

/// Écrit jusqu'à `count` octets depuis `buf` vers `fd`.
pub unsafe fn write(fd: usize, buf: *const u8, count: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    let file = match unsafe { &mut OPEN_FILES[fd] } {
        Some(f) => f,
        None => return Err(-9),
    };

    match file.inode.file_type {
        FileType::Regular => {
            // Écriture dans l'initrd : non supportée pour l'instant.
            Err(-30) // -EROFS
        }
        FileType::CharDev => {
            let ops = char_dev_ops(file.inode.minor)
                .ok_or(-19)?;
            let write_fn = ops.write.ok_or(-38)?;
            let n = write_fn(buf, count, file.pos, file as *const FileDesc);
            if let Some(ref mut f) = unsafe { OPEN_FILES[fd] } {
                f.pos += n;
            }
            Ok(n)
        }
        FileType::Directory => Err(-21),
        FileType::Pipe => pipe_write(fd, buf, count),
    }
}

/// Renvoie la position courante dans `fd`.
pub unsafe fn tell(fd: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    match unsafe { OPEN_FILES[fd] } {
        Some(ref f) => Ok(f.pos),
        None => Err(-9),
    }
}

/// Change la position dans `fd`.
pub unsafe fn seek(fd: usize, pos: usize) -> Result<usize, i64> {
    if fd >= MAX_FILES {
        return Err(-9);
    }
    if let Some(ref mut f) = unsafe { OPEN_FILES[fd] } {
        f.pos = pos;
        Ok(pos)
    } else {
        Err(-9)
    }
}

/* ------------------------------------------------------------ *
 * Initialisation de /dev au démarrage
 * ------------------------------------------------------------ */

pub unsafe fn init_dev() {
    // Enregistrer les périphériques caractères dans le lookup /dev.
    // Le lookup se fait par nom, donc on prépare juste la table des ops.
    register_char_dev(DEV_NULL_MINOR, &DEV_NULL_OPS);
    register_char_dev(DEV_ZERO_MINOR, &DEV_ZERO_OPS);
    register_char_dev(DEV_RANDOM_MINOR, &DEV_RANDOM_OPS);
    register_char_dev(DEV_TTY_MINOR, &DEV_TTY_OPS);
    crate::console::print(format_args!("vfs : /dev (null zero random tty) pret\n"));
}
