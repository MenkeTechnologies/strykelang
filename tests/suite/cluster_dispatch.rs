//! End-to-end tests for the persistent cluster dispatcher.
//!
//! Two layers exercised here:
//!
//! 1. **Wire protocol vs a real `fo --remote-worker` subprocess.** Spawns the worker
//!    directly (no ssh), drives the v3 handshake (HELLO → SESSION_INIT → JOB → JOB_RESP →
//!    SHUTDOWN) using the public [`forge::remote_wire`] helpers, and verifies many JOBs
//!    flow over a single session. This is the closest we can get to a real cluster without
//!    actually setting up SSH in CI.
//!
//! 2. **Full dispatcher with a fake `ssh` shim.** Drops a tiny shell script into a temp
//!    `PATH` directory that just `exec`s `fo --remote-worker` (ignoring its host argument),
//!    points the dispatcher's `PATH` at it, and runs `cluster::run_cluster` end-to-end.
//!    This validates the per-slot worker thread, work-stealing queue, and result ordering
//!    without needing a real remote host.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Global lock to serialize tests that modify `PATH` via `PrependPathGuard`.
/// `std::env::set_var` is process-global and not thread-safe; without this mutex,
/// parallel tests corrupt each other's `PATH` and the wrong `ssh` binary gets invoked.
static SSH_SHIM_LOCK: Mutex<()> = Mutex::new(());

use forge::cluster::perl_items_to_json;
use forge::remote_wire::{
    frame_kind, read_typed_frame, send_msg, write_typed_frame, HelloAck, HelloMsg, JobMsg,
    JobRespMsg, SessionAck, SessionInit, PROTO_VERSION,
};
use forge::value::{PerlSub, PerlValue};

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forge-cluster-{}-{}-{}",
        std::process::id(),
        tag,
        rand::random::<u32>()
    ))
}

/// Temp directory containing an `ssh` executable that skips ssh flags, host, and `pe_path`,
/// then `exec`s the test `fo` binary — matches what [`forge::cluster`] passes to `ssh`.
fn make_fake_ssh_shim_dir(tag: &str) -> PathBuf {
    let pe_exe = env!("CARGO_BIN_EXE_fo");
    let shim_dir = tmp_path(tag);
    fs::create_dir_all(&shim_dir).unwrap();
    let shim_path = shim_dir.join("ssh");
    {
        let mut f = fs::File::create(&shim_path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "while [ \"${{1#-}}\" != \"$1\" ]; do").unwrap();
        writeln!(f, "  case \"$1\" in").unwrap();
        writeln!(f, "    -o) shift 2 ;;").unwrap();
        writeln!(f, "    *)  shift ;;").unwrap();
        writeln!(f, "  esac").unwrap();
        writeln!(f, "done").unwrap();
        writeln!(f, "shift  # drop host").unwrap();
        writeln!(f, "shift  # drop pe_path").unwrap();
        writeln!(f, "exec {} \"$@\"", pe_exe).unwrap();
    }
    let mut perms = fs::metadata(&shim_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&shim_path, perms).unwrap();
    shim_dir
}

struct PrependPathGuard {
    saved: Option<String>,
}

impl PrependPathGuard {
    fn prepend(shim_dir: &std::path::Path) -> Self {
        let saved = std::env::var("PATH").ok();
        let tail = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", shim_dir.display(), tail));
        Self { saved }
    }
}

impl Drop for PrependPathGuard {
    fn drop(&mut self) {
        if let Some(p) = &self.saved {
            std::env::set_var("PATH", p);
        } else {
            std::env::remove_var("PATH");
        }
    }
}

/// Spawn the local `fo --remote-worker` and return the live child.
fn spawn_local_worker() -> Child {
    let exe = env!("CARGO_BIN_EXE_fo");
    Command::new(exe)
        .arg("--remote-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fo --remote-worker")
}

#[test]
fn perl_items_to_json_maps_scalars() {
    let items = vec![
        PerlValue::integer(3),
        PerlValue::integer(-1),
        PerlValue::string("x".into()),
    ];
    let j = perl_items_to_json(&items).expect("marshal");
    assert_eq!(j.len(), 3);
    assert_eq!(j[0], serde_json::json!(3));
    assert_eq!(j[1], serde_json::json!(-1));
    assert_eq!(j[2], serde_json::json!("x"));
}

#[test]
fn perl_items_to_json_rejects_code_reference_items() {
    let cb = PerlValue::code_ref(Arc::new(PerlSub {
        name: "cb".into(),
        params: vec![],
        body: vec![],
        closure_env: None,
        prototype: None,
        fib_like: None,
    }));
    let err = perl_items_to_json(&[cb]).expect_err("CODE items cannot be marshalled to JSON");
    assert!(
        err.contains("not supported") || err.contains("CODE"),
        "unexpected error: {err}"
    );
}

#[test]
fn run_cluster_empty_items_returns_ok_without_touching_slots() {
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let cluster = RemoteCluster {
        slots: vec![],
        job_timeout_ms: 1,
        max_attempts: 1,
        connect_timeout_ms: 1,
    };
    let out = run_cluster(&cluster, String::new(), "$_;".to_string(), vec![], vec![])
        .expect("empty input should succeed");
    assert!(out.is_empty());
}

#[test]
fn run_cluster_errors_when_no_slots_and_nonempty_items() {
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let cluster = RemoteCluster {
        slots: vec![],
        job_timeout_ms: 1,
        max_attempts: 1,
        connect_timeout_ms: 1,
    };
    let err = run_cluster(
        &cluster,
        String::new(),
        "$_;".to_string(),
        vec![],
        vec![serde_json::json!(1)],
    )
    .expect_err("no slots");
    assert!(err.contains("no slots"), "unexpected message: {err:?}");
}

#[test]
fn worker_session_rejects_invalid_block_with_nack() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: "{{{ not valid perl".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (kind, body) = read_typed_frame(&mut stdout).unwrap();
    assert_eq!(kind, frame_kind::SESSION_ACK);
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(!ack.ok, "expected session failure, got ok ack");
    assert!(
        ack.err_msg.to_lowercase().contains("parse"),
        "err_msg should mention parse: {:?}",
        ack.err_msg
    );
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(
        !status.success(),
        "worker should exit non-zero after failed session init"
    );
}

#[test]
fn worker_session_rejects_invalid_subs_prelude_with_nack() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: "sub broken { {{{".to_string(),
        block_src: "$_;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (kind, body) = read_typed_frame(&mut stdout).unwrap();
    assert_eq!(kind, frame_kind::SESSION_ACK);
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(!ack.ok);
    assert!(
        ack.err_msg.to_lowercase().contains("parse"),
        "unexpected err: {:?}",
        ack.err_msg
    );
    drop(stdin);
    assert!(!child.wait().unwrap().success());
}

#[test]
fn worker_session_rejects_subs_prelude_runtime_die() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: r#"die "prelude-stop";"#.to_string(),
        block_src: "$_;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (kind, body) = read_typed_frame(&mut stdout).unwrap();
    assert_eq!(kind, frame_kind::SESSION_ACK);
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(!ack.ok);
    assert!(
        ack.err_msg.contains("prelude-stop") || ack.err_msg.to_lowercase().contains("prelude"),
        "unexpected err: {:?}",
        ack.err_msg
    );
    drop(stdin);
    assert!(!child.wait().unwrap().success());
}

#[test]
fn worker_session_exits_on_proto_version_mismatch() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    send_msg(
        &mut stdin,
        frame_kind::HELLO,
        &HelloMsg {
            proto_version: PROTO_VERSION.wrapping_add(999),
            pe_version: "test".to_string(),
        },
    )
    .unwrap();
    let got = read_typed_frame(&mut stdout);
    assert!(
        got.is_err(),
        "worker must not emit HELLO_ACK on proto mismatch, got {got:?}"
    );
    drop(stdin);
    assert!(!child.wait().unwrap().success());
}

#[test]
fn worker_session_shutdown_without_jobs_exits_cleanly() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: "$_ * 3;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn worker_session_exits_on_unknown_frame_kind_after_session() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: "$_;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    const UNKNOWN_KIND: u8 = 0xEE;
    write_typed_frame(&mut stdin, UNKNOWN_KIND, &[]).unwrap();
    drop(stdin);
    assert!(!child.wait().unwrap().success());
}

#[test]
fn worker_session_continues_after_one_job_failure() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: r#"if ($_ == 0) { die "zero-item"; } $_ * 2;"#.to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    send_msg(
        &mut stdin,
        frame_kind::JOB,
        &JobMsg {
            seq: 0,
            item: serde_json::json!(0),
        },
    )
    .unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
    assert!(!resp.ok);
    assert!(resp.err_msg.contains("zero-item"), "{:?}", resp.err_msg);

    send_msg(
        &mut stdin,
        frame_kind::JOB,
        &JobMsg {
            seq: 1,
            item: serde_json::json!(21),
        },
    )
    .unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
    assert!(resp.ok, "{}", resp.err_msg);
    assert_eq!(resp.result, serde_json::json!(42));

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn worker_session_job_die_surfaces_in_job_resp() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: r#"die "remote-failure";"#.to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    send_msg(
        &mut stdin,
        frame_kind::JOB,
        &JobMsg {
            seq: 0,
            item: serde_json::json!(null),
        },
    )
    .unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
    assert_eq!(resp.seq, 0);
    assert!(!resp.ok);
    assert!(
        resp.err_msg.contains("remote-failure"),
        "unexpected err_msg: {:?}",
        resp.err_msg
    );

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn worker_session_capture_visible_in_block() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: String::new(),
        block_src: "$_ + $factor;".to_string(),
        capture: vec![("$factor".to_string(), serde_json::json!(100))],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    send_msg(
        &mut stdin,
        frame_kind::JOB,
        &JobMsg {
            seq: 7,
            item: serde_json::json!(5),
        },
    )
    .unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
    assert!(resp.ok, "{}", resp.err_msg);
    assert_eq!(resp.result, serde_json::json!(105));

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    let _ = child.wait();
}

#[test]
fn worker_session_package_state_persists_across_jobs() {
    let mut child = spawn_local_worker();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

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

    let init = SessionInit {
        subs_prelude: "our $acc = 0;".to_string(),
        block_src: "$acc++; $acc;".to_string(),
        capture: vec![],
    };
    send_msg(&mut stdin, frame_kind::SESSION_INIT, &init).unwrap();
    let (_, body) = read_typed_frame(&mut stdout).unwrap();
    let ack: SessionAck = bincode::deserialize(&body).unwrap();
    assert!(ack.ok, "{}", ack.err_msg);

    for expect in 1i64..=5 {
        send_msg(
            &mut stdin,
            frame_kind::JOB,
            &JobMsg {
                seq: expect as u64,
                item: serde_json::json!(null),
            },
        )
        .unwrap();
        let (_, body) = read_typed_frame(&mut stdout).unwrap();
        let resp: JobRespMsg = bincode::deserialize(&body).unwrap();
        assert!(resp.ok, "job {expect}: {}", resp.err_msg);
        assert_eq!(resp.result, serde_json::json!(expect));
    }

    send_msg::<_, ()>(&mut stdin, frame_kind::SHUTDOWN, &()).unwrap();
    drop(stdin);
    let _ = child.wait();
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
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let _lock = SSH_SHIM_LOCK.lock().unwrap();
    let shim_dir = make_fake_ssh_shim_dir("ssh-shim");
    let _path = PrependPathGuard::prepend(&shim_dir);

    // Build a 2-slot "cluster" pointing at fake hosts. The pe_path is irrelevant because
    // our shim hardcodes the test binary; we still set it for realism.
    let cluster = RemoteCluster {
        slots: vec![
            forge::value::RemoteSlot {
                host: "fake1".to_string(),
                pe_path: "fo".to_string(),
            },
            forge::value::RemoteSlot {
                host: "fake2".to_string(),
                pe_path: "fo".to_string(),
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

    drop(_path);
    let _ = fs::remove_dir_all(&shim_dir);

    let values = result.expect("dispatcher run");
    assert_eq!(values.len(), 20);
    for (i, v) in values.iter().enumerate() {
        let expected = ((i + 1) as i64).pow(2);
        let got = v.to_int();
        assert_eq!(got, expected, "result[{i}] = {got}, want {expected}");
    }
}

#[test]
fn dispatcher_single_slot_preserves_input_order() {
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let _lock = SSH_SHIM_LOCK.lock().unwrap();
    let shim_dir = make_fake_ssh_shim_dir("ssh-shim-one");
    let _path = PrependPathGuard::prepend(&shim_dir);

    let cluster = RemoteCluster {
        slots: vec![forge::value::RemoteSlot {
            host: "solo".to_string(),
            pe_path: "fo".to_string(),
        }],
        job_timeout_ms: 30_000,
        max_attempts: RemoteCluster::DEFAULT_MAX_ATTEMPTS,
        connect_timeout_ms: 10_000,
    };

    let n = 40i64;
    let items: Vec<serde_json::Value> = (1..=n).map(|i| serde_json::json!(i)).collect();
    let result = run_cluster(&cluster, String::new(), "$_;".to_string(), vec![], items);

    drop(_path);
    let _ = fs::remove_dir_all(&shim_dir);

    let values = result.expect("dispatcher run");
    assert_eq!(values.len(), n as usize);
    for (i, v) in values.iter().enumerate() {
        assert_eq!(v.to_int(), (i + 1) as i64, "position {i}");
    }
}

#[test]
fn dispatcher_applies_lexical_capture_from_run_cluster() {
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let _lock = SSH_SHIM_LOCK.lock().unwrap();
    let shim_dir = make_fake_ssh_shim_dir("ssh-shim-cap");
    let _path = PrependPathGuard::prepend(&shim_dir);

    let cluster = RemoteCluster {
        slots: vec![
            forge::value::RemoteSlot {
                host: "cap1".to_string(),
                pe_path: "fo".to_string(),
            },
            forge::value::RemoteSlot {
                host: "cap2".to_string(),
                pe_path: "fo".to_string(),
            },
        ],
        job_timeout_ms: 30_000,
        max_attempts: RemoteCluster::DEFAULT_MAX_ATTEMPTS,
        connect_timeout_ms: 10_000,
    };

    let items: Vec<serde_json::Value> = (1..=12i64).map(|i| serde_json::json!(i)).collect();
    let capture = vec![("$bias".to_string(), serde_json::json!(1000))];
    let result = run_cluster(
        &cluster,
        String::new(),
        "$_ + $bias;".to_string(),
        capture,
        items,
    );

    drop(_path);
    let _ = fs::remove_dir_all(&shim_dir);

    let values = result.expect("dispatcher with capture");
    assert_eq!(values.len(), 12);
    for (i, v) in values.iter().enumerate() {
        let expected = (i + 1) as i64 + 1000;
        assert_eq!(v.to_int(), expected);
    }
}

#[test]
fn dispatcher_surfaces_permanent_block_failure_from_worker() {
    use forge::cluster::run_cluster;
    use forge::value::RemoteCluster;

    let _lock = SSH_SHIM_LOCK.lock().unwrap();
    let shim_dir = make_fake_ssh_shim_dir("ssh-shim-die");
    let _path = PrependPathGuard::prepend(&shim_dir);

    let cluster = RemoteCluster {
        slots: vec![forge::value::RemoteSlot {
            host: "diehost".to_string(),
            pe_path: "fo".to_string(),
        }],
        job_timeout_ms: 30_000,
        max_attempts: RemoteCluster::DEFAULT_MAX_ATTEMPTS,
        connect_timeout_ms: 10_000,
    };

    let err = run_cluster(
        &cluster,
        String::new(),
        r#"die "pmap-block-boom";"#.to_string(),
        vec![],
        vec![serde_json::json!(1)],
    )
    .expect_err("die in block should fail the map");

    drop(_path);
    let _ = fs::remove_dir_all(&shim_dir);

    assert!(
        err.contains("failed permanently") && err.contains("pmap-block-boom"),
        "unexpected error: {err}"
    );
}
