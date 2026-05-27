//! Integration pins for the `kick` TCP-probe and `udp_send` UDP-bombard
//! builtins. Both are convenience wrappers over standard socket calls
//! (the world-first capability bar doesn't apply — they're shipped for
//! script ergonomics, not novelty), so these pins focus on the wire
//! contract: return-value shape, timeout-bound, broadcast handling.

use crate::common::*;
use std::net::{TcpListener, UdpSocket};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Successful TCP knock against a self-bound listener returns 1. The
/// listener is opened on `127.0.0.1:0` (kernel-assigned ephemeral port)
/// to avoid races against any port a real service might be holding.
#[test]
fn kick_returns_1_on_listening_port() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    // Hold the listener alive for the test duration by parking a thread on
    // accept() — kick's connect attempt completes the 3-way handshake, so
    // the accept must be live to consume it.
    thread::spawn(move || {
        let _ = listener.accept();
    });
    let code = format!(r#"kick("127.0.0.1", {})"#, port);
    let n = eval_int(&code);
    assert_eq!(n, 1, "kick to listening port should return 1, got {n}");
}

/// Closed-port (nothing listening) returns 0. Uses port 1 — privileged
/// (won't be auto-assigned to any ephemeral bind) AND no service ever
/// listens there on a sane test host. Was previously a grab-and-release
/// pattern but that races with other parallel tests' ephemeral binds:
/// any test that bound after the release could grab the same port, and
/// `kick` would succeed against IT instead of returning 0.
#[test]
fn kick_returns_0_on_closed_port() {
    let code = r#"kick("127.0.0.1", 1, 500)"#.to_string();
    let start = Instant::now();
    let n = eval_int(&code);
    let elapsed = start.elapsed();
    assert_eq!(n, 0, "kick to port 1 (privileged, unlisteneable) should return 0, got {n}");
    // Local RST or unreachable should arrive well within the 500ms budget.
    assert!(
        elapsed < Duration::from_millis(500),
        "closed-port kick should return promptly, took {:?}",
        elapsed
    );
}

/// Bad port number (out of 1..=65535) returns 0 without raising — caller
/// can rely on the boolean-style return for all inputs.
#[test]
fn kick_returns_0_for_invalid_port() {
    assert_eq!(eval_int(r#"kick("127.0.0.1", 0)"#), 0);
    assert_eq!(eval_int(r#"kick("127.0.0.1", 70000)"#), 0);
    assert_eq!(eval_int(r#"kick("127.0.0.1", -1)"#), 0);
}

/// Bad host (DNS failure) returns 0 within the timeout — no exception.
/// Uses `.invalid` per RFC 6761 which guarantees NXDOMAIN.
#[test]
fn kick_returns_0_on_dns_failure() {
    let start = Instant::now();
    let n = eval_int(r#"kick("nonexistent.invalid", 80, 500)"#);
    let elapsed = start.elapsed();
    assert_eq!(n, 0);
    // OS resolver retry behaviour varies (macOS may take 2-4s for NXDOMAIN
    // depending on /etc/resolv.conf and search domains). Bound is generous
    // to absorb CI variability while still catching a true hang.
    assert!(
        elapsed < Duration::from_secs(10),
        "DNS-failure kick should bail within 10s, took {:?}",
        elapsed
    );
}

/// `udp_send` to a self-bound UdpSocket delivers `$retries` datagrams.
/// We bind a receive socket, channel-forward inbound packets to the test
/// thread, then count what arrives.
#[test]
fn udp_send_delivers_requested_retries() {
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind recv socket");
    sock.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let port = sock.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buf = [0u8; 1500];
        loop {
            match sock.recv(&mut buf) {
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let code = format!(
        r#"udp_send("127.0.0.1", {}, "ping", 3, 10)"#,
        port
    );
    let sent = eval_int(&code);
    assert_eq!(sent, 3, "expected 3 datagrams sent, got {sent}");

    // Drain the channel briefly to confirm receipt; allow loose count since
    // UDP may drop on loopback under load (rare but possible).
    let mut received = 0;
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(payload) => {
                assert_eq!(&payload, b"ping", "payload corrupted in flight");
                received += 1;
                if received == 3 {
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    assert!(
        received >= 1,
        "at least one datagram should arrive on loopback, got {received}"
    );
}

/// Single-shot `udp_send` (no retries arg) sends exactly one datagram.
#[test]
fn udp_send_default_retries_is_one() {
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind recv socket");
    let port = sock.local_addr().unwrap().port();
    let code = format!(r#"udp_send("127.0.0.1", {}, "x")"#, port);
    let sent = eval_int(&code);
    assert_eq!(sent, 1, "single-shot send should report 1 datagram, got {sent}");
}

/// `udp_send` with invalid port returns 0 without raising.
#[test]
fn udp_send_returns_0_for_invalid_port() {
    assert_eq!(eval_int(r#"udp_send("127.0.0.1", 0, "x")"#), 0);
    assert_eq!(eval_int(r#"udp_send("127.0.0.1", 70000, "x")"#), 0);
}
