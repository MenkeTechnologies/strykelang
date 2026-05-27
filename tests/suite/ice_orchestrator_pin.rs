//! Integration pin for the stryke-source ICE-lite orchestrator
//! (`examples/ice_orchestrator.stk`). The orchestrator is pure stryke
//! source defining `ice::connect` over the v1.3 NAT-traversal builtins;
//! this pin verifies it composes correctly end-to-end via the actual
//! stryke binary, not just via lib-level wiring.
//!
//! Two layers of coverage:
//!   1. Subprocess smoke: shell out to `./target/debug/st examples/ice_
//!      orchestrator.stk` and assert exit 0 + expected "connected via
//!      direct" output. Catches regressions where the demo file's
//!      bottom block (loopback peer + ice::connect) breaks for any
//!      reason — bad syntax, missing builtin, wrong API shape, etc.
//!   2. In-process via eval_string: source the orchestrator's function
//!      definitions, call ice::connect against a self-bound loopback
//!      peer, assert on the returned hashref's `method` / `socket`
//!      fields. Catches regressions in the ladder's priority logic
//!      (direct rung must short-circuit; punch/relay rungs must NOT
//!      execute when direct succeeds).

use crate::common::*;
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::thread;

fn stryke_binary() -> Option<PathBuf> {
    let cands = ["target/debug/st", "target/release/st"];
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for cand in cands {
        let p = PathBuf::from(cand);
        if let Ok(meta) = std::fs::metadata(&p) {
            if let Ok(m) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| m > *t) {
                    best = Some((p, m));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

/// Run the orchestrator demo file as a subprocess and verify it succeeds
/// + emits the expected "connected via direct" log line. The demo's
/// bottom `__demo__` block spawns a background loopback peer + calls
/// `ice::connect` against it, so the direct rung must short-circuit.
#[test]
fn ice_orchestrator_demo_runs_end_to_end_via_subprocess() {
    let Some(bin) = stryke_binary() else {
        eprintln!("no built stryke binary; skipping");
        return;
    };
    let output = Command::new(&bin)
        .arg("examples/ice_orchestrator.stk")
        .output()
        .unwrap_or_else(|e| panic!("invoke {}: {e}", bin.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "orchestrator demo exit status: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains("connected via direct"),
        "expected 'connected via direct' in orchestrator output:\n{stdout}"
    );
    assert!(
        stdout.contains("round-trip via direct"),
        "expected followup round-trip line:\n{stdout}"
    );
}

/// In-process: source `ice::connect` from the orchestrator file, call it
/// against a known-listening loopback peer, assert the return hashref
/// reports `method == "direct"`. Verifies the priority-ladder logic at
/// the language level — no subprocess, just stryke source.
///
/// Loopback peer: a Rust thread holding a UdpSocket that echoes one
/// datagram. ice::connect calls udp_send_to + udp_recv_from on its own
/// socket; the echo round-trips, direct rung wins, no fallback path is
/// touched.
#[test]
fn ice_connect_direct_rung_short_circuits_with_live_peer() {
    // Bind a UDP socket as the peer; thread echoes one datagram.
    let peer = UdpSocket::bind("127.0.0.1:0").expect("bind peer");
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok();
    let peer_addr = peer.local_addr().unwrap();
    thread::spawn(move || {
        let mut buf = [0u8; 1500];
        if let Ok((n, src)) = peer.recv_from(&mut buf) {
            let _ = peer.send_to(&buf[..n], src);
        }
    });

    // Source the orchestrator's ice::connect (and helpers) inline by
    // reading the file and prepending it to our test snippet.
    let orch = std::fs::read_to_string("examples/ice_orchestrator.stk")
        .expect("read ice_orchestrator.stk");
    // The file's bottom block spawns its own peer + prints stuff that'd
    // race our assertion. Strip from `## Demo` onward — keep only the
    // package + fn definitions.
    let cutoff = orch
        .find("# ── Demo")
        .or_else(|| orch.find("package main"))
        .unwrap_or(orch.len());
    let prelude = &orch[..cutoff];

    let code = format!(
        r#"{prelude}

# Call ice::connect against the live Rust peer; direct rung should win.
my $conn = ice::connect({{
    peer_host_addr => "127.0.0.1:{port}",
    timeout_ms     => 1000,
}})
udp_close($conn->{{socket}}) if $conn->{{ok}}
sprintf("ok=%d|method=%s", $conn->{{ok}}, $conn->{{method}} // "none")
"#,
        prelude = prelude,
        port = peer_addr.port()
    );
    let s = eval_string(&code);
    assert_eq!(s.trim(), "ok=1|method=direct");
}

/// `ice::connect` with NO reachable rungs returns `{ok=0, reason=...}`
/// — the explicit-failure contract. We pass only a closed peer address;
/// punch and relay options aren't supplied, so the orchestrator should
/// bail at the direct-rung failure.
#[test]
fn ice_connect_returns_failure_hash_when_no_rung_works() {
    // Grab + release a port → guaranteed closed.
    let probe = TcpListener::bind("127.0.0.1:0").expect("probe");
    let dead = probe.local_addr().unwrap().port();
    drop(probe);

    let orch = std::fs::read_to_string("examples/ice_orchestrator.stk")
        .expect("read");
    let cutoff = orch
        .find("# ── Demo")
        .or_else(|| orch.find("package main"))
        .unwrap_or(orch.len());
    let prelude = &orch[..cutoff];

    let code = format!(
        r#"{prelude}

my $conn = ice::connect({{
    peer_host_addr => "127.0.0.1:{port}",
    timeout_ms     => 200,
}})
sprintf("ok=%d|has_reason=%d", $conn->{{ok}}, defined $conn->{{reason}} ? 1 : 0)
"#,
        prelude = prelude,
        port = dead
    );
    let s = eval_string(&code);
    assert_eq!(s.trim(), "ok=0|has_reason=1");
}
