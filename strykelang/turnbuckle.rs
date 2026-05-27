//! `turnbuckle($peer_pid)` — 1:1 peer-pair keepalive over UNIX-domain datagram.
//!
//! Companion to `teleport` / `arrive`: where teleport is 1:N value broadcast,
//! turnbuckle is a 1:1 *liveness* primitive. Each side calls
//! `turnbuckle($peer_pid)`; a background thread exchanges single-byte
//! heartbeats every `interval_ms` over symmetric UDS sockets at
//! `/tmp/stryke_turnbuckle_<my_pid>_<peer_pid>.sock` (we bind ours; we send
//! to `/tmp/stryke_turnbuckle_<peer_pid>_<my_pid>.sock`). A peer is
//! considered alive when at least one heartbeat has been received within
//! the configured `timeout_ms` window.
//!
//! Why a separate primitive instead of layering on `arrive()`: arrive is
//! request-blocking, single-receiver-bound, and serializes payloads. For
//! pure liveness you want a non-blocking, per-pair socket with no
//! serialization cost. The two systems share no state.
//!
//! ## Wire shape
//!
//!   * Frame: one byte `0x01` (heartbeat).
//!   * Transport: `UnixDatagram`, bound at `my_path`, send-to `peer_path`.
//!   * Cadence: background thread sends every `interval_ms`; same thread
//!     `recv_from`s with a read-timeout equal to `interval_ms`, so one
//!     syscall serves both as a "sleep" and as the drain step.
//!   * Liveness: `alive(id)` returns true iff `now - last_heard_ms <
//!     timeout_ms`. `timeout_ms` should be ≥ 2× `interval_ms` to absorb
//!     one missed heartbeat without flapping.
//!
//! ## Lifecycle
//!
//!   * `open` registers state in a process-global registry, spawns the
//!     heartbeat thread, returns a `u64` id.
//!   * `close` sets a stop flag, joins the thread, unbinds the socket
//!     file, and drops the registry entry.
//!   * `alive` is a non-blocking timestamp comparison.
//!
//! ## Limits
//!
//!   * Local-only — UDS sockets, single host. Cross-host keepalive should
//!     use `turn_client` + an application-layer ping.
//!   * No authentication — any local process that knows the path can
//!     forge heartbeats.
//!   * Each side must bind ITS OWN socket and know the peer's PID; there
//!     is no discovery channel.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Single heartbeat byte. One byte is enough — turnbuckle carries no
/// payload, just "I'm still here." The frame stays under one packet so
/// `recv_from` always returns it in a single read.
const HEARTBEAT: [u8; 1] = [0x01];

/// Per-pair state. Lives in the process-global [`REGISTRY`] keyed by id.
struct State {
    /// Wall-clock millis of the most recent heartbeat received from the
    /// peer. `0` means "never heard from them yet" — `alive()` treats
    /// that as not-alive until the first heartbeat lands.
    last_heard_ms: AtomicU64,
    /// Liveness window. `now - last_heard_ms < timeout_ms` ⇒ alive.
    timeout_ms: u64,
    /// Signal the heartbeat thread to exit on its next iteration.
    stop: AtomicBool,
    /// JoinHandle so `close` can wait for the thread to actually exit
    /// before unbinding the socket file (avoids EBADF races).
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Bound socket path — used by `close` to unlink the file.
    my_path: PathBuf,
}

static REGISTRY: LazyLock<Mutex<HashMap<u64, Arc<State>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Compose this process's bind path for a pair with `peer_pid`.
fn my_path_for(peer_pid: i32) -> PathBuf {
    let me = std::process::id();
    PathBuf::from(format!("/tmp/stryke_turnbuckle_{}_{}.sock", me, peer_pid))
}

/// Compose the peer's bind path (where we send heartbeats to).
fn peer_path_for(peer_pid: i32) -> PathBuf {
    let me = std::process::id();
    PathBuf::from(format!("/tmp/stryke_turnbuckle_{}_{}.sock", peer_pid, me))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Open a pair with `peer_pid`. Binds a fresh UDS, spawns the heartbeat
/// thread, registers state, returns the handle id. Returns `0` on bind
/// failure (typically: stale socket file the caller can't unlink).
pub fn open(peer_pid: i32, interval_ms: u64, timeout_ms: u64) -> u64 {
    if peer_pid <= 0 {
        return 0;
    }
    let interval_ms = interval_ms.max(1);
    let timeout_ms = timeout_ms.max(interval_ms * 2);

    let my_path = my_path_for(peer_pid);
    let peer_path = peer_path_for(peer_pid);

    // Best-effort cleanup of a stale socket file (prior crashed instance
    // or earlier same-process pair to the same peer that wasn't closed
    // cleanly). Ignore failure — bind below will surface a real error
    // if the file truly can't be replaced.
    let _ = std::fs::remove_file(&my_path);

    let sock = match UnixDatagram::bind(&my_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if sock
        .set_read_timeout(Some(Duration::from_millis(interval_ms)))
        .is_err()
    {
        let _ = std::fs::remove_file(&my_path);
        return 0;
    }

    let state = Arc::new(State {
        last_heard_ms: AtomicU64::new(0),
        timeout_ms,
        stop: AtomicBool::new(false),
        thread: Mutex::new(None),
        my_path: my_path.clone(),
    });

    // Background heartbeat loop. One syscall budget per iteration: send,
    // then recv_from with the read timeout serving as the sleep. If a
    // heartbeat lands during the window we update last_heard_ms; if
    // recv times out we just loop and retry the send.
    let state_for_thread = Arc::clone(&state);
    let handle = std::thread::Builder::new()
        .name(format!("turnbuckle_{peer_pid}"))
        .spawn(move || {
            let mut buf = [0u8; 1];
            loop {
                if state_for_thread.stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = sock.send_to(&HEARTBEAT, &peer_path);
                match sock.recv_from(&mut buf) {
                    Ok((n, _)) if n >= 1 && buf[0] == HEARTBEAT[0] => {
                        state_for_thread
                            .last_heard_ms
                            .store(now_ms(), Ordering::Relaxed);
                    }
                    _ => {
                        // Timeout or stray frame — just loop. The next
                        // iteration will re-send and re-poll.
                    }
                }
            }
        })
        .expect("turnbuckle: spawn heartbeat thread");

    *state.thread.lock() = Some(handle);

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    REGISTRY.lock().insert(id, state);
    id
}

/// Non-blocking liveness check. Returns true iff a heartbeat from the
/// peer has arrived within the configured `timeout_ms` window.
pub fn alive(id: u64) -> bool {
    let Some(state) = REGISTRY.lock().get(&id).map(Arc::clone) else {
        return false;
    };
    let last = state.last_heard_ms.load(Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    now_ms().saturating_sub(last) < state.timeout_ms
}

/// Force an immediate heartbeat (in addition to the background cadence).
/// Returns true on syscall success. Useful when the caller wants to
/// pre-warm the peer's `last_heard_ms` ahead of a deadline.
pub fn ping(id: u64) -> bool {
    let Some(state) = REGISTRY.lock().get(&id).map(Arc::clone) else {
        return false;
    };
    // Reuse a fresh unbound socket — we can't borrow the bg thread's
    // socket without coordinating. Cost: one extra syscall. Worth it
    // for a primitive that's expected to be called infrequently.
    let Ok(sock) = UnixDatagram::unbound() else {
        return false;
    };
    let _ = sock.set_write_timeout(Some(Duration::from_millis(50)));
    // Derive peer PID from the bound path: ".../stryke_turnbuckle_ME_PEER.sock"
    let Some(peer_pid) = peer_pid_from_my_path(&state.my_path) else {
        return false;
    };
    sock.send_to(&HEARTBEAT, peer_path_for(peer_pid)).is_ok()
}

fn peer_pid_from_my_path(p: &std::path::Path) -> Option<i32> {
    let stem = p.file_stem()?.to_str()?;
    // Format: "stryke_turnbuckle_<me>_<peer>"
    let mut parts = stem.rsplitn(2, '_');
    let peer = parts.next()?;
    peer.parse().ok()
}

/// Close the pair: signal the thread, join it, drop the registry entry,
/// unbind the socket file. Returns true if the id was known.
pub fn close(id: u64) -> bool {
    let Some(state) = REGISTRY.lock().remove(&id) else {
        return false;
    };
    state.stop.store(true, Ordering::Relaxed);
    if let Some(handle) = state.thread.lock().take() {
        let _ = handle.join();
    }
    let _ = std::fs::remove_file(&state.my_path);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same-process self-loopback: my PID points back at my own bind
    /// path. The heartbeat we send goes to OURSELVES — proving the
    /// send + recv plumbing without needing a real peer process.
    #[test]
    fn self_loopback_marks_alive_after_first_heartbeat() {
        // Hack the symmetry: use our own PID as the peer. Both
        // my_path_for(me) and peer_path_for(me) collapse to the same
        // path (".../stryke_turnbuckle_ME_ME.sock"), so the thread
        // sends to itself. Perfect for unit-testing the wire without
        // forking.
        let me = std::process::id() as i32;
        let id = open(me, 20, 200);
        assert_ne!(id, 0, "open should succeed");

        // Give the bg thread ~3 cycles to send + recv to itself.
        std::thread::sleep(Duration::from_millis(150));
        assert!(alive(id), "self-loopback should mark itself alive");

        assert!(close(id));
        assert!(!alive(id), "alive() must return false after close");
    }

    #[test]
    fn close_returns_false_for_unknown_id() {
        assert!(!close(u64::MAX), "unknown id → false");
    }

    #[test]
    fn alive_returns_false_for_unknown_id() {
        assert!(!alive(u64::MAX), "unknown id → not alive");
    }

    #[test]
    fn open_rejects_nonpositive_pid() {
        assert_eq!(open(0, 50, 200), 0);
        assert_eq!(open(-1, 50, 200), 0);
    }
}
