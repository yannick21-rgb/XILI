//! J5 — client DNS minimal (UDP, noyau).
//!
//! Résout un nom d'hôte en IPv4 via le DNS de QEMU user net (10.0.2.3).
//! Pas de cache, pas de TCP fallback, pas de DNSSEC.

use crate::net::tcp;
use crate::net::virtio_net;

const SRC_IP: [u8; 4] = [10, 0, 2, 15];
const DNS_SERVER: [u8; 4] = [10, 0, 2, 3];
const SRC_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

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

/// Encode un nom DNS (ex "example.com") en QNAME.
fn encode_name(buf: &mut [u8], name: &str) -> usize {
    let mut pos = 0;
    for label in name.split('.') {
        let lb = label.as_bytes();
        if lb.is_empty() {
            continue;
        }
        if pos + 1 + lb.len() > buf.len() {
            return 0;
        }
        buf[pos] = lb.len() as u8;
        pos += 1;
        buf[pos..pos + lb.len()].copy_from_slice(lb);
        pos += lb.len();
    }
    if pos < buf.len() {
        buf[pos] = 0;
        pos += 1;
    }
    pos
}

pub unsafe fn dns_resolve(name: &str) -> Option<[u8; 4]> {
    if !virtio_net::is_ready() {
        return None;
    }
    let dst_mac = match tcp::mac_for(DNS_SERVER) {
        Some(m) => m,
        None => return None,
    };

    // Construire la requête DNS.
    let mut dns = [0u8; 256];
    // Header
    dns[0] = 0x12; // id high
    dns[1] = 0x34; // id low
    dns[2] = 0x01; // flags high (recursion desired)
    dns[3] = 0x00;
    dns[4] = 0x00; // qdcount high
    dns[5] = 0x01; // qdcount low
    dns[6] = 0x00;
    dns[7] = 0x00;
    dns[8] = 0x00;
    dns[9] = 0x00;
    dns[10] = 0x00;
    dns[11] = 0x00;
    // Question
    let mut pos = 12;
    let nlen = encode_name(&mut dns[pos..], name);
    if nlen == 0 {
        return None;
    }
    pos += nlen;
    // QTYPE A = 1
    dns[pos] = 0x00;
    dns[pos + 1] = 0x01;
    pos += 2;
    // QCLASS IN = 1
    dns[pos] = 0x00;
    dns[pos + 1] = 0x01;
    pos += 2;
    let dns_len = pos;

    // Construire le paquet UDP/IP/Ethernet.
    let udp_len = 8 + dns_len;
    let ip_total = 20 + udp_len;
    let frame_len = 14 + ip_total;
    let mut frame = [0u8; 512];
    // Ethernet
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&SRC_MAC);
    frame[12..14].copy_from_slice(&[0x08, 0x00]);
    // IP
    let ip_off = 14;
    frame[ip_off] = 0x45;
    frame[ip_off + 1] = 0;
    frame[ip_off + 2] = (ip_total >> 8) as u8;
    frame[ip_off + 3] = (ip_total & 0xFF) as u8;
    frame[ip_off + 4] = 0;
    frame[ip_off + 5] = 2;
    frame[ip_off + 6] = 0;
    frame[ip_off + 7] = 0;
    frame[ip_off + 8] = 64;
    frame[ip_off + 9] = 17; // UDP
    frame[ip_off + 10] = 0;
    frame[ip_off + 11] = 0;
    frame[ip_off + 12..ip_off + 16].copy_from_slice(&SRC_IP);
    frame[ip_off + 16..ip_off + 20].copy_from_slice(&DNS_SERVER);
    let ck = ip_checksum(&frame[ip_off..ip_off + 20]);
    frame[ip_off + 10] = (ck >> 8) as u8;
    frame[ip_off + 11] = (ck & 0xFF) as u8;
    // UDP
    let udp_off = ip_off + 20;
    // src port 12345
    frame[udp_off] = (12345 >> 8) as u8;
    frame[udp_off + 1] = (12345 & 0xFF) as u8;
    // dst port 53
    frame[udp_off + 2] = 0;
    frame[udp_off + 3] = 53;
    frame[udp_off + 4] = (udp_len >> 8) as u8;
    frame[udp_off + 5] = (udp_len & 0xFF) as u8;
    frame[udp_off + 6] = 0; // checksum 0 (optional)
    frame[udp_off + 7] = 0;
    // payload
    frame[udp_off + 8..udp_off + 8 + dns_len].copy_from_slice(&dns[..dns_len]);

    let sent = virtio_net::send(&frame[..frame_len]);
    let _ = sent;

    // Attendre la réponse.
    let start = crate::irq::ticks();
    let mut buf = [0u8; 512];
    while crate::irq::ticks() - start < 300 {
        let got = virtio_net::recv(&mut buf);
        if got >= 42 {
            // Vérifier IP + UDP + DNS id
            let ipo = 14;
            if buf[ipo + 9] == 17 {
                let src = [buf[ipo + 12], buf[ipo + 13], buf[ipo + 14], buf[ipo + 15]];
                let dst = [buf[ipo + 16], buf[ipo + 17], buf[ipo + 18], buf[ipo + 19]];
                if src == DNS_SERVER && dst == SRC_IP {
                    let uo = ipo + 20;
                    let src_p = ((buf[uo] as u16) << 8) | (buf[uo + 1] as u16);
                    let dst_p = ((buf[uo + 2] as u16) << 8) | (buf[uo + 3] as u16);
                    if src_p == 53 && dst_p == 12345 {
                        // DNS payload
                        let dns_off = uo + 8;
                        // id check
                        if buf[dns_off] == 0x12 && buf[dns_off + 1] == 0x34 {
                            // qdcount
                            let qd = ((buf[dns_off + 4] as usize) << 8) | (buf[dns_off + 5] as usize);
                            let an = ((buf[dns_off + 6] as usize) << 8) | (buf[dns_off + 7] as usize);
                            if an == 0 {
                                continue;
                            }
                            // Skip question section.
                            let mut p = dns_off + 12;
                            for _ in 0..qd {
                                // skip name
                                loop {
                                    if p >= got {
                                        return None;
                                    }
                                    let b = buf[p];
                                    p += 1;
                                    if b == 0 {
                                        break;
                                    }
                                    if b & 0xC0 == 0xC0 {
                                        p += 1;
                                        break;
                                    }
                                    p += b as usize;
                                }
                                p += 4; // qtype + qclass
                            }
                            // Parse first answer.
                            for _ in 0..an {
                                // skip name
                                loop {
                                    if p >= got {
                                        return None;
                                    }
                                    let b = buf[p];
                                    p += 1;
                                    if b == 0 {
                                        break;
                                    }
                                    if b & 0xC0 == 0xC0 {
                                        p += 1;
                                        break;
                                    }
                                    p += b as usize;
                                }
                                if p + 10 > got {
                                    return None;
                                }
                                let rtype = ((buf[p] as u16) << 8) | (buf[p + 1] as u16);
                                // class
                                // let rclass = ...
                                let rdlen = ((buf[p + 8] as usize) << 8) | (buf[p + 9] as usize);
                                p += 10;
                                if rtype == 1 && rdlen == 4 {
                                    let ip = [buf[p], buf[p + 1], buf[p + 2], buf[p + 3]];
                                    return Some(ip);
                                }
                                p += rdlen;
                            }
                        }
                    }
                }
            }
        }
        // petite pause
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
    None
}

pub unsafe fn dns_self_test() {
    crate::console::print(format_args!("J5.5: DNS self-test begin\n"));
    match dns_resolve("example.com") {
        Some(ip) => {
            crate::console::print(format_args!(
                "J5.5: DNS OK -> {}.{}.{}.{}\n",
                ip[0], ip[1], ip[2], ip[3]
            ));
        }
        None => {
            crate::console::print(format_args!("J5.5: DNS ECHEC\n"));
        }
    }
}
