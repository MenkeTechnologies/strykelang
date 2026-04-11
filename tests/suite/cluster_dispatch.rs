//! End-to-end tests for the persistent cluster dispatcher.
//!
//! Two layers exercised here:
//!
//! 1. **Wire protocol vs a real `pe --remote-worker` subprocess.** Spawns the worker
//!    directly (no ssh), drives the v3 handshake (HELLO → SESSION_INIT → JOB → JOB_RESP →
//!    SHUTDOWN) using the public [`perlrs::remote_wire`] helpers, and verifies many JOBs
//!    flow over a single session. This is the closest we can get to a real cluster without
//!    actually setting up SSH in CI.
//!
//! 2. **Full dispatcher with a fake `ssh` shim.** Drops a tiny shell script into a temp
//!    `PATH` directory that just `exec`s `pe --remote-worker` (ignoring its host argument),
//!    points the dispatcher's `PATH` at it, and runs `cluster::run_cluster` end-to-end.
//!    This validates the per-slot worker thread, work-stealing queue, and result ordering
//!    without needing a real remote host.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use perlrs::remote_wire::{
    frame_kind, read_typed_frame, send_msg, HelloAck, HelloMsg, JobMsg, JobRespMsg, SessionAck,
    SessionInit, PROTO_VERSION,
};

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "perlrs-cluster-{}-{}-{}",
        std::process::id(),
        tag,
        rand::random::<u32>()
    ))
}

/// Spawn the local `pe --remote-worker` and return the live child.
fn spawn_local_worker() -> Child {
    let exe = env!("CARGO_BIN_EXE_pe");
    Command::new(exe)
        .arg("--remote-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pe --remote-worker")
}

#[test]
fn worker_session_handles_many_jobs_over_one_pipe() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");

    // Drain stderr in the background so the worker can't deadlock on a full pipe.
    let stderr_handle = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut s);
        s
    });

    // 1. HELLO handshake.
    let hello = HelloMsg {
        proto_version: PROTO_VERSION,
        pe_version: "test".to_string(),
    };
    send_msg(&mut stdin, frame_kind::HELLO, &hello).expect("send HELLO");
    let (kind, body) = read_typed_frame(&mut stdout).expect("read HELLO_ACK");
    assert_eq!(kind, frame_kind::HELLO_ACK);
    let ack: HelloAck = bincode::deserialize(&body).expect("decode HELLO_ACK");
    assert_eq!(ack.proto_version, PROTO_VERSION);
    assert!(!ack.pe_version.is_empty());

    // 2. SESSION_INIT — empty subs prelude, single statement block that doubles `$_`.
    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: "$_ * 2;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).expect("send SESSION_INIT");
    let (kind, body) = read_typed_frame(&mut stdout).expect("read SESSION_ACK");
    assert_eq!(kind, frame_kind::SESSION_ACK);
    let sack: SessionAck = bincode::deserialize(&body).expect("decode SESSION_ACK");
    assert!(sack.ok, "session init failed: {}", sack.err_msg);

    // 3. Send 50 JOB frames over the SAME stdin — the whole point of the persistent
    // session is that we don't pay parser+compiler cost per item.
    for i in 0..50u64 {
        let job = JobMsg {
            seq: i,
            item: serde_json::json!(i as i64),
        };
        send_msg(&mut stdin, frame_kind::JOB, &job).expect("send JOB");
        let (kind, body) = read_typed_frame(&mut stdout).expect("read JOB_RESP");
        assert_eq!(kind, frame_kind::JOB_RESP);
        let resp: JobRespMsg = bincode::deserialize(&body).expect("decode JOB_RESP");
        assert_eq!(resp.seq, i);
        assert!(resp.ok, "job {i} failed: {}", resp.err_msg);
        assert_eq!(resp.result, serde_json::json!(2 * i as i64));
    }

    // 4. SHUTDOWN — clean exit.
    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).expect("send SHUTDOWN");
    drop(stdin);
    let status = child.wait().expect("wait worker");
    assert!(status.success(), "worker exited non-zero: {status:?}");
    let _ = stderr_handle.join();
}

#[test]
fn worker_session_runs_subs_prelude_once_visible_to_jobs() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // HELLO.
    send_msg(
        &mut stdin,
        frame_kind::HELLO,
        &HelloMsg {
            proto_version: PROTO_VERSION,
            pe_version: "test".to_string(),
        },
    )
    .unwrap();
    let _ = read_typed_frame(&mut stdout).unwrap();

    // SESSION_INIT defines a sub `triple`. Subsequent JOBs must see it.
    let init = SessionInit {
        subs_prelude: "sub triple { return $_[0] * 3 }".to_string(),
        block_src: "triple($_);".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    for i in 1..=5i64 {
        send_msg(
            &mut stdin,
            frame_kind::JOB,
            &JobMsg {
                seq: i as u64,
                item: serde_json::json!(i),
            },
        )
        .unwrap();
        let (_, body) = read_typed_frame(&mut stdout).unwrap();
        let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
        assert!(resp.ok, "{}", resp.err_msg);
        assert_eq!(resp.result, serde_json::json!(i * 3));
    }

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn dispatcher_runs_against_fake_ssh_with_two_slots() {
    use perlrs::cluster::run_cluster;
    use perlrs::value::RemoteCluster;

    // Build a fake `ssh` shim that ignores the host argument and execs the local pe binary
    // in --remote-worker mode. The dispatcher will pick this up via PATH.
    let pe_exe = env!("CARGO_BIN_EXE_pe");
    let shim_dir = tmp_path("ssh-shim");
    fs::create_dir_all(&shim_dir).unwrap();
    let shim_path = shim_dir.join("ssh");
    {
        let mut f = fs::File::create(&shim_path).unwrap();
        // Skip flags + host arg, then exec whatever follows. The real ssh layout is
        // `ssh [flags] HOST CMD ARGS...`; here we walk past flags and the host before exec.
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "while [ \"${{1#-}}\" != \"$1\" ]; do").unwrap();
        writeln!(f, "  case \"$1\" in").unwrap();
        writeln!(f, "    -o) shift 2 ;;").unwrap();
        writeln!(f, "    *)  shift ;;").unwrap();
        writeln!(f, "  esac").unwrap();
        writeln!(f, "done").unwrap();
        writeln!(f, "shift  # drop host").unwrap();
        // Replace the user's `pe_path` argument with the test binary path.
        writeln!(f, "shift  # drop pe_path").unwrap();
        writeln!(f, "exec {} \"$@\"", pe_exe).unwrap();
    }
    let mut perms = fs::metadata(&shim_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&shim_path, perms).unwrap();

    // Prepend our shim dir to PATH so the dispatcher's `Command::new("ssh")` resolves to
    // our shim, not the real ssh binary.
    let original_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", shim_dir.display(), original_path);
    let saved_path = std::env::var("PATH").ok();
    std::env::set_var("PATH", &new_path);

    // Build a 2-slot "cluster" pointing at fake hosts. The pe_path is irrelevant because
    // our shim hardcodes the test binary; we still set it for realism.
    let cluster = RemoteCluster {
        slots: vec![
            perlrs::value::RemoteSlot {
                host: "fake1".to_string(),
                pe_path: "pe".to_string(),
            },
            perlrs::value::RemoteSlot {
                host: "fake2".to_string(),
                pe_path: "pe".to_string(),
            },
        ],
        job_timeout_ms: 30_000,
        max_attempts: RemoteCluster::DEFAULT_MAX_ATTEMPTS,
        connect_timeout_ms: 10_000,
    };

    let items: Vec<serde_json::Value> = (1..=20i64).map(|i| serde_json::json!(i)).collect();
    let result = run_cluster(
        &cluster,
        "sub sq { return $_[0] * $_[0] }".to_string(),
        "sq($_);".to_string(),
        vec![],
        items,
    );

    // Restore PATH no matter what.
    if let Some(p) = saved_path {
        std::env::set_var("PATH", p);
    } else {
        std::env::remove_var("PATH");
    }
    let _ = fs::remove_dir_all(&shim_dir);

    let values = result.expect("dispatcher run");
    assert_eq!(values.len(), 20);
    for (i, v) in values.iter().enumerate() {
        let expected = ((i + 1) as i64).pow(2);
        let got = v.to_int();
        assert_eq!(got, expected, "result[{i}] = {got}, want {expected}");
    }
}
