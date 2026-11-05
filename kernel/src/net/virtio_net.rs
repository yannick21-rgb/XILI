//! virtio-net — driver réseau pour QEMU (virtio PCI).
//!
//! Ce module implémente le squelette du driver virtio-net :
//! - Détection du périphérique PCI
//! - Configuration des virtqueues RX/TX
//! - Initialisation de l'appareil
//!
//! Les transferts de paquets seront ajoutés dans les itérations suivantes.

use crate::io::{inb, inl, outl};
use crate::mm::phys::alloc_page;

// ---------------------------------------------------------------------------
// Constantes PCI / virtio
// ---------------------------------------------------------------------------

const PCI_VENDOR_ID: u16 = 0x1AF4;
const PCI_DEVICE_ID: u16 = 0x1000;

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

const PCI_BAR0: u8 = 0x10;

// ---------------------------------------------------------------------------
// Device virtio (offsets depuis la base BAR0)
// ---------------------------------------------------------------------------

const VIRTIO_MAGIC: u32 = 0x74726976; // "virt"
const VIRTIO_VERSION: u32 = 0x1;
const VIRTIO_DEVICE_ID_NET: u32 = 0x1;

// Registres du device virtio
const VIRTIO_OFF_MAGIC: usize = 0x000;
const VIRTIO_OFF_VERSION: usize = 0x004;
const VIRTIO_OFF_DEVICE_ID: usize = 0x008;
const VIRTIO_OFF_GUEST_FEATURES: usize = 0x020;
const VIRTIO_OFF_QUEUE_SEL: usize = 0x030;
const VIRTIO_OFF_QUEUE_NUM: usize = 0x034;
const VIRTIO_OFF_QUEUE_PFN: usize = 0x038;
const VIRTIO_OFF_QUEUE_READY: usize = 0x044;
const VIRTIO_OFF_STATUS: usize = 0x070;
const VIRTIO_OFF_CONFIG_GEN: usize = 0x0FC;

const VIRTIO_STATUS_ACK: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;
const VIRTIO_STATUS_FAILED: u32 = 128;

// ---------------------------------------------------------------------------
// Virtqueue
// ---------------------------------------------------------------------------

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; 256],
}

#[repr(C)]
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

struct Virtq {
    desc: *mut VirtqDesc,
    avail: *mut VirtqAvail,
    used: *mut VirtqUsed,
    queue_idx: u16,
    num: u16,
    last_used: u16,
}

// ---------------------------------------------------------------------------
// État global du driver
// ---------------------------------------------------------------------------

static mut VIRTIO_NET_BASE: u32 = 0;
static mut VIRTIO_NET_IRQ: u8 = 0;
static mut VIRTIO_NET_IS_IO: bool = false;

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
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            for func in 0..8u8 {
                let cfg = pci_read_config(bus, slot, func, 0);
                let v = (cfg & 0xFFFF) as u16;
                let d = ((cfg >> 16) & 0xFFFF) as u16;
                if v == vendor && d == device {
                    let bar0 = pci_read_config(bus, slot, func, PCI_BAR0);
                    return Some((bus, slot, func, bar0));
                }
            }
        }
    }
    None
}

/// Scan PCI et affiche tous les devices présents (pour debug).
unsafe fn pci_dump_all() {
    crate::console::print(format_args!("PCI scan begin\n"));
    for bus in 0..=0u8 {
        for slot in 0..8u8 {
            for func in 0..8u8 {
                let cfg = pci_read_config(bus, slot, func, 0);
                let v = (cfg & 0xFFFF) as u16;
                let d = ((cfg >> 16) & 0xFFFF) as u16;
                if v == 0xFFFF {
                    continue;
                }
                let cls = pci_read_config(bus, slot, func, 0x08);
                let class_code = (cls >> 24) & 0xFF;
                let subclass = (cls >> 16) & 0xFF;
                let bar0 = pci_read_config(bus, slot, func, PCI_BAR0);
                crate::console::print(format_args!(
                    "PCI {:02x}:{:02x}.{} vend=0x{:04x} dev=0x{:04x} class=0x{:02x}:0x{:02x} bar0=0x{:08x}\n",
                    bus, slot, func, v, d, class_code, subclass, bar0
                ));
            }
        }
    }
    crate::console::print(format_args!("PCI scan end\n"));
}

// ---------------------------------------------------------------------------
// Helpers virtio MMIO
// ---------------------------------------------------------------------------

unsafe fn virtio_read(base: u32, off: usize) -> u32 {
    if VIRTIO_NET_IS_IO {
        inl((base + off as u32) as u16)
    } else {
        core::ptr::read_volatile((base as *const u32).add(off / 4))
    }
}

unsafe fn virtio_write(base: u32, off: usize, val: u32) {
    if VIRTIO_NET_IS_IO {
        outl((base + off as u32) as u16, val);
    } else {
        core::ptr::write_volatile((base as *mut u32).add(off / 4), val);
    }
}

// ---------------------------------------------------------------------------
// Initialisation du driver
// ---------------------------------------------------------------------------

unsafe fn virtq_init(base: u32, idx: u16, num: u16) -> Virtq {
    // Allouer une page pour les descriptors, avail et used.
    let page = alloc_page().expect("virtio_net: alloc_page");
    let va = page as u64;

    let desc = va as *mut VirtqDesc;
    let avail = (va + (num as u64) * core::mem::size_of::<VirtqDesc>() as u64) as *mut VirtqAvail;
    let used = (va
        + (num as u64) * core::mem::size_of::<VirtqDesc>() as u64
        + (2 + num as u64) * core::mem::size_of::<u16>() as u64) as *mut VirtqUsed;

    // Sélectionner la queue
    virtio_write(base, VIRTIO_OFF_QUEUE_SEL, idx as u32);
    virtio_write(base, VIRTIO_OFF_QUEUE_NUM, num as u32);
    virtio_write(base, VIRTIO_OFF_QUEUE_PFN, (va >> 12) as u32);

    Virtq {
        desc,
        avail,
        used,
        queue_idx: idx,
        num,
        last_used: 0,
    }
}

/// Initialise le driver virtio-net.
/// Retourne `true` si un périphérique a été trouvé et configuré.
pub unsafe fn init() -> bool {
    // Dump PCI pour debug.
    pci_dump_all();

    if let Some((bus, slot, func, bar0)) = pci_find_device(PCI_VENDOR_ID, PCI_DEVICE_ID) {
        let base = bar0 & 0xFFFFFFF0;
        VIRTIO_NET_BASE = base;
        VIRTIO_NET_IS_IO = (bar0 & 1) != 0;

        // Lecture basique des registres pour valider la présence.
        let magic = virtio_read(base, VIRTIO_OFF_MAGIC);
        let version = virtio_read(base, VIRTIO_OFF_VERSION);
        let device_id = virtio_read(base, VIRTIO_OFF_DEVICE_ID);

        crate::console::print(format_args!(
            "virtio-net: bus={} slot={} base=0x{:x} magic=0x{:x} ver={} dev={}\n",
            bus, slot, base, magic, version, device_id
        ));

        if magic != VIRTIO_MAGIC || version != VIRTIO_VERSION || device_id != VIRTIO_DEVICE_ID_NET {
            crate::console::print(format_args!(
                "virtio-net: rejet magic=0x{:x} ver={} dev={}\n",
                magic, version, device_id
            ));
            return false;
        }

        // Acquittement du device.
        virtio_write(base, VIRTIO_OFF_STATUS, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER);

        // Activer les features (pour l'instant, aucune feature spécifique).
        virtio_write(base, VIRTIO_OFF_GUEST_FEATURES, 0);
        virtio_write(base, VIRTIO_OFF_STATUS, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);

        // Configurer les queues RX/TX (queue 0 = RX, queue 1 = TX).
        let _rx = virtq_init(base, 0, 256);
        let _tx = virtq_init(base, 1, 256);

        // Driver OK.
        virtio_write(base, VIRTIO_OFF_STATUS, VIRTIO_STATUS_ACK | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);

        // IRQ
        let irq_cfg = pci_read_config(bus, slot, func, 0x3C);
        VIRTIO_NET_IRQ = (irq_cfg & 0xFF) as u8;

        crate::console::print(format_args!(
            "virtio-net: initialisee, IRQ={}, base=0x{:x}\n",
            VIRTIO_NET_IRQ, base
        ));

        true
    } else {
        false
    }
}

/// Envoie un paquet brut (frame Ethernet).
/// Retourne le nombre d'octets envoyés, ou 0 en cas d'erreur.
pub unsafe fn send(_data: &[u8]) -> usize {
    // À implémenter dans la prochaine itération.
    0
}

/// Reçoit un paquet brut (frame Ethernet).
/// Retourne le nombre d'octets reçus, ou 0 si aucun paquet disponible.
pub unsafe fn recv(_buf: &mut [u8]) -> usize {
    // À implémenter dans la prochaine itération.
    0
}

/// Diagnostic minimal : envoie une sonde ARP "who-has" et attend une
/// réponse. Ce n'est pas un round-trip complet, mais ça valide que le
/// périphérique virtio-net est bien présent et que le chainage RX/TX
/// est fonctionnel.
pub unsafe fn arp_probe() -> bool {
    // TODO : implémenter le round-trip ARP complet.
    false
}
