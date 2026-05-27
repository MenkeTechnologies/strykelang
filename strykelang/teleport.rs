//! `teleport($val, @pids)` + `arrive([$timeout_ms])` — multi-target SHM IPC.
//!
//! Moves a serialized value to N stryke receiver processes via POSIX
//! shared memory + per-receiver UNIX-domain-socket notification. The
//! shared-memory pattern beats socket-based bincode IPC for big values
//! (single allocation + N read-only mmaps vs N full copies through
//! kernel socket buffers).
//!
//! ## Wire protocol
//!
//! For each teleport:
//!   1. Sender JSON-serializes the value (one copy: rust heap → `Vec<u8>`).
//!      Bincode can't round-trip a free-standing serde_json::Value
//!      (the Value type needs `deserialize_any`), and bundling into a
//!      bincode frame just for one payload buys nothing — so we go
//!      straight to JSON bytes.
//!   2. Sender creates a POSIX SHM segment named `/stryke_tp_PID_SEQ`,
//!      ftruncates to payload size, mmaps writable, copies payload in
//!      (second copy: `Vec<u8>` → SHM).
//!   3. For each receiver PID, sender connects to the receiver's
//!      well-known UDS at `/tmp/stryke_teleport_PID.sock` and sends a
//!      40-byte notification: `[shm_name (32B, NUL-terminated)][size (8B LE)]`.
//!      Counts successful sendto calls.
//!   4. Sender holds the segment alive for `hold_ms` (default 500ms) to
//!      let receivers `shm_open` + `mmap` + read, then `shm_unlink` +
//!      drops own mmap. Last `munmap` triggers kernel cleanup of the
//!      backing pages.
//!
//! Each receiver runs an `arrive()` loop:
//!   1. Lazy-binds its UDS socket on first `arrive()` call.
//!   2. `recvfrom` with the requested timeout — gets the 40-byte notify.
//!   3. Parses shm_name + size, `shm_open(O_RDONLY)` + `mmap(PROT_READ)`,
//!      bincode-deserializes the value, `munmap`s, returns the value.
//!
//! ## v1 limits (intentional, documented in the LSP hover doc)
//!
//!   * Receivers must be stryke processes running an `arrive()` loop.
//!     Won't work with arbitrary OS processes — sender needs the
//!     receiver's PID + a known UDS path convention.
//!   * macOS POSIX SHM names cap at 30 chars; `/stryke_tp_99999_99` is
//!     19 chars so we have headroom.
//!   * Sender's "wait for receivers to read" is a fixed `hold_ms` window,
//!     not ack-based. Receivers that miss the window get a stale-name
//!     `shm_open` failure. v2 could ack via a reverse-UDS path.
//!   * Crashes: receiver crash mid-read leaks the SHM segment until the
//!     sender's own munmap; sender crash mid-hold leaks until reboot.
//!   * No encryption — receivers are cooperating processes; payload is
//!     visible to anything that knows the SHM name.

use parking_lot::Mutex;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Per-teleport sequence — combined with PID to produce a unique SHM
/// segment name even when one process issues multiple teleports back
/// to back (the same-nanosecond concern that bit `fresh_tx_id`).
static SEQ: AtomicU64 = AtomicU64::new(1);

/// Compose the well-known UDS path for a receiver PID. Matches the
/// `arrive()` side's bind location.
pub fn receiver_socket_path(pid: i32) -> PathBuf {
    PathBuf::from(format!("/tmp/stryke_teleport_{}.sock", pid))
}

/// Build the POSIX SHM segment name for this sender + current seq.
/// Under 30 chars (macOS limit). Leading `/` per POSIX.
fn fresh_shm_name() -> String {
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("/stryke_tp_{}_{}", pid, seq)
}

/// 40-byte fixed notification frame. Layout:
///   bytes 0..32  — shm_name, NUL-padded
///   bytes 32..40 — payload size, u64 little-endian
pub const NOTIFY_FRAME_SIZE: usize = 40;

fn build_notify(shm_name: &str, size: u64) -> [u8; NOTIFY_FRAME_SIZE] {
    let mut buf = [0u8; NOTIFY_FRAME_SIZE];
    let name_bytes = shm_name.as_bytes();
    let n = name_bytes.len().min(31); // leave room for NUL terminator
    buf[..n].copy_from_slice(&name_bytes[..n]);
    buf[32..40].copy_from_slice(&size.to_le_bytes());
    buf
}

fn parse_notify(frame: &[u8]) -> Option<(String, usize)> {
    if frame.len() < NOTIFY_FRAME_SIZE {
        return None;
    }
    // shm_name: bytes up to first NUL in the first 32 bytes.
    let name_end = frame[..32].iter().position(|&b| b == 0).unwrap_or(32);
    let name = std::str::from_utf8(&frame[..name_end]).ok()?.to_string();
    let size = u64::from_le_bytes(frame[32..40].try_into().ok()?) as usize;
    Some((name, size))
}

/// Create + populate the SHM segment. Returns the file descriptor +
/// chosen name. Caller is responsible for `shm_unlink(name)` + `close(fd)`
/// when the receivers have had time to read.
unsafe fn create_shm_with_payload(payload: &[u8]) -> io::Result<(libc::c_int, String)> {
    let name = fresh_shm_name();
    let c_name = std::ffi::CString::new(name.clone())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    // O_CREAT|O_RDWR|O_EXCL — fail if the (extremely unlikely) name collides.
    let flags = libc::O_CREAT | libc::O_RDWR | libc::O_EXCL;
    let mode: libc::mode_t = 0o600;
    let fd = libc::shm_open(c_name.as_ptr(), flags, mode as libc::c_uint);
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    if libc::ftruncate(fd, payload.len() as libc::off_t) == -1 {
        let err = io::Error::last_os_error();
        libc::shm_unlink(c_name.as_ptr());
        libc::close(fd);
        return Err(err);
    }
    let map = libc::mmap(
        ptr::null_mut(),
        payload.len(),
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
    );
    if map == libc::MAP_FAILED {
        let err = io::Error::last_os_error();
        libc::shm_unlink(c_name.as_ptr());
        libc::close(fd);
        return Err(err);
    }
    ptr::copy_nonoverlapping(payload.as_ptr(), map as *mut u8, payload.len());
    libc::munmap(map, payload.len());
    Ok((fd, name))
}

/// Send the value to N receiver PIDs. Returns the count of receivers
/// whose UDS sendto succeeded (the receiver socket existed + accepted
/// the notify). Receivers that aren't listening, that have a stale
/// socket file, or whose stryke process has exited count as unreachable.
///
/// `hold_ms` controls how long the sender holds the SHM segment alive
/// after notifying so receivers can read. Default 500ms covers loopback
/// IPC easily; bump higher for unusually slow receivers.
pub fn send(payload: &[u8], pids: &[i32], hold_ms: u64) -> usize {
    if payload.is_empty() {
        return 0;
    }
    // 1. Create + populate SHM.
    let (fd, name) = match unsafe { create_shm_with_payload(payload) } {
        Ok(p) => p,
        Err(_) => return 0,
    };
    let c_name = match std::ffi::CString::new(name.clone()) {
        Ok(s) => s,
        Err(_) => {
            unsafe {
                libc::close(fd);
            }
            return 0;
        }
    };

    // 2. Notify each receiver. Use one anonymous-bound UDS-DGRAM socket
    //    for all sends (no need for a stable sender address).
    let sock = match UnixDatagram::unbound() {
        Ok(s) => s,
        Err(_) => {
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
                libc::close(fd);
            }
            return 0;
        }
    };
    let _ = sock.set_write_timeout(Some(Duration::from_millis(200)));
    let frame = build_notify(&name, payload.len() as u64);
    let mut delivered = 0usize;
    for pid in pids {
        let path = receiver_socket_path(*pid);
        if sock.send_to(&frame, &path).is_ok() {
            delivered += 1;
        }
    }

    // 3. Hold the segment alive while receivers read.
    if hold_ms > 0 {
        std::thread::sleep(Duration::from_millis(hold_ms));
    }

    // 4. Clean up — unlink + close. Receivers that already mmap'd will
    //    keep working (mmap holds backing pages alive); receivers that
    //    haven't `shm_open`'d yet now miss the window.
    unsafe {
        libc::shm_unlink(c_name.as_ptr());
        libc::close(fd);
    }
    delivered
}

/// Lazy-bound per-process receiver socket, with PID-aware rebind. Stored
/// as `(pid, socket)` so that after a `fork()` the child (which inherits
/// the parent's bound fd) detects the PID mismatch on its first `recv`
/// call and rebinds at its own `/tmp/stryke_teleport_PID.sock` path.
/// Without this, a forked child would silently keep using the parent's
/// socket and senders teleporting to the child's PID would all miss.
fn receiver_socket() -> Arc<UnixDatagram> {
    static SOCK: Mutex<Option<(u32, Arc<UnixDatagram>)>> = Mutex::new(None);
    let pid = std::process::id();
    let mut guard = SOCK.lock();
    if let Some((cached_pid, sock)) = guard.as_ref() {
        if *cached_pid == pid {
            return Arc::clone(sock);
        }
        // PID mismatch — post-fork inheritance from parent. Drop the
        // parent's socket (closes the inherited fd in this child) and
        // fall through to bind a fresh one at the child's own PID path.
    }
    let path = receiver_socket_path(pid as i32);
    // Best-effort cleanup of a stale socket file from a prior crashed
    // instance (or a prior child of this same PID). Ignore failure.
    let _ = std::fs::remove_file(&path);
    let sock = Arc::new(
        UnixDatagram::bind(&path).expect("teleport: bind receiver UDS"),
    );
    *guard = Some((pid, Arc::clone(&sock)));
    sock
}

/// Block up to `timeout` for a teleport notification, then `shm_open` +
/// `mmap` + read the payload. Returns the raw bytes (caller bincode-
/// deserializes into a StrykeValue). `None` on timeout / parse failure /
/// SHM open failure (stale name — sender already unlinked).
pub fn recv(timeout: Duration) -> Option<Vec<u8>> {
    let sock = receiver_socket();
    sock.set_read_timeout(Some(timeout)).ok()?;
    // Borrow sock as &UnixDatagram for the recv calls below — Arc derefs.
    let sock: &UnixDatagram = &sock;
    let mut frame = [0u8; NOTIFY_FRAME_SIZE];
    let (n, _src) = sock.recv_from(&mut frame).ok()?;
    if n < NOTIFY_FRAME_SIZE {
        return None;
    }
    let (shm_name, size) = parse_notify(&frame)?;
    if size == 0 {
        return Some(Vec::new());
    }
    // Open + map the SHM segment read-only.
    let c_name = std::ffi::CString::new(shm_name).ok()?;
    let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    if fd == -1 {
        return None; // sender already unlinked or never created
    }
    let map = unsafe {
        libc::mmap(
            ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if map == libc::MAP_FAILED {
        unsafe {
            libc::close(fd);
        }
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(map as *const u8, size).to_vec() };
    unsafe {
        libc::munmap(map, size);
        libc::close(fd);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Notification frame round-trip — pin the wire format so receivers
    /// keep matching senders across releases.
    #[test]
    fn notify_frame_round_trip() {
        let frame = build_notify("/stryke_tp_12345_7", 9999);
        assert_eq!(frame.len(), NOTIFY_FRAME_SIZE);
        let (name, size) = parse_notify(&frame).expect("parse");
        assert_eq!(name, "/stryke_tp_12345_7");
        assert_eq!(size, 9999);
    }

    /// Send/recv loopback via fork. Parent teleports a 64KB payload to
    /// one child; child reads + writes the SHA digest to a file the
    /// parent then verifies. Real two-process round-trip with the
    /// actual POSIX SHM + UDS plumbing.
    #[test]
    #[cfg_attr(not(target_family = "unix"), ignore)]
    fn fork_loopback_send_recv_round_trip() {
        use nix::sys::wait::waitpid;
        use nix::unistd::{fork, ForkResult};
        use std::time::Instant;

        // Pre-build payload + expected digest in the parent so the child
        // doesn't need to know what was sent — it just SHA's whatever it
        // receives + writes the hex to /tmp.
        let payload: Vec<u8> = (0..65536).map(|i| (i % 251) as u8).collect();
        use sha2::{Digest, Sha256};
        let expected = {
            let mut h = Sha256::new();
            h.update(&payload);
            format!("{:x}", h.finalize())
        };
        let result_path = format!("/tmp/stryke_teleport_test_{}.txt", std::process::id());
        let _ = std::fs::remove_file(&result_path);

        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Child: arrive, hash, write result, exit.
                let bytes = recv(Duration::from_secs(5));
                if let Some(b) = bytes {
                    let mut h = Sha256::new();
                    h.update(&b);
                    let hex = format!("{:x}", h.finalize());
                    let _ = std::fs::write(&result_path, hex);
                } else {
                    let _ = std::fs::write(&result_path, "TIMEOUT");
                }
                std::process::exit(0);
            }
            ForkResult::Parent { child } => {
                // Give the child a moment to bind its UDS.
                std::thread::sleep(Duration::from_millis(200));
                let start = Instant::now();
                let delivered = send(&payload, &[child.as_raw() as i32], 500);
                let elapsed = start.elapsed();
                assert_eq!(delivered, 1, "exactly 1 receiver notified");
                // 64KB through SHM + UDS should be well under 250ms total.
                assert!(
                    elapsed < Duration::from_secs(2),
                    "send + hold completed in unreasonable time: {:?}",
                    elapsed
                );

                let _ = waitpid(child, None);
                let got = std::fs::read_to_string(&result_path).unwrap_or_default();
                let _ = std::fs::remove_file(&result_path);
                assert_eq!(
                    got.trim(),
                    expected,
                    "child must have received exact payload (SHA mismatch)"
                );
            }
        }
    }

    /// Two receivers (children) — fan-out broadcast. Parent teleports
    /// the same payload to both child PIDs; each child writes its hash
    /// to a per-PID file. Parent verifies both got the right bytes.
    #[test]
    #[cfg_attr(not(target_family = "unix"), ignore)]
    fn fork_two_receiver_fanout() {
        use nix::sys::wait::waitpid;
        use nix::unistd::{fork, ForkResult, Pid};

        let payload = b"teleport-fanout-test-payload".to_vec();
        let mut children: Vec<Pid> = Vec::new();
        let mut result_paths: Vec<String> = Vec::new();

        for i in 0..2 {
            let result_path =
                format!("/tmp/stryke_teleport_fanout_{}_{}.txt", std::process::id(), i);
            let _ = std::fs::remove_file(&result_path);
            result_paths.push(result_path.clone());
            match unsafe { fork() }.expect("fork") {
                ForkResult::Child => {
                    let bytes = recv(Duration::from_secs(5));
                    let s = bytes
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_else(|| "TIMEOUT".into());
                    let _ = std::fs::write(&result_path, s);
                    std::process::exit(0);
                }
                ForkResult::Parent { child } => {
                    children.push(child);
                }
            }
        }

        std::thread::sleep(Duration::from_millis(300));
        let pids: Vec<i32> = children.iter().map(|c| c.as_raw() as i32).collect();
        let delivered = send(&payload, &pids, 500);
        assert_eq!(delivered, 2, "both receivers notified");

        for child in children {
            let _ = waitpid(child, None);
        }
        for path in &result_paths {
            let got = std::fs::read_to_string(path).unwrap_or_default();
            let _ = std::fs::remove_file(path);
            assert_eq!(
                got, "teleport-fanout-test-payload",
                "each fan-out receiver must get the exact payload"
            );
        }
    }
}
