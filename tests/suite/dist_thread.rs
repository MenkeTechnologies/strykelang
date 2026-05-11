//! End-to-end tests for `~d>` — the **distributed** thread macro.
//!
//! Same chunk-block semantics as `~p>` (stages run on `@_` = chunk elements),
//! but the chunks are shipped to remote workers via the existing
//! `cluster::run_cluster` SSH dispatcher. For tests we bypass real SSH via
//! the `STRYKE_CLUSTER_LOCAL_BIN` env knob added in `cluster.rs:open()` —
//! when set, slot worker spawns the binary directly with `--remote-worker`
//! instead of going through `ssh HOST`. That gives us the full session
//! handshake + JOB-frame wire without needing sshd / ssh keys in CI.

#![cfg(unix)]

use std::sync::Arc;

use parking_lot::Mutex;

use stryke::value::{PerlValue, RemoteCluster, RemoteSlot};
use stryke::vm_helper::VMHelper;

/// `STRYKE_CLUSTER_LOCAL_BIN` is process-global; serialize tests that set it
/// so parallel `cargo test` doesn't race on the env var.
static LOCAL_BIN_LOCK: Mutex<()> = Mutex::new(());

/// Snapshot + restore the env var so a test failure doesn't leak the
/// override to later tests that might run without it.
struct EnvGuard {
    key: &'static str,
    saved: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let saved = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.saved {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn build_local_cluster(n_slots: usize) -> PerlValue {
    let slots = (0..n_slots)
        .map(|i| RemoteSlot {
            host: format!("local-{i}"),
            pe_path: "stryke".to_string(),
        })
        .collect();
    let c = RemoteCluster {
        slots,
        job_timeout_ms: 30_000,
        max_attempts: RemoteCluster::DEFAULT_MAX_ATTEMPTS,
        connect_timeout_ms: 10_000,
    };
    PerlValue::remote_cluster(Arc::new(c))
}

/// Set up the env override, declare `$c` in scope as a local-loopback
/// cluster, parse + run `code`, return the result.
fn run_with_local_cluster(n_slots: usize, code: &str) -> PerlValue {
    let _lock = LOCAL_BIN_LOCK.lock();
    let _guard = EnvGuard::set("STRYKE_CLUSTER_LOCAL_BIN", env!("CARGO_BIN_EXE_st"));
    let mut interp = VMHelper::new();
    interp
        .scope
        .declare_scalar("c", build_local_cluster(n_slots));
    let program = stryke::parse(code).expect("parse failed");
    interp.execute(&program).expect("execute failed")
}

#[test]
fn dist_thread_map_doubles_each_element() {
    let v = run_with_local_cluster(2, "~d> on $c 1:10 map { _ * 2 }");
    let got: Vec<i64> = v
        .as_array_vec()
        .or_else(|| v.as_array_ref().map(|a| a.read().clone()))
        .expect("result should be a list")
        .iter()
        .map(|x| x.to_int())
        .collect();
    assert_eq!(got, vec![2, 4, 6, 8, 10, 12, 14, 16, 18, 20]);
}

#[test]
fn dist_thread_preserves_source_order_across_chunks() {
    // 40 items × 4 slots — each slot handles a chunk; final result must be
    // in source order regardless of which slot finished first.
    let v = run_with_local_cluster(4, "~d> on $c 1:40 map { _ + 100 }");
    let got: Vec<i64> = v
        .as_array_vec()
        .or_else(|| v.as_array_ref().map(|a| a.read().clone()))
        .expect("list")
        .iter()
        .map(|x| x.to_int())
        .collect();
    let want: Vec<i64> = (101..=140).collect();
    assert_eq!(got, want);
}

#[test]
fn dist_thread_empty_source_returns_empty() {
    let v = run_with_local_cluster(2, "~d> on $c () map { _ * 2 }");
    let got: Vec<PerlValue> = v
        .as_array_vec()
        .or_else(|| v.as_array_ref().map(|a| a.read().clone()))
        .expect("empty list");
    assert!(got.is_empty(), "expected empty array, got {:?}", got);
}

#[test]
fn dist_thread_rejects_non_cluster_operand() {
    // `on EXPR` parses; runtime rejects when EXPR is not a cluster value.
    let _lock = LOCAL_BIN_LOCK.lock();
    let _guard = EnvGuard::set("STRYKE_CLUSTER_LOCAL_BIN", env!("CARGO_BIN_EXE_st"));
    let program =
        stryke::parse(r#"my $c = "not a cluster"; ~d> on $c 1:5 map { _ * 2 }"#).expect("parse");
    let mut interp = VMHelper::new();
    let err = interp
        .execute(&program)
        .expect_err("should fail with non-cluster operand");
    let msg = format!("{}", err);
    assert!(
        msg.contains("expected cluster(...) value"),
        "unexpected error: {msg}"
    );
}

#[test]
fn dist_thread_missing_on_keyword_is_parse_error() {
    // `~d> SOURCE stages` without `on EXPR` should fail at parse time.
    let err = stryke::parse("~d> 1:5 map { _ * 2 }").expect_err("expected parse error");
    let msg = format!("{}", err);
    assert!(
        msg.contains("expected `on <cluster-expr>`"),
        "unexpected parse error: {msg}"
    );
}
