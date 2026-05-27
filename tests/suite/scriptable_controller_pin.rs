//! Integration pins for the scriptable distributed-compute API:
//! `Controller::spawn` + `ControllerHandle::{muster, welcome, scatter,
//! gather, shutdown}` + the `controller::register_*` global handle
//! registries. Together these are the Rust-side backing for the
//! `congregation` / `pray` / `annex` builtins in `builtins.rs`.
//!
//! Strategy: every test that crosses a process boundary uses `fork(2)` so
//! the child can run a real `run_agent_with_explicit` against the parent's
//! controller. Same pattern as `teleport.rs::fork_loopback_send_recv_round_trip`
//! (teleport.rs:283-339) — proven to work in CI on Linux and macOS.
//!
//! All children explicitly `waitpid` at the end of each test so cargo
//! test doesn't accumulate zombies across the suite.
//!
//! Cleanup pattern: parent sends a final `EVAL` of `exit 0` to every
//! agent before waitpid. The agent runs that in its persistent VM,
//! `std::process::exit(0)` fires, child process dies cleanly, parent
//! reaps via waitpid.

#![cfg(unix)]

use std::time::{Duration, Instant};
use stryke::agent::run_agent_with_explicit;
use stryke::controller::{
    cathedral_lookup, cathedral_register, cathedral_unregister, get_controller,
    get_current_controller, register_controller, register_divination, spawn_controller,
    unregister_divination,
};

/// Spawn a controller on a free loopback port, fork `n` agent children
/// pointed at it, wait until all `n` have registered. Returns the
/// (controller_handle, child_pids). Caller must waitpid each child PID
/// before the test ends.
fn forge_congregation(n: usize) -> (std::sync::Arc<stryke::controller::ControllerHandle>, Vec<nix::unistd::Pid>) {
    use nix::unistd::{fork, ForkResult};

    let handle = spawn_controller("127.0.0.1", 0).expect("spawn_controller");
    let listen_addr = handle.listen_addr();
    let host = listen_addr.ip().to_string();
    let port = listen_addr.port();

    let mut children = Vec::with_capacity(n);
    for i in 0..n {
        match unsafe { fork() }.expect("fork") {
            ForkResult::Parent { child } => {
                children.push(child);
            }
            ForkResult::Child => {
                let name = format!("test-agent-{:02}", i);
                let code = run_agent_with_explicit(&host, port, Some(&name));
                std::process::exit(code);
            }
        }
    }

    assert!(
        handle.welcome(n, Duration::from_secs(10)),
        "only {}/{} agents registered within 10s",
        handle.agent_count(),
        n
    );

    (handle, children)
}

/// Tell every agent to `exit 0`, then waitpid each child so the test
/// process leaves no zombies behind. The "exit 0" path matches the
/// production cleanup story for the congregation builtins (the
/// future-tier `excommunicate` verb formalizes this same flow).
fn dismiss(
    handle: &std::sync::Arc<stryke::controller::ControllerHandle>,
    children: Vec<nix::unistd::Pid>,
) {
    use nix::sys::wait::waitpid;

    let ids = handle.muster();
    // Fire-and-forget exit; we don't gather because the agent dies
    // before sending its EVAL_RESULT and we don't need the answer.
    let _ = handle.scatter("exit(0)", &ids);

    for pid in children {
        // Bounded wait — if the agent ignored the exit, hard-kill so
        // cargo test doesn't hang the whole suite on one stuck child.
        let start = Instant::now();
        let mut reaped = false;
        while start.elapsed() < Duration::from_secs(5) {
            match waitpid(pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(_) => {
                    reaped = true;
                    break;
                }
                Err(_) => {
                    reaped = true;
                    break;
                }
            }
        }
        if !reaped {
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
            let _ = waitpid(pid, None);
        }
    }
}

/// Smoke pin — single fork-spawned agent, parent scatters one EVAL,
/// gathers one EVAL_RESULT, asserts the output matches the expected
/// stryke evaluation of the expression. Locks the whole round-trip
/// shape (spawn → fork → connect → handshake → EVAL → reply → gather).
#[test]
fn controller_handle_round_trips_one_eval_across_real_fork() {
    let (handle, children) = forge_congregation(1);

    let session_ids = handle.muster();
    assert_eq!(session_ids.len(), 1, "muster returns the one registered agent");

    let petition_id = handle
        .scatter("2 + 3", &session_ids)
        .expect("scatter EVAL");
    let results = handle
        .gather(petition_id, Duration::from_secs(10))
        .expect("gather EVAL_RESULT");

    assert_eq!(results.len(), 1, "exactly one reply for one agent");
    let r = &results[&session_ids[0]];
    assert!(r.ok, "agent must report ok=true");
    assert_eq!(
        r.output.trim(),
        "5",
        "agent must evaluate `2 + 3` to `5`, got: {:?}",
        r.output
    );

    dismiss(&handle, children);
}

/// Three-agent scatter-gather — every agent gets the same prayer,
/// every reply makes it back, and the result hash is keyed by the
/// session-ids reported by `muster`. Pins the parallel fan-out path
/// (`scatter` writes to all three in succession; `gather` reads each
/// reply with per-agent timeout) end-to-end across real forks.
#[test]
fn controller_handle_fans_out_to_three_agents_and_demuxes_replies() {
    let (handle, children) = forge_congregation(3);

    let session_ids = handle.muster();
    assert_eq!(session_ids.len(), 3, "three agents registered");

    let petition_id = handle
        .scatter("7 * 6", &session_ids)
        .expect("scatter EVAL to 3 agents");
    let results = handle
        .gather(petition_id, Duration::from_secs(10))
        .expect("gather 3 EVAL_RESULTs");

    assert_eq!(results.len(), 3, "all three agents must reply");
    for sid in &session_ids {
        let r = results
            .get(sid)
            .unwrap_or_else(|| panic!("agent {} missing from results", sid));
        assert!(r.ok, "agent {} reported error: {:?}", sid, r.output);
        assert_eq!(
            r.output.trim(),
            "42",
            "agent {} computed wrong value: {:?}",
            sid,
            r.output
        );
    }

    dismiss(&handle, children);
}

/// Gather on an unknown petition_id is a clean error, not a panic or
/// silent empty hash. Pins the divination-not-found path.
#[test]
fn gather_on_unknown_petition_id_returns_not_found_error() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn_controller");
    let result = handle.gather(99999, Duration::from_millis(100));
    let err = result.expect_err("gather of unknown petition must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    handle.shutdown();
    // Note: no children to dismiss; no agents were spawned.
}

/// Same divination can't be gathered twice — second call returns
/// NotFound because the first removes it from the pending table.
/// Pins the consume-on-gather semantics that prevents double-counting
/// when the same divination_id is accidentally annexed twice.
#[test]
fn divination_consumed_on_gather_so_second_gather_errors() {
    let (handle, children) = forge_congregation(1);
    let session_ids = handle.muster();

    let pid = handle.scatter("1 + 1", &session_ids).expect("scatter");
    let first = handle.gather(pid, Duration::from_secs(5)).expect("first gather");
    assert_eq!(first.len(), 1);

    let second = handle.gather(pid, Duration::from_millis(100));
    assert!(
        second.is_err(),
        "second gather on the same petition must error, got: {:?}",
        second
    );

    dismiss(&handle, children);
}

/// Global registries are real — register_controller → get_controller
/// round-trips an Arc that resolves to the same underlying handle.
/// Pins the script ↔ Rust bridge that builtins.rs::builtin_pray uses
/// to find a live controller by its integer id.
#[test]
fn controller_registry_round_trips_handle_via_integer_id() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn_controller");
    let listen_addr = handle.listen_addr();
    let id = register_controller(std::sync::Arc::clone(&handle));
    assert!(id >= 1, "ids start at 1");

    let resurrected = get_controller(id).expect("registry hit");
    assert_eq!(
        resurrected.listen_addr(),
        listen_addr,
        "registry returns the same handle (same bound port)"
    );

    // Sanity: current controller defaults to None until set.
    // (Note: GLOBAL state — other tests in the suite may have set it.
    // We don't assert None here; only that set/get is consistent.)
    stryke::controller::set_current_controller(id);
    assert_eq!(get_current_controller(), Some(id));

    handle.shutdown();
}

/// Divination registry round-trips the (controller_id, petition_id)
/// pair via an opaque divination_id. unregister consumes it.
#[test]
fn divination_registry_round_trips_pair_via_integer_id() {
    let div_id = register_divination(7, 42);
    let pair = stryke::controller::get_divination(div_id).expect("present");
    assert_eq!(pair, (7, 42));

    let consumed = unregister_divination(div_id).expect("first removal");
    assert_eq!(consumed, (7, 42));

    let after = stryke::controller::get_divination(div_id);
    assert!(after.is_none(), "second lookup after unregister returns None");
}

// ─── Tier 1-3 pins ──────────────────────────────────────────────────────────

/// `excommunicate` sends SHUTDOWN to the named agents and drops them
/// from the roster. Subsequent `muster` returns the remaining agents.
/// Pins the agent-removal path that's separate from full controller
/// shutdown.
#[test]
fn excommunicate_removes_targeted_agents_from_roster() {
    let (handle, children) = forge_congregation(3);
    let session_ids = handle.muster();
    assert_eq!(session_ids.len(), 3);

    // Excommunicate the first two; third should remain.
    let count = handle.excommunicate(&session_ids[..2]);
    assert_eq!(count, 2, "two agents notified");

    let remaining = handle.muster();
    assert_eq!(remaining.len(), 1, "one agent remains after excommunication");
    assert_eq!(
        remaining[0], session_ids[2],
        "the un-excommunicated agent stays"
    );

    // Reap the excommunicated children (they exited on SHUTDOWN) plus
    // the survivor via dismiss().
    use nix::sys::wait::waitpid;
    for pid in &children[..2] {
        let _ = waitpid(*pid, None);
    }
    dismiss(&handle, vec![children[2]]);
}

/// `pilgrimage` succeeds when every dispatched agent replies in time.
/// Pins the barrier-success path — all-agents-ready synchronization.
#[test]
fn pilgrimage_returns_true_when_all_agents_rendezvous() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    let ok = handle.pilgrimage("'arrived'", &session_ids, Duration::from_secs(5));
    assert!(ok, "all agents must rendezvous within the timeout");

    dismiss(&handle, children);
}

/// Parallel scatter — three agents, large enough payload that serial
/// fanout would show. The test asserts correctness (all replied with
/// expected value), not raw timing (which is flaky in CI). The Rayon
/// par_iter implementation in scatter() is the code under test.
#[test]
fn parallel_scatter_dispatches_to_all_agents_concurrently() {
    let (handle, children) = forge_congregation(3);
    let session_ids = handle.muster();

    // Push 1KB of arithmetic to make the EVAL non-trivial.
    let big_code = "my $x = 0; for (1:100) { $x += $_ }; $x";
    let petition_id = handle.scatter(big_code, &session_ids).expect("scatter");
    let results = handle.gather(petition_id, Duration::from_secs(10)).expect("gather");

    assert_eq!(results.len(), 3, "all three agents replied");
    for sid in &session_ids {
        let r = &results[sid];
        assert_eq!(r.output.trim(), "5050", "sum 1..100 = 5050");
    }

    dismiss(&handle, children);
}

/// Soul harvest round-trip: master tells workers to populate %soul, then
/// licks each %soul back via JSON. Pins the lick wire path used by the
/// Tier 3 `lick` / `peruse` builtins — every step is an EVAL through the
/// existing wire protocol, no special soul-harvest frame.
#[test]
fn lick_via_to_json_round_trips_worker_soul_state() {
    use std::time::Duration;

    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    // Workaround for stryke `\%hash` bug (returns empty ref): pass
    // %soul flat to to_json — flattens to a list, JSON-encodes as an
    // array of [k1, v1, k2, v2]. Master pairs them back into a hash.
    // The Tier 3 lick builtin uses the same workaround.
    let lick_code = "our %soul = (k1 => 'v1', k2 => 'v2'); to_json(%soul)";
    let pid2 = handle.scatter(lick_code, &session_ids).expect("lick");
    let lick_results = handle.gather(pid2, Duration::from_secs(5)).expect("lick gather");

    for sid in &session_ids {
        let json_str = lick_results[sid].output.trim();
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .unwrap_or_else(|e| panic!("agent {} json parse failed on {:?}: {}", sid, json_str, e));
        let arr = parsed.as_array().expect("JSON array (hash flattened)");
        // Expect [k1, v1, k2, v2] in some order (hash key order isn't
        // guaranteed). Build a map from the flat list to verify presence.
        let mut got = std::collections::HashMap::new();
        let mut iter = arr.iter();
        while let (Some(k), Some(v)) = (iter.next(), iter.next()) {
            got.insert(k.as_str().unwrap_or("").to_string(), v.as_str().unwrap_or("").to_string());
        }
        assert_eq!(
            got.get("k1").map(String::as_str),
            Some("v1"),
            "k1 round-tripped on agent {}: {}",
            sid,
            json_str
        );
        assert_eq!(
            got.get("k2").map(String::as_str),
            Some("v2"),
            "k2 round-tripped on agent {}: {}",
            sid,
            json_str
        );
    }

    // Lick is non-destructive — re-run the SAME EVAL, get the same
    // contents (each EVAL re-runs the `our %soul = (...)` initialization
    // but the user-visible guarantee is stable output across calls).
    let pid3 = handle.scatter(lick_code, &session_ids).expect("re-lick");
    let again = handle.gather(pid3, Duration::from_secs(5)).expect("re-lick gather");
    for sid in &session_ids {
        let json_str = again[sid].output.trim();
        assert!(
            json_str.contains("k1") && json_str.contains("v1"),
            "lick must produce stable output; got {:?}",
            json_str
        );
    }

    dismiss(&handle, children);
}

// ─── Tier 4 pins ────────────────────────────────────────────────────────────

/// Chant rescatter: master starts a chant against an empty congregation,
/// then forks a new agent. The accept_loop should fire the active chant
/// at the new joiner so it ends up with the same state as anyone who
/// was there at chant time.
#[test]
fn chant_fires_at_new_joiners_after_chant_started() {
    use nix::unistd::{fork, ForkResult};

    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    let listen_addr = handle.listen_addr();
    let host = listen_addr.ip().to_string();
    let port = listen_addr.port();

    // Start an active chant BEFORE any agents have joined. With no agents,
    // the chant scatters to zero recipients but the chant state stays
    // active in the controller's chants table.
    //
    // Use %main::soul (package-qualified) instead of `our %soul = ...`
    // because stryke's `our` is scoped per-EVAL in the agent's persistent
    // VM (separate stryke bug; verified 2026-05-27). Package-qualified
    // assignment persists across EVAL boundaries reliably.
    let chant_id = handle
        .chant("%main::soul = (chanted => 'yes'); 'ok'", &[])
        .expect("chant register");

    // Fork an agent. accept_loop will fire active chants at it on join.
    let child = match unsafe { fork() }.expect("fork") {
        ForkResult::Parent { child } => child,
        ForkResult::Child => {
            let code = stryke::agent::run_agent_with_explicit(&host, port, Some("late-joiner"));
            std::process::exit(code);
        }
    };

    assert!(
        handle.welcome(1, Duration::from_secs(5)),
        "late joiner must register"
    );

    // Give the accept_loop a moment to fire the chant — fire happens in
    // the accept thread after agent insertion.
    std::thread::sleep(Duration::from_millis(200));

    // Verify the chant landed on the new agent by reading %soul.
    //
    // Wire-protocol caveat: EVAL_RESULT frames don't carry petition_ids
    // today (Tier 5 work). The chant's reply ("ok") sits in the agent's
    // outbound socket buffer ahead of any subsequent gather. First do a
    // discard scatter to drain the chant's queued reply, then a real
    // scatter whose gather sees the discard's reply (== drained chant
    // reply), then a third scatter+gather whose reply is the actual
    // readback. The two-step drain pattern is what scripts have to do
    // until petition_id demux lands.
    let session_ids = handle.muster();
    // Drain the chant's queued EVAL_RESULT first (chant fired on accept,
    // its "ok" reply sits in the agent's outbound buffer).
    let drain_pid = handle
        .scatter("'drain'", &session_ids)
        .expect("drain scatter");
    let _ = handle
        .gather(drain_pid, Duration::from_secs(5))
        .expect("drain absorbs chant reply");

    // Now do the real readback. After drain, the next gather reads the
    // drain's "drain" reply, then the readback's reply is queued. Need
    // one more round trip to flush.
    let pid = handle
        .scatter("to_json(%main::soul)", &session_ids)
        .expect("readback scatter");
    let _ = handle.gather(pid, Duration::from_secs(5)).expect("gather");

    let pid2 = handle
        .scatter("to_json(%main::soul)", &session_ids)
        .expect("readback scatter 2");
    let results2 = handle.gather(pid2, Duration::from_secs(5)).expect("gather 2");
    let json_str = results2[&session_ids[0]].output.trim();
    assert!(
        json_str.contains("chanted") && json_str.contains("yes"),
        "late joiner must have received the active chant; got {:?}",
        json_str
    );

    // amen_chant stops the rescatter; future joiners won't get it.
    assert!(handle.amen_chant(chant_id), "amen_chant removes from active");
    assert!(!handle.amen_chant(chant_id), "second amen returns false");

    dismiss(&handle, vec![child]);
}

/// Cloistered controller rejects agents that don't send AGENT_AUTH with
/// a valid token. Pins the ACL — open mode (default) accepts; cloistered
/// requires AUTH; bad token = rejection ACK.
#[test]
fn cloistered_controller_rejects_agents_without_valid_auth_token() {
    use nix::sys::wait::waitpid;
    use nix::unistd::{fork, ForkResult};

    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    handle.set_cloistered(Some("secret-token"));
    let listen_addr = handle.listen_addr();
    let host = listen_addr.ip().to_string();
    let port = listen_addr.port();

    // Agent WITHOUT token — should be rejected by accept_loop.
    let reject_child = match unsafe { fork() }.expect("fork") {
        ForkResult::Parent { child } => child,
        ForkResult::Child => {
            // No STRYKE_AGENT_TOKEN env — agent skips the AUTH frame.
            std::env::remove_var("STRYKE_AGENT_TOKEN");
            let code = stryke::agent::run_agent_with_explicit(&host, port, Some("no-token"));
            // Controller rejection causes run_agent_with_explicit to
            // return non-zero. We exit with that to signal the test.
            std::process::exit(code);
        }
    };

    // Give the rejection a moment to land.
    std::thread::sleep(Duration::from_millis(800));

    let status = waitpid(reject_child, None).expect("waitpid reject child");
    use nix::sys::wait::WaitStatus;
    if let WaitStatus::Exited(_, code) = status {
        assert_ne!(code, 0, "no-token agent must exit non-zero (was rejected)");
    } else {
        panic!("unexpected exit status: {:?}", status);
    }
    assert_eq!(
        handle.agent_count(),
        0,
        "no agents in roster after rejection"
    );

    // Agent WITH correct token — should be accepted.
    let accept_child = match unsafe { fork() }.expect("fork") {
        ForkResult::Parent { child } => child,
        ForkResult::Child => {
            std::env::set_var("STRYKE_AGENT_TOKEN", "secret-token");
            let code = stryke::agent::run_agent_with_explicit(&host, port, Some("with-token"));
            std::process::exit(code);
        }
    };

    assert!(
        handle.welcome(1, Duration::from_secs(5)),
        "authenticated agent must register"
    );

    dismiss(&handle, vec![accept_child]);
}

/// `interrogate($pid)` dumps OS-level process state. Asserts self-PID
/// interrogation produces a hash with at least the core metadata fields
/// populated. Pins the polymorphic dispatch (single scalar = PID path)
/// and the sysinfo-backed return shape.
#[test]
fn interrogate_self_pid_returns_process_state_hash() {
    use crate::common::*;
    let pid = std::process::id();
    let code = format!(
        r#"
        my $h = interrogate({});
        defined($h) ? "pid=" . $h->{{pid}} . "|name=" . $h->{{name}} . "|has_exe=" . (defined $h->{{exe}} ? 1 : 0) : "undef"
        "#,
        pid
    );
    let out = eval_string(&code);
    let trimmed = out.trim();
    assert!(
        trimmed.contains(&format!("pid={}", pid)),
        "interrogate(self) must return our own pid: {:?}",
        trimmed
    );
    assert!(
        trimmed.contains("|name="),
        "interrogate must return process name: {:?}",
        trimmed
    );
    assert!(
        trimmed.contains("|has_exe=1"),
        "interrogate must return exe path: {:?}",
        trimmed
    );
}

/// `interrogate($bogus_pid)` returns undef cleanly — no panic, no error.
/// Pins the not-found path so callers can `unless defined` to detect
/// dead processes.
#[test]
fn interrogate_nonexistent_pid_returns_undef() {
    use crate::common::*;
    // 99999999 is larger than kern.maxproc on any reasonable system,
    // so guaranteed-absent. (kern.maxproc is 16k on this Darwin box;
    // pid_max defaults to 32k or 4M on Linux.)
    let out = eval_string("defined(interrogate(99999999)) ? 'defined' : 'undef'");
    assert_eq!(out.trim(), "undef");
}

/// Cathedral registry: register, lookup, unregister, names — the in-process
/// name → endpoint binding that `profess` resolves against.
#[test]
fn cathedral_register_lookup_unregister_round_trip() {
    let prior = cathedral_register("test-cong", "127.0.0.1:12345");
    // (prior may be Some if other tests left a binding — accept either)
    let _ = prior;

    let got = cathedral_lookup("test-cong").expect("registered");
    assert_eq!(got, "127.0.0.1:12345");

    let removed = cathedral_unregister("test-cong").expect("first removal");
    assert_eq!(removed, "127.0.0.1:12345");

    let after = cathedral_lookup("test-cong");
    assert!(after.is_none(), "lookup after unregister returns None");
}

/// Smite wipes worker %soul without disconnecting the agent. Pins the
/// scenario where you want to reset state mid-session without losing
/// the agent (vs excommunicate which kills the connection).
#[test]
fn smite_zeroes_worker_soul_without_killing_agent() {
    use std::time::Duration;

    let (handle, children) = forge_congregation(1);
    let session_ids = handle.muster();

    // Set %soul to non-empty.
    let set_code = "our %soul = (k => 'v'); 'set'";
    let pid1 = handle.scatter(set_code, &session_ids).expect("set");
    let _ = handle.gather(pid1, Duration::from_secs(5)).expect("set gather");

    // Smite — equivalent to `our %soul = (); our %gift = (); 'smitten'`.
    let smite_code = "our %soul = (); our %gift = (); 'smitten'";
    let pid2 = handle.scatter(smite_code, &session_ids).expect("smite");
    let smite_results = handle.gather(pid2, Duration::from_secs(5)).expect("smite gather");
    assert_eq!(smite_results.len(), 1, "agent acknowledged smite");

    // Verify %soul is empty. Same `to_json(%soul)` workaround as the
    // lick pin (flat-list output vs the broken `\%soul` ref path) so
    // we get []/empty-array meaning "no entries" rather than the
    // falsely-empty "{}" the buggy hashref deref would give.
    let check_code = "our %soul; to_json(%soul)";
    let pid3 = handle.scatter(check_code, &session_ids).expect("check");
    let check_results = handle.gather(pid3, Duration::from_secs(5)).expect("check gather");
    let json_str = check_results[&session_ids[0]].output.trim();
    // Empty hash flattens to empty list — to_json's representation of
    // that depends on context (may be "[]" or "null" depending on
    // internal stringification path). Either signals "no entries".
    assert!(
        json_str == "[]" || json_str == "null",
        "smite must leave %soul empty; got {:?}",
        json_str
    );

    // Agent is still alive (still in roster).
    assert_eq!(handle.muster().len(), 1, "agent survived smite");

    dismiss(&handle, children);
}
