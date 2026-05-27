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

/// Parse a STUN Binding Response, returning the public (`IpAddr`, port)
/// the server saw us coming from. Handles both IPv4 (family=0x01,
/// 4-byte address) and IPv6 (family=0x02, 16-byte address). The IPv6
/// XOR computation per RFC 8489 §14.2 is: byte 0..3 of the XOR-addr is
/// XOR'd with the magic cookie, bytes 4..15 are XOR'd with the
/// transaction ID — so we need the 12-byte tx_id for IPv6 unscrambling.
///
/// Returns `None` on:
///   * not a Binding Response (msg type ≠ 0x0101)
///   * truncated packet
///   * no XOR-MAPPED-ADDRESS or MAPPED-ADDRESS attribute
///   * unknown family
pub fn parse_binding_response(pkt: &[u8]) -> Option<(std::net::IpAddr, u16)> {
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
    let mut tx_id = [0u8; 12];
    tx_id.copy_from_slice(&pkt[8..20]);

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
            // Layout: 1B reserved, 1B family (0x01 = IPv4, 0x02 = IPv6),
            // 2B xor_port, then 4B (IPv4) or 16B (IPv6) xor_addr.
            0x0020 if attr_len >= 8 => {
                let family = pkt[val_start + 1];
                let xor_port = u16::from_be_bytes([pkt[val_start + 2], pkt[val_start + 3]]);
                let port = xor_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
                match family {
                    0x01 => {
                        let xor_addr = u32::from_be_bytes([
                            pkt[val_start + 4],
                            pkt[val_start + 5],
                            pkt[val_start + 6],
                            pkt[val_start + 7],
                        ]);
                        let addr = xor_addr ^ STUN_MAGIC_COOKIE;
                        let ip = std::net::Ipv4Addr::from(addr.to_be_bytes());
                        return Some((std::net::IpAddr::V4(ip), port));
                    }
                    0x02 if attr_len >= 20 => {
                        // IPv6: 16-byte XOR address. First 4 bytes XOR'd
                        // with the magic cookie, remaining 12 XOR'd with
                        // the transaction ID.
                        let mut octets = [0u8; 16];
                        octets.copy_from_slice(&pkt[val_start + 4..val_start + 20]);
                        let cookie_be = STUN_MAGIC_COOKIE.to_be_bytes();
                        for i in 0..4 {
                            octets[i] ^= cookie_be[i];
                        }
                        for i in 0..12 {
                            octets[4 + i] ^= tx_id[i];
                        }
                        let ip = std::net::Ipv6Addr::from(octets);
                        return Some((std::net::IpAddr::V6(ip), port));
                    }
                    _ => {}
                }
            }
            // MAPPED-ADDRESS (legacy, RFC 3489) — same layout sans XOR.
            0x0001 if attr_len >= 8 => {
                let family = pkt[val_start + 1];
                let port = u16::from_be_bytes([pkt[val_start + 2], pkt[val_start + 3]]);
                match family {
                    0x01 => {
                        let ip = std::net::Ipv4Addr::new(
                            pkt[val_start + 4],
                            pkt[val_start + 5],
                            pkt[val_start + 6],
                            pkt[val_start + 7],
                        );
                        return Some((std::net::IpAddr::V4(ip), port));
                    }
                    0x02 if attr_len >= 20 => {
                        let mut octets = [0u8; 16];
                        octets.copy_from_slice(&pkt[val_start + 4..val_start + 20]);
                        let ip = std::net::Ipv6Addr::from(octets);
                        return Some((std::net::IpAddr::V6(ip), port));
                    }
                    _ => {}
                }
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

/// Generate a 12-byte transaction ID from time + a process-monotonic
/// counter + PID. Back-to-back calls within the same nanosecond MUST
/// produce different IDs — the counter is what guarantees that (time
/// alone wasn't enough; modern CPUs can issue many calls per nanosecond
/// granularity, observed empirically in the full test suite).
///
/// Layout: bytes 0..8 = time nanos (low 64 bits), bytes 8..12 = atomic
/// counter (low 32 bits). PID XOR'd into the time region so two
/// concurrent processes that happen to issue at the same nanosecond
/// with the same counter value still differ.
fn fresh_tx_id() -> [u8; 12] {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut id = [0u8; 12];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    id[0..8].copy_from_slice(&(nanos as u64).to_le_bytes());
    id[8..12].copy_from_slice(&(counter as u32).to_le_bytes());
    let pid = std::process::id();
    let pid_be = pid.to_le_bytes();
    for i in 0..4 {
        id[i] ^= pid_be[i];
    }
    id
}

/// Query a STUN server via socket `id`. Returns `(public_ip, public_port)`
/// the server reports it saw, or `None` on timeout / parse failure. The
/// returned IP is `IpAddr::V4` or `IpAddr::V6` depending on the server's
/// XOR-MAPPED-ADDRESS family.
pub fn stun_query(
    socket_id: u64,
    stun_host: &str,
    stun_port: u16,
    timeout: Duration,
) -> Option<(std::net::IpAddr, u16)> {
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

/// Default STUN servers used by `classify_nat` when the caller doesn't
/// supply a list. Mix of Google + Cloudflare + Nextcloud means at least
/// one survives any single-provider outage; the diverse paths make
/// symmetric-NAT detection more reliable than a same-provider trio
/// (some symmetric NATs use destination-IP-buckets, so two STUN servers
/// in the same /24 may produce matching ports even on a symmetric NAT).
pub const DEFAULT_STUN_SERVERS: &[(&str, u16)] = &[
    ("stun.l.google.com", 19302),
    ("stun.cloudflare.com", 3478),
    ("stun.nextcloud.com", 443),
];

/// Per-server STUN observation collected during NAT classification.
#[derive(Debug, Clone)]
pub struct StunObservation {
    pub server: String,
    pub ok: bool,
    pub public_ip: Option<std::net::IpAddr>,
    pub public_port: Option<u16>,
}

/// Result of NAT classification.
///
/// `nat_type` interpretation:
///   * `"cone"`      — all responding servers reported the SAME public
///                     port → the NAT uses a single mapping per source
///                     socket regardless of destination. `punch` will
///                     work to any peer.
///   * `"symmetric"` — responding servers reported DIFFERENT public
///                     ports → the NAT allocates a fresh public port per
///                     destination. `punch` will FAIL because the port
///                     the peer punches at (learned from one STUN
///                     server) won't be the port your traffic to them
///                     uses. Requires a TURN relay to recover.
///   * `"unknown"`   — fewer than 2 servers responded → can't classify;
///                     don't draw conclusions.
#[derive(Debug, Clone)]
pub struct NatClassification {
    pub nat_type: &'static str,
    pub public_ip: Option<std::net::IpAddr>,
    pub observations: Vec<StunObservation>,
    pub queried: u32,
    pub succeeded: u32,
}

/// Query multiple STUN servers via the SAME socket and classify the NAT
/// based on whether the reported public ports match across servers. All
/// queries must use the same socket so we're testing the SAME NAT
/// mapping behaviour across destinations — that's the whole point.
pub fn classify_nat(
    socket_id: u64,
    servers: &[(&str, u16)],
    timeout: Duration,
) -> NatClassification {
    let mut observations: Vec<StunObservation> = Vec::with_capacity(servers.len());
    let mut ports_seen: Vec<u16> = Vec::with_capacity(servers.len());
    let mut ip_seen: Option<std::net::IpAddr> = None;

    for (host, port) in servers {
        let result = stun_query(socket_id, host, *port, timeout);
        let observation = match result {
            Some((ip, p)) => {
                ports_seen.push(p);
                // The public IP should be identical across all servers
                // (it's our outbound interface IP). If they disagree
                // something weird is happening (multi-WAN load balancing?)
                // — record the first.
                if ip_seen.is_none() {
                    ip_seen = Some(ip);
                }
                StunObservation {
                    server: format!("{}:{}", host, port),
                    ok: true,
                    public_ip: Some(ip),
                    public_port: Some(p),
                }
            }
            None => StunObservation {
                server: format!("{}:{}", host, port),
                ok: false,
                public_ip: None,
                public_port: None,
            },
        };
        observations.push(observation);
    }

    let queried = servers.len() as u32;
    let succeeded = ports_seen.len() as u32;
    let nat_type = if succeeded < 2 {
        "unknown"
    } else if ports_seen.iter().all(|p| *p == ports_seen[0]) {
        "cone"
    } else {
        "symmetric"
    };

    NatClassification {
        nat_type,
        public_ip: ip_seen,
        observations,
        queried,
        succeeded,
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
        assert_eq!(&pkt[4..8], &STUN_MAGIC_COOKIE.to_be_bytes(), "magic cookie");
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
        assert_eq!(parsed.0, std::net::IpAddr::V4(real_ip));
        assert_eq!(parsed.1, real_port);
    }

    /// IPv6 XOR-MAPPED-ADDRESS: 16-byte address scrambled with magic-
    /// cookie || tx_id. Pin the full round-trip so future RFC 8489 reads
    /// don't accidentally regress the encoding.
    #[test]
    fn parse_xor_mapped_address_ipv6_round_trip() {
        let real_ip = std::net::Ipv6Addr::new(
            0x2001, 0x0db8, 0xdead, 0xbeef, 0xcafe, 0xface, 0x1234, 0x5678,
        );
        let real_port: u16 = 51234;
        let tx_id: [u8; 12] = [
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
        ];
        let xor_port = real_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
        let mut xor_addr = real_ip.octets();
        let cookie_be = STUN_MAGIC_COOKIE.to_be_bytes();
        for i in 0..4 {
            xor_addr[i] ^= cookie_be[i];
        }
        for i in 0..12 {
            xor_addr[4 + i] ^= tx_id[i];
        }

        let mut pkt = vec![0u8; 44]; // 20 header + 4 attr_hdr + 20 attr_val
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        pkt[2..4].copy_from_slice(&24u16.to_be_bytes()); // 4 + 20 = 24 bytes of attrs
        pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
        pkt[8..20].copy_from_slice(&tx_id);
        pkt[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
        pkt[22..24].copy_from_slice(&20u16.to_be_bytes()); // attr len = 20
        pkt[24] = 0x00; // reserved
        pkt[25] = 0x02; // family IPv6
        pkt[26..28].copy_from_slice(&xor_port.to_be_bytes());
        pkt[28..44].copy_from_slice(&xor_addr);

        let parsed = parse_binding_response(&pkt).expect("must parse IPv6 XOR-MAPPED");
        assert_eq!(parsed.0, std::net::IpAddr::V6(real_ip));
        assert_eq!(parsed.1, real_port);
    }

    /// RFC 8489 §14: "Clients MUST ignore comprehension-optional
    /// attributes they don't understand". Pin via a Binding Response
    /// that has an unknown attribute (type 0x9001 — high bit set
    /// indicates comprehension-optional per §14) sandwiched between
    /// the header and the XOR-MAPPED-ADDRESS. Parser must skip the
    /// junk attribute and still recover the address correctly.
    ///
    /// Catches a future regression where the attribute-walk loop
    /// accidentally bails on unknown types instead of skipping them.
    #[test]
    fn parse_ignores_unknown_optional_attributes() {
        let real_ip = std::net::Ipv4Addr::new(192, 0, 2, 99);
        let real_port: u16 = 12345;
        let xor_port = real_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
        let xor_addr = u32::from_be_bytes(real_ip.octets()) ^ STUN_MAGIC_COOKIE;

        // Layout: header (20) + UNKNOWN attr (8) + XOR-MAPPED-ADDR (12)
        // unknown attr: type=0x9001 (comprehension-optional), len=4,
        // value="JUNK"
        let mut pkt = vec![0u8; 40];
        pkt[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        pkt[2..4].copy_from_slice(&20u16.to_be_bytes()); // 8 + 12 = 20 bytes of attrs
        pkt[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());

        // Unknown attr at offset 20.
        pkt[20..22].copy_from_slice(&0x9001u16.to_be_bytes());
        pkt[22..24].copy_from_slice(&4u16.to_be_bytes());
        pkt[24..28].copy_from_slice(b"JUNK");

        // XOR-MAPPED-ADDRESS at offset 28.
        pkt[28..30].copy_from_slice(&0x0020u16.to_be_bytes());
        pkt[30..32].copy_from_slice(&8u16.to_be_bytes());
        pkt[32] = 0x00; // reserved
        pkt[33] = 0x01; // family IPv4
        pkt[34..36].copy_from_slice(&xor_port.to_be_bytes());
        pkt[36..40].copy_from_slice(&xor_addr.to_be_bytes());

        let parsed = parse_binding_response(&pkt).expect("must parse despite unknown attribute");
        assert_eq!(parsed.0, std::net::IpAddr::V4(real_ip));
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
        assert_eq!(
            ip,
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(198, 51, 100, 7))
        );
        assert_eq!(port, 50001);
        udp_sockets::close(client);
    }

    /// Multi-server NAT classification using TWO in-process fake STUN
    /// servers that DISAGREE on the port they report. This is the
    /// "symmetric NAT" signature: same socket, different destinations,
    /// different reported public ports. Pins the classifier's logic
    /// end-to-end through the real STUN protocol path.
    #[test]
    fn classify_nat_detects_symmetric_when_servers_report_different_ports() {
        use std::net::UdpSocket;
        use std::thread;

        // Helper: spin up a fake STUN that always claims the requester
        // is at `claim_ip:claim_port`. Returns server's bound addr.
        fn spawn_fake_stun(claim_port: u16) -> std::net::SocketAddr {
            let server = UdpSocket::bind("127.0.0.1:0").expect("bind fake");
            let addr = server.local_addr().unwrap();
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let (_, src) = server.recv_from(&mut buf).expect("recv");
                let tx_id = &buf[8..20];
                let claim_ip = std::net::Ipv4Addr::new(198, 51, 100, 1);
                let xor_port = claim_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
                let xor_addr = u32::from_be_bytes(claim_ip.octets()) ^ STUN_MAGIC_COOKIE;
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
            });
            addr
        }

        let a = spawn_fake_stun(40001);
        let b = spawn_fake_stun(40002); // DIFFERENT port → symmetric signature

        let client = udp_sockets::open("127.0.0.1", 0).expect("bind client");
        let a_host = a.ip().to_string();
        let b_host = b.ip().to_string();
        let servers = [(a_host.as_str(), a.port()), (b_host.as_str(), b.port())];
        let result = classify_nat(client, &servers, Duration::from_secs(2));
        udp_sockets::close(client);

        assert_eq!(result.nat_type, "symmetric");
        assert_eq!(result.queried, 2);
        assert_eq!(result.succeeded, 2);
        assert_eq!(result.observations.len(), 2);
        assert_eq!(
            result.public_ip,
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                198, 51, 100, 1
            )))
        );
    }

    /// Two fake STUN servers that REPORT THE SAME port → cone NAT.
    #[test]
    fn classify_nat_detects_cone_when_servers_agree() {
        use std::net::UdpSocket;
        use std::thread;
        fn spawn(claim_port: u16) -> std::net::SocketAddr {
            let s = UdpSocket::bind("127.0.0.1:0").unwrap();
            let a = s.local_addr().unwrap();
            thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let (_, src) = s.recv_from(&mut buf).unwrap();
                let tx_id = &buf[8..20];
                let claim_ip = std::net::Ipv4Addr::new(198, 51, 100, 9);
                let xp = claim_port ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
                let xa = u32::from_be_bytes(claim_ip.octets()) ^ STUN_MAGIC_COOKIE;
                let mut r = vec![0u8; 32];
                r[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
                r[2..4].copy_from_slice(&12u16.to_be_bytes());
                r[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
                r[8..20].copy_from_slice(tx_id);
                r[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
                r[22..24].copy_from_slice(&8u16.to_be_bytes());
                r[24] = 0x00;
                r[25] = 0x01;
                r[26..28].copy_from_slice(&xp.to_be_bytes());
                r[28..32].copy_from_slice(&xa.to_be_bytes());
                let _ = s.send_to(&r, src);
            });
            a
        }
        let a = spawn(45000);
        let b = spawn(45000); // SAME port → cone
        let client = udp_sockets::open("127.0.0.1", 0).unwrap();
        let aip = a.ip().to_string();
        let bip = b.ip().to_string();
        let servers = [(aip.as_str(), a.port()), (bip.as_str(), b.port())];
        let result = classify_nat(client, &servers, Duration::from_secs(2));
        udp_sockets::close(client);
        assert_eq!(result.nat_type, "cone");
        assert_eq!(result.succeeded, 2);
    }

    /// Only one STUN server responded → can't classify, returns
    /// "unknown". Important contract: the caller shouldn't conclude
    /// "punch will work" from a single-server result.
    #[test]
    fn classify_nat_returns_unknown_when_only_one_server_responds() {
        use std::net::UdpSocket;
        use std::thread;
        // One working fake.
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let working = s.local_addr().unwrap();
        thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let (_, src) = s.recv_from(&mut buf).unwrap();
            let tx_id = &buf[8..20];
            let claim_ip = std::net::Ipv4Addr::new(198, 51, 100, 9);
            let xp = 12345u16 ^ ((STUN_MAGIC_COOKIE >> 16) as u16);
            let xa = u32::from_be_bytes(claim_ip.octets()) ^ STUN_MAGIC_COOKIE;
            let mut r = vec![0u8; 32];
            r[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
            r[2..4].copy_from_slice(&12u16.to_be_bytes());
            r[4..8].copy_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
            r[8..20].copy_from_slice(tx_id);
            r[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
            r[22..24].copy_from_slice(&8u16.to_be_bytes());
            r[24] = 0x00;
            r[25] = 0x01;
            r[26..28].copy_from_slice(&xp.to_be_bytes());
            r[28..32].copy_from_slice(&xa.to_be_bytes());
            let _ = s.send_to(&r, src);
        });

        // Silent fake (bind, never reply).
        let silent_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let silent = silent_sock.local_addr().unwrap();
        drop(silent_sock);

        let client = udp_sockets::open("127.0.0.1", 0).unwrap();
        let wip = working.ip().to_string();
        let sip = silent.ip().to_string();
        let servers = [
            (wip.as_str(), working.port()),
            (sip.as_str(), silent.port()),
        ];
        let result = classify_nat(client, &servers, Duration::from_millis(300));
        udp_sockets::close(client);
        assert_eq!(result.nat_type, "unknown");
        assert_eq!(result.queried, 2);
        assert_eq!(result.succeeded, 1, "exactly one server responded");
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
