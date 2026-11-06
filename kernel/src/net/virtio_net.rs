//! virtio-net — driver réseau pour QEMU (virtio PCI, mode legacy).
//!
//! Implémente :
//! - Détection et configuration du périphérique PCI
//! - Virtqueues RX (0) et TX (1) en mode legacy (registre PFN)
//! - `send()` : émission d'une trame Ethernet
//! - `recv()` : réception d'une trame Ethernet
//! - `arp_probe()` : émet un ARP « who-has » et attend la réponse

use crate::io::{inl, outl};
use crate::mm::phys::alloc_page;

// ---------------------------------------------------------------------------
// Constantes PCI / virtio
// ---------------------------------------------------------------------------

const PCI_VENDOR_ID: u16 = 0x1AF4;
const PCI_DEVICE_ID: u16 = 0x1000;

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

const PCI_BAR0: u8 = 0x10;
const PCI_COMMAND: u8 = 0x04;

const VIRTIO_MAGIC: u32 = 0x7472_6976; // "virt"
const VIRTIO_DEVICE_ID_NET: u32 = 0x1;

const VIRTIO_OFF_MAGIC: usize = 0x000;
const VIRTIO_OFF_VERSION: usize = 0x004;
const VIRTIO_OFF_DEVICE_ID: usize = 0x008;
const VIRTIO_OFF_DEVICE_FEATURES: usize = 0x010;
const VIRTIO_OFF_DEVICE_FEATURES_SEL: usize = 0x014;
const VIRTIO_OFF_DRIVER_FEATURES: usize = 0x020;
const VIRTIO_OFF_DRIVER_FEATURES_SEL: usize = 0x024;
const VIRTIO_OFF_QUEUE_SEL: usize = 0x030;
const VIRTIO_OFF_QUEUE_NUM_MAX: usize = 0x034;
const VIRTIO_OFF_QUEUE_NUM: usize = 0x038;
const VIRTIO_OFF_QUEUE_READY: usize = 0x044;
const VIRTIO_OFF_QUEUE_NOTIFY: usize = 0x050;
const VIRTIO_OFF_STATUS: usize = 0x070;

const VIRTIO_STATUS_ACK: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;

// Feature bits (mode moderne)
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;

// ---------------------------------------------------------------------------
// Virtqueue (format legacy)
// ---------------------------------------------------------------------------

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; 256],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; 256],
}

const VIRTQ_NUM: usize = 8;
const VIRTQ_DESC_SIZE: usize = VIRTQ_NUM * 16;
const VIRTQ_AVAIL_SIZE: usize = 4 + VIRTQ_NUM * 2;
const VIRTQ_USED_SIZE: usize = 4 + VIRTQ_NUM * 8;

struct Virtq {
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
    next: u16,
    last_used: u16,
    phys: u64,
}

// ---------------------------------------------------------------------------
// État global du driver
// ---------------------------------------------------------------------------

static mut VIRTIO_NET_BASE: u32 = 0;
static mut VIRTIO_NET_IRQ: u8 = 0;
static mut VIRTIO_NET_IS_IO: bool = false;
static mut VIRTIO_NET_READY: bool = false;

static mut RXQ: Virtq = Virtq {
    desc: core::ptr::null_mut(),
    avail: core::ptr::null_mut(),
    used: core::ptr::null_mut(),
    next: 0,
    last_used: 0,
    phys: 0,
};

static mut TXQ: Virtq = Virtq {
    desc: core::ptr::null_mut(),
    avail: core::ptr::null_mut(),
    used: core::ptr::null_mut(),
    next: 0,
    last_used: 0,
    phys: 0,
};

static mut RX_COUNT: u64 = 0;
static mut TX_COUNT: u64 = 0;

// ---------------------------------------------------------------------------
// Helpers PCI
// ---------------------------------------------------------------------------

unsafe fn pci_read_config(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(PCI_CONFIG_ADDRESS, address);
    inl(PCI_CONFIG_DATA)
}

unsafe fn pci_write_config(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let address = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(PCI_CONFIG_ADDRESS, address);
    outl(PCI_CONFIG_DATA, value);
}

unsafe fn pci_find_device(vendor: u16, device: u16) -> Option<(u8, u8, u8, u32)> {
    // Le bus 0 suffit sur QEMU (tous les périphériques y sont).
    for slot in 0..32u8 {
        for func in 0..8u8 {
            let cfg = pci_read_config(0, slot, func, 0);
            let v = (cfg & 0xFFFF) as u16;
            let d = ((cfg >> 16) & 0xFFFF) as u16;
            if v == vendor && d == device {
                let bar0 = pci_read_config(0, slot, func, PCI_BAR0);
                return Some((0, slot, func, bar0));
            }
        }
    }
    None
}

unsafe fn pci_dump_all() {
    crate::console::print(format_args!("PCI scan begin\n"));
    for slot in 0..32u8 {
        let cfg = pci_read_config(0, slot, 0, 0);
        let v = (cfg & 0xFFFF) as u16;
        let d = ((cfg >> 16) & 0xFFFF) as u16;
        if v == 0xFFFF {
            continue;
        }
        let cls = pci_read_config(0, slot, 0, 0x08);
        let class_code = (cls >> 24) & 0xFF;
        let subclass = (cls >> 16) & 0xFF;
        let bar0 = pci_read_config(0, slot, 0, PCI_BAR0);
        crate::console::print(format_args!(
            "PCI 00:{:02x}.0 vend=0x{:04x} dev=0x{:04x} class=0x{:02x}:0x{:02x} bar0=0x{:08x}\n",
            slot, v, d, class_code, subclass, bar0
        ));
    }
    crate::console::print(format_args!("PCI scan end\n"));
}

// ---------------------------------------------------------------------------
// Helpers virtio
// ---------------------------------------------------------------------------

unsafe fn virtio_read(base: u32, off: usize) -> u32 {
    if VIRTIO_NET_IS_IO {
        // Le registre virtio legacy est accessible en 8 bits.
        let b0 = crate::io::inb((base + off as u32) as u16) as u32;
        let b1 = crate::io::inb((base + off as u32 + 1) as u16) as u32;
        let b2 = crate::io::inb((base + off as u32 + 2) as u16) as u32;
        let b3 = crate::io::inb((base + off as u32 + 3) as u16) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    } else {
        core::ptr::read_volatile((base as *const u32).add(off / 4))
    }
}

unsafe fn virtio_write(base: u32, off: usize, val: u32) {
    if VIRTIO_NET_IS_IO {
        crate::io::outb((base + off as u32) as u16, val as u8);
        crate::io::outb((base + off as u32 + 1) as u16, (val >> 8) as u8);
        crate::io::outb((base + off as u32 + 2) as u16, (val >> 16) as u8);
        crate::io::outb((base + off as u32 + 3) as u16, (val >> 24) as u8);
    } else {
        core::ptr::write_volatile((base as *mut u32).add(off / 4), val);
    }
}

/// Lit un mot de 64 bits de features (2 registres de 32 bits).
unsafe fn virtio_features(base: u32) -> u64 {
    virtio_write(base, VIRTIO_OFF_DEVICE_FEATURES_SEL, 0);
    let lo = virtio_read(base, VIRTIO_OFF_DEVICE_FEATURES) as u64;
    virtio_write(base, VIRTIO_OFF_DEVICE_FEATURES_SEL, 1);
    let hi = virtio_read(base, VIRTIO_OFF_DEVICE_FEATURES) as u64;
    lo | (hi << 32)
}

/// Écrit un mot de 64 bits de features du driver.
unsafe fn virtio_driver_features(base: u32, features: u64) {
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES_SEL, 0);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES, features as u32);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES_SEL, 1);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES, (features >> 32) as u32);
}

unsafe fn virtq_init(base: u32, idx: u16, q: *mut Virtq) {
    let page = alloc_page().expect("virtio_net: alloc_page");
    let va = page as u64;
    core::ptr::write_bytes(va as *mut u8, 0, 0x1000);

    let desc = va as *mut VirtqDesc;
    let avail = (va + VIRTQ_DESC_SIZE as u64) as *mut VirtqAvail;
    let used = (va + (VIRTQ_DESC_SIZE + VIRTQ_AVAIL_SIZE) as u64) as *mut VirtqUsed;

    virtio_write(base, VIRTIO_OFF_QUEUE_SEL, idx as u32);
    let num_max = virtio_read(base, VIRTIO_OFF_QUEUE_NUM_MAX);
    if num_max < VIRTQ_NUM as u32 {
        crate::console::print(format_args!(
            "virtio-net: queue {} num_max {} < {}\n",
            idx, num_max, VIRTQ_NUM
        ));
    }
    virtio_write(base, VIRTIO_OFF_QUEUE_NUM, VIRTQ_NUM as u32);

    // Adresses physiques des structures (mode moderne).
    let desc_pa = va;
    let avail_pa = va + VIRTQ_DESC_SIZE as u64;
    let used_pa = va + (VIRTQ_DESC_SIZE + VIRTQ_AVAIL_SIZE) as u64;
    virtio_write(base, 0x060, desc_pa as u32);
    virtio_write(base, 0x064, (desc_pa >> 32) as u32);
    virtio_write(base, 0x068, avail_pa as u32);
    virtio_write(base, 0x06C, (avail_pa >> 32) as u32);
    virtio_write(base, 0x070, used_pa as u32);
    virtio_write(base, 0x074, (used_pa >> 32) as u32);

    // Activer la queue.
    virtio_write(base, VIRTIO_OFF_QUEUE_READY, 1);

    (*q).desc = desc;
    (*q).avail = avail;
    (*q).used = used;
    (*q).next = 0;
    (*q).last_used = 0;
    (*q).phys = va;
}

// ---------------------------------------------------------------------------
// RX : poster des descripteurs
// ---------------------------------------------------------------------------

unsafe fn rx_post(q: *mut Virtq, buf_phys: u64, len: u16) {
    let i = (*q).next as usize;
    let desc = &mut *(*q).desc.add(i);
    desc.addr = buf_phys;
    desc.len = len as u32;
    desc.flags = VIRTQ_DESC_F_WRITE;
    desc.next = 0;

    let avail = &mut *(*q).avail;
    let avail_idx = avail.idx as usize;
    avail.ring[avail_idx % VIRTQ_NUM] = i as u16;
    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    avail.idx = avail_idx.wrapping_add(1) as u16;

    (*q).next = ((*q).next + 1) % VIRTQ_NUM as u16;
}

// ---------------------------------------------------------------------------
// API publique
// ---------------------------------------------------------------------------

/// Initialise le driver virtio-net.
pub unsafe fn init() -> bool {
    pci_dump_all();

    let (bus, slot, func, bar0) = match pci_find_device(PCI_VENDOR_ID, PCI_DEVICE_ID) {
        Some(v) => v,
        None => return false,
    };

    let base = bar0 & 0xFFFF_FFF0;
    VIRTIO_NET_BASE = base;
    VIRTIO_NET_IS_IO = (bar0 & 1) != 0;

    // Activer le périphérique AVANT les lectures : I/O space + bus master.
    // Sans le bit 0 du command register, les accès ports I/O sont ignorés.
    let cmd = pci_read_config(bus, slot, func, PCI_COMMAND);
    pci_write_config(bus, slot, func, PCI_COMMAND, cmd | 0x05);

    // Le registre virtio est dans le BAR4 (MMIO, 0xfe00000c → base 0xfe000000).
    // C'est le registre moderne : le magic est à l'offset 0x000.
    let mut mmio_base: u32 = 0;
    for i in 0..6u8 {
        let bar = pci_read_config(bus, slot, func, PCI_BAR0 + i * 4);
        if bar != 0 && bar & 1 == 0 {
            mmio_base = bar & 0xFFFF_FFF0;
        }
    }
    if mmio_base != 0 {
        // Mapper le BAR MMIO : 16 pages (64 Kio) couvrent les registres.
        for i in 0..16u64 {
            let vaddr = mmio_base as u64 + i * 0x1000;
            let pa = mmio_base as u64 + i * 0x1000;
            unsafe {
                crate::mm::paging::map_page(
                    vaddr,
                    pa,
                    crate::mm::paging::PRESENT | crate::mm::paging::WRITABLE,
                );
            }
        }
        VIRTIO_NET_BASE = mmio_base;
        VIRTIO_NET_IS_IO = false;
    }

    // Activer le périphérique : I/O space + bus master.
    let cmd = pci_read_config(bus, slot, func, PCI_COMMAND);
    pci_write_config(bus, slot, func, PCI_COMMAND, cmd | 0x05);

    // Lire le magic APRÈS l'activation du command register.
    let magic = virtio_read(VIRTIO_NET_BASE, VIRTIO_OFF_MAGIC);
    let version = virtio_read(VIRTIO_NET_BASE, VIRTIO_OFF_VERSION);
    let device_id = virtio_read(VIRTIO_NET_BASE, VIRTIO_OFF_DEVICE_ID);

    if magic != VIRTIO_MAGIC || device_id != VIRTIO_DEVICE_ID_NET {
        crate::console::print(format_args!(
            "virtio-net: rejet magic=0x{:x} dev={}\n",
            magic, device_id
        ));
        return false;
    }

    // Négociation de features (mode moderne).
    let features = virtio_features(VIRTIO_NET_BASE);
    crate::console::print(format_args!(
        "virtio-net: device_features=0x{:x}\n",
        features
    ));
    let wanted = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
    virtio_driver_features(VIRTIO_NET_BASE, wanted);
    virtio_write(
        VIRTIO_NET_BASE,
        VIRTIO_OFF_STATUS,
        VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK,
    );
    let status = virtio_read(VIRTIO_NET_BASE, VIRTIO_OFF_STATUS);
    if status & VIRTIO_STATUS_FEATURES_OK == 0 {
        crate::console::print(format_args!(
            "virtio-net: FEATURES_OK refuse (status=0x{:x})\n",
            status
        ));
        return false;
    }

    virtq_init(VIRTIO_NET_BASE, 0, &mut RXQ);
    virtq_init(VIRTIO_NET_BASE, 1, &mut TXQ);

    virtio_write(
        VIRTIO_NET_BASE,
        VIRTIO_OFF_STATUS,
        VIRTIO_STATUS_ACK
            | VIRTIO_STATUS_DRIVER
            | VIRTIO_STATUS_FEATURES_OK
            | VIRTIO_STATUS_DRIVER_OK,
    );

    let irq_cfg = pci_read_config(bus, slot, func, 0x3C);
    VIRTIO_NET_IRQ = (irq_cfg & 0xFF) as u8;
    VIRTIO_NET_READY = true;

    crate::console::print(format_args!(
        "virtio-net: device PCI trouve, IRQ={}, base=0x{:x}\n",
        VIRTIO_NET_IRQ, base
    ));

    true
}

/// Poste les descripteurs RX initiaux.
pub unsafe fn rx_init_buffers() {
    if !VIRTIO_NET_READY {
        return;
    }
    for _ in 0..VIRTQ_NUM {
        let page = alloc_page().expect("virtio_net: rx buffer");
        core::ptr::write_bytes(page as *mut u8, 0, 0x1000);
        rx_post(&mut RXQ, page as u64, 0x1000);
    }
    // Notifier le device qu'il y a des descripteurs disponibles.
    virtio_write(VIRTIO_NET_BASE, VIRTIO_OFF_QUEUE_NOTIFY, 0);
    crate::console::print(format_args!("virtio-net: {} tampons RX postes\n", VIRTQ_NUM));
}

/// Envoie une trame Ethernet brute.
pub unsafe fn send(data: &[u8]) -> usize {
    if !VIRTIO_NET_READY || data.is_empty() {
        return 0;
    }

    let page = match alloc_page() {
        Some(p) => p,
        None => return 0,
    };
    let n = core::cmp::min(data.len(), 0x1000);
    core::ptr::copy_nonoverlapping(data.as_ptr(), page as *mut u8, n);

    let i = TXQ.next as usize;
    let desc = &mut *TXQ.desc.add(i);
    desc.addr = page as u64;
    desc.len = n as u32;
    desc.flags = 0;
    desc.next = 0;

    let avail = &mut *TXQ.avail;
    let avail_idx = avail.idx as usize;
    avail.ring[avail_idx % VIRTQ_NUM] = i as u16;
    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    avail.idx = avail_idx.wrapping_add(1) as u16;
    TXQ.next = (TXQ.next + 1) % VIRTQ_NUM as u16;

    // Notifier le device.
    virtio_write(VIRTIO_NET_BASE, VIRTIO_OFF_QUEUE_NOTIFY, 1);

    TX_COUNT += 1;
    n
}

/// Reçoit une trame Ethernet brute.
pub unsafe fn recv(buf: &mut [u8]) -> usize {
    if !VIRTIO_NET_READY {
        return 0;
    }

    let rx_used = (*RXQ.used).idx as usize;
    if rx_used == RXQ.last_used as usize {
        return 0;
    }

    let elem = (*RXQ.used).ring[rx_used % VIRTQ_NUM];
    let elem_id = elem.id;
    let elem_len = elem.len;
    RXQ.last_used = rx_used.wrapping_add(1) as u16;
    RX_COUNT += 1;

    let desc = &*RXQ.desc.add(elem_id as usize);
    let src = desc.addr as *const u8;
    let n = core::cmp::min(elem_len as usize, buf.len());
    core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), n);

    n
}

/// Traite les interruptions réseau (appelé depuis le handler IRQ).
pub unsafe fn handle_irq() {
    if !VIRTIO_NET_READY {
        return;
    }
    let rx_used = (*RXQ.used).idx as usize;
    if rx_used != RXQ.last_used as usize {
        RXQ.last_used = rx_used as u16;
        RX_COUNT += 1;
        crate::serial::write_byte(b'R');
    }
}

pub fn rx_count() -> u64 {
    unsafe { RX_COUNT }
}

pub fn tx_count() -> u64 {
    unsafe { TX_COUNT }
}

/// Construit une trame Ethernet + ARP « who-has » (42 octets).
pub fn build_arp_who_has(
    buf: &mut [u8],
    src_mac: &[u8; 6],
    sender_ip: [u8; 4],
    target_ip: [u8; 4],
) -> usize {
    buf[0..6].copy_from_slice(&[0xFF; 6]);
    buf[6..12].copy_from_slice(src_mac);
    buf[12..14].copy_from_slice(&[0x08, 0x06]);
    buf[14..16].copy_from_slice(&[0x00, 0x01]);
    buf[16..18].copy_from_slice(&[0x08, 0x00]);
    buf[18] = 6;
    buf[19] = 4;
    buf[20..22].copy_from_slice(&[0x00, 0x01]);
    buf[22..28].copy_from_slice(src_mac);
    buf[28..32].copy_from_slice(&sender_ip);
    buf[32..38].copy_from_slice(&[0x00; 6]);
    buf[38..42].copy_from_slice(&target_ip);
    42
}

/// Vérifie si une trame est une réponse ARP pour `target_ip`.
pub fn is_arp_reply(buf: &[u8], target_ip: [u8; 4]) -> bool {
    if buf.len() < 42 {
        return false;
    }
    if buf[12..14] != [0x08, 0x06] {
        return false;
    }
    if buf[20..22] != [0x00, 0x02] {
        return false;
    }
    buf[38..42] == target_ip
}

/// Sonde ARP : émet un « who-has » et attend la réponse.
pub unsafe fn arp_probe(target_ip: [u8; 4], timeout_ticks: u64) -> bool {
    if !VIRTIO_NET_READY {
        return false;
    }

    let src_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let sender_ip = [10, 0, 2, 15];

    let mut frame = [0u8; 64];
    let n = build_arp_who_has(&mut frame, &src_mac, sender_ip, target_ip);
    let sent = send(&frame[..n]);
    crate::console::print(format_args!(
        "virtio-net: ARP who-has {:?} envoye ({} octets)\n",
        target_ip, sent
    ));

    let start = crate::irq::ticks();
    let mut buf = [0u8; 64];
    while crate::irq::ticks() - start < timeout_ticks {
        let got = recv(&mut buf);
        if got >= 42 && is_arp_reply(&buf, target_ip) {
            crate::console::print(format_args!(
                "virtio-net: ARP reply recu de {:?}\n",
                target_ip
            ));
            return true;
        }
    }

    crate::console::print(format_args!(
        "virtio-net: ARP timeout apres {} ticks\n",
        timeout_ticks
    ));
    false
}
