//! Integration pins for the 5 TURN builtins exercised via stryke source.
//!
//! Unit-level coverage of the wire protocol + the full mock-server E2E
//! lives in `strykelang/turn_client.rs::tests`. This file exercises the
//! `turn_allocate` / `turn_permission` / `turn_send` / `turn_recv` /
//! `turn_refresh` BUILTIN surface — proving the script-side contract
//! (return hashref shapes, defaults, error paths) matches the
//! documented LSP hover docs.
//!
//! All tests use an in-process mock TURN server bound on loopback — no
//! external coturn dependency. Real-coturn interop must be verified
//! locally by the user once they have a server.

use crate::common::*;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::thread;
use stryke::turn_client::{
    attr, build_message, msg_type, parse_xor_addr, push_attr, push_xor_addr_attr,
};

mod helpers {
    use super::*;
    use stryke::turn_client::message_type;

    pub fn spawn_mock_turn() -> std::net::SocketAddr {
        let server = UdpSocket::bind("127.0.0.1:0").expect("bind mock");
        let addr = server.local_addr().unwrap();
        thread::spawn(move || {
            let mut buf = [0u8; 2048];
            // Round 1: unauth Allocate → 401.
            let (_, src) = server.recv_from(&mut buf).expect("recv 1");
            let tx1: [u8; 12] = buf[8..20].try_into().unwrap();
            let mut a1: Vec<u8> = Vec::new();
            push_attr(&mut a1, attr::ERROR_CODE, &[0, 0, 4, 1]);
            push_attr(&mut a1, attr::REALM, b"turn.test");
            push_attr(&mut a1, attr::NONCE, b"NONCE-XYZ");
            let r1 = build_message(msg_type::ALLOCATE_ERROR, &tx1, &a1);
            server.send_to(&r1, src).unwrap();

            // Round 2: auth Allocate → success.
            let (_, _) = server.recv_from(&mut buf).expect("recv 2");
            let tx2: [u8; 12] = buf[8..20].try_into().unwrap();
            let mut a2: Vec<u8> = Vec::new();
            push_xor_addr_attr(
                &mut a2,
                attr::XOR_RELAYED_ADDRESS,
                IpAddr::V4(Ipv4Addr::new(198, 51, 100, 77)),
                49000,
                &tx2,
            );
            push_attr(&mut a2, attr::LIFETIME, &600u32.to_be_bytes());
            let r2 = build_message(msg_type::ALLOCATE_SUCCESS, &tx2, &a2);
            server.send_to(&r2, src).unwrap();

            // CreatePermission → success.
            let (_, _) = server.recv_from(&mut buf).expect("recv 3");
            let tx3: [u8; 12] = buf[8..20].try_into().unwrap();
            let r3 = build_message(msg_type::CREATE_PERMISSION_SUCCESS, &tx3, &[]);
            server.send_to(&r3, src).unwrap();

            // SendIndication → echo as DataIndication.
            let (n4, _) = server.recv_from(&mut buf).expect("recv 4");
            assert_eq!(message_type(&buf[..n4]), msg_type::SEND_INDICATION);
            let tx4: [u8; 12] = buf[8..20].try_into().unwrap();
            // Walk attributes — replicate iter_attrs since it's private.
            let mut peer: Option<(IpAddr, u16)> = None;
            let mut data: Option<Vec<u8>> = None;
            let body_end = 20
                + u16::from_be_bytes([buf[2], buf[3]]) as usize;
            let mut off = 20;
            while off + 4 <= body_end {
                let t = u16::from_be_bytes([buf[off], buf[off + 1]]);
                let l = u16::from_be_bytes([buf[off + 2], buf[off + 3]]) as usize;
                let vs = off + 4;
                let ve = vs + l;
                if ve > body_end {
                    break;
                }
                if t == attr::XOR_PEER_ADDRESS {
                    peer = parse_xor_addr(&buf[vs..ve], &tx4);
                } else if t == attr::DATA {
                    data = Some(buf[vs..ve].to_vec());
                }
                off = ve;
                if off % 4 != 0 {
                    off += 4 - (off % 4);
                }
            }
            let (pip, pport) = peer.unwrap();
            let payload = data.unwrap();
            let mut a4: Vec<u8> = Vec::new();
            push_xor_addr_attr(&mut a4, attr::XOR_PEER_ADDRESS, pip, pport, &tx4);
            push_attr(&mut a4, attr::DATA, &payload);
            let r4 = build_message(msg_type::DATA_INDICATION, &tx4, &a4);
            server.send_to(&r4, src).unwrap();
        });
        addr
    }
}

/// Stryke-source path: udp_open → turn_allocate → turn_permission →
/// turn_send → turn_recv. Full TURN session driven from `.stk` code
/// against the in-process mock server. If this passes, the 5 turn_*
/// builtins compose correctly end-to-end at the BUILTIN level.
#[test]
fn turn_full_session_via_stryke_source() {
    let turn_addr = helpers::spawn_mock_turn();
    let code = format!(
        r#"
        my $sock = udp_open()
        if ($sock == 0) {{ "BIND-FAIL" }}
        else {{
            my $alloc = turn_allocate($sock, "{ip}", {port}, "alice", "wonderland", 2000)
            if (!defined $alloc) {{
                udp_close($sock)
                "ALLOC-FAIL"
            }} else {{
                my $perm = turn_permission($sock, "192.0.2.10", 2000)
                my $sent = turn_send($sock, "192.0.2.10", 9999, "hello via turn")
                my $reply = turn_recv($sock, 2000)
                udp_close($sock)
                if (!defined $reply) {{ "RECV-FAIL" }}
                else {{
                    sprintf(
                        "relay=%s:%d|lifetime=%d|perm=%d|sent=%d|payload=%s|peer=%s:%d",
                        $alloc->{{relay_ip}}, $alloc->{{relay_port}}, $alloc->{{lifetime_secs}},
                        $perm, $sent, $reply->{{payload}},
                        $reply->{{peer_ip}}, $reply->{{peer_port}})
                }}
            }}
        }}
        "#,
        ip = turn_addr.ip(),
        port = turn_addr.port()
    );
    let s = eval_string(&code);
    let s = s.trim();
    assert!(
        s.starts_with("relay=198.51.100.77:49000"),
        "expected relay address from mock TURN, got: {s}"
    );
    assert!(s.contains("lifetime=600"), "lifetime field, got: {s}");
    assert!(s.contains("perm=1"), "permission success, got: {s}");
    assert!(s.contains("sent="), "send byte count, got: {s}");
    assert!(s.contains("payload=hello via turn"), "echo payload, got: {s}");
    assert!(s.contains("peer=192.0.2.10:9999"), "peer addr, got: {s}");
}

/// `turn_allocate` against an unreachable server returns `undef` within
/// the timeout — no exception. Caller can use `defined` to branch on
/// success.
#[test]
fn turn_allocate_returns_undef_on_unreachable_server() {
    // Bind+drop to get a guaranteed-dead port.
    let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
    let dead = probe.local_addr().unwrap().port();
    drop(probe);
    let code = format!(
        r#"
        my $sock = udp_open()
        my $r = turn_allocate($sock, "127.0.0.1", {dead}, "u", "p", 200)
        udp_close($sock)
        defined $r ? "got" : "undef"
        "#,
        dead = dead
    );
    let s = eval_string(&code);
    assert_eq!(s.trim(), "undef");
}

/// `turn_permission` / `turn_send` / `turn_recv` / `turn_refresh` against
/// a socket that never had a successful allocation return 0 / 0 / undef / 0.
/// Pins the documented "no allocation" branch.
#[test]
fn turn_ops_without_allocation_return_clean_failure_codes() {
    let s = eval_string(
        r#"
        my $sock = udp_open()
        my $perm = turn_permission($sock, "192.0.2.1")
        my $sent = turn_send($sock, "192.0.2.1", 9999, "x")
        my $recv = defined turn_recv($sock, 50) ? 1 : 0
        my $refr = turn_refresh($sock)
        udp_close($sock)
        sprintf("perm=%d sent=%d recv=%d refresh=%d", $perm, $sent, $recv, $refr)
        "#,
    );
    assert_eq!(s.trim(), "perm=0 sent=0 recv=0 refresh=0");
}
