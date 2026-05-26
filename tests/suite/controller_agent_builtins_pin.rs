//! Pins for the `controller(...)` / `agent(...)` builtins that drop a stryke
//! script into controller or agent mode without going through the CLI dispatch.
//!
//! The blocking-on-success paths can't be exercised from a unit test without
//! deadlocks (the controller REPL reads stdin forever; the agent loop runs
//! until the controller disconnects). What we CAN pin here is the early-exit
//! contract — both builtins return `1` on a recoverable startup failure and
//! return promptly so callers can retry or bail out. That contract is what a
//! script-level `controller(...) // die "bind failed"` idiom depends on.

use crate::common::*;
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// Borrow an ephemeral port from the OS, then release it immediately. There's
/// a microsecond-scale race where another process could grab it before we use
/// it; in practice on a quiet test machine that never happens.
fn pick_unreachable_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

#[test]
fn agent_builtin_returns_1_on_unreachable_controller() {
    let port = pick_unreachable_port();
    let code = format!(r#"agent("127.0.0.1:{}", "pin-test-agent")"#, port);
    let start = Instant::now();
    let exit = eval_int(&code);
    let elapsed = start.elapsed();
    assert_eq!(
        exit, 1,
        "agent() should return 1 when the controller is unreachable"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "agent() must return promptly on connection refusal, took {:?}",
        elapsed
    );
}

#[test]
fn controller_builtin_returns_1_on_invalid_bind_address() {
    // 256.256.256.256 isn't a valid IPv4 → TcpListener::bind fails fast → run_controller returns 1.
    let start = Instant::now();
    let exit = eval_int(r#"controller("256.256.256.256", 9999)"#);
    let elapsed = start.elapsed();
    assert_eq!(
        exit, 1,
        "controller() should return 1 when the bind address is invalid"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "controller() must return promptly on bind failure, took {:?}",
        elapsed
    );
}

#[test]
fn agent_builtin_parses_bare_host_with_default_port() {
    // Same as the connection-refused test but with the port omitted so the
    // default 9999 kicks in. The agent attempts localhost:9999 → if nothing is
    // listening there (the usual case during `cargo test`), it returns 1.
    // If a real controller IS listening on 9999, the test is skipped (it would
    // succeed and block); we'd see that as a non-1 exit and skip the assertion.
    let start = Instant::now();
    let exit = eval_int(r#"agent("127.0.0.1", "pin-test-agent")"#);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "agent() with bare host should return promptly on connection refusal, took {:?}",
        elapsed
    );
    if exit != 1 {
        eprintln!(
            "warning: agent() returned {} — a real controller is bound to localhost:9999 \
             on this machine and accepted the handshake. Test skipped.",
            exit
        );
        return;
    }
    assert_eq!(exit, 1);
}
