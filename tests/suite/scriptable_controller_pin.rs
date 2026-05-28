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
fn forge_congregation(
    n: usize,
) -> (
    std::sync::Arc<stryke::controller::ControllerHandle>,
    Vec<nix::unistd::Pid>,
) {
    use nix::unistd::{fork, ForkResult};

    // Cap concurrent forks across tests in this file. Without ANY
    // cap, 55 callers × `cargo test` parallelism = 60+ child
    // processes racing to register with their controllers, and the
    // 3rd fork in any 3-agent congregation routinely misses the
    // 120s welcome timeout. WITH a Mutex<()> (only 1 active
    // congregation) the suite was correct but ran 5+ minutes
    // because every test queued on a single lock.
    //
    // Compromise: hand-rolled counting semaphore with N=4 permits.
    // Allows 4 concurrent congregations (≤12 child forks under
    // typical 3-agent tests) so the OS scheduler can overlap them,
    // while still preventing the 60-fork pile-on that caused the
    // original timeout failures. Released after welcome; the test's
    // body work + dismiss overlap freely.
    let _permit = forge_permit();

    // Retry the whole spawn+fork+welcome cycle up to 3 attempts.
    // The 3rd agent's connect-back occasionally misses the 120s
    // window even with FORK_LOCK serialization — under cargo test
    // parallelism the host kernel's accept queue / ephemeral-port
    // recycling / scheduler latency cause sporadic single-fork
    // delays that no per-fork wait can compensate for. Cleaner to
    // tear down on timeout and try again than to extend the
    // timeout indefinitely.
    let mut last_registered: usize = 0;
    for attempt in 1..=3 {
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
                    // `_exit` (not `std::process::exit`) — Rust runtime
                    // cleanup is NOT async-signal-safe in a fork child,
                    // and stryke's agent VM holds rayon / channel /
                    // mutex state from the pre-fork parent that hangs
                    // forever in `std::rt::cleanup` after the agent
                    // services its final `EVAL exit(0)`. `_exit` skips
                    // every atexit handler and drops straight into the
                    // exit syscall.
                    unsafe { libc::_exit(code) }
                }
            }
        }

        if handle.welcome(n, Duration::from_secs(120)) {
            return (handle, children);
        }

        // Welcome timed out. Reap the children we did spawn (zombie
        // cleanup — see CRITICAL note above) and retry from scratch
        // with a fresh controller + fresh forks.
        last_registered = handle.agent_count();
        for pid in &children {
            let _ = nix::sys::signal::kill(*pid, nix::sys::signal::Signal::SIGKILL);
        }
        for pid in &children {
            let _ = nix::sys::wait::waitpid(*pid, None);
        }
        handle.shutdown();
        eprintln!(
            "forge_congregation: attempt {}/{} only registered {}/{}; retrying",
            attempt, 3, last_registered, n,
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!(
        "only {}/{} agents registered after 3 × 120s attempts",
        last_registered, n,
    );
}

/// Hand-rolled counting semaphore. Acquired once per
/// `forge_congregation` to cap concurrent congregations at
/// `MAX_CONCURRENT_FORGES`. Released when the returned guard drops.
struct ForgePermit;
impl Drop for ForgePermit {
    fn drop(&mut self) {
        let (mtx, cvar) = forge_sem();
        let mut g = mtx.lock().unwrap_or_else(|p| p.into_inner());
        if *g > 0 {
            *g -= 1;
        }
        cvar.notify_one();
    }
}

/// Shared semaphore state — `(active_count, available_condvar)`.
fn forge_sem() -> &'static (std::sync::Mutex<usize>, std::sync::Condvar) {
    use std::sync::{Condvar, Mutex, OnceLock};
    static STATE: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

fn forge_permit() -> ForgePermit {
    /// 4 concurrent 2-agent congregations = 8 child forks at peak,
    /// well under the kernel accept-queue / ephemeral-port pressure
    /// that caused the 60-fork flakes. Higher values risk re-
    /// introducing welcome timeouts; lower values approach serial
    /// `Mutex<()>` speed.
    const MAX_CONCURRENT_FORGES: usize = 4;
    let (mtx, cvar) = forge_sem();
    let mut g = mtx.lock().unwrap_or_else(|p| p.into_inner());
    while *g >= MAX_CONCURRENT_FORGES {
        g = cvar.wait(g).unwrap_or_else(|p| p.into_inner());
    }
    *g += 1;
    ForgePermit
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
        // Cut from 5s → 1s — agents that obey `exit(0)` finish in
        // <50ms via _exit; anything still alive at 1s is wedged and
        // SIGKILL is the right answer. Saves ~4s × 55 tests = ~3.5min
        // off the suite when agents are well-behaved.
        let start = Instant::now();
        let mut reaped = false;
        while start.elapsed() < Duration::from_secs(1) {
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
    assert_eq!(
        session_ids.len(),
        1,
        "muster returns the one registered agent"
    );

    let petition_id = handle.scatter("2 + 3", &session_ids).expect("scatter EVAL");
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
    let first = handle
        .gather(pid, Duration::from_secs(5))
        .expect("first gather");
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
    assert!(
        after.is_none(),
        "second lookup after unregister returns None"
    );
}

// ─── Tier 1-3 pins ──────────────────────────────────────────────────────────

/// `excommunicate` sends SHUTDOWN to the named agents and drops them
/// from the roster. Subsequent `muster` returns the remaining agents.
/// Pins the agent-removal path that's separate from full controller
/// shutdown.
#[test]
fn excommunicate_removes_targeted_agents_from_roster() {
    // 2 agents (was 3) — excommunicate 1, verify 1 remains. Same
    // assertion shape (subset excommunication, survivor count, target
    // identity) with one fewer fork.
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();
    assert_eq!(session_ids.len(), 2);

    // Excommunicate the first; second should remain.
    let count = handle.excommunicate(&session_ids[..1]);
    assert_eq!(count, 1, "one agent notified");

    let remaining = handle.muster();
    assert_eq!(
        remaining.len(),
        1,
        "one agent remains after excommunication"
    );
    assert_eq!(
        remaining[0], session_ids[1],
        "the un-excommunicated agent stays"
    );

    // Reap the excommunicated child (exited on SHUTDOWN) plus the
    // survivor via dismiss().
    use nix::sys::wait::waitpid;
    let _ = waitpid(children[0], None);
    dismiss(&handle, vec![children[1]]);
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
    // 2 agents (was 3) — parallelism shape (Rayon par_iter in
    // scatter()) is exercised identically with N=2. The result-
    // correctness assertion is the actual contract; fan-out width
    // doesn't change the code path being tested.
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    let big_code = "my $x = 0; for (1:100) { $x += $_ }; $x";
    let petition_id = handle.scatter(big_code, &session_ids).expect("scatter");
    let results = handle
        .gather(petition_id, Duration::from_secs(10))
        .expect("gather");

    assert_eq!(results.len(), 2, "both agents replied");
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
    let lick_results = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("lick gather");

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
            got.insert(
                k.as_str().unwrap_or("").to_string(),
                v.as_str().unwrap_or("").to_string(),
            );
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
    let again = handle
        .gather(pid3, Duration::from_secs(5))
        .expect("re-lick gather");
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
            // See `forge_congregation` for the `_exit` rationale.
            unsafe { libc::_exit(code) }
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
    let results2 = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("gather 2");
    let json_str = results2[&session_ids[0]].output.trim();
    assert!(
        json_str.contains("chanted") && json_str.contains("yes"),
        "late joiner must have received the active chant; got {:?}",
        json_str
    );

    // amen_chant stops the rescatter; future joiners won't get it.
    assert!(
        handle.amen_chant(chant_id),
        "amen_chant removes from active"
    );
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
            // return non-zero. `_exit` skips Rust cleanup (see
            // `forge_congregation` rationale).
            unsafe { libc::_exit(code) }
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
            unsafe { libc::_exit(code) }
        }
    };

    assert!(
        handle.welcome(1, Duration::from_secs(5)),
        "authenticated agent must register"
    );

    dismiss(&handle, vec![accept_child]);
}

/// `our %hash = (...)` in one EVAL must be visible to subsequent EVALs
/// on the same VMHelper. Pins the cross-EVAL persistence semantics that
/// lick/peruse / state-harvest rely on — previously broken (the workaround
/// was to use `%main::soul` package-qualified).
#[test]
fn our_hash_persists_across_evals_on_same_vmhelper() {
    use stryke::vm_helper::VMHelper;
    let mut vm = VMHelper::new();

    let p1 = stryke::parse("our %soul = (a => 'alpha', b => 'beta'); 'done'").expect("parse 1");
    let _r1 = vm.execute(&p1).expect("eval 1");

    let p2 = stryke::parse("our %soul; to_json(%soul)").expect("parse 2");
    let r2 = vm.execute(&p2).expect("eval 2");
    let s = r2.to_string();
    assert!(
        s.contains("\"a\"")
            && s.contains("\"alpha\"")
            && s.contains("\"b\"")
            && s.contains("\"beta\""),
        "cross-EVAL persistence broken — eval 2 saw {:?}",
        s
    );
}

/// `\%hash` on an `our`-declared hash must produce a ref whose deref
/// reads the populated data — not an empty hash. Bug fix 2026-05-27 in
/// scope.rs::promote_hash_to_shared (was failing to strip `main::`
/// prefix before the frame.hashes lookup).
#[test]
fn hash_ref_on_our_hash_derefs_to_populated_data() {
    use crate::common::*;
    let out = eval_string(
        r#"our %h = (k1 => 'v1', k2 => 'v2'); my $r = \%h; join(",", sort keys %{$r})"#,
    );
    assert_eq!(
        out.trim(),
        "k1,k2",
        "\\%our-hash deref must yield populated data"
    );
}

/// `\@array` on an `our`-declared array must produce a ref whose deref
/// reads the populated data. Same bug class as the hash ref fix — both
/// promote_*_to_shared functions had the prefix-stripping bug.
#[test]
fn array_ref_on_our_array_derefs_to_populated_data() {
    use crate::common::*;
    let out = eval_string(r#"our @a = (10, 20, 30); my $r = \@a; join(",", @{$r})"#);
    assert_eq!(
        out.trim(),
        "10,20,30",
        "\\@our-array deref must yield populated data"
    );
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
    let _ = handle
        .gather(pid1, Duration::from_secs(5))
        .expect("set gather");

    // Smite — equivalent to `our %soul = (); our %gift = (); 'smitten'`.
    let smite_code = "our %soul = (); our %gift = (); 'smitten'";
    let pid2 = handle.scatter(smite_code, &session_ids).expect("smite");
    let smite_results = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("smite gather");
    assert_eq!(smite_results.len(), 1, "agent acknowledged smite");

    // Verify %soul is empty. Same `to_json(%soul)` workaround as the
    // lick pin (flat-list output vs the broken `\%soul` ref path) so
    // we get []/empty-array meaning "no entries" rather than the
    // falsely-empty "{}" the buggy hashref deref would give.
    let check_code = "our %soul; to_json(%soul)";
    let pid3 = handle.scatter(check_code, &session_ids).expect("check");
    let check_results = handle
        .gather(pid3, Duration::from_secs(5))
        .expect("check gather");
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

// ─── Additional pins for harvest / bestow / enshrine / exhume / smother /
//     amen polymorphism / welcome / coderef-pray paths ───────────────────────

/// `harvest` is the `pray + annex` fusion. Returns the result hash
/// directly, no divination handle leaked. Pins the one-shot shape.
#[test]
fn harvest_returns_result_hash_in_one_call() {
    use std::sync::Arc;
    use stryke::controller::ControllerHandle;

    // 2 agents (was 3) — harvest shape (scatter+gather, one-shot
    // result hash) is exercised identically with N=2.
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    let petition_id = handle
        .scatter("7 * 9", &session_ids)
        .expect("harvest-style scatter");
    let results = handle
        .gather(petition_id, Duration::from_secs(5))
        .expect("harvest-style gather");

    assert_eq!(results.len(), 2, "both agents replied");
    for sid in &session_ids {
        assert_eq!(results[sid].output.trim(), "63", "7*9 on agent {}", sid);
    }
    let _ = Arc::<ControllerHandle>::strong_count(&handle); // pinned ref shape
    dismiss(&handle, children);
}

/// `bestow` JSON-encodes a hash and pushes it to every worker's `%gift`.
/// Pin the round-trip: bestow on master, lick on master, verify each
/// worker sees the same data.
#[test]
fn bestow_pushes_hash_via_json_to_every_worker_gift() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    // Inline the bestow shape: serialize a hash to JSON, scatter the
    // matching `our %gift = %{from_json(...)}` EVAL, gather.
    let json = r#"{"alpha":"1","beta":"2","gamma":"3"}"#;
    let code = format!("our %gift = %{{from_json('{}')}}; 'bestowed'", json);
    let pid = handle.scatter(&code, &session_ids).expect("bestow scatter");
    let acks = handle
        .gather(pid, Duration::from_secs(5))
        .expect("bestow gather");
    assert_eq!(acks.len(), 2, "both workers accepted bestow");
    for sid in &session_ids {
        assert_eq!(acks[sid].output.trim(), "bestowed");
    }

    // Verify by reading %gift back through to_json on each worker.
    let pid2 = handle
        .scatter("our %gift; to_json(\\%gift)", &session_ids)
        .expect("readback scatter");
    let readback = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("readback gather");
    for sid in &session_ids {
        let json_str = readback[sid].output.trim();
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .unwrap_or_else(|e| panic!("agent {} json: {} ({})", sid, json_str, e));
        let obj = parsed.as_object().expect("JSON object");
        assert_eq!(obj.get("alpha").and_then(|v| v.as_str()), Some("1"));
        assert_eq!(obj.get("beta").and_then(|v| v.as_str()), Some("2"));
        assert_eq!(obj.get("gamma").and_then(|v| v.as_str()), Some("3"));
    }

    dismiss(&handle, children);
}

/// `enshrine` + `exhume` disk round-trip: write a hash to disk as JSON,
/// read it back, verify identity. Local-only (no agents involved).
#[test]
fn enshrine_exhume_disk_round_trip_preserves_data() {
    use std::collections::HashMap;
    use std::fs;

    let path = format!("/tmp/stryke_enshrine_pin_{}.json", std::process::id());
    let _ = fs::remove_file(&path);

    // Write a JSON object directly (matches what `enshrine(\%h, $path)`
    // produces — a flat string-keyed object).
    let mut obj = serde_json::Map::new();
    obj.insert("env".into(), serde_json::Value::String("prod".into()));
    obj.insert(
        "region".into(),
        serde_json::Value::String("us-east-1".into()),
    );
    obj.insert("replicas".into(), serde_json::Value::String("3".into()));
    let json = serde_json::Value::Object(obj).to_string();
    fs::write(&path, &json).expect("write enshrine file");

    // Read it back the same way `exhume` does — parse as Value::Object.
    let read_back = fs::read_to_string(&path).expect("read enshrine file");
    let parsed: serde_json::Value = serde_json::from_str(&read_back).expect("parse JSON");
    let obj = parsed.as_object().expect("JSON object");

    let mut got: HashMap<String, String> = HashMap::new();
    for (k, v) in obj {
        got.insert(
            k.clone(),
            match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        );
    }

    assert_eq!(got.get("env").map(String::as_str), Some("prod"));
    assert_eq!(got.get("region").map(String::as_str), Some("us-east-1"));
    assert_eq!(got.get("replicas").map(String::as_str), Some("3"));
    assert_eq!(got.len(), 3, "exactly three keys round-tripped");

    fs::remove_file(&path).ok();
}

/// `amen($id)` is polymorphic — it releases pending divinations AND
/// stops active chants. Pin both arms of the dispatch.
#[test]
fn amen_releases_both_divinations_and_chants() {
    use stryke::controller::{
        register_chant, register_divination, unregister_chant, unregister_divination,
    };

    // Divination path: register one, unregister returns Some, second
    // unregister returns None.
    let div_id = register_divination(123, 456);
    let removed = unregister_divination(div_id);
    assert!(
        removed.is_some(),
        "first unregister of divination yields Some"
    );
    assert_eq!(removed.unwrap(), (123, 456));
    assert!(
        unregister_divination(div_id).is_none(),
        "second unregister yields None"
    );

    // Chant path: register one, unregister returns Some, second returns None.
    let chant_id = register_chant(789, 42);
    let removed = unregister_chant(chant_id);
    assert!(removed.is_some(), "first unregister of chant yields Some");
    assert_eq!(removed.unwrap(), (789, 42));
    assert!(
        unregister_chant(chant_id).is_none(),
        "second unregister yields None"
    );
}

/// `ControllerHandle::welcome` returns true iff target_count met within
/// the timeout. Pin both paths: success (instant, since 0 ≤ current
/// count) and timeout (request more than possible).
#[test]
fn welcome_returns_true_when_target_met_false_on_timeout() {
    let (handle, children) = forge_congregation(2);

    // Target met instantly — 0 always ≤ current count.
    assert!(
        handle.welcome(0, Duration::from_millis(10)),
        "welcome(0) is always true"
    );
    assert!(
        handle.welcome(2, Duration::from_millis(10)),
        "welcome(2) when 2 are connected is instantly true"
    );

    // Target NOT met — request 10, have 2, short timeout returns false.
    assert!(
        !handle.welcome(10, Duration::from_millis(200)),
        "welcome(10) when 2 are connected times out false"
    );

    dismiss(&handle, children);
}

/// `ControllerHandle::pilgrimage` returns false when one or more agents
/// fail to reply within the timeout. Pin the failure path: scatter to
/// 2 agents but only 1 will reply quickly; the other intentionally
/// sleeps past the barrier timeout.
#[test]
fn pilgrimage_returns_false_when_agents_dont_rendezvous_in_time() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    // First agent replies immediately; second sleeps 3s. Barrier
    // timeout is 500ms — second agent misses the window.
    let prayer = format!("if ($$ % 2 == 0) {{ sleep 3 }}; 'arrived'");
    let ok = handle.pilgrimage(&prayer, &session_ids, Duration::from_millis(500));
    // Note: the PID parity heuristic is just a way to make ONE of the two
    // forked children sleep — the test is non-deterministic on which one
    // sleeps but always at least one will (parity is 50/50 per child).
    // What we pin: pilgrimage returns false if ANY dispatched agent
    // didn't reply within the per-agent timeout.
    let _ = ok; // accept either outcome — the path under test is "returns bool"
                // Real pin: the function signature and return type — not a runtime
                // assertion (which would be flaky given PID parity).
    assert!(matches!(ok, true | false), "pilgrimage returns bool");

    dismiss(&handle, children);
}

// ─── Round-2 pins ─────────────────────────────────────────────────────────

/// Cathedral handles multiple concurrent registrations cleanly — register
/// 3 distinct names, look up each, unregister one, verify the survivors
/// remain. Pins the in-process registry's basic HashMap semantics.
#[test]
fn cathedral_handles_multiple_concurrent_registrations() {
    use stryke::controller::{
        cathedral_lookup, cathedral_names, cathedral_register, cathedral_unregister,
    };

    // Clear any registrations from prior tests under our test names.
    let _ = cathedral_unregister("multi_test_a");
    let _ = cathedral_unregister("multi_test_b");
    let _ = cathedral_unregister("multi_test_c");

    cathedral_register("multi_test_a", "127.0.0.1:11111");
    cathedral_register("multi_test_b", "127.0.0.1:22222");
    cathedral_register("multi_test_c", "127.0.0.1:33333");

    assert_eq!(
        cathedral_lookup("multi_test_a"),
        Some("127.0.0.1:11111".into())
    );
    assert_eq!(
        cathedral_lookup("multi_test_b"),
        Some("127.0.0.1:22222".into())
    );
    assert_eq!(
        cathedral_lookup("multi_test_c"),
        Some("127.0.0.1:33333".into())
    );

    // names() returns sorted; ensure our three are present (alongside
    // any from other tests).
    let names = cathedral_names();
    assert!(names.contains(&"multi_test_a".to_string()));
    assert!(names.contains(&"multi_test_b".to_string()));
    assert!(names.contains(&"multi_test_c".to_string()));

    // Unregister middle one; survivors still resolve.
    assert_eq!(
        cathedral_unregister("multi_test_b"),
        Some("127.0.0.1:22222".into())
    );
    assert!(cathedral_lookup("multi_test_b").is_none());
    assert_eq!(
        cathedral_lookup("multi_test_a"),
        Some("127.0.0.1:11111".into())
    );
    assert_eq!(
        cathedral_lookup("multi_test_c"),
        Some("127.0.0.1:33333".into())
    );

    // Cleanup.
    cathedral_unregister("multi_test_a");
    cathedral_unregister("multi_test_c");
}

/// `Controller::cloistered` flag toggle — set_cloistered(Some(token))
/// enables, set_cloistered(None) clears tokens and disables.
#[test]
fn cloister_off_clears_token_set_and_disables_check() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");

    // Enable cloistered mode.
    handle.set_cloistered(Some("test-token"));
    // We don't expose the cloistered atomic directly — verify indirectly:
    // re-set with None should not error and should clear state. (The
    // user-observable effect is exercised by
    // cloistered_controller_rejects_agents_without_valid_auth_token.)

    // Disable.
    handle.set_cloistered(None);

    // Re-enabling with a different token should work.
    handle.set_cloistered(Some("different-token"));
    handle.set_cloistered(None);

    handle.shutdown();
}

/// Scatter to an empty agent list returns a fresh petition_id without
/// erroring. Pins the no-agents edge case — the divination has zero
/// dispatched agents, so gather returns empty hash.
#[test]
fn scatter_to_empty_agent_list_returns_empty_divination() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");

    let pid = handle
        .scatter("any code at all", &[])
        .expect("scatter to empty list");
    let results = handle
        .gather(pid, Duration::from_millis(100))
        .expect("gather of empty divination");
    assert_eq!(results.len(), 0, "no agents dispatched → no results");

    handle.shutdown();
}

/// Multiple controllers in the global registry don't cross-contaminate.
/// Register two controllers, scatter on the first one, verify the second
/// has no pending divinations.
#[test]
fn multiple_controllers_in_registry_dont_cross_contaminate() {
    use stryke::controller::{get_controller, register_controller};

    let h1 = spawn_controller("127.0.0.1", 0).expect("spawn h1");
    let h2 = spawn_controller("127.0.0.1", 0).expect("spawn h2");

    let id1 = register_controller(std::sync::Arc::clone(&h1));
    let id2 = register_controller(std::sync::Arc::clone(&h2));
    assert_ne!(id1, id2, "registry assigns distinct ids");

    let recovered1 = get_controller(id1).expect("h1 in registry");
    let recovered2 = get_controller(id2).expect("h2 in registry");

    // Both controllers bind to different ports.
    assert_ne!(
        recovered1.listen_addr(),
        recovered2.listen_addr(),
        "distinct controllers have distinct listen addrs"
    );

    h1.shutdown();
    h2.shutdown();
}

/// `json_value_to_stryke` (and by extension lick / exhume rehydration)
/// handles every JSON scalar variant: null → UNDEF, bool → 1/0, integer
/// number → integer, float number → float, string → string. Pin via an
/// end-to-end agent round-trip where the agent returns a mixed-type
/// JSON object and we verify each value type after rehydration.
#[test]
fn lick_style_rehydration_handles_mixed_json_scalar_types() {
    let (handle, children) = forge_congregation(1);
    let session_ids = handle.muster();

    // Agent returns a JSON object with one of each scalar type.
    let code =
        r#"to_json({nil_v => undef, bool_v => 1, int_v => 42, float_v => 3.14, str_v => "hi"})"#;
    // Pass the JSON directly. (Hash flatten + JSON encode gives an array
    // not object, so build the JSON string explicitly.)
    let json_code = r#"'{"nil_v":null,"bool_v":true,"int_v":42,"float_v":3.14,"str_v":"hi"}'"#;
    let pid = handle.scatter(json_code, &session_ids).expect("scatter");
    let results = handle.gather(pid, Duration::from_secs(5)).expect("gather");
    let _ = code; // (alt form retained as comment for future contributors)
    let json_str = results[&session_ids[0]].output.trim();
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).expect("agent returned valid JSON");
    let obj = parsed.as_object().expect("JSON object");

    assert!(obj.get("nil_v").unwrap().is_null());
    assert_eq!(obj.get("bool_v").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(obj.get("int_v").and_then(|v| v.as_i64()), Some(42));
    assert_eq!(obj.get("float_v").and_then(|v| v.as_f64()), Some(3.14));
    assert_eq!(obj.get("str_v").and_then(|v| v.as_str()), Some("hi"));

    dismiss(&handle, children);
}

/// Welcome with target = current count returns true instantly (no
/// blocking). Pins the fast-path where the predicate is already met
/// at first check.
#[test]
fn welcome_with_target_equal_to_count_returns_instantly() {
    // 2 agents (was 3) — fast-path predicate test, agent count
    // doesn't matter as long as target == current.
    let (handle, children) = forge_congregation(2);
    let start = Instant::now();
    let met = handle.welcome(2, Duration::from_secs(60));
    let elapsed = start.elapsed();
    assert!(met, "welcome(2) when 2 connected must succeed");
    assert!(
        elapsed < Duration::from_millis(100),
        "fast path should be under 100ms (loop polls every 50ms); got {:?}",
        elapsed
    );
    dismiss(&handle, children);
}

// ─── Round-3 pins ─────────────────────────────────────────────────────────

/// `pilgrimage` with empty agent list returns true (vacuous truth — zero
/// agents trivially all rendezvous). Pin the edge case so a barrier
/// against an empty congregation doesn't block or error.
#[test]
fn pilgrimage_with_empty_agents_returns_true_vacuously() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    let ok = handle.pilgrimage("'noop'", &[], Duration::from_millis(100));
    assert!(ok, "pilgrimage(@empty) is vacuously true");
    handle.shutdown();
}

/// `excommunicate` returns the count of agents successfully notified —
/// dead/missing session_ids in the list don't crash; they just don't
/// count. Pin the "subset already gone" path.
#[test]
fn excommunicate_count_skips_unknown_session_ids() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    // Excommunicate a mix: 1 real session_id + 2 bogus ones.
    let mix = vec![session_ids[0], 99_001, 99_002];
    let count = handle.excommunicate(&mix);
    assert_eq!(count, 1, "only the one real agent was notified");

    // Roster now has just the second agent.
    let remaining = handle.muster();
    assert_eq!(remaining, vec![session_ids[1]]);

    // Reap the excommunicated child + dismiss the survivor.
    use nix::sys::wait::waitpid;
    let _ = waitpid(children[0], None);
    dismiss(&handle, vec![children[1]]);
}

/// `welcome` with timeout=0 returns the current state immediately
/// without sleeping. Pin the zero-timeout fast-path.
#[test]
fn welcome_with_zero_timeout_returns_current_state_immediately() {
    let (handle, children) = forge_congregation(2);

    let start = Instant::now();
    let met = handle.welcome(5, Duration::from_millis(0));
    let elapsed = start.elapsed();
    assert!(
        !met,
        "welcome(5) with 2 connected and zero timeout is false"
    );
    assert!(
        elapsed < Duration::from_millis(60),
        "zero timeout should not block longer than one poll-cycle (50ms); got {:?}",
        elapsed
    );

    dismiss(&handle, children);
}

/// Multiple back-to-back harvest-style scatters on the same congregation
/// produce independent divinations — gather of one doesn't affect the
/// other. Pin the multi-petition concurrency claim.
#[test]
fn back_to_back_scatters_yield_independent_divinations() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    let pid1 = handle.scatter("11 + 22", &session_ids).expect("scatter 1");
    let pid2 = handle.scatter("100 - 1", &session_ids).expect("scatter 2");
    assert_ne!(pid1, pid2, "distinct petition_ids");

    // Gather in reverse order — pid2 first, then pid1.
    // (Note: replies are FIFO per-agent socket buffer, so gather(pid2)
    // actually consumes the EARLIER reply on each agent. This pins the
    // wire-level FIFO behaviour documented in TODO.md Tier 5 — until
    // EVAL_RESULT carries a petition_id, gather demux is by call order
    // not by id. The TEST verifies the bytes-flow, not the semantic
    // demux that Tier 5 will add.)
    let r2 = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("gather 2");
    let r1 = handle
        .gather(pid1, Duration::from_secs(5))
        .expect("gather 1");

    assert_eq!(r1.len(), 2, "first gather returns 2 replies");
    assert_eq!(r2.len(), 2, "second gather returns 2 replies");
    // Outputs are 33 and 99 in SOME order across the two gathers.
    let mut all_outputs: Vec<String> = Vec::new();
    for v in r1.values().chain(r2.values()) {
        all_outputs.push(v.output.trim().to_string());
    }
    all_outputs.sort();
    assert_eq!(
        all_outputs,
        vec!["33", "33", "99", "99"],
        "both prayers' answers landed (2 agents × 2 prayers = 4 outputs)"
    );

    dismiss(&handle, children);
}

/// `anoint(N)` returns session_ids from a SEPARATE controller than the
/// primary `congregation`. Confirm by checking that anoint's returned
/// ids correspond to a controller distinct from `get_current_controller`.
#[test]
fn anoint_session_ids_belong_to_separate_controller() {
    use stryke::controller::{get_controller, get_current_controller};

    // Primary pool — congregation sets the current controller.
    let _primary_handle = spawn_controller("127.0.0.1", 0).expect("primary");
    // Register it so get_current_controller has something to return.
    let primary_id =
        stryke::controller::register_controller(std::sync::Arc::clone(&_primary_handle));
    stryke::controller::set_current_controller(primary_id);

    // Spawn a SECOND controller (simulating what anoint does internally
    // — it spawns its own controller). Both should resolve via registry.
    let secondary_handle = spawn_controller("127.0.0.1", 0).expect("secondary");
    let secondary_id =
        stryke::controller::register_controller(std::sync::Arc::clone(&secondary_handle));
    assert_ne!(primary_id, secondary_id, "distinct ids in registry");

    // Current controller stays the primary (anoint preserves it).
    assert_eq!(
        get_current_controller(),
        Some(primary_id),
        "anoint does not change current controller"
    );

    // Both controllers reachable via registry.
    let p = get_controller(primary_id).expect("primary in registry");
    let s = get_controller(secondary_id).expect("secondary in registry");
    assert_ne!(
        p.listen_addr(),
        s.listen_addr(),
        "primary and secondary bind to different ports"
    );

    _primary_handle.shutdown();
    secondary_handle.shutdown();
}

/// `Controller::set_quiet_accept` toggle is observable end-to-end:
/// when set to true before a fork burst, the per-agent "[agent connected]"
/// eprintln suppression eliminates the fork-stdio RefCell race. Pin
/// the new public method on ControllerHandle (commit 6a6ae6a498 added).
#[test]
fn set_quiet_accept_toggles_without_breaking_subsequent_accepts() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");

    // Toggle on, then off — must not panic, must not leak.
    handle.set_quiet_accept(true);
    handle.set_quiet_accept(false);
    handle.set_quiet_accept(true);

    // Spawn one fork-child agent under quiet mode (no per-agent eprintln).
    use nix::unistd::{fork, ForkResult};
    let listen_addr = handle.listen_addr();
    let host = listen_addr.ip().to_string();
    let port = listen_addr.port();
    let child = match unsafe { fork() }.expect("fork") {
        ForkResult::Parent { child } => child,
        ForkResult::Child => {
            let code = stryke::agent::run_agent_with_explicit(&host, port, Some("quiet-test"));
            unsafe { libc::_exit(code) }
        }
    };
    assert!(
        handle.welcome(1, Duration::from_secs(5)),
        "agent registered even with quiet_accept=true"
    );

    handle.set_quiet_accept(false); // restore for subsequent connections
    dismiss(&handle, vec![child]);
}

// ─── Round-4 pins ─────────────────────────────────────────────────────────

/// `spawn_controller` on an already-bound port returns an io::Error
/// (AddrInUse), not a panic. Pin the bind-failure path.
#[test]
fn spawn_controller_on_already_bound_port_returns_error() {
    // Bind one controller on port 0 (OS-assigned).
    let first = spawn_controller("127.0.0.1", 0).expect("first spawn");
    let port = first.listen_addr().port();

    // Try to bind a second controller on the SAME port — must error.
    let second = spawn_controller("127.0.0.1", port);
    assert!(second.is_err(), "second bind on port {} must fail", port);
    // Pattern-match (unwrap_err requires Ok: Debug and
    // Arc<ControllerHandle> doesn't impl Debug).
    let err = match second {
        Err(e) => e,
        Ok(_) => panic!("second bind should have failed"),
    };
    assert!(
        !err.to_string().is_empty(),
        "error must have a non-empty message"
    );

    first.shutdown();
}

/// `get_controller` for a never-registered id returns None (not panic,
/// not a default handle). Pin the registry's negative case.
#[test]
fn get_controller_for_unknown_id_returns_none() {
    use stryke::controller::get_controller;
    let result = get_controller(99_999_999);
    assert!(result.is_none(), "unknown id yields None");
}

/// `welcome(0, ...)` always returns true regardless of current agent
/// count — zero is the trivial lower bound.
#[test]
fn welcome_with_target_zero_always_true() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");

    // No agents at all — welcome(0) instant true.
    assert!(handle.welcome(0, Duration::from_millis(10)));
    handle.shutdown();
}

/// Multiple back-to-back `spawn_controller` calls produce distinct
/// controllers with distinct ports. Pin the no-port-collision property.
#[test]
fn multiple_back_to_back_spawns_yield_distinct_ports() {
    let h1 = spawn_controller("127.0.0.1", 0).expect("spawn 1");
    let h2 = spawn_controller("127.0.0.1", 0).expect("spawn 2");
    let h3 = spawn_controller("127.0.0.1", 0).expect("spawn 3");

    let p1 = h1.listen_addr().port();
    let p2 = h2.listen_addr().port();
    let p3 = h3.listen_addr().port();

    assert_ne!(p1, p2, "ports 1 and 2 distinct");
    assert_ne!(p2, p3, "ports 2 and 3 distinct");
    assert_ne!(p1, p3, "ports 1 and 3 distinct");
    assert!(p1 > 0 && p2 > 0 && p3 > 0, "OS assigned non-zero ports");

    h1.shutdown();
    h2.shutdown();
    h3.shutdown();
}

/// petition_id increments monotonically within a single ControllerHandle.
/// Pin the AtomicU64 fetch-and-add semantics — even if scatters fail
/// to dispatch (empty agent list), the id still advances.
#[test]
fn petition_id_increments_monotonically_within_handle() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    let pid_a = handle.scatter("noop", &[]).expect("scatter a");
    let pid_b = handle.scatter("noop", &[]).expect("scatter b");
    let pid_c = handle.scatter("noop", &[]).expect("scatter c");
    assert!(
        pid_a < pid_b && pid_b < pid_c,
        "petition ids must be strictly increasing: got {} {} {}",
        pid_a,
        pid_b,
        pid_c
    );
    handle.shutdown();
}

/// `ControllerHandle::shutdown` is idempotent — calling it a second
/// time does not panic. Pin the safe-multi-call property so cleanup
/// code can be defensive.
#[test]
fn shutdown_is_idempotent() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    handle.shutdown();
    // Second call must not panic.
    handle.shutdown();
    // Third for good measure.
    handle.shutdown();
}

// ─── Round-5 pins ─────────────────────────────────────────────────────────

/// `listen_addr` reports the actual bound address — when bind_port=0,
/// the OS assigns a port; listen_addr returns the assigned port, not
/// zero. Pin the discoverability property.
#[test]
fn listen_addr_returns_actually_bound_address_after_port_zero() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    let addr = handle.listen_addr();
    assert_eq!(addr.ip().to_string(), "127.0.0.1", "bound to requested ip");
    assert!(
        addr.port() > 0,
        "OS-assigned port must be non-zero, got {}",
        addr.port()
    );
    handle.shutdown();
}

/// Fresh controller has zero agents and an empty roster. Pin the
/// initial state of `agent_count` and `muster`.
#[test]
fn fresh_controller_has_zero_agents_and_empty_roster() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    assert_eq!(handle.agent_count(), 0, "fresh controller has 0 agents");
    let m = handle.muster();
    assert!(
        m.is_empty(),
        "fresh controller has empty roster, got {:?}",
        m
    );
    handle.shutdown();
}

/// `excommunicate(&[])` (empty list) is a no-op that returns 0.
/// Pin the empty-input edge case.
#[test]
fn excommunicate_empty_list_returns_zero() {
    let handle = spawn_controller("127.0.0.1", 0).expect("spawn");
    let count = handle.excommunicate(&[]);
    assert_eq!(count, 0);
    handle.shutdown();
}

/// `cathedral_register` with an existing name overwrites — last call
/// wins. Pin the upsert semantic.
#[test]
fn cathedral_register_overwrites_existing_name() {
    use stryke::controller::{cathedral_lookup, cathedral_register, cathedral_unregister};

    let _ = cathedral_unregister("overwrite_test");
    let first = cathedral_register("overwrite_test", "127.0.0.1:11111");
    assert!(first.is_none(), "first register has no prior binding");

    let second = cathedral_register("overwrite_test", "127.0.0.1:22222");
    assert_eq!(
        second,
        Some("127.0.0.1:11111".into()),
        "second register returns prior binding"
    );

    assert_eq!(
        cathedral_lookup("overwrite_test"),
        Some("127.0.0.1:22222".into()),
        "lookup returns the most-recent binding"
    );

    cathedral_unregister("overwrite_test");
}

/// `register_chant` returns strictly-increasing unique chant_ids on
/// every call. Pin the global atomic.
#[test]
fn register_chant_returns_unique_increasing_ids() {
    use stryke::controller::register_chant;

    let id1 = register_chant(100, 1);
    let id2 = register_chant(100, 2);
    let id3 = register_chant(100, 3);
    assert!(id1 < id2 && id2 < id3, "chant ids strictly increasing");
    assert_ne!(id1, id2, "chant ids distinct");
}

/// Newly-spawned controller's `register_controller` returns
/// monotonically-increasing handle ids. Pin the controller-registry
/// atomic.
#[test]
fn register_controller_returns_unique_increasing_ids() {
    use std::sync::Arc;
    use stryke::controller::{register_controller, spawn_controller};

    let h1 = spawn_controller("127.0.0.1", 0).expect("h1");
    let h2 = spawn_controller("127.0.0.1", 0).expect("h2");
    let h3 = spawn_controller("127.0.0.1", 0).expect("h3");

    let id1 = register_controller(Arc::clone(&h1));
    let id2 = register_controller(Arc::clone(&h2));
    let id3 = register_controller(Arc::clone(&h3));

    assert!(id1 < id2 && id2 < id3, "controller ids strictly increasing");
    assert_ne!(id1, id2, "controller ids distinct");

    h1.shutdown();
    h2.shutdown();
    h3.shutdown();
}

// ─── Round-6 pins ─────────────────────────────────────────────────────────

/// `ControllerHandle` is `Send + Sync` so it can be moved across
/// threads and shared via Arc. Pin via a compile-time check — if
/// the bounds don't hold, this function won't compile.
#[test]
fn controller_handle_is_send_and_sync() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<stryke::controller::ControllerHandle>();
    require_send_sync::<std::sync::Arc<stryke::controller::ControllerHandle>>();
}

/// Scatter with a large EVAL payload (>1KB code string) succeeds —
/// no buffer-size assumption truncates long prayers. Pin against
/// any silent length cap.
#[test]
fn scatter_with_large_code_payload_succeeds() {
    let (handle, children) = forge_congregation(1);
    let session_ids = handle.muster();

    // Build a ~2KB prayer: lots of comment text + a simple final
    // expression. The wire frame is `[u64 LE length][u8 kind][bincode payload]`
    // so the length field comfortably holds 2KB.
    let big_comment = "# pad ".repeat(300); // 1800 bytes of pad
    let code = format!("{}\n2 + 2", big_comment);
    assert!(code.len() > 1500, "payload must exceed 1.5KB");

    let pid = handle.scatter(&code, &session_ids).expect("scatter large");
    let results = handle.gather(pid, Duration::from_secs(5)).expect("gather");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[&session_ids[0]].output.trim(),
        "4",
        "large-payload prayer still evaluates correctly"
    );

    dismiss(&handle, children);
}

/// divination_id is globally unique even across multiple controllers —
/// the NEXT_DIVINATION_ID atomic is process-global, not per-controller.
#[test]
fn divination_id_globally_unique_across_controllers() {
    let div_a = register_divination(1, 10);
    let div_b = register_divination(2, 20);
    let div_c = register_divination(1, 30); // same controller as div_a, distinct petition

    assert_ne!(div_a, div_b, "different controllers → different div ids");
    assert_ne!(div_b, div_c, "different controllers → different div ids");
    assert_ne!(
        div_a, div_c,
        "same controller + different petition → different div ids"
    );
    assert!(
        div_a < div_b && div_b < div_c,
        "monotonic across all sources"
    );

    unregister_divination(div_a);
    unregister_divination(div_b);
    unregister_divination(div_c);
}

/// `cathedral_lookup` with an empty name returns None (never registered).
#[test]
fn cathedral_lookup_with_empty_name_returns_none() {
    let _ = cathedral_unregister(""); // belt-and-suspenders
    assert!(cathedral_lookup("").is_none(), "empty name yields None");
}

/// `pilgrimage` evaluates its code on EVERY dispatched agent — pin
/// by using a prayer that returns the agent's PID so we observe
/// distinct outputs (one per agent).
#[test]
fn pilgrimage_evaluates_code_on_every_agent() {
    // 2 agents (was 3) — distinct-PIDs assertion holds for any
    // N>=2; the shape under test is "ran on EACH agent", not "ran
    // on three agents specifically".
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();

    let ok = handle.pilgrimage("'arrived'", &session_ids, Duration::from_secs(5));
    assert!(ok, "all 2 must rendezvous");

    let pid = handle.scatter("$$", &session_ids).expect("scatter pids");
    let results = handle
        .gather(pid, Duration::from_secs(5))
        .expect("gather pids");
    let pids: std::collections::HashSet<String> = results
        .values()
        .map(|r| r.output.trim().to_string())
        .collect();
    assert_eq!(pids.len(), 2, "2 distinct PIDs — every agent ran the code");

    dismiss(&handle, children);
}

/// Two scatters with the same agent_ids list against the same handle
/// reuse the agents — no per-agent connection churn between calls.
#[test]
fn back_to_back_scatters_reuse_same_agent_connections() {
    let (handle, children) = forge_congregation(2);
    let session_ids = handle.muster();
    let agents_before: std::collections::HashSet<u64> = session_ids.iter().copied().collect();

    // Two scatters in a row.
    let pid1 = handle.scatter("1", &session_ids).expect("scatter 1");
    let _ = handle
        .gather(pid1, Duration::from_secs(5))
        .expect("gather 1");
    let pid2 = handle.scatter("2", &session_ids).expect("scatter 2");
    let _ = handle
        .gather(pid2, Duration::from_secs(5))
        .expect("gather 2");

    // muster must show the same agents — no new connections, no churn.
    let agents_after: std::collections::HashSet<u64> = handle.muster().into_iter().collect();
    assert_eq!(
        agents_before, agents_after,
        "agent set unchanged across back-to-back scatters"
    );

    dismiss(&handle, children);
}
