//! J5 — TCP client minimal (noyau).
//!
//! Implémente un client TCP synchrone (connect/send/recv/close) pour
//! un seul TCB global. Pas de retransmission, pas de fenêtre glissante
//! au-delà du buffer ; suffisant pour HTTP GET sur QEMU user net.
//!
//! Conçu pour être testé par `tcp_self_test()` au boot (marker J5.4).

use crate::io::{inb, inl, inw, outb, outl, outw};
use crate::net::virtio_net;

const SRC_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
const SRC_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

// ---- État du TCB ----
const ST_CLOSED: u8 = 0;
const ST_SYN_SENT: u8 = 1;
const ST_ESTABLISHED: u8 = 2;
const ST_FIN_WAIT: u8 = 3;
const ST_LISTEN: u8 = 10;
const ST_SYN_RCVD: u8 = 11;

struct Tcb {
    state: u8,
    dst_ip: [u8; 4],
    dst_port: u16,
    src_port: u16,
    seq: u32,
    ack: u32,
    isn: u32,
    rx_buf: [u8; 4096],
    rx_len: usize,
    rx_head: usize,
}

static mut TCB: Tcb = Tcb {
    state: ST_CLOSED,
    dst_ip: [0; 4],
    dst_port: 0,
    src_port: 0,
    seq: 0,
    ack: 0,
    isn: 0x1234_5678,
    rx_buf: [0; 4096],
    rx_len: 0,
    rx_head: 0,
};

static mut TICKS_BASE: u64 = 0;

fn now_ticks() -> u64 {
    unsafe { crate::irq::ticks() }
}

pub fn mac_for(ip: [u8; 4]) -> Option<[u8; 6]> {
    // Sous-réseau local : résoudre directement ; sinon passerelle.
    let target = if ip[0] == 10 && ip[1] == 0 && ip[2] == 2 {
        ip
    } else {
        GATEWAY_IP
    };
    unsafe { virtio_net::arp_resolve(target, 100) }
}

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

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], tcp_bytes: &[u8]) -> u16 {
    // Pseudo-en-tête + TCP.
    let mut sum: u32 = 0;
    // pseudo header (12 bytes)
    let mut pseudo = [0u8; 12];
    pseudo[0..4].copy_from_slice(&src_ip);
    pseudo[4..8].copy_from_slice(&dst_ip);
    pseudo[8] = 0;
    pseudo[9] = 6; // protocol TCP
    let tcp_len = tcp_bytes.len() as u16;
    pseudo[10] = (tcp_len >> 8) as u8;
    pseudo[11] = (tcp_len & 0xFF) as u8;

    // accumulate pseudo (12 bytes = 6 words)
    let mut i = 0;
    while i + 1 < pseudo.len() {
        let w = ((pseudo[i] as u32) << 8) | (pseudo[i + 1] as u32);
        sum = sum.wrapping_add(w);
        i += 2;
    }
    // accumulate tcp bytes
    i = 0;
    while i + 1 < tcp_bytes.len() {
        let w = ((tcp_bytes[i] as u32) << 8) | (tcp_bytes[i + 1] as u32);
        sum = sum.wrapping_add(w);
        i += 2;
    }
    if i < tcp_bytes.len() {
        sum = sum.wrapping_add((tcp_bytes[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

fn build_tcp_packet(
    frame: &mut [u8],
    dst_mac: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> usize {
    // Ethernet 14
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&SRC_MAC);
    frame[12..14].copy_from_slice(&[0x08, 0x00]);
    // IP 20
    let ip_off = 14;
    frame[ip_off] = 0x45;
    frame[ip_off + 1] = 0;
    let total_len: u16 = 20 + 20 + payload.len() as u16;
    frame[ip_off + 2] = (total_len >> 8) as u8;
    frame[ip_off + 3] = (total_len & 0xFF) as u8;
    frame[ip_off + 4] = 0;
    frame[ip_off + 5] = 1;
    frame[ip_off + 6] = 0;
    frame[ip_off + 7] = 0;
    frame[ip_off + 8] = 64;
    frame[ip_off + 9] = 6; // TCP
    frame[ip_off + 10] = 0;
    frame[ip_off + 11] = 0;
    frame[ip_off + 12..ip_off + 16].copy_from_slice(&src_ip);
    frame[ip_off + 16..ip_off + 20].copy_from_slice(&dst_ip);
    let ip_cksum = ip_checksum(&frame[ip_off..ip_off + 20]);
    frame[ip_off + 10] = (ip_cksum >> 8) as u8;
    frame[ip_off + 11] = (ip_cksum & 0xFF) as u8;

    // TCP 20
    let tcp_off = ip_off + 20;
    frame[tcp_off] = (src_port >> 8) as u8;
    frame[tcp_off + 1] = (src_port & 0xFF) as u8;
    frame[tcp_off + 2] = (dst_port >> 8) as u8;
    frame[tcp_off + 3] = (dst_port & 0xFF) as u8;
    // seq
    frame[tcp_off + 4] = ((seq >> 24) & 0xFF) as u8;
    frame[tcp_off + 5] = ((seq >> 16) & 0xFF) as u8;
    frame[tcp_off + 6] = ((seq >> 8) & 0xFF) as u8;
    frame[tcp_off + 7] = (seq & 0xFF) as u8;
    // ack
    frame[tcp_off + 8] = ((ack >> 24) & 0xFF) as u8;
    frame[tcp_off + 9] = ((ack >> 16) & 0xFF) as u8;
    frame[tcp_off + 10] = ((ack >> 8) & 0xFF) as u8;
    frame[tcp_off + 11] = (ack & 0xFF) as u8;
    // data offset (5) + reserved (0) + flags
    frame[tcp_off + 12] = 0x50; // 5 words
    frame[tcp_off + 13] = flags;
    // window
    frame[tcp_off + 14] = 0x0F;
    frame[tcp_off + 15] = 0xFF; // 4095
    frame[tcp_off + 16] = 0; // checksum placeholder
    frame[tcp_off + 17] = 0;
    frame[tcp_off + 18] = 0; // urgent
    frame[tcp_off + 19] = 0;
    // payload
    if !payload.is_empty() {
        frame[tcp_off + 20..tcp_off + 20 + payload.len()].copy_from_slice(payload);
    }
    // checksum
    let tcp_len = 20 + payload.len();
    let cksum = tcp_checksum(src_ip, dst_ip, &frame[tcp_off..tcp_off + tcp_len]);
    frame[tcp_off + 16] = (cksum >> 8) as u8;
    frame[tcp_off + 17] = (cksum & 0xFF) as u8;

    14 + 20 + 20 + payload.len()
}

/// Tente de lire un paquet TCP entrant et de le traiter.
unsafe fn poll_incoming() {
    let mut buf = [0u8; 1500];
    let n = virtio_net::recv(&mut buf);
    if n < 54 {
        return;
    }
    // Ethernet
    if buf[12..14] != [0x08, 0x00] {
        return;
    }
    let ip_off = 14;
    if (buf[ip_off] >> 4) != 4 {
        return;
    }
    if buf[ip_off + 9] != 6 {
        return; // TCP only
    }
    let tcp_off = ip_off + 20;
    let src_ip = [buf[ip_off + 12], buf[ip_off + 13], buf[ip_off + 14], buf[ip_off + 15]];
    let dst_ip = [buf[ip_off + 16], buf[ip_off + 17], buf[ip_off + 18], buf[ip_off + 19]];
    if dst_ip != SRC_IP {
        return;
    }
    let src_port = ((buf[tcp_off] as u16) << 8) | (buf[tcp_off + 1] as u16);
    let dst_port = ((buf[tcp_off + 2] as u16) << 8) | (buf[tcp_off + 3] as u16);
    let seq = ((buf[tcp_off + 4] as u32) << 24)
        | ((buf[tcp_off + 5] as u32) << 16)
        | ((buf[tcp_off + 6] as u32) << 8)
        | (buf[tcp_off + 7] as u32);
    let ackn = ((buf[tcp_off + 8] as u32) << 24)
        | ((buf[tcp_off + 9] as u32) << 16)
        | ((buf[tcp_off + 10] as u32) << 8)
        | (buf[tcp_off + 11] as u32);
    let flags = buf[tcp_off + 13];
    let data_off = ((buf[tcp_off + 12] >> 4) as usize) * 4;
    let payload_start = tcp_off + data_off;
    let payload_len = n.saturating_sub(payload_start);

    // Vérifier que c'est pour notre TCB.
    if dst_port != TCB.src_port {
        return;
    }
    if src_ip != TCB.dst_ip || src_port != TCB.dst_port {
        return;
    }

    let syn = flags & 0x02 != 0;
    let ack = flags & 0x10 != 0;
    let fin = flags & 0x01 != 0;
    let psh = flags & 0x08 != 0;

    match TCB.state {
        ST_SYN_SENT => {
            if syn && ack && ackn == TCB.seq.wrapping_add(1) {
                TCB.ack = seq.wrapping_add(1);
                TCB.seq = TCB.seq.wrapping_add(1);
                // envoyer ACK
                let dst_mac = match mac_for(TCB.dst_ip) {
                    Some(m) => m,
                    None => return,
                };
                let mut frame = [0u8; 128];
                let flen = build_tcp_packet(
                    &mut frame,
                    dst_mac,
                    SRC_IP,
                    TCB.dst_ip,
                    TCB.src_port,
                    TCB.dst_port,
                    TCB.seq,
                    TCB.ack,
                    0x10, // ACK
                    &[],
                );
                virtio_net::send(&frame[..flen]);
                TCB.state = ST_ESTABLISHED;
            }
        }
        ST_ESTABLISHED => {
            if ack {
                // données ?
                if payload_len > 0 {
                    // copier dans rx_buf
                    let space = TCB.rx_buf.len() - TCB.rx_len;
                    let to_copy = core::cmp::min(payload_len, space);
                    if to_copy > 0 {
                        TCB.rx_buf[TCB.rx_len..TCB.rx_len + to_copy]
                            .copy_from_slice(&buf[payload_start..payload_start + to_copy]);
                        TCB.rx_len += to_copy;
                    }
                    TCB.ack = TCB.ack.wrapping_add(payload_len as u32);
                    // ACK
                    let dst_mac = match mac_for(TCB.dst_ip) {
                        Some(m) => m,
                        None => return,
                    };
                    let mut frame = [0u8; 128];
                    let flen = build_tcp_packet(
                        &mut frame,
                        dst_mac,
                        SRC_IP,
                        TCB.dst_ip,
                        TCB.src_port,
                        TCB.dst_port,
                        TCB.seq,
                        TCB.ack,
                        0x10,
                        &[],
                    );
                    virtio_net::send(&frame[..flen]);
                }
            }
            if fin {
                TCB.ack = TCB.ack.wrapping_add(1);
                let dst_mac = match mac_for(TCB.dst_ip) {
                    Some(m) => m,
                    None => return,
                };
                let mut frame = [0u8; 128];
                let flen = build_tcp_packet(
                    &mut frame,
                    dst_mac,
                    SRC_IP,
                    TCB.dst_ip,
                    TCB.src_port,
                    TCB.dst_port,
                    TCB.seq,
                    TCB.ack,
                    0x11, // FIN+ACK
                    &[],
                );
                virtio_net::send(&frame[..flen]);
                TCB.state = ST_CLOSED;
            }
        }
        _ => {}
    }
    let _ = (psh,);
}

pub unsafe fn tcp_connect(dst_ip: [u8; 4], dst_port: u16) -> bool {
    TCB.state = ST_CLOSED;
    TCB.dst_ip = dst_ip;
    TCB.dst_port = dst_port;
    TCB.src_port = 49152;
    TCB.seq = TCB.isn;
    TCB.ack = 0;
    TCB.rx_len = 0;
    TCB.rx_head = 0;

    let dst_mac = match mac_for(dst_ip) {
        Some(m) => m,
        None => {
            crate::console::print(format_args!("tcp: ARP echec pour {:?}\n", dst_ip));
            return false;
        }
    };

    // SYN
    let mut frame = [0u8; 128];
    let flen = build_tcp_packet(
        &mut frame,
        dst_mac,
        SRC_IP,
        dst_ip,
        TCB.src_port,
        dst_port,
        TCB.seq,
        0,
        0x02, // SYN
        &[],
    );
    let sent = virtio_net::send(&frame[..flen]);
    let _ = sent;
    TCB.state = ST_SYN_SENT;

    // Attendre SYN-ACK
    let start = now_ticks();
    while now_ticks() - start < 200 {
        poll_incoming();
        if TCB.state == ST_ESTABLISHED {
            crate::console::print(format_args!("tcp: connecte a {:?}:{}\n", dst_ip, dst_port));
            return true;
        }
        // petite pause
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
    crate::console::print(format_args!("tcp: timeout connect\n"));
    false
}

pub unsafe fn tcp_send(data: &[u8]) -> usize {
    if TCB.state != ST_ESTABLISHED {
        return 0;
    }
    let dst_mac = match mac_for(TCB.dst_ip) {
        Some(m) => m,
        None => return 0,
    };
    // envoyer par chunks de 512
    let mut sent_total = 0usize;
    let mut off = 0usize;
    while off < data.len() {
        let chunk = core::cmp::min(512, data.len() - off);
        let mut frame = [0u8; 768];
        let flen = build_tcp_packet(
            &mut frame,
            dst_mac,
            SRC_IP,
            TCB.dst_ip,
            TCB.src_port,
            TCB.dst_port,
            TCB.seq,
            TCB.ack,
            0x18, // PSH+ACK
            &data[off..off + chunk],
        );
        virtio_net::send(&frame[..flen]);
        TCB.seq = TCB.seq.wrapping_add(chunk as u32);
        sent_total += chunk;
        off += chunk;
        // attendre un peu pour ACK
        let start = now_ticks();
        while now_ticks() - start < 20 {
            poll_incoming();
        }
    }
    sent_total
}

pub unsafe fn tcp_recv(buf: &mut [u8]) -> usize {
    if TCB.state != ST_ESTABLISHED && TCB.rx_len == 0 {
        // peut encore y avoir des données
    }
    // Poll pour de nouvelles données
    let start = now_ticks();
    while TCB.rx_len == 0 && (TCB.state == ST_ESTABLISHED) && now_ticks() - start < 200 {
        poll_incoming();
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
    // copier rx_buf -> buf
    let n = core::cmp::min(buf.len(), TCB.rx_len);
    if n > 0 {
        buf[..n].copy_from_slice(&TCB.rx_buf[..n]);
        // décaler
        if n < TCB.rx_len {
            TCB.rx_buf.copy_within(n..TCB.rx_len, 0);
        }
        TCB.rx_len -= n;
    }
    n
}

pub unsafe fn tcp_listen(port: u16) {
    TCB.state = ST_CLOSED;
    TCB.src_port = port;
    TCB.dst_ip = [0; 4];
    TCB.dst_port = 0;
    TCB.seq = TCB.isn;
    TCB.ack = 0;
    TCB.rx_len = 0;
    TCB.rx_head = 0;
    TCB.state = ST_LISTEN;
}

pub unsafe fn tcp_server_self_test() {
    crate::console::print(format_args!("J5.6: TCP server test begin
"));
    tcp_listen(8080);
    crate::console::print(format_args!("J5.6: listening on port 8080
"));
    let start = now_ticks();
    while TCB.state != ST_ESTABLISHED {
        poll_incoming();
        if now_ticks() - start > 1000 {
            crate::console::print(format_args!("J5.6: timeout waiting for connection
"));
            return;
        }
    }
    crate::console::print(format_args!("J5.6: connection established
"));
    let mut buf = [0u8; 256];
    let mut total = 0usize;
    let start2 = now_ticks();
    while total == 0 && now_ticks() - start2 < 200 {
        poll_incoming();
        let n = tcp_recv(&mut buf[total..]);
        total += n;
    }
    if total > 0 {
        crate::console::print(format_args!("J5.6: received {} bytes
", total));
    }
    let response = b"HTTP/1.0 200 OK
Content-Length: 2

OK";
    tcp_send(response);
    crate::console::print(format_args!("J5.6: response sent
"));
    tcp_close();
    crate::console::print(format_args!("J5.6: TCP server OK
"));
}

pub unsafe fn tcp_close() {
    if TCB.state == ST_ESTABLISHED {
        let dst_mac = match mac_for(TCB.dst_ip) {
            Some(m) => m,
            None => return,
        };
        let mut frame = [0u8; 128];
        let flen = build_tcp_packet(
            &mut frame,
            dst_mac,
            SRC_IP,
            TCB.dst_ip,
            TCB.src_port,
            TCB.dst_port,
            TCB.seq,
            TCB.ack,
            0x11, // FIN+ACK
            &[],
        );
        virtio_net::send(&frame[..flen]);
        TCB.seq = TCB.seq.wrapping_add(1);
        TCB.state = ST_CLOSED;
    }
}

pub unsafe fn tcp_self_test() {
    // Tente un HTTP GET vers la passerelle (hôte) port 80.
    crate::console::print(format_args!("J5.4: TCP self-test begin\n"));
    if !virtio_net::is_ready() {
        crate::console::print(format_args!("J5.4: virtio-net non pret\n"));
        return;
    }
    let ip = GATEWAY_IP;
    if !tcp_connect(ip, 8080) {
        crate::console::print(format_args!("J5.4: TCP ECHEC (connect)\n"));
        return;
    }
    let req = b"GET / HTTP/1.0\r\nHost: 10.0.2.2:8080\r\n\r\n";
    let sent = tcp_send(req);
    crate::console::print(format_args!("J5.4: sent {} bytes\n", sent));
    // recevoir
    let mut buf = [0u8; 512];
    let mut total = 0usize;
    let start = now_ticks();
    while now_ticks() - start < 300 && total < buf.len() {
        let n = tcp_recv(&mut buf[total..]);
        if n == 0 {
            // petit délai
            for _ in 0..5000 {
                core::hint::spin_loop();
            }
            if total > 0 {
                break;
            }
            continue;
        }
        total += n;
    }
    crate::console::print(format_args!("J5.4: recv {} bytes\n", total));
    if total > 0 {
        crate::console::print(format_args!("J5.4: TCP OK\n"));
    } else {
        crate::console::print(format_args!("J5.4: TCP ECHEC (recv)\n"));
    }
    tcp_close();
}
