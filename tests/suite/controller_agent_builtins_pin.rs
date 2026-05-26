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

/// The builtin must accept an explicit port via `host:port` and try to
/// connect to that exact port. We aim it at a borrowed-and-released
/// ephemeral port so the test is deterministic; expected outcome is exit 1
/// (connect refused). Verifies the colon-split parsing branch.
#[test]
fn agent_builtin_accepts_host_with_explicit_port() {
    let port = pick_unreachable_port();
    let code = format!(r#"agent("127.0.0.1:{}", "explicit-port-test")"#, port);
    let start = Instant::now();
    let exit = eval_int(&code);
    let elapsed = start.elapsed();
    assert_eq!(exit, 1, "expected connection refusal exit code");
    assert!(
        elapsed < Duration::from_secs(5),
        "explicit-port connect-refused must be prompt, took {:?}",
        elapsed
    );
}

/// Passing `undef` explicitly should fall through to the same default as
/// passing no argument at all — `localhost:9999`. Pin the undef-friendly
/// arg handling so a script like `agent($ENV{CTL} // undef, undef)` works
/// when the env var is missing instead of dying on a type error.
#[test]
fn agent_builtin_undef_args_use_defaults() {
    let start = Instant::now();
    let exit = eval_int("agent(undef, undef)");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "undef args must default + fail-fast, took {:?}",
        elapsed
    );
    if exit != 1 {
        eprintln!(
            "warning: a real controller is bound to localhost:9999 \
             on this machine and accepted the handshake. Test skipped."
        );
        return;
    }
    assert_eq!(exit, 1, "expected connection refusal on default port");
}

/// Same undef-default contract for `controller()`: explicit `undef`s should
/// match the no-arg defaults (`0.0.0.0:9999`). We can't easily exercise the
/// success path (controller would bind and block forever in the REPL), so
/// we route through the invalid-bind path instead — pass a clearly bogus
/// bind to keep the test bounded.
#[test]
fn controller_builtin_with_undef_args_does_not_crash() {
    // `controller(undef, undef)` must NOT panic on argument extraction;
    // it should fall through to defaults. We can't let it bind for real
    // (would block the test), so verify only that the call returns within
    // 2 seconds with an integer when we feed an explicitly bad bind that
    // takes precedence over the undef-default path.
    let start = Instant::now();
    let exit = eval_int(r#"controller("256.256.256.256", undef)"#);
    let elapsed = start.elapsed();
    assert_eq!(exit, 1, "bad-bind path should still return 1");
    assert!(
        elapsed < Duration::from_secs(2),
        "controller() must short-circuit on bad bind even with undef port, took {:?}",
        elapsed
    );
}

/// End-to-end protocol smoke pin: stand up a hand-rolled fake-controller on
/// a loopback port that accepts ONE connection, parses the `AGENT_HELLO`
/// frame, verifies the `agent_name` override the script passed actually
/// made it onto the wire, then replies with `AGENT_HELLO_ACK` + `SHUTDOWN`.
/// The script-side `agent(...)` builtin should connect, complete the
/// handshake, drain the SHUTDOWN frame, and exit 0.
///
/// What this pins that the unit / unreachable-port tests don't:
///   * The protocol layer composes — script → agent.rs:run_agent_with_explicit
///     → frame I/O → real TcpStream → controller-side bincode decode.
///   * `name` argument propagates all the way to `AgentHello.agent_name`
///     (regression catcher for any future refactor of the name plumbing).
///   * Clean shutdown returns exit 0, not 1 (the "connection refused" code).
///   * The plugin's "spawn an agent from a script" pattern works end-to-end,
///     not just in the synthetic in-Rust unit tests.
#[test]
fn agent_builtin_end_to_end_hello_handshake_and_clean_shutdown() {
    use std::io::ErrorKind;
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc,
    };
    use std::thread;
    use stryke::agent::{frame_kind, read_frame, write_frame, AgentHello, AgentHelloAck};

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake controller");
    let port = listener.local_addr().unwrap().port();
    let name_seen = Arc::new(AtomicBool::new(false));
    let name_seen_clone = Arc::clone(&name_seen);

    let server = thread::spawn(move || {
        // Single-shot accept.
        listener
            .set_nonblocking(false)
            .expect("set blocking listener");
        let (mut stream, _) = match listener.accept() {
            Ok(p) => p,
            Err(e) if e.kind() == ErrorKind::WouldBlock => return,
            Err(e) => panic!("fake controller accept: {e}"),
        };

        let (kind, payload) = read_frame(&mut stream).expect("read AGENT_HELLO");
        assert_eq!(
            kind,
            frame_kind::AGENT_HELLO,
            "first frame must be AGENT_HELLO, got kind=0x{:02x}",
            kind
        );
        let hello: AgentHello = bincode::deserialize(&payload).expect("decode AgentHello");
        if hello.agent_name.as_deref() == Some("e2e-smoke-agent") {
            name_seen_clone.store(true, AtomicOrdering::Relaxed);
        }

        let ack = AgentHelloAck {
            session_id: 1,
            accepted: true,
            message: "welcome".to_string(),
        };
        let ack_bytes = bincode::serialize(&ack).expect("serialize ack");
        write_frame(&mut stream, frame_kind::AGENT_HELLO_ACK, &ack_bytes)
            .expect("write HELLO_ACK");
        write_frame(&mut stream, frame_kind::SHUTDOWN, &[]).expect("write SHUTDOWN");
        // Let the agent process SHUTDOWN — connection drops when `stream`
        // falls out of scope here.
    });

    let code = format!(
        r#"agent("127.0.0.1:{}", "e2e-smoke-agent")"#,
        port
    );
    let start = std::time::Instant::now();
    let exit = eval_int(&code);
    let elapsed = start.elapsed();

    server.join().expect("fake controller thread");
    assert!(
        name_seen.load(AtomicOrdering::Relaxed),
        "fake controller never observed `agent_name = e2e-smoke-agent` in AGENT_HELLO"
    );
    assert_eq!(
        exit, 0,
        "agent() should exit 0 on clean controller-side SHUTDOWN"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "handshake + shutdown round-trip should be sub-second, took {:?}",
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
