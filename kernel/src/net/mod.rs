//! J5 — Réseau
//!
//! Plan d'implémentation :
//! 1. `virtio_net` : driver PCI/virtio pour QEMU.
//! 2. `ethernet` : trames Ethernet basiques (MAC, ARP).
//! 3. `ip` : IPv4 minimal (ICMP ping).
//! 4. `socket` : API BSD (`AF_INET`, `AF_UNIX`).
//! 5. `lwip` : intégration de la pile TCP/IP (ultérieur).

#![allow(dead_code)]

pub mod virtio_net;
pub mod tcp;
pub mod dns;
