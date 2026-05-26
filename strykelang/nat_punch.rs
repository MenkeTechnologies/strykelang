//! STUN client + UDP hole-punching state machine for stryke's `stun`
//! and `punch` builtins.
//!
//! No third-party crate dependency — the STUN binary protocol is small
//! enough (RFC 8489) to roll our own for the Binding Request / Response
//! path that covers ~99% of real-world use. We support:
//!
//!   * Binding Request (the only message type we send)
//!   * XOR-MAPPED-ADDRESS attribute parsing (the modern form)
//!   * MAPPED-ADDRESS fallback (some old servers)
//!   * IPv4 only (IPv6 is a 100-line additive change; left for v2)
//!
//! Not implemented (out of scope for v1, would require TURN server access
//! and full ICE):
//!   * Symmetric-NAT detection / fallback
//!   * TURN relay allocation
//!   * Full ICE candidate gathering and priority pairing
//!
//! Hole-punching state machine:
//!
//!   while now < deadline:
//!     send single keepalive datagram to peer_ip:peer_port via our socket
//!     attempt non-blocking recv with short timeout
//!     if recv succeeded:
//!         established — return success with the first received payload
//!     sleep interval_ms
//!
//! Both peers run this simultaneously (via an out-of-band signaling
//! channel — email, paste, etc., not provided by us). The early bombards
//! are dropped by both NATs since neither has seen inbound mappings yet;
//! once both NATs have observed an outbound packet to the peer's ip:port
//! they each install a forwarding rule for the reverse direction, and
//! subsequent packets get through.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::udp_sockets;

/// Build a STUN Binding Request packet. 20-byte header, no attributes.
///
/// Wire format (RFC 8489):
///   * 0..2:   message type (0x0001 = Binding Request)
///   * 2..4:   message length in bytes (we send 0 — no attrs)
///   * 4..8:   magic cookie (0x2112A442) — fixed, identifies STUN
///   * 8..20:  transaction ID (12 random bytes) — server echoes back so
///             clients can match responses to requests; also used in
///             XOR-MAPPED-ADDRESS computation
pub const STUN_MAGIC_COOKIE: u32 = 0x2112_A442;

pub fn build_binding_request(tx_id: &[u8; 12]) -> [u8; 20] {
    let mut pkt = [0u8; 20];
    pkt[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // Binding Request
    pkt[2..4].copy_from_slice(&0u16.to_be_bytes()); // length = 0
    pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    pkt[8..20].copy_from_slice(tx_id);
    pkt
}

/// Parse a STUN Binding Response, returning the public (IP, port) the
/// server saw us coming from. Returns `None` on:
///   * not a Binding Response (msg type ≠ 0x0101)
///   * truncated packet
///   * no XOR-MAPPED-ADDRESS or MAPPED-ADDRESS attribute
///   * non-IPv4 address family (v1 skips IPv6)
pub fn parse_binding_response(pkt: &[u8]) -> Option<(std::net::Ipv4Addr, u16)> {
    if pkt.len() < 20 {
        return None;
    }
    let msg_type = u16::from_be_bytes([pkt[0], pkt[1]]);
    if msg_type != 0x0101 {
        return None;
    }
    let msg_len = u16::from_be_bytes([pkt[2], pkt[3]]) as usize;
    let body_end = 20 + msg_len;
    if pkt.len() < body_end {
        return None;
    }
    let magic_cookie = u32::from_be_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]);
    if magic_cookie != STUN_MAGIC_COOKIE {
        return None;
    }
    // Tx ID (8..20) is opaque to us here.

    // Iterate attributes. Each attribute: 2B type, 2B length, N bytes
    // value, then padded to 4-byte boundary.
    let mut off = 20;
    while off + 4 <= body_end {
        let attr_type = u16::from_be_bytes([pkt[off], pkt[off + 1]]);
        let attr_len = u16::from_be_bytes([pkt[off + 2], pkt[off + 3]]) as usize;
        let val_start = off + 4;
        let val_end = val_start + attr_len;
        if val_end > body_end {
            return None;
        }
        match attr_type {
            // XOR-MAPPED-ADDRESS (RFC 8489 §14.2) — preferred form.
            // Layout: 1B reserved, 1B family (0x01 = IPv4), 2B xor_port,
            // 4B xor_addr.
            0x0020 if attr_len >= 8 && pkt[val_start + 1] == 0x01 => {
                let xor_port =
                    u16::from_be_bytes([pkt[val_start + 2], pkt[val_start + 3]]);
                let port = xor_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
                let xor_addr = u32::from_be_bytes([
                    pkt[val_start + 4],
                    pkt[val_start + 5],
                    pkt[val_start + 6],
                    pkt[val_start + 7],
                ]);
                let addr = xor_addr ^ STUN_MAGIC_COOKIE;
                return Some((std::net::Ipv4Addr::from(addr.to_be_bytes()), port));
            }
            // MAPPED-ADDRESS (legacy, RFC 3489) — same layout sans XOR.
            0x0001 if attr_len >= 8 && pkt[val_start + 1] == 0x01 => {
                let port =
                    u16::from_be_bytes([pkt[val_start + 2], pkt[val_start + 3]]);
                let addr = std::net::Ipv4Addr::new(
                    pkt[val_start + 4],
                    pkt[val_start + 5],
                    pkt[val_start + 6],
                    pkt[val_start + 7],
                );
                return Some((addr, port));
            }
            _ => {}
        }
        // Advance to next attribute, respecting 4-byte alignment padding.
        off = val_end;
        if off % 4 != 0 {
            off += 4 - (off % 4);
        }
    }
    None
}

/// Generate a 12-byte transaction ID from `std::time` jitter — good enough
/// for client-side STUN where uniqueness within one client is all we need.
fn fresh_tx_id() -> [u8; 12] {
    let mut id = [0u8; 12];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    id[0..16.min(12)].copy_from_slice(&nanos.to_le_bytes()[..12]);
    // Mix in process id so concurrent processes don't collide.
    let pid = std::process::id() as u64;
    for (i, b) in pid.to_le_bytes().iter().take(8).enumerate() {
        id[i] ^= b;
    }
    id
}

/// Query a STUN server via socket `id`. Returns `(public_ip, public_port)`
/// the server reports it saw, or `None` on timeout / parse failure.
pub fn stun_query(
    socket_id: u64,
    stun_host: &str,
    stun_port: u16,
    timeout: Duration,
) -> Option<(std::net::Ipv4Addr, u16)> {
    let socket = udp_sockets::get(socket_id)?;
    let addr = udp_sockets::resolve_one(stun_host, stun_port)?;
    let tx_id = fresh_tx_id();
    let pkt = build_binding_request(&tx_id);
    socket.send_to(&pkt, addr).ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    let mut buf = [0u8; 1024];
    loop {
        let (n, src) = socket.recv_from(&mut buf).ok()?;
        // Only accept responses from the STUN server we queried, and
        // only if the response's tx_id matches ours (defends against
        // races if the script is also exchanging traffic on this socket).
        if src != addr {
            continue;
        }
        if n < 20 || buf[8..20] != tx_id {
            continue;
        }
        return parse_binding_response(&buf[..n]);
    }
}

/// Result of a hole-punch attempt.
#[derive(Debug, Clone)]
pub struct PunchResult {
    pub established: bool,
    pub latency_ms: u64,
    pub bombards_sent: u32,
    pub peer_msg: Option<Vec<u8>>,
    pub peer_addr: Option<SocketAddr>,
}

/// Bombard the peer's `ip:port` at `interval` until we receive any
/// datagram on the socket (= bidirectional flow established) OR `timeout`
/// elapses (= NAT punch failed — both peers' NATs are likely too strict).
///
/// Sends a small probe payload each bombard. Receivers should treat the
/// FIRST inbound datagram on a hole-punched socket as the connection
/// confirmation; subsequent application traffic flows normally via
/// `udp_send_to` / `udp_recv` on the same socket.
pub fn hole_punch(
    socket_id: u64,
    peer_host: &str,
    peer_port: u16,
    timeout: Duration,
    interval: Duration,
    probe_payload: &[u8],
) -> PunchResult {
    let mut result = PunchResult {
        established: false,
        latency_ms: 0,
        bombards_sent: 0,
        peer_msg: None,
        peer_addr: None,
    };
    let Some(socket) = udp_sockets::get(socket_id) else {
        return result;
    };
    let Some(peer_addr) = udp_sockets::resolve_one(peer_host, peer_port) else {
        return result;
    };
    let start = Instant::now();
    let deadline = start + timeout;
    // Short per-loop recv timeout so we bombard at roughly `interval` rate.
    let recv_timeout = std::cmp::min(interval, Duration::from_millis(50));
    if socket.set_read_timeout(Some(recv_timeout)).is_err() {
        return result;
    }
    let mut buf = [0u8; 65_535];
    while Instant::now() < deadline {
        if socket.send_to(probe_payload, peer_addr).is_ok() {
            result.bombards_sent += 1;
        }
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                // Accept the first inbound — even if it's not from the
                // expected peer ip:port (a NAT might rewrite the src
                // port). Application code can verify identity at higher
                // protocol level.
                result.established = true;
                result.latency_ms = start.elapsed().as_millis() as u64;
                result.peer_msg = Some(buf[..n].to_vec());
                result.peer_addr = Some(src);
                return result;
            }
            Err(_) => {
                // Timeout / would-block — loop and bombard again.
            }
        }
        std::thread::sleep(interval);
    }
    result.latency_ms = start.elapsed().as_millis() as u64;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_request_packet_shape() {
        let tx = [0u8; 12];
        let pkt = build_binding_request(&tx);
        assert_eq!(pkt.len(), 20);
        assert_eq!(&pkt[0..2], &[0x00, 0x01], "msg type = Binding Request");
        assert_eq!(&pkt[2..4], &[0x00, 0x00], "msg length = 0");
        assert_eq!(
            &pkt[4..8],
            &STUN_MAGIC_COOKIE.to_be_bytes(),
            "magic cookie"
        );
        assert_eq!(&pkt[8..20], &tx, "transaction ID");
    }

    #[test]
    fn parse_xor_mapped_address_round_trip() {
        // Build a synthetic Binding Response with XOR-MAPPED-ADDRESS for
        // 203.0.113.45:51234 and verify the parser recovers it.
        let real_ip = std::net::Ipv4Addr::new(203, 0, 113, 45);
        let real_port: u16 = 51234;
        let xor_port = real_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
        let real_ip_u32 = u32::from_be_bytes(real_ip.octets());
        let xor_addr = real_ip_u32 ^ STUN_MAGIC_COOKIE;

        let mut pkt = vec![0u8; 32];
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes()); // Binding Response
        pkt[2..4].copy_from_slice(&12u16.to_be_bytes()); // 12 bytes of attrs
        pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        // tx id 8..20 stays zero
        // Attribute at 20: XOR-MAPPED-ADDRESS, 8-byte value
        pkt[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
        pkt[22..24].copy_from_slice(&8u16.to_be_bytes());
        pkt[24] = 0x00; // reserved
        pkt[25] = 0x01; // family IPv4
        pkt[26..28].copy_from_slice(&xor_port.to_be_bytes());
        pkt[28..32].copy_from_slice(&xor_addr.to_be_bytes());

        let parsed = parse_binding_response(&pkt).expect("must parse");
        assert_eq!(parsed.0, real_ip);
        assert_eq!(parsed.1, real_port);
    }

    #[test]
    fn parse_rejects_wrong_message_type() {
        let mut pkt = [0u8; 20];
        pkt[0] = 0x00;
        pkt[1] = 0x02; // not Binding Response
        pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        assert!(parse_binding_response(&pkt).is_none());
    }

    #[test]
    fn parse_rejects_wrong_magic_cookie() {
        let mut pkt = [0u8; 20];
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        pkt[4..8].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        assert!(parse_binding_response(&pkt).is_none());
    }

    #[test]
    fn parse_rejects_truncated_packet() {
        assert!(parse_binding_response(&[0u8; 4]).is_none());
        assert!(parse_binding_response(&[]).is_none());
    }

    #[test]
    fn fresh_tx_id_is_12_bytes() {
        let id = fresh_tx_id();
        assert_eq!(id.len(), 12);
        // Two consecutive calls must differ — same-nanosecond collision
        // chance is vanishingly small; if this fails the test rig is wrong.
        let id2 = fresh_tx_id();
        assert_ne!(id, id2, "two tx ids should differ — random source broken");
    }

    /// In-process fake STUN server: bind a UdpSocket, accept one Binding
    /// Request, reply with a synthetic Binding Response claiming the
    /// requester is at 198.51.100.7:50001. Verifies the full client path:
    /// build → send → recv → parse.
    #[test]
    fn stun_query_against_local_fake_server() {
        use std::net::UdpSocket;
        use std::thread;

        let server = UdpSocket::bind("127.0.0.1:0").expect("bind fake stun");
        let server_addr = server.local_addr().unwrap();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let (n, src) = server.recv_from(&mut buf).expect("recv req");
            // Echo tx_id back in a synthetic response advertising
            // 198.51.100.7:50001.
            let tx_id = &buf[8..20];
            let claim_ip = std::net::Ipv4Addr::new(198, 51, 100, 7);
            let claim_port: u16 = 50001;
            let xor_port = claim_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
            let claim_ip_u32 = u32::from_be_bytes(claim_ip.octets());
            let xor_addr = claim_ip_u32 ^ STUN_MAGIC_COOKIE;
            let mut resp = vec![0u8; 32];
            resp[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
            resp[2..4].copy_from_slice(&12u16.to_be_bytes());
            resp[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            resp[8..20].copy_from_slice(tx_id);
            resp[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
            resp[22..24].copy_from_slice(&8u16.to_be_bytes());
            resp[24] = 0x00;
            resp[25] = 0x01;
            resp[26..28].copy_from_slice(&xor_port.to_be_bytes());
            resp[28..32].copy_from_slice(&xor_addr.to_be_bytes());
            let _ = server.send_to(&resp, src);
            assert!(n >= 20, "request must be ≥ 20 bytes");
        });

        let client = udp_sockets::open("127.0.0.1", 0).expect("bind client");
        let result = stun_query(
            client,
            &server_addr.ip().to_string(),
            server_addr.port(),
            Duration::from_secs(2),
        );
        let (ip, port) = result.expect("STUN query must round-trip");
        assert_eq!(ip, std::net::Ipv4Addr::new(198, 51, 100, 7));
        assert_eq!(port, 50001);
        udp_sockets::close(client);
    }

    /// Two pool sockets bombard each other → first to receive returns
    /// established=true. Mirrors the v1 punch state machine on loopback.
    #[test]
    fn hole_punch_loopback_establishes() {
        let a = udp_sockets::open("127.0.0.1", 0).expect("bind a");
        let b = udp_sockets::open("127.0.0.1", 0).expect("bind b");
        let a_addr = udp_sockets::local_addr(a).unwrap();
        let b_addr = udp_sockets::local_addr(b).unwrap();

        // Run B's punch on a background thread aimed at A; main thread
        // runs A's punch aimed at B. Both should establish within ~50ms.
        let b_addr_str = b_addr.ip().to_string();
        let a_addr_str = a_addr.ip().to_string();
        let b_handle = std::thread::spawn(move || {
            hole_punch(
                b,
                &a_addr_str,
                a_addr.port(),
                Duration::from_secs(2),
                Duration::from_millis(20),
                b"hi from b",
            )
        });
        let a_result = hole_punch(
            a,
            &b_addr_str,
            b_addr.port(),
            Duration::from_secs(2),
            Duration::from_millis(20),
            b"hi from a",
        );
        let b_result = b_handle.join().expect("thread");
        assert!(a_result.established, "a should see b's bombard");
        assert!(b_result.established, "b should see a's bombard");
        assert!(a_result.bombards_sent >= 1);
        assert!(b_result.bombards_sent >= 1);
        assert!(a_result.peer_msg.is_some());
        assert!(b_result.peer_msg.is_some());

        udp_sockets::close(a);
        udp_sockets::close(b);
    }
}
