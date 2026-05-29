//! Integration pins for the `teleport` / `arrive` builtins. Multi-process
//! by construction (POSIX SHM + UDS notify), so these tests fork via
//! `nix::unistd::fork` so the receiver runs in a real second process.
//!
//! The lower-level wire (SHM segment lifecycle + UDS notify framing) is
//! covered by `strykelang/teleport.rs`'s unit tests; this file pins the
//! script-level surface: the `teleport(...)` builtin's arg-handling
//! shapes (spread PIDs / arrayref / opts hash) and `arrive()`'s
//! return-value contract.
//!
//! `nix::unistd::fork` is `unsafe` (the child only inherits the calling
//! thread, so any allocator/mutex state in non-main threads is gone) —
//! the children here only do tightly-scoped stryke calls and immediately
//! `std::process::exit`, so the unsafety is bounded.

use crate::common::*;

/// `arrive` with a tiny timeout against no sender returns `undef`.
/// Pins the timeout-bound contract — `arrive(50)` must NOT block
/// forever when nothing teleports.
#[test]
fn arrive_returns_undef_when_no_sender_within_timeout() {
    let code = r#"
        my $msg = arrive(50)
        defined $msg ? "got" : "undef"
    "#;
    let s = eval_string(code);
    assert_eq!(s.trim(), "undef", "arrive() must return undef on timeout");
}

/// `teleport` with no receiver PIDs returns 0. Pins the "nothing to do"
/// shortcut — caller passed a payload but no targets, builtin doesn't
/// create the SHM segment and reports 0 delivered.
#[test]
fn teleport_with_zero_receivers_returns_zero() {
    let code = r#"teleport({ a => 1 })"#;
    let n = eval_int(code);
    assert_eq!(n, 0, "teleport with no PIDs → 0 delivered");
}

/// `teleport` to a PID that has no `arrive` loop bound returns 0. The
/// receiver UDS socket doesn't exist, so `send_to` fails and the
/// builtin reports 0 delivered. Pin guarantees we don't false-positive
/// on unreachable receivers.
#[test]
fn teleport_to_unbound_pid_returns_zero() {
    // PID 1 is init on every Unix and never has a stryke UDS socket
    // bound at `/tmp/stryke_teleport_1.sock`. Even if the file somehow
    // existed (it doesn't), init wouldn't be a stryke process running
    // arrive(). Reliable "unreachable" target.
    let code = r#"teleport({ data => "test" }, 1)"#;
    let n = eval_int(code);
    assert_eq!(n, 0, "teleport to unbound PID → 0 delivered");
}

/// Full round-trip: parent forks a child, child arrives, parent
/// teleports a nested hashref + arrayref, child writes the
/// reconstructed value to /tmp, parent verifies field-by-field.
///
/// Pins the deep-refs contract: top-level hashref AND nested arrayref
/// arrive as refs (not flat array/hash) so `$msg->{outer}->[i]` works
/// without re-wrapping.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn fork_teleport_round_trip_via_st_dispatch() {
    use nix::sys::wait::waitpid;
    use nix::unistd::{fork, ForkResult};
    use std::time::Duration;

    let result_path = format!("/tmp/stryke_teleport_pin_{}.txt", std::process::id());
    let _ = std::fs::remove_file(&result_path);
    let result_path_for_child = result_path.clone();

    match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            // Child: arrive(), reconstruct the round-trip string, write
            // to result file, exit. Has to call into the stryke runtime
            // via eval() so dispatch + json_to_deep_refs gets exercised.
            let code = r#"
                my $msg = arrive(5000)
                if (defined $msg) {
                    sprintf("count=%d|tag=%s|items=%s|nested=%d",
                        $msg->{count},
                        $msg->{tag},
                        join(",", @{$msg->{items}}),
                        $msg->{nested}->{depth})
                } else {
                    "TIMEOUT"
                }
            "#;
            let got = eval_string(code);
            let _ = std::fs::write(&result_path_for_child, got);
            // `_exit` over `std::process::exit` in fork children — Rust
            // runtime cleanup is NOT async-signal-safe, and pre-fork
            // global state (rayon, channels) hangs forever in
            // `std::rt::cleanup` on shutdown. See scriptable_controller_pin.
            unsafe { libc::_exit(0) }
        }
        ForkResult::Parent { child } => {
            // Give the child time to parse + execute its eval, reach
            // arrive(), and bind its UDS. fork() from multi-threaded
            // test runtime kills all worker threads in the child;
            // re-initializing the parse pipeline takes noticeably
            // longer than the same code under a fresh `st` invocation.
            std::thread::sleep(Duration::from_millis(4000));
            let kid_pid = child.as_raw() as i64;
            let code = format!(
                r#"
                my $payload = {{
                    count  => 42,
                    tag    => "round-trip",
                    items  => [10, 20, 30],
                    nested => {{ depth => 2 }},
                }}
                teleport($payload, {kid_pid})
                "#
            );
            let delivered = eval_int(&code);
            assert_eq!(
                delivered, 1,
                "exactly 1 receiver notified (child PID {kid_pid})"
            );

            let _ = waitpid(child, None);
            let got = std::fs::read_to_string(&result_path).unwrap_or_default();
            let _ = std::fs::remove_file(&result_path);
            assert_eq!(
                got, "count=42|tag=round-trip|items=10,20,30|nested=2",
                "child must have reconstructed every field via deep refs"
            );
        }
    }
}

/// Fan-out: parent forks 3 children, then teleports the SAME hashref
/// to all three PIDs in a single call. All three must receive the
/// exact same content. Pins the multi-target broadcast contract.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn fork_teleport_fan_out_to_three_children() {
    use nix::sys::wait::waitpid;
    use nix::unistd::{fork, ForkResult, Pid};
    use std::time::Duration;

    let mut children: Vec<Pid> = Vec::new();
    let mut result_paths: Vec<String> = Vec::new();
    for i in 0..3 {
        let path = format!(
            "/tmp/stryke_teleport_pin_fanout_{}_{i}.txt",
            std::process::id()
        );
        let _ = std::fs::remove_file(&path);
        result_paths.push(path.clone());
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                let code = r#"
                    my $msg = arrive(5000)
                    defined $msg ? "$msg->{tag}|$msg->{n}" : "TIMEOUT"
                "#;
                let got = eval_string(code);
                let _ = std::fs::write(&path, got);
                // `_exit` over `std::process::exit` in fork children — Rust
                // runtime cleanup is NOT async-signal-safe, and pre-fork
                // global state (rayon, channels) hangs forever in
                // `std::rt::cleanup` on shutdown. See scriptable_controller_pin.
                unsafe { libc::_exit(0) }
            }
            ForkResult::Parent { child } => {
                children.push(child);
            }
        }
    }

    std::thread::sleep(Duration::from_millis(4000));
    let pid_list = children
        .iter()
        .map(|c| c.as_raw().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        r#"
        my $payload = {{ tag => "fanout", n => 99 }}
        teleport($payload, {pid_list})
        "#
    );
    let delivered = eval_int(&code);
    assert_eq!(
        delivered, 3,
        "all 3 children's UDS sockets must accept the notify"
    );

    for child in children {
        let _ = waitpid(child, None);
    }
    for path in &result_paths {
        let got = std::fs::read_to_string(path).unwrap_or_default();
        let _ = std::fs::remove_file(path);
        assert_eq!(
            got, "fanout|99",
            "every child must reconstruct the exact same payload"
        );
    }
}

/// Arrayref-form PIDs + opts-hash form both flatten through the same
/// path as bare spread PIDs. Pins the surface variants documented in
/// the LSP hover so refactoring the dispatch doesn't silently drop
/// support for one form.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn fork_teleport_arrayref_pids_with_opts_hash() {
    use nix::sys::wait::waitpid;
    use nix::unistd::{fork, ForkResult, Pid};
    use std::time::Duration;

    let mut children: Vec<Pid> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for i in 0..2 {
        let p = format!(
            "/tmp/stryke_teleport_pin_arrayref_{}_{i}.txt",
            std::process::id()
        );
        let _ = std::fs::remove_file(&p);
        paths.push(p.clone());
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                let code = r#"
                    my $msg = arrive(5000)
                    defined $msg ? $msg->{tag} : "TIMEOUT"
                "#;
                let _ = std::fs::write(&p, eval_string(code));
                // `_exit` over `std::process::exit` in fork children — Rust
                // runtime cleanup is NOT async-signal-safe, and pre-fork
                // global state (rayon, channels) hangs forever in
                // `std::rt::cleanup` on shutdown. See scriptable_controller_pin.
                unsafe { libc::_exit(0) }
            }
            ForkResult::Parent { child } => {
                children.push(child);
            }
        }
    }

    std::thread::sleep(Duration::from_millis(4000));
    let pid_arr = children
        .iter()
        .map(|c| c.as_raw().to_string())
        .collect::<Vec<_>>()
        .join(",");
    // Arrayref `[@pids]` + opts hash `{ hold_ms => 800 }`. Both surfaces
    // exercised in one call.
    let code = format!(
        r#"
        my @pids = ({pid_arr})
        teleport({{ tag => "arrayref-opts" }}, [@pids], {{ hold_ms => 800 }})
        "#
    );
    let delivered = eval_int(&code);
    assert_eq!(
        delivered, 2,
        "both children notified via arrayref-form PIDs"
    );

    for c in children {
        let _ = waitpid(c, None);
    }
    for p in &paths {
        let got = std::fs::read_to_string(p).unwrap_or_default();
        let _ = std::fs::remove_file(p);
        assert_eq!(
            got, "arrayref-opts",
            "deep-refs round-trip works in arrayref form"
        );
    }
}
