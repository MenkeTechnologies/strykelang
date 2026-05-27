//! Persistent UDP socket pool for stryke's hole-punching builtins.
//!
//! The `udp_send($host, $port, $payload)` builtin binds a fresh ephemeral
//! socket per call — fine for fire-and-forget broadcasts but useless for
//! NAT hole-punching. Hole-punching requires the SAME socket throughout:
//! the NAT mapping (your public ip:port as seen from outside) is bound to
//! the local socket's (src_ip, src_port) tuple, so STUN discovery, the
//! bombard phase, and the subsequent peer-to-peer traffic all have to go
//! through one stable socket.
//!
//! This module provides that stable storage. Stryke scripts get a `u64`
//! handle (returned by `udp_open`) and pass it to `stun`, `punch`,
//! `udp_send_to`, `udp_recv`, `udp_close`. Behind the handle is an
//! `Arc<UdpSocket>` in a process-global `HashMap`, looked up by id.
//!
//! Thread-safety: the pool itself is `Mutex<HashMap>`; individual sockets
//! are `Arc<UdpSocket>` so a recv on one thread can run concurrently with
//! a send on another (`UdpSocket` is `Sync` on every supported OS).

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn pool() -> &'static Mutex<HashMap<u64, Arc<UdpSocket>>> {
    static POOL: OnceLock<Mutex<HashMap<u64, Arc<UdpSocket>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Bind a UDP socket at the given address (default `0.0.0.0:0` — kernel
/// picks an ephemeral port). Returns the handle ID on success, `None` on
/// bind failure. The socket is registered in the pool until `close(id)`.
pub fn open(bind_host: &str, bind_port: u16) -> Option<u64> {
    let bind_str = format!("{}:{}", bind_host, bind_port);
    let socket = UdpSocket::bind(&bind_str).ok()?;
    // SO_BROADCAST so subnet broadcasts (WoL, SSDP) work without extra setup.
    let _ = socket.set_broadcast(true);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut g) = pool().lock() {
        g.insert(id, Arc::new(socket));
        Some(id)
    } else {
        None
    }
}

/// Look up a socket handle. Returns an Arc clone — callers can hold this
/// across long operations (recv loops, STUN exchanges) without blocking
/// other handles in the pool.
pub fn get(id: u64) -> Option<Arc<UdpSocket>> {
    pool().lock().ok()?.get(&id).cloned()
}

/// Local bound address of a socket (the ephemeral port the kernel assigned).
/// Needed to print "tell the peer to punch ip:THISPORT" in the demo.
pub fn local_addr(id: u64) -> Option<SocketAddr> {
    get(id)?.local_addr().ok()
}

/// Resolve a `(host, port)` pair via the OS resolver. Returns the first
/// SocketAddr or `None` on DNS failure. Used by `send_to` / `stun` / `punch`.
pub fn resolve_one(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port).to_socket_addrs().ok()?.next()
}

/// Send `payload` to `(host, port)` on the pool socket. Returns bytes
/// sent on success, `None` on DNS / socket failure.
pub fn send_to(id: u64, host: &str, port: u16, payload: &[u8]) -> Option<usize> {
    let socket = get(id)?;
    let addr = resolve_one(host, port)?;
    socket.send_to(payload, addr).ok()
}

/// Receive one datagram on the pool socket, blocking up to `timeout`.
/// Returns `(payload, source_addr)` on success, `None` on timeout / error.
/// `timeout = None` blocks indefinitely.
pub fn recv(id: u64, timeout: Option<Duration>) -> Option<(Vec<u8>, SocketAddr)> {
    let socket = get(id)?;
    // SO_RCVTIMEO is per-socket — temporarily set it, do one recv, then
    // restore. Allocating a fresh buf each call is cheap vs the syscall
    // cost; max-MTU sized (1500 + headroom for jumbo) covers normal use.
    if socket.set_read_timeout(timeout).is_err() {
        return None;
    }
    let mut buf = vec![0u8; 65_535];
    match socket.recv_from(&mut buf) {
        Ok((n, src)) => {
            buf.truncate(n);
            Some((buf, src))
        }
        Err(_) => None,
    }
}

/// Drop a socket from the pool. Returns true if the id was present.
/// Idempotent — calling on an unknown id returns false without error.
pub fn close(id: u64) -> bool {
    pool()
        .lock()
        .ok()
        .is_some_and(|mut g| g.remove(&id).is_some())
}

/// Count of live sockets — used by tests to verify cleanup.
#[cfg(test)]
pub fn pool_size() -> usize {
    pool().lock().map(|g| g.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_then_close_round_trip() {
        let id = open("127.0.0.1", 0).expect("bind ephemeral");
        assert!(get(id).is_some(), "socket must be in pool after open");
        assert!(close(id), "close should report removal");
        assert!(get(id).is_none(), "socket must be gone after close");
        assert!(!close(id), "second close is a no-op");
    }

    #[test]
    fn loopback_send_recv() {
        let sender = open("127.0.0.1", 0).expect("bind sender");
        let receiver = open("127.0.0.1", 0).expect("bind receiver");
        let recv_addr = local_addr(receiver).expect("recv addr");
        let port = recv_addr.port();

        let n = send_to(sender, "127.0.0.1", port, b"hello").expect("send");
        assert_eq!(n, 5);

        let (payload, src) =
            recv(receiver, Some(Duration::from_millis(500))).expect("recv must arrive on loopback");
        assert_eq!(&payload, b"hello");
        // src port should be the sender's ephemeral port.
        let sender_local = local_addr(sender).unwrap();
        assert_eq!(src.port(), sender_local.port());

        assert!(close(sender));
        assert!(close(receiver));
    }

    #[test]
    fn recv_timeout_returns_none() {
        let id = open("127.0.0.1", 0).expect("bind");
        let start = std::time::Instant::now();
        let result = recv(id, Some(Duration::from_millis(100)));
        let elapsed = start.elapsed();
        assert!(result.is_none(), "no traffic → recv must return None");
        assert!(
            elapsed >= Duration::from_millis(90),
            "timeout should be respected, slept {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "timeout should not overshoot wildly, slept {:?}",
            elapsed
        );
        close(id);
    }

    #[test]
    fn get_on_unknown_id_returns_none() {
        assert!(get(99_999_999).is_none());
    }
}
