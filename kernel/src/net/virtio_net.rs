//! virtio-net — driver réseau pour QEMU (virtio PCI, mode legacy).
//!
//! Implémente :
//! - Détection et configuration du périphérique PCI
//! - Virtqueues RX (0) et TX (1) en mode legacy (registre PFN)
//! - `send()` : émission d'une trame Ethernet
//! - `recv()` : réception d'une trame Ethernet
//! - `arp_probe()` : émet un ARP « who-has » et attend la réponse

use crate::io::{inb, inl, inw, outb, outl, outw};
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
const PCI_STATUS: u8 = 0x06;
const PCI_CAPABILITY_LIST: u8 = 0x34;

// Capability IDs
const PCI_CAP_VENDOR: u8 = 0x09; // vendor-specific (virtio)

// Structure capability virtio (PCI_CAP_VENDOR)
// offset 0: cap_id (1), next (1), cap_len (2)
// offset 4: cfg_type (1), bar (1), offset (4), length (4)
const VIRTIO_CAP_CFG_TYPE: usize = 0x04;
const VIRTIO_CAP_BAR: usize = 0x05;
const VIRTIO_CAP_OFFSET: usize = 0x08;
const VIRTIO_CAP_LENGTH: usize = 0x0C;

// cfg_type values
const VIRTIO_CAP_COMMON: u8 = 1;
const VIRTIO_CAP_NOTIFY: u8 = 2;
const VIRTIO_CAP_ISR: u8 = 3;
const VIRTIO_CAP_DEVICE: u8 = 4;
const VIRTIO_CAP_CFG: u8 = 5;

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

const VIRTQ_NUM: usize = 256;
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

static mut VIRTIO_NET_BASE: u64 = 0;
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

pub unsafe fn pci_read_config(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(PCI_CONFIG_ADDRESS, address);
    inl(PCI_CONFIG_DATA)
}

unsafe fn pci_read_config_byte(bus: u8, slot: u8, func: u8, offset: u8) -> u8 {
    let dword = pci_read_config(bus, slot, func, offset);
    ((dword >> ((offset & 0x03) * 8)) & 0xFF) as u8
}

/// Parcourt la liste des capacités PCI et retourne les structures virtio.
/// Retourne (common_base, common_off, notify_base, notify_off, device_base, device_off).
unsafe fn pci_find_virtio_caps(
    bus: u8,
    slot: u8,
    func: u8,
) -> (u32, u32, u32, u32, u32, u32) {
    let status = pci_read_config(bus, slot, func, PCI_STATUS) >> 16;
    if status & 0x10 == 0 {
        // bit 4 : capabilities list present
        crate::console::print(format_args!("virtio-net: pas de capabilities\n"));
        return (0, 0, 0, 0, 0, 0);
    }

    let mut cap_ptr = pci_read_config_byte(bus, slot, func, PCI_CAPABILITY_LIST);
    let mut common_base = 0u32;
    let mut common_off = 0u32;
    let mut notify_base = 0u32;
    let mut notify_off = 0u32;
    let mut device_base = 0u32;
    let mut device_off = 0u32;

    while cap_ptr != 0 && cap_ptr != 0xFF {
        let cap_id = pci_read_config_byte(bus, slot, func, cap_ptr);
        let next = pci_read_config_byte(bus, slot, func, cap_ptr + 1);

        if cap_id == PCI_CAP_VENDOR {
            // Structure capability virtio
            // offset 0: cap_id (1), next (1), cap_len (2)
            // offset 4: cfg_type (1), bar (1), offset (4), length (4)
            let cap_dword = pci_read_config(bus, slot, func, cap_ptr);
            let cfg_type = ((cap_dword >> 24) & 0xFF) as u8;
            let bar = ((cap_dword >> 16) & 0xFF) as u8;
            let off = pci_read_config(bus, slot, func, cap_ptr + 8);
            let len = pci_read_config(bus, slot, func, cap_ptr + 12);

            let bar_val = pci_read_config(bus, slot, func, PCI_BAR0 + bar as u8 * 4);
            // Le BAR peut être I/O (bit 0 = 1) ou MMIO (bit 0 = 0).
            // Pour le mode moderne, on veut le BAR MMIO.
            // Si le BAR est I/O, on prend le BAR1 (MMIO) à la place.
            let base = if bar_val & 1 == 0 {
                bar_val & 0xFFFF_FFF0
            } else {
                // BAR I/O : lire le BAR1 (MMIO) pour les registres modernes.
                let bar1 = pci_read_config(bus, slot, func, PCI_BAR0 + 4);
                if bar1 & 1 == 0 {
                    bar1 & 0xFFFF_FFF0
                } else {
                    0
                }
            };

            crate::console::print(format_args!(
                "virtio-net: cap virtio type={} bar={} off=0x{:x} len=0x{:x} base=0x{:x}\n",
                cfg_type, bar, off, len, base
            ));

            match cfg_type {
                VIRTIO_CAP_COMMON => {
                    common_base = base;
                    common_off = off;
                }
                VIRTIO_CAP_NOTIFY => {
                    notify_base = base;
                    notify_off = off;
                }
                VIRTIO_CAP_DEVICE => {
                    device_base = base;
                    device_off = off;
                }
                _ => {}
            }
        }

        cap_ptr = next;
    }

    (common_base, common_off, notify_base, notify_off, device_base, device_off)
}

pub unsafe fn pci_write_config(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let address = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(PCI_CONFIG_ADDRESS, address);
    outl(PCI_CONFIG_DATA, value);
}

pub unsafe fn pci_find_class(class: u8, subclass: u8) -> Option<(u8, u8, u8, u32)> {
    for slot in 0..32u8 {
        for func in 0..8u8 {
            let cfg = pci_read_config(0, slot, func, 0);
            let v = (cfg & 0xFFFF) as u16;
            if v == 0xFFFF {
                continue;
            }
            let cls = pci_read_config(0, slot, func, 0x08);
            let class_code = ((cls >> 24) & 0xFF) as u8;
            let sub = ((cls >> 16) & 0xFF) as u8;
            if class_code == class && sub == subclass {
                let bar0 = pci_read_config(0, slot, func, 0x10);
                return Some((0, slot, func, bar0));
            }
        }
    }
    None
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

unsafe fn virtio_read(base: u64, off: usize) -> u32 {
    if VIRTIO_NET_IS_IO {
        // Le registre virtio legacy est accessible en 8 bits.
        let b0 = crate::io::inb((base + off as u64) as u16) as u32;
        let b1 = crate::io::inb((base + off as u64 + 1) as u16) as u32;
        let b2 = crate::io::inb((base + off as u64 + 2) as u16) as u32;
        let b3 = crate::io::inb((base + off as u64 + 3) as u16) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    } else {
        core::ptr::read_volatile((base as *const u32).add(off / 4))
    }
}

unsafe fn virtio_write(base: u64, off: usize, val: u32) {
    if VIRTIO_NET_IS_IO {
        crate::io::outb((base + off as u64) as u16, val as u8);
        crate::io::outb((base + off as u64 + 1) as u16, (val >> 8) as u8);
        crate::io::outb((base + off as u64 + 2) as u16, (val >> 16) as u8);
        crate::io::outb((base + off as u64 + 3) as u16, (val >> 24) as u8);
    } else {
        core::ptr::write_volatile((base as *mut u32).add(off / 4), val);
    }
}

/// Lit un mot de 64 bits de features (2 registres de 32 bits).
unsafe fn virtio_features(base: u64) -> u64 {
    virtio_write(base, VIRTIO_OFF_DEVICE_FEATURES_SEL, 0);
    let lo = virtio_read(base, VIRTIO_OFF_DEVICE_FEATURES) as u64;
    virtio_write(base, VIRTIO_OFF_DEVICE_FEATURES_SEL, 1);
    let hi = virtio_read(base, VIRTIO_OFF_DEVICE_FEATURES) as u64;
    lo | (hi << 32)
}

/// Écrit un mot de 64 bits de features du driver.
unsafe fn virtio_driver_features(base: u64, features: u64) {
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES_SEL, 0);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES, features as u32);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES_SEL, 1);
    virtio_write(base, VIRTIO_OFF_DRIVER_FEATURES, (features >> 32) as u32);
}

/// Initialise le driver virtio-net (mode legacy, BAR0 I/O).
pub unsafe fn init() -> bool {
    pci_dump_all();

    let (bus, slot, func, bar0) = match pci_find_device(PCI_VENDOR_ID, PCI_DEVICE_ID) {
        Some(v) => v,
        None => return false,
    };

    let io_base = (bar0 & 0xFFFF_FFFC) as u16;

    // Activer le périphérique : I/O space + mémoire + bus master.
    let cmd = pci_read_config(bus, slot, func, PCI_COMMAND);
    pci_write_config(bus, slot, func, PCI_COMMAND, cmd | 0x07);

    VIRTIO_NET_BASE = io_base as u64;
    VIRTIO_NET_IS_IO = true;

    // Reset puis ACK + DRIVER.
    outb(io_base + 18, 0);
    outb(io_base + 18, (VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER) as u8);

    // Features : on accepte VIRTIO_NET_F_MAC (bit 5).
    let _host_features = inl(io_base);
    outl(io_base + 4, VIRTIO_NET_F_MAC as u32);

    // Queues RX (0) et TX (1).
    virtq_init(VIRTIO_NET_BASE, 0, &mut RXQ);
    virtq_init(VIRTIO_NET_BASE, 1, &mut TXQ);

    // DRIVER_OK.
    outb(
        io_base + 18,
        (VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_DRIVER_OK) as u8,
    );

    let irq_cfg = pci_read_config(bus, slot, func, 0x3C);
    VIRTIO_NET_IRQ = (irq_cfg & 0xFF) as u8;
    VIRTIO_NET_READY = true;

    crate::console::print(format_args!(
        "virtio-net: device PCI trouve, IRQ={}, base=0x{:x}\n",
        VIRTIO_NET_IRQ, io_base
    ));

    true
}

unsafe fn virtq_init(base: u64, idx: u16, q: *mut Virtq) {
    // Allouer 4 pages contiguës pour le vring (desc@0, avail@4096, used@8192).
    let page = alloc_page().expect("virtio_net: alloc_page");
    let p2 = alloc_page().expect("virtio_net: alloc_page2");
    let p3 = alloc_page().expect("virtio_net: alloc_page3");
    let p4 = alloc_page().expect("virtio_net: alloc_page4");
    let va = page as u64;
    // Vérifier la contiguïté.
    if p2 != page + 0x1000 || p3 != page + 0x2000 || p4 != page + 0x3000 {
        crate::console::print(format_args!(
            "virtio-net: pages non contigues {} {} {} {}\n",
            page, p2, p3, p4
        ));
    }
    core::ptr::write_bytes(va as *mut u8, 0, 0x4000);

    let desc = va as *mut VirtqDesc;
    let avail = (va + 0x1000) as *mut VirtqAvail;
    let used = (va + 0x2000) as *mut VirtqUsed;

    let port = base as u16;
    // Sélectionner la queue.
    outw(port + 14, idx);
    // Réduire la taille de la queue (optionnel) + lecture du max.
    outw(port + 12, VIRTQ_NUM as u16);
    let num_max = inw(port + 12);
    if num_max < VIRTQ_NUM as u16 {
        crate::console::print(format_args!(
            "virtio-net: queue {} num_max {} < {}\n",
            idx, num_max, VIRTQ_NUM
        ));
    }
    // Adresse physique de la queue (PFN = page >> 12).
    outl(port + 8, (page >> 12) as u32);

    (*q).desc = desc;
    (*q).avail = avail;
    (*q).used = used;
    (*q).next = 0;
    (*q).last_used = 0;
    (*q).phys = va;
}

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

/// Poste les descripteurs RX initiaux.
pub unsafe fn rx_init_buffers() {
    if !VIRTIO_NET_READY {
        return;
    }
    for _ in 0..16 {
        let page = alloc_page().expect("virtio_net: rx buffer");
        core::ptr::write_bytes(page as *mut u8, 0, 0x1000);
        rx_post(&mut RXQ, page as u64, 0x1000);
    }
// Notifier le device qu'il y a des descripteurs disponibles.
    crate::io::outw(VIRTIO_NET_BASE as u16 + 16, 0);
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
    let n = core::cmp::min(data.len(), 0x1000 - 10);
    // En-tête virtio-net (10 octets) suivi de la trame Ethernet.
    core::ptr::write_bytes(page as *mut u8, 0, 10);
    core::ptr::copy_nonoverlapping(data.as_ptr(), (page as *mut u8).add(10), n);

    let i = TXQ.next as usize;
    let desc = &mut *TXQ.desc.add(i);
    desc.addr = page as u64;
    desc.len = (n + 10) as u32;
    desc.flags = 0;
    desc.next = 0;

    let avail = &mut *TXQ.avail;
    let avail_idx = avail.idx as usize;
    avail.ring[avail_idx % VIRTQ_NUM] = i as u16;
    core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
    avail.idx = avail_idx.wrapping_add(1) as u16;
    TXQ.next = (TXQ.next + 1) % VIRTQ_NUM as u16;

    // Notifier le device.
    crate::io::outw(VIRTIO_NET_BASE as u16 + 16, 1);

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

    let elem = (*RXQ.used).ring[RXQ.last_used as usize % VIRTQ_NUM];
    let elem_id = elem.id;
    let elem_len = elem.len;
    RXQ.last_used = RXQ.last_used.wrapping_add(1) as u16;
    RX_COUNT += 1;

    let desc = &*RXQ.desc.add(elem_id as usize);
    let src = desc.addr as *const u8;
    let total = elem_len as usize;
    let payload = total.saturating_sub(10);
    let n = core::cmp::min(payload, buf.len());
    core::ptr::copy_nonoverlapping(src.add(10), buf.as_mut_ptr(), n);

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
    buf[28..32] == target_ip
}

/// Résout une IP en MAC via ARP. Retourne la MAC de l'émetteur.
pub unsafe fn arp_resolve(target_ip: [u8; 4], timeout_ticks: u64) -> Option<[u8; 6]> {
    if !VIRTIO_NET_READY {
        return None;
    }
    let src_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let sender_ip = [10, 0, 2, 15];

    let mut frame = [0u8; 64];
    let n = build_arp_who_has(&mut frame, &src_mac, sender_ip, target_ip);
    send(&frame[..n]);

    let start = crate::irq::ticks();
    let mut buf = [0u8; 64];
    while crate::irq::ticks() - start < timeout_ticks {
        let got = recv(&mut buf);
        if got >= 42 && buf[12..14] == [0x08, 0x06] && buf[20..22] == [0x00, 0x02] {
            // ARP reply : sender MAC at offset 22, sender IP at offset 28
            if buf[28..32] == target_ip {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&buf[22..28]);
                return Some(mac);
            }
        }
    }
    None
}

/// Calcule le checksum IP (sur l'en-tête IP de 20 octets).
fn ip_checksum(hdr: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < hdr.len() {
        let word = ((hdr[i] as u32) << 8) | (hdr[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < hdr.len() {
        sum = sum.wrapping_add((hdr[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Calcule le checksum ICMP (sur l'en-tête + payload).
fn icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Ping ICMP : envoie un echo request et attend l'echo reply.
pub unsafe fn ping(target_ip: [u8; 4], timeout_ticks: u64) -> bool {
    if !VIRTIO_NET_READY {
        return false;
    }
    let src_mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let src_ip = [10, 0, 2, 15];

    // Résoudre la MAC de la cible.
    let dst_mac = match arp_resolve(target_ip, 100) {
        Some(m) => m,
        None => {
            crate::console::print(format_args!("ping: ARP resolve echec\n"));
            return false;
        }
    };
    crate::console::print(format_args!(
        "ping: MAC cible = {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
        dst_mac[0], dst_mac[1], dst_mac[2], dst_mac[3], dst_mac[4], dst_mac[5]
    ));

    // Construire la trame ICMP echo request.
    let mut frame = [0u8; 64];
    // Ethernet header (14 bytes)
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&src_mac);
    frame[12..14].copy_from_slice(&[0x08, 0x00]); // IPv4

    // IP header (20 bytes, offset 14)
    let ip_off = 14;
    frame[ip_off] = 0x45; // version 4, IHL 5
    frame[ip_off + 1] = 0; // TOS
    let total_len: u16 = 20 + 8 + 4; // IP + ICMP + 4 bytes payload
    frame[ip_off + 2] = (total_len >> 8) as u8;
    frame[ip_off + 3] = (total_len & 0xFF) as u8;
    frame[ip_off + 4] = 0; // ID high
    frame[ip_off + 5] = 0x01; // ID low
    frame[ip_off + 6] = 0; // flags/frag high
    frame[ip_off + 7] = 0; // frag low
    frame[ip_off + 8] = 64; // TTL
    frame[ip_off + 9] = 1; // protocol = ICMP
    frame[ip_off + 10] = 0; // checksum high (à calculer)
    frame[ip_off + 11] = 0; // checksum low
    frame[ip_off + 12..ip_off + 16].copy_from_slice(&src_ip);
    frame[ip_off + 16..ip_off + 20].copy_from_slice(&target_ip);

    // Calculer le checksum IP
    let ip_cksum = ip_checksum(&frame[ip_off..ip_off + 20]);
    frame[ip_off + 10] = (ip_cksum >> 8) as u8;
    frame[ip_off + 11] = (ip_cksum & 0xFF) as u8;

    // ICMP header (8 bytes, offset 34)
    let icmp_off = ip_off + 20;
    frame[icmp_off] = 8; // type = echo request
    frame[icmp_off + 1] = 0; // code
    frame[icmp_off + 2] = 0; // checksum high
    frame[icmp_off + 3] = 0; // checksum low
    frame[icmp_off + 4] = 0; // id high
    frame[icmp_off + 5] = 0x01; // id low
    frame[icmp_off + 6] = 0; // seq high
    frame[icmp_off + 7] = 0x01; // seq low
    // Payload (4 bytes)
    frame[icmp_off + 8] = 0xDE;
    frame[icmp_off + 9] = 0xAD;
    frame[icmp_off + 10] = 0xBE;
    frame[icmp_off + 11] = 0xEF;

    // Calculer le checksum ICMP
    let icmp_cksum = icmp_checksum(&frame[icmp_off..icmp_off + 12]);
    frame[icmp_off + 2] = (icmp_cksum >> 8) as u8;
    frame[icmp_off + 3] = (icmp_cksum & 0xFF) as u8;

    let frame_len = 14 + 20 + 8 + 4; // 46 bytes
    let sent = send(&frame[..frame_len]);
    crate::console::print(format_args!("ping: echo request envoye ({} octets)\n", sent));

    // Attendre la reply
    let start = crate::irq::ticks();
    let mut buf = [0u8; 64];
    while crate::irq::ticks() - start < timeout_ticks {
        let got = recv(&mut buf);
        if got >= 42 {
            // Vérifier Ethernet + IP + ICMP
            if buf[12..14] == [0x08, 0x00] {
                let proto = buf[ip_off + 9];
                if proto == 1 {
                    let icmp_type = buf[icmp_off];
                    if icmp_type == 0 {
                        // Echo reply
                        let reply_id = ((buf[icmp_off + 4] as u16) << 8) | (buf[icmp_off + 5] as u16);
                        let reply_seq = ((buf[icmp_off + 6] as u16) << 8) | (buf[icmp_off + 7] as u16);
                        if reply_id == 0x0001 && reply_seq == 0x0001 {
                            crate::console::print(format_args!("ping: echo reply recu de {:?}\n", target_ip));
                            return true;
                        }
                    }
                }
            }
        }
    }
    crate::console::print(format_args!("ping: timeout apres {} ticks\n", timeout_ticks));
    false
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
