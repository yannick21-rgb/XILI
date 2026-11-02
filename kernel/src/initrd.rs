/* ============================================================
 * initrd.rs — parseur d'archive cpio newc (J3.5)
 *
 * Parse une archive cpio au format newc (ASCII hex) incluse via
 * include_bytes!. Fournit find() pour sys_openat/sys_newfstatat.
 * ============================================================ */

const CPIO_MAGIC: &[u8] = b"070701";
const MAX_FILES: usize = 32;
const MAX_NAME: usize = 64;

#[derive(Clone, Copy)]
struct InitrdEntry {
    name: [u8; MAX_NAME],
    name_len: usize,
    data: *const u8,
    size: usize,
}

static mut ENTRIES: [InitrdEntry; MAX_FILES] = [
    InitrdEntry {
        name: [0; MAX_NAME],
        name_len: 0,
        data: core::ptr::null(),
        size: 0,
    };
    MAX_FILES
];
static mut ENTRY_COUNT: usize = 0;

/// Parse 8 hex chars → u64 (NUL-terminated in cpio newc header fields).
unsafe fn parse_hex8(data: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    for i in 0..8 {
        let c = *data.get_unchecked(off + i);
        if c == 0 {
            break;
        }
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u64,
            b'a'..=b'f' => (c - b'a' + 10) as u64,
            b'A'..=b'F' => (c - b'A' + 10) as u64,
            _ => break,
        };
        v = v * 16 + d;
    }
    v
}

/// Initialise le module initrd en parsant l'archive cpio.
///
/// Appelée une fois depuis kernel_main après l'inclusion de l'archive
/// via include_bytes!("../../assets/initrd.cpio").
pub fn init(archive: &'static [u8]) {
    unsafe {
        let mut off: usize = 0;
        let mut count: usize = 0;

        while off + 110 <= archive.len() {
            if &archive[off..off + 6] != CPIO_MAGIC {
                break;
            }

            let filesize = parse_hex8(archive, off + 54) as usize;
            let namesize = parse_hex8(archive, off + 94) as usize;

            off += 110;

            if off + namesize > archive.len() {
                break;
            }
            let name = &archive[off..off + namesize];

            let name_padded = (namesize + 3) & !3;
            off += name_padded;

            if off + filesize > archive.len() {
                break;
            }
            let data = archive.as_ptr().add(off);
            let data_padded = (filesize + 3) & !3;
            off += data_padded;

            // TRAILER!!! marque la fin de l'archive.
            if name.starts_with(b"TRAILER!!!") {
                break;
            }

            if count < MAX_FILES && namesize > 1 {
                let e = &mut ENTRIES[count];
                let nl = (namesize - 1).min(MAX_NAME - 1);
                e.name[..nl].copy_from_slice(&name[..nl]);
                e.name_len = nl;
                e.data = data;
                e.size = filesize;
                count += 1;
            }
        }

        ENTRY_COUNT = count;
        crate::console::print(format_args!(
            "initrd : {} fichier(s) charge(s) depuis cpio newc\n",
            count
        ));
    }
}

/// Recherche un fichier par nom (sans NUL) dans l'initrd.
///
/// Renvoie (pointeur_vers_donnees, taille) si trouvé.
pub fn find(name: &[u8]) -> Option<(*const u8, usize)> {
    unsafe {
        for i in 0..ENTRY_COUNT {
            let e = &ENTRIES[i];
            if e.name_len == name.len() && &e.name[..e.name_len] == name {
                return Some((e.data, e.size));
            }
        }
    }
    None
}
