//! ide.rs — driver IDE legacy (primary master) pour QEMU.
//!
//! J6.1 (variante IDE) : lecture d'un secteur via les ports 0x1F0-0x1F7.

use crate::io::{inb, inw, outb, outw};

const IDE_DATA: u16 = 0x1F0;
const IDE_ERROR: u16 = 0x1F1;
const IDE_SECCOUNT: u16 = 0x1F2;
const IDE_LBA_LO: u16 = 0x1F3;
const IDE_LBA_MID: u16 = 0x1F4;
const IDE_LBA_HI: u16 = 0x1F5;
const IDE_DEVICE: u16 = 0x1F6;
const IDE_STATUS: u16 = 0x1F7;
const IDE_COMMAND: u16 = 0x1F7;
const IDE_ALTSTATUS: u16 = 0x3F6;

const STATUS_BSY: u8 = 0x80;
const STATUS_DRDY: u8 = 0x40;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;

pub unsafe fn init() -> bool {
    // Attendre que le device soit prêt
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_BSY == 0 {
            break;
        }
    }
    let st = inb(IDE_STATUS);
    crate::console::print(format_args!("ide: status=0x{:x}\\n", st));
    // Sélectionner le master (device 0)
    outb(IDE_DEVICE, 0xE0);
    for _ in 0..10000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_BSY == 0 {
            break;
        }
    }
    crate::console::print(format_args!("ide: init OK\\n"));
    true
}

pub unsafe fn read_sector(lba: u32, buf: &mut [u8; 512]) -> bool {
    // Attendre BSY=0
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_BSY == 0 {
            break;
        }
    }

    // Paramètres
    outb(IDE_SECCOUNT, 1);
    outb(IDE_LBA_LO, (lba & 0xFF) as u8);
    outb(IDE_LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(IDE_LBA_HI, ((lba >> 16) & 0xFF) as u8);
    outb(IDE_DEVICE, 0xE0 | (((lba >> 24) & 0x0F) as u8));

    // Commande READ SECTORS
    outb(IDE_COMMAND, 0x20);

    // Attendre DRQ
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_DRQ != 0 {
            break;
        }
        if st & STATUS_ERR != 0 {
            crate::console::print(format_args!("ide: erreur status=0x{:x}\\n", st));
            return false;
        }
    }

    let st = inb(IDE_STATUS);
    if st & STATUS_DRQ == 0 {
        crate::console::print(format_args!("ide: timeout DRQ, status=0x{:x}\\n", st));
        return false;
    }

    // Lire 256 mots (512 octets)
    for i in 0..256usize {
        let word = inw(IDE_DATA);
        buf[i * 2] = (word & 0xFF) as u8;
        buf[i * 2 + 1] = ((word >> 8) & 0xFF) as u8;
    }

    crate::console::print(format_args!(
        "ide: secteur {} lu (premiers octets: {:02x} {:02x} {:02x} {:02x})\\n",
        lba, buf[0], buf[1], buf[2], buf[3]
    ));
    true
}

/// Écrit un secteur (512 octets) au LBA donné.
pub unsafe fn write_sector(lba: u32, buf: &[u8; 512]) -> bool {
    // Attendre BSY=0
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_BSY == 0 { break; }
    }

    outb(IDE_SECCOUNT, 1);
    outb(IDE_LBA_LO, (lba & 0xFF) as u8);
    outb(IDE_LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(IDE_LBA_HI, ((lba >> 16) & 0xFF) as u8);
    outb(IDE_DEVICE, 0xE0 | (((lba >> 24) & 0x0F) as u8));

    // Commande WRITE SECTORS
    outb(IDE_COMMAND, 0x30);

    // Attendre DRQ
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_DRQ != 0 { break; }
        if st & STATUS_ERR != 0 {
            crate::console::print(format_args!("ide: write erreur status=0x{:x}\n", st));
            return false;
        }
    }

    let st = inb(IDE_STATUS);
    if st & STATUS_DRQ == 0 {
        crate::console::print(format_args!("ide: write timeout DRQ, status=0x{:x}\n", st));
        return false;
    }

    // Écrire 256 mots (512 octets)
    for i in 0..256usize {
        let word = (buf[i * 2] as u16) | ((buf[i * 2 + 1] as u16) << 8);
        outw(IDE_DATA, word);
    }

    // Attendre la fin de l'écriture
    for _ in 0..100000u32 {
        let st = inb(IDE_STATUS);
        if st & STATUS_BSY == 0 { break; }
    }

    let st = inb(IDE_STATUS);
    if st & STATUS_ERR != 0 {
        crate::console::print(format_args!("ide: write erreur finale status=0x{:x}\n", st));
        return false;
    }

    true
}

pub fn is_ready() -> bool {
    true
}
