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
    get_controller, get_current_controller, register_controller, register_divination, spawn_controller,
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
