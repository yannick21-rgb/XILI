//! ext2.rs — lecture minimale ext2 (superblock, inodes, répertoires, fichiers).

use crate::storage::ide;

const EXT2_MAGIC: u16 = 0xEF53;
const EXT2_ROOT_INO: u32 = 2;

// Superblock offsets
const SB_INODES_COUNT: usize = 0;
const SB_BLOCKS_COUNT: usize = 4;
const SB_FIRST_DATA_BLOCK: usize = 20;
const SB_LOG_BLOCK_SIZE: usize = 24;
const SB_BLOCKS_PER_GROUP: usize = 32;
const SB_INODES_PER_GROUP: usize = 40;
const SB_MAGIC: usize = 56;
const SB_FIRST_INO: usize = 84;
const SB_INODE_SIZE: usize = 88;

// Inode offsets
const INO_MODE: usize = 0;
const INO_SIZE: usize = 4;
const INO_BLOCKS: usize = 28;
const INO_BLOCK: usize = 40; // i_block[0]

// Directory entry offsets
const DENT_INODE: usize = 0;
const DENT_REC_LEN: usize = 4;
const DENT_NAME_LEN: usize = 6;
const DENT_FILE_TYPE: usize = 7;
const DENT_NAME: usize = 8;

// Group descriptor offsets
const GD_INODE_TABLE: usize = 8;

pub static mut BLOCK_SIZE: u32 = 1024;
pub static mut INODE_SIZE: u32 = 128;
pub static mut INODES_PER_GROUP: u32 = 0;
pub static mut FIRST_DATA_BLOCK: u32 = 0;

fn read_u16(buf: &[u8], off: usize) -> u16 {
    (buf[off] as u16) | ((buf[off + 1] as u16) << 8)
}

fn read_u32(buf: &[u8], off: usize) -> u32 {
    (buf[off] as u32) | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16) | ((buf[off + 3] as u32) << 24)
}

/// Lit un bloc (block_size octets) dans un buffer.
/// Le buffer doit être assez grand (au moins block_size).
pub unsafe fn read_block(block_num: u32, buf: &mut [u8]) -> bool {
    let bs = BLOCK_SIZE as usize;
    if buf.len() < bs {
        return false;
    }
    let sectors_per_block = bs / 512;
    let lba = block_num * sectors_per_block as u32;
    for i in 0..sectors_per_block {
        let mut sector_buf = [0u8; 512];
        if !ide::read_sector(lba + i as u32, &mut sector_buf) {
            return false;
        }
        let off = i * 512;
        buf[off..off + 512].copy_from_slice(&sector_buf);
    }
    true
}

/// Lit le superblock et initialise les paramètres.
pub unsafe fn init() -> bool {
    // Superblock is at byte offset 1024
    let mut sector0 = [0u8; 512];
    let mut sector1 = [0u8; 512];
    if !ide::read_sector(0, &mut sector0) { return false; }
    if !ide::read_sector(1, &mut sector1) { return false; }

    // Superblock starts at offset 1024 = sector 2, offset 0
    // Actually, offset 1024 = sector 2 (1024/512=2)
    let mut sb_sector = [0u8; 512];
    if !ide::read_sector(2, &mut sb_sector) { return false; }

    // Superblock is at offset 0 within sector 2 (since 1024 = 2*512)
    let sb = &sb_sector;

    let magic = read_u16(sb, SB_MAGIC);
    if magic != EXT2_MAGIC {
        crate::console::print(format_args!("ext2: magic invalide 0x{:x}\\n", magic));
        return false;
    }

    let log_bs = read_u32(sb, SB_LOG_BLOCK_SIZE);
    BLOCK_SIZE = 1024 << log_bs;
    INODES_PER_GROUP = read_u32(sb, SB_INODES_PER_GROUP);
    FIRST_DATA_BLOCK = read_u32(sb, SB_FIRST_DATA_BLOCK);
    INODE_SIZE = read_u16(sb, SB_INODE_SIZE) as u32;
    if INODE_SIZE == 0 { INODE_SIZE = 128; }

    crate::console::print(format_args!(
        "ext2: magic=0x{:x} block_size={} inodes_per_group={} first_data_block={} inode_size={}\\n",
        magic, BLOCK_SIZE, INODES_PER_GROUP, FIRST_DATA_BLOCK, INODE_SIZE
    ));

    let inodes_count = read_u32(sb, SB_INODES_COUNT);
    let blocks_count = read_u32(sb, SB_BLOCKS_COUNT);
    crate::console::print(format_args!(
        "ext2: inodes={} blocks={}\\n", inodes_count, blocks_count
    ));

    true
}

/// Lit un inode par numéro.
pub unsafe fn read_inode(ino: u32, buf: &mut [u8]) -> bool {
    if buf.len() < INODE_SIZE as usize { return false; }
    // Inode table is in group 0's group descriptor
    // Group descriptor is at block (FIRST_DATA_BLOCK + 1) for 1024-byte blocks
    // Actually, group descriptors start at block FIRST_DATA_BLOCK + 1
    let gd_block = FIRST_DATA_BLOCK + 1;
    let mut gd_buf = [0u8; 4096];
    if !read_block(gd_block, &mut gd_buf) { return false; }

    let inode_table_block = read_u32(&gd_buf, GD_INODE_TABLE);

    // Inode number to index (1-based)
    let idx = (ino - 1) as u64;
    let byte_offset = idx * INODE_SIZE as u64;
    let block_offset = (byte_offset / BLOCK_SIZE as u64) as u32;
    let off_in_block = (byte_offset % BLOCK_SIZE as u64) as usize;

    let target_block = inode_table_block + block_offset;
    let mut block_buf = [0u8; 4096];
    if !read_block(target_block, &mut block_buf) { return false; }

    let bs = BLOCK_SIZE as usize;
    for i in 0..INODE_SIZE as usize {
        buf[i] = block_buf[off_in_block + i];
    }
    true
}

/// Liste les entrées d'un répertoire (inode de répertoire).
pub unsafe fn list_dir(ino: u32) {
    let mut inode_buf = [0u8; 256];
    if !read_inode(ino, &mut inode_buf) {
        crate::console::print(format_args!("ext2: impossible de lire inode {}\\n", ino));
        return;
    }

    let size = read_u32(&inode_buf, INO_SIZE) as usize;
    crate::console::print(format_args!("ext2: inode {} size={}\\n", ino, size));

    // Lire les blocs directs
    for b in 0..12usize {
        let block_num = read_u32(&inode_buf, INO_BLOCK + b * 4);
        if block_num == 0 { break; }

        let mut block_buf = [0u8; 4096];
        if !read_block(block_num, &mut block_buf) { continue; }

        let mut off = 0usize;
        while off < BLOCK_SIZE as usize {
            let dent_ino = read_u32(&block_buf, off + DENT_INODE);
            if dent_ino == 0 { break; }
            let rec_len = read_u16(&block_buf, off + DENT_REC_LEN) as usize;
            if rec_len == 0 { break; }
            let name_len = block_buf[off + DENT_NAME_LEN] as usize;
            let file_type = block_buf[off + DENT_FILE_TYPE];

            // Print name
            crate::console::print(format_args!("ext2:   inode={} type={} name=", dent_ino, file_type));
            for i in 0..name_len {
                let c = block_buf[off + DENT_NAME + i];
                if c >= 0x20 && c < 0x7F {
                    crate::serial::write_byte(c);
                } else {
                    crate::serial::write_byte(b'?');
                }
            }
            crate::serial::write_byte(b'\n');

            off += rec_len;
        }
    }
}

/// Lit le contenu d'un fichier (inode) dans un buffer. Retourne le nombre d'octets lus.
pub unsafe fn read_file(ino: u32, buf: &mut [u8]) -> usize {
    let mut inode_buf = [0u8; 256];
    if !read_inode(ino, &mut inode_buf) { return 0; }

    let size = read_u32(&inode_buf, INO_SIZE) as usize;
    let to_read = size.min(buf.len());
    let mut total = 0usize;

    // Blocs directs (0..11)
    for b in 0..12usize {
        if total >= to_read { break; }
        let block_num = read_u32(&inode_buf, INO_BLOCK + b * 4);
        if block_num == 0 { break; }

        let mut block_buf = [0u8; 4096];
        if !read_block(block_num, &mut block_buf) { break; }

        let chunk = (BLOCK_SIZE as usize).min(to_read - total);
        buf[total..total + chunk].copy_from_slice(&block_buf[..chunk]);
        total += chunk;
    }

    // Bloc indirect simple (i_block[12])
    if total < to_read {
        let indirect_block = read_u32(&inode_buf, INO_BLOCK + 12 * 4);
        if indirect_block != 0 {
            let mut ind_buf = [0u8; 4096];
            if read_block(indirect_block, &mut ind_buf) {
                let entries_per_block = BLOCK_SIZE as usize / 4;
                for e in 0..entries_per_block {
                    if total >= to_read { break; }
                    let block_num = read_u32(&ind_buf, e * 4);
                    if block_num == 0 { break; }

                    let mut block_buf = [0u8; 4096];
                    if !read_block(block_num, &mut block_buf) { break; }

                    let chunk = (BLOCK_SIZE as usize).min(to_read - total);
                    buf[total..total + chunk].copy_from_slice(&block_buf[..chunk]);
                    total += chunk;
                }
            }
        }
    }

    total
}

pub fn is_ready() -> bool { true }
