//! Integration pins for the `turnbuckle` peer-pair keepalive primitive.
//! Cross-process by construction (each side binds its own UDS socket), so
//! the liveness test forks via `nix::unistd::fork` to get a real second
//! process. The lower-level wire (socket bind, heartbeat thread,
//! `last_heard` timestamp comparison) is covered by unit tests inside
//! `strykelang/turnbuckle.rs`; this file pins the script-level surface —
//! handle shape, drop detection across processes, and the `tb_close`
//! cleanup contract.
//!
//! `nix::unistd::fork` is unsafe (child only inherits the calling thread),
//! so each child does a single tightly-scoped stryke `eval` then
//! `std::process::exit`.
//!
//! Each non-fork test uses a UNIQUE synthetic peer PID (in the 99900+
//! range, well above any real PID the test runner could collide with) so
//! parallel cargo-test execution doesn't race on the same UDS bind path.
//! Self-loopback (peer == own PID) cannot be used in parallel tests
//! because the two sides resolve to the same `/tmp/stryke_turnbuckle_X_X.sock`
//! file — only the first binder wins.

use crate::common::*;

/// Handle shape: `turnbuckle($pid)` returns a hashref with `_tb_id` and
/// `peer_pid` keys. Pins the handle schema so callers that introspect
/// the hash don't break across refactors.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn turnbuckle_returns_handle_with_id_and_peer() {
    let code = r#"
        my $peer = 99901
        my $tb = turnbuckle($peer, { interval_ms => 50, timeout_ms => 200 })
        my $r  = defined($tb)
              && $tb->{_tb_id} > 0
              && $tb->{peer_pid} == $peer
        tb_close($tb)
        $r ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1, "handle must expose _tb_id + peer_pid");
}

/// Nonpositive PIDs are rejected at the builtin layer. Pins the
/// defensive guard — a typo'd `turnbuckle(0)` or `turnbuckle(-1)` must
/// NOT bind a socket.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn turnbuckle_rejects_nonpositive_pid() {
    assert_eq!(
        eval_string(r#"defined(turnbuckle(0))   ? "yes" : "no""#),
        "no",
        "turnbuckle(0) must return undef",
    );
    assert_eq!(
        eval_string(r#"defined(turnbuckle(-1))  ? "yes" : "no""#),
        "no",
        "turnbuckle(-1) must return undef",
    );
}

/// `tb_alive` / `tb_ping` / `tb_close` against a closed handle all
/// return 0. Pins the "no segfault on stale handle" contract — every
/// op on a stale id is inert, and double-close is idempotent.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn closed_handle_is_inert() {
    let code = r#"
        my $tb = turnbuckle(99902)
        tb_close($tb)
        my $r = tb_alive($tb) == 0
             && tb_ping($tb)  == 0
             && tb_close($tb) == 0
        $r ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

/// `tb_alive` returns 0 when the peer has never heartbeated. Pins the
/// "no peer = not alive" contract — synthetic peer PID 99903 has no
/// real process behind it, so no heartbeats ever arrive.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn alive_is_zero_when_peer_never_heartbeated() {
    let code = r#"
        my $tb = turnbuckle(99903, { interval_ms => 20, timeout_ms => 100 })
        sleep(0.15)
        my $a = tb_alive($tb)
        tb_close($tb)
        $a
    "#;
    assert_eq!(eval_int(code), 0, "no peer process → not alive");
}

/// `tb_ping` to an unreachable peer returns 0. Pins the negative ping
/// contract — sendto failure (peer socket file missing) is observable.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn tb_ping_returns_zero_for_unreachable_peer() {
    let code = r#"
        my $tb = turnbuckle(99904)
        my $r  = tb_ping($tb)
        tb_close($tb)
        $r
    "#;
    assert_eq!(eval_int(code), 0, "ping to unbound peer path → 0");
}

/// Full cross-process pair: parent forks a child, both open a
/// turnbuckle against the other. After heartbeats have had time to
/// exchange, both sides see each other as alive. Then the child exits
/// — heartbeats stop — and the parent's `tb_alive` flips to 0 within
/// the timeout window.
///
/// Pins the only behavior turnbuckle exists to provide: detect peer
/// liveness, and detect peer drop. This is the ONLY test where the
/// real wire actually carries data — the others above use synthetic
/// unreachable peers to validate the API surface in isolation.
#[test]
#[cfg_attr(not(target_family = "unix"), ignore)]
fn fork_pair_detects_alive_then_drop() {
    use nix::sys::wait::waitpid;
    use nix::unistd::{fork, ForkResult};
    use std::time::Duration;

    let result_path = format!("/tmp/stryke_turnbuckle_pin_{}.txt", std::process::id());
    let _ = std::fs::remove_file(&result_path);
    let result_path_for_child = result_path.clone();
    let parent_pid = std::process::id() as i64;

    match unsafe { fork() }.expect("fork") {
        ForkResult::Child => {
            // Child opens against parent immediately. Sleeps long enough
            // for parent to also open + send a few heartbeats, then
            // checks alive. Keeps heartbeating a bit longer so parent's
            // initial alive check has data to see, then closes.
            //
            // Timeline (child wall-clock from fork):
            //   t=~0:     child binds + bg thread starts
            //   t=400ms:  parent opens (after its 400ms wait); first
            //             heartbeats land in child's socket within ~30ms
            //   t=600ms:  child checks alive — sees ~5 heartbeats
            //   t=900ms:  child closes (parent has gotten ~450ms of
            //             child's heartbeats by now)
            let code = format!(
                r#"
                my $tb = turnbuckle({parent_pid}, {{ interval_ms => 30, timeout_ms => 150 }})
                sleep(0.6)
                my $saw_alive = tb_alive($tb)
                sleep(0.3)
                tb_close($tb)
                sprintf("child_saw_parent_alive=%d", $saw_alive)
                "#,
            );
            let got = eval_string(&code);
            let _ = std::fs::write(&result_path_for_child, got);
            std::process::exit(0);
        }
        ForkResult::Parent { child } => {
            // Give the child time to fork, parse stryke, bind UDS, and
            // start its heartbeat thread before we open our own side.
            std::thread::sleep(Duration::from_millis(400));
            let kid_pid = child.as_raw() as i64;

            // Parent timeline (wall-clock from fork):
            //   t=400ms:  open (child has been heartbeating ~400ms)
            //   t=800ms:  alive check — child's heartbeats have been
            //             landing for ~400ms, last_heard is ~30ms ago
            //   t=1800ms: alive check after drop — child closed at
            //             ~900ms wall, so last_heard is ~900ms ago,
            //             well past 150ms timeout
            let code = format!(
                r#"
                my $tb = turnbuckle({kid_pid}, {{ interval_ms => 30, timeout_ms => 150 }})
                sleep(0.4)
                my $alive_initial = tb_alive($tb)
                sleep(1.0)
                my $alive_after_drop = tb_alive($tb)
                tb_close($tb)
                sprintf("%d,%d", $alive_initial, $alive_after_drop)
                "#,
            );
            let got = eval_string(&code);
            let _ = waitpid(child, None);

            assert_eq!(
                got, "1,0",
                "parent must see child alive while heartbeating, then dead after child closes (got {got:?})"
            );

            let child_result = std::fs::read_to_string(&result_path).unwrap_or_default();
            let _ = std::fs::remove_file(&result_path);
            assert_eq!(
                child_result, "child_saw_parent_alive=1",
                "child must have seen parent's heartbeats (got {child_result:?})"
            );
        }
    }
}
