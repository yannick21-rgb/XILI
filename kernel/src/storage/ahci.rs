//! ahci.rs — driver AHCI (SATA) pour QEMU (ich9-ahci).
//!
//! J6.1 : détection, ABAR, init port 0, lecture d'un secteur.

use crate::io::{inl, outl};

const AHCI_CAP: usize = 0x00;
const AHCI_GHC: usize = 0x04;
const AHCI_PI: usize = 0x0C;
const AHCI_VS: usize = 0x10;

const GHC_AE: u32 = 1 << 31;
const GHC_HR: u32 = 1 << 0;

const PORT_BASE: usize = 0x100;
const PORT_STRIDE: usize = 0x80;
const PORT_CLB: usize = 0x00;
const PORT_CLBU: usize = 0x04;
const PORT_FB: usize = 0x08;
const PORT_FBU: usize = 0x0C;
const PORT_IS: usize = 0x10;
const PORT_IE: usize = 0x14;
const PORT_CMD: usize = 0x18;
const PORT_TFD: usize = 0x20;
const PORT_SIG: usize = 0x24;
const PORT_SSTS: usize = 0x28;
const PORT_SCTL: usize = 0x2C;
const PORT_SERR: usize = 0x30;
const PORT_SACT: usize = 0x34;
const PORT_CI: usize = 0x38;

const PORT_CMD_ST: u32 = 1 << 0;
const PORT_CMD_FRE: u32 = 1 << 4;
const PORT_CMD_FR: u32 = 1 << 14;
const PORT_CMD_CR: u32 = 1 << 15;

const SSTS_DET_MASK: u32 = 0x0F;
const SSTS_DET_PRESENT: u32 = 0x03;

static mut AHCI_BASE: u64 = 0;
static mut AHCI_READY: bool = false;

// Port 0 command structures (allocated at init)
static mut CMD_LIST: u64 = 0;   // command list base (1 page)
static mut FIS_BASE: u64 = 0;   // received FIS (1 page)
static mut CMD_TABLE: u64 = 0;  // command table (1 page)
static mut DATA_BUF: u64 = 0;   // data buffer (1 page = 512 bytes min)

pub unsafe fn init() -> bool {
    let dev = match crate::net::virtio_net::pci_find_class(0x01, 0x06) {
        Some(v) => v,
        None => {
            crate::console::print(format_args!("ahci: aucun controleur
"));
            return false;
        }
    };
    let (bus, slot, func, _bar0) = dev;

    let abar = crate::net::virtio_net::pci_read_config(bus, slot, func, 0x24);
    if abar == 0 {
        crate::console::print(format_args!("ahci: ABAR nul
"));
        return false;
    }
    let abar_pa = (abar & 0xFFFF_FFF0) as u64;

    let cmd = crate::net::virtio_net::pci_read_config(bus, slot, func, 0x04);
    crate::net::virtio_net::pci_write_config(bus, slot, func, 0x04, cmd | 0x06);

    let vma = crate::mm::paging::HIGHER_HALF + abar_pa;
    for i in 0..4u64 {
        crate::mm::paging::map_page(
            vma + i * 0x1000,
            abar_pa + i * 0x1000,
            crate::mm::paging::PRESENT | crate::mm::paging::WRITABLE,
        );
    }
    AHCI_BASE = vma;

    let cap = mmio_read(AHCI_CAP);
    let pi = mmio_read(AHCI_PI);
    let vs = mmio_read(AHCI_VS);
    crate::console::print(format_args!(
        "ahci: CAP=0x{:x} PI=0x{:x} VS=0x{:x}
", cap, pi, vs
    ));

    let num_ports = (cap & 0x1F) + 1;
    crate::console::print(format_args!("ahci: {} ports
", num_ports));

    // Enable AHCI mode
    let ghc = mmio_read(AHCI_GHC);
    mmio_write(AHCI_GHC, ghc | GHC_AE);

    // Check port 0
    let port_base = PORT_BASE; // port 0
    let ssts = mmio_read(port_base + PORT_SSTS);
    let sig = mmio_read(port_base + PORT_SIG);
    crate::console::print(format_args!(
        "ahci: port0 SSTS=0x{:x} SIG=0x{:x}
", ssts, sig
    ));

    if (ssts & SSTS_DET_MASK) != SSTS_DET_PRESENT {
        crate::console::print(format_args!("ahci: pas de device sur port0
"));
        AHCI_READY = true;
        return true;
    }

    // Allocate command structures
    CMD_LIST = alloc_page();
    FIS_BASE = alloc_page();
    CMD_TABLE = alloc_page();
    DATA_BUF = alloc_page();
    zero_page(CMD_LIST);
    zero_page(FIS_BASE);
    zero_page(CMD_TABLE);
    zero_page(DATA_BUF);

    // Initialize port 0
    // 1. Disable command engine
    let mut cmd = mmio_read(port_base + PORT_CMD);
    cmd &= !(PORT_CMD_ST | PORT_CMD_FRE);
    mmio_write(port_base + PORT_CMD, cmd);
    // Wait for CR and FR to clear
    for _ in 0..100000u32 {
        let c = mmio_read(port_base + PORT_CMD);
        if c & (PORT_CMD_CR | PORT_CMD_FR) == 0 {
            break;
        }
    }

    // 2. Clear SERR
    mmio_write(port_base + PORT_SERR, 0xFFFFFFFF);

    // 3. Set CLB and FB
    mmio_write(port_base + PORT_CLB, CMD_LIST as u32);
    mmio_write(port_base + PORT_CLBU, (CMD_LIST >> 32) as u32);
    mmio_write(port_base + PORT_FB, FIS_BASE as u32);
    mmio_write(port_base + PORT_FBU, (FIS_BASE >> 32) as u32);
    let clb_rb = mmio_read(port_base + PORT_CLB);
    let clbu_rb = mmio_read(port_base + PORT_CLBU);
    crate::console::print(format_args!("ahci: CLB write=0x{:x} readback=0x{:x} upper=0x{:x}\n", CMD_LIST as u32, clb_rb, clbu_rb));

    // 4. Enable FIS receive
    cmd = mmio_read(port_base + PORT_CMD);
    cmd |= PORT_CMD_FRE;
    mmio_write(port_base + PORT_CMD, cmd);

    // 5. Enable command engine
    cmd = mmio_read(port_base + PORT_CMD);
    cmd |= PORT_CMD_ST;
    mmio_write(port_base + PORT_CMD, cmd);

    crate::console::print(format_args!("ahci: port0 initialise
"));

    AHCI_READY = true;
    true
}

unsafe fn alloc_page() -> u64 {
    crate::mm::phys::alloc_page().expect("ahci: alloc_page") as u64
}

unsafe fn zero_page(addr: u64) {
    core::ptr::write_bytes(addr as *mut u8, 0, 0x1000);
}

unsafe fn mmio_read(off: usize) -> u32 {
    core::ptr::read_volatile((AHCI_BASE as usize + off) as *const u32)
}

unsafe fn mmio_write(off: usize, val: u32) {
    core::ptr::write_volatile((AHCI_BASE as usize + off) as *mut u32, val);
}

/// Lit un secteur (512 octets) depuis le LBA donné sur le port 0.
pub unsafe fn read_sector(lba: u64, buf: &mut [u8; 512]) -> bool {
    if !AHCI_READY || buf.len() < 512 {
        return false;
    }
    let port_base = PORT_BASE;

    // Construire le command FIS (Register H2D) dans le command table
    let ct = CMD_TABLE;
    // CFIS à l'offset 0 (64 octets)
    let cfis = ct as *mut u8;
    // FIS type = Register H2D
    *cfis.add(0) = 0x27;
    *cfis.add(1) = 0x80; // C bit
    *cfis.add(2) = 0x25; // READ DMA EXT
    *cfis.add(3) = 0;    // features low
    // LBA low 32
    *cfis.add(4) = (lba & 0xFF) as u8;
    *cfis.add(5) = ((lba >> 8) & 0xFF) as u8;
    *cfis.add(6) = ((lba >> 16) & 0xFF) as u8;
    *cfis.add(7) = ((lba >> 24) & 0xFF) as u8;
    *cfis.add(8) = 0x40; // device = LBA mode
    *cfis.add(9) = 0;    // features high
    *cfis.add(10) = 0;
    *cfis.add(11) = 0;
    // LBA high 32
    *cfis.add(12) = ((lba >> 32) & 0xFF) as u8;
    *cfis.add(13) = ((lba >> 40) & 0xFF) as u8;
    *cfis.add(14) = ((lba >> 48) & 0xFF) as u8;
    *cfis.add(15) = ((lba >> 56) & 0xFF) as u8;
    // Count (1 sector)
    *cfis.add(16) = 1; // count low
    *cfis.add(17) = 0; // count high
    *cfis.add(18) = 0;
    *cfis.add(19) = 0;

    // PRDT à l'offset 0x80 dans le command table (1 entrée)
    let prdt = (ct + 0x80) as *mut u8;
    // Data base address low
    let dba = DATA_BUF as u32;
    *prdt.add(0) = (dba & 0xFF) as u8;
    *prdt.add(1) = ((dba >> 8) & 0xFF) as u8;
    *prdt.add(2) = ((dba >> 16) & 0xFF) as u8;
    *prdt.add(3) = ((dba >> 24) & 0xFF) as u8;
    // DBAU
    *prdt.add(4) = ((DATA_BUF >> 32) & 0xFF) as u8;
    *prdt.add(5) = (((DATA_BUF >> 32) >> 8) & 0xFF) as u8;
    *prdt.add(6) = (((DATA_BUF >> 32) >> 16) & 0xFF) as u8;
    *prdt.add(7) = (((DATA_BUF >> 32) >> 24) & 0xFF) as u8;
    // Reserved
    *prdt.add(8) = 0;
    *prdt.add(9) = 0;
    *prdt.add(10) = 0;
    *prdt.add(11) = 0;
    // Byte count: 512 bytes, IOC bit set
    let bc: u32 = 511 | (1 << 31); // IOC
    *prdt.add(12) = (bc & 0xFF) as u8;
    *prdt.add(13) = ((bc >> 8) & 0xFF) as u8;
    *prdt.add(14) = ((bc >> 16) & 0xFF) as u8;
    *prdt.add(15) = ((bc >> 24) & 0xFF) as u8;

    // Command header (slot 0)
    let cl = CMD_LIST as *mut u8;
    // CFL = 5 (20 bytes CFIS / 4), no flags
    *cl.add(0) = 5; // CFL
    *cl.add(1) = 0; // A, W, P, R, B, C flags = 0
    // PRDTL = 1
    *cl.add(2) = 1;
    *cl.add(3) = 0;
    // PRDBC = 0 (cleared by HBA)
    *cl.add(4) = 0;
    *cl.add(5) = 0;
    *cl.add(6) = 0;
    *cl.add(7) = 0;
    // CTBA (low 32)
    let ctba = CMD_TABLE as u32;
    *cl.add(8) = (ctba & 0xFF) as u8;
    *cl.add(9) = ((ctba >> 8) & 0xFF) as u8;
    *cl.add(10) = ((ctba >> 16) & 0xFF) as u8;
    *cl.add(11) = ((ctba >> 24) & 0xFF) as u8;
    // CTBAU
    *cl.add(12) = ((CMD_TABLE >> 32) & 0xFF) as u8;
    *cl.add(13) = (((CMD_TABLE >> 32) >> 8) & 0xFF) as u8;
    *cl.add(14) = (((CMD_TABLE >> 32) >> 16) & 0xFF) as u8;
    *cl.add(15) = (((CMD_TABLE >> 32) >> 24) & 0xFF) as u8;

    // Fill DATA_BUF with 0xAA (test)
    core::ptr::write_bytes(DATA_BUF as *mut u8, 0xAA, 512);

    // Clear SERR
    mmio_write(port_base + PORT_SERR, 0xFFFFFFFF);

    // Clear IS
    mmio_write(port_base + PORT_IS, 0xFFFFFFFF);

    // Issue command (slot 0)
    mmio_write(port_base + PORT_CI, 1);

    // Wait for completion
    for _ in 0..1000000u32 {
        let ci = mmio_read(port_base + PORT_CI);
        if ci & 1 == 0 {
            break;
        }
    }

    let ci = mmio_read(port_base + PORT_CI);
    let tfd = mmio_read(port_base + PORT_TFD);
    let is = mmio_read(port_base + PORT_IS);

    if ci & 1 != 0 {
        crate::console::print(format_args!(
            "ahci: read_sector timeout, CI=0x{:x} TFD=0x{:x} IS=0x{:x}
",
            ci, tfd, is
        ));
        return false;
    }
    if tfd & 0x01 != 0 {
        crate::console::print(format_args!(
            "ahci: read_sector error, TFD=0x{:x}
", tfd
        ));
        return false;
    }

    let serr = mmio_read(port_base + PORT_SERR);

    // Copier les données
    core::ptr::copy_nonoverlapping(DATA_BUF as *const u8, buf.as_mut_ptr(), 512);

    true
}

pub fn is_ready() -> bool {
    unsafe { AHCI_READY }
}
