//! Persistent SSH worker pool dispatcher for `pmap_on`.
//!
//! ## Architecture
//!
//! ```text
//!                                   ┌── slot 0 (ssh host1) ────┐
//!                                   │  worker thread + ssh proc │
//!                                   │  HELLO + SESSION_INIT     │
//!                                   │  loop: take JOB from work │
//!                                   │        send + read        │
//!                                   │        push to results    │
//!                                   └────────────────────────────┘
//!                                   ┌── slot 1 (ssh host1) ────┐
//!                                   │  worker thread + ssh proc │
//!  main thread                      │  ...                      │
//!  ┌─────────────────┐              └────────────────────────────┘
//!  │ enqueue all jobs├──► work_tx ─►┌── slot 2 (ssh host2) ────┐
//!  │ collect results │              │  ...                      │
//!  └─────────────────┘              └────────────────────────────┘
//!         ▲                                    │
//!         │                                    ▼
//!         └────────── result_rx ────────────────┘
//! ```
//!
//! Each slot is one persistent `ssh HOST PE_PATH --remote-worker` process. The HELLO and
//! SESSION_INIT handshakes happen once per slot lifetime, then the slot pulls JOB messages
//! from a shared crossbeam channel and pushes responses to a result channel. Work-stealing
//! emerges naturally: fast slots drain the queue faster, slow slots take fewer jobs.
//!
//! ## Fault tolerance
//!
//! When a slot's read or write fails (ssh died, network blip, remote crash), the worker
//! thread re-enqueues the in-flight job to the shared queue with `attempts++` and exits.
//! Other living slots pick the job up. A job is permanently failed when its attempt count
//! reaches `cluster.max_attempts`. The whole map fails only when **every** slot is dead or
//! every queued job has exhausted its retry budget.
//!
//! ## Per-job timeout
//!
//! Each `recv` from a slot's stdout uses a per-slot helper thread + bounded channel so the
//! main wait is `crossbeam::channel::recv_timeout(cluster.job_timeout_ms)`. On timeout the
//! ssh child is killed (SIGKILL), the slot is marked dead, and the in-flight job is
//! re-enqueued just like any other slot failure.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam::channel::{bounded, select, unbounded, Receiver, RecvTimeoutError, Sender};

use crate::remote_wire::{
    frame_kind, perl_to_json_value, read_typed_frame, send_msg, HelloAck, HelloMsg, JobMsg,
    JobRespMsg, SessionAck, SessionInit, PROTO_VERSION,
};
use crate::value::{PerlValue, RemoteCluster, RemoteSlot};

/// One unit of work tracked by the dispatcher. Carries the original sequence number for
/// order-preserving result collection plus an attempt counter for retry accounting.
#[derive(Debug, Clone)]
pub struct DispatchJob {
    pub seq: u64,
    pub item: serde_json::Value,
    pub attempts: u32,
}

/// One result reported back to the main thread. `seq` matches the originating
/// [`DispatchJob::seq`] so the dispatcher can stitch results back into source order.
#[derive(Debug)]
pub struct DispatchResult {
    pub seq: u64,
    pub outcome: Result<PerlValue, String>,
}

/// Run a `pmap_on` against a [`RemoteCluster`]. Blocks until every job has either succeeded
/// or exhausted its retry budget. Returns the per-item results in the original list order
/// or the first permanent failure.
///
/// `subs_prelude` and `block_src` are sent **once** per slot at session init.
/// `capture` is the captured-lexical snapshot from the calling scope.
/// `items` is the list of work items (already JSON-marshalled).
pub fn run_cluster(
    cluster: &RemoteCluster,
    subs_prelude: String,
    block_src: String,
    capture: Vec<(String, serde_json::Value)>,
    items: Vec<serde_json::Value>,
) -> Result<Vec<PerlValue>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    if cluster.slots.is_empty() {
        return Err("cluster: no slots".to_string());
    }

    // Shared work queue: every slot pulls from here, and slot threads re-enqueue on failure.
    // Bounded so a misbehaving producer can't memory-blow; size is `slot_count * 2` to give
    // each slot something to grab on the next iteration without blocking.
    let work_capacity = (cluster.slots.len() * 2).max(8);
    let (work_tx, work_rx) = bounded::<DispatchJob>(work_capacity);
    let (result_tx, result_rx) = unbounded::<DispatchResult>();
    // Shutdown signal: slot workers hold their own `work_tx` clones for re-enqueue, so the
    // work channel never closes on its own once every initial job is sent. When all results
    // have been collected the main thread drops `shutdown_tx`, which closes `shutdown_rx`
    // and breaks the slot workers out of their blocking `recv` in `select!`.
    let (shutdown_tx, shutdown_rx) = bounded::<()>(0);

    // Spawn one worker thread per slot.
    let mut handles = Vec::with_capacity(cluster.slots.len());
    let session_init = Arc::new(SessionInit {
        subs_prelude,
        block_src,
        capture,
    });
    let cluster_arc = Arc::new(cluster.clone());

    for (slot_idx, slot) in cluster.slots.iter().enumerate() {
        let slot = slot.clone();
        let work_rx = work_rx.clone();
        let work_tx = work_tx.clone();
        let result_tx = result_tx.clone();
        let shutdown_rx = shutdown_rx.clone();
        let init = Arc::clone(&session_init);
        let cluster = Arc::clone(&cluster_arc);
        handles.push(thread::spawn(move || {
            slot_worker_loop(
                slot_idx,
                slot,
                init,
                cluster,
                work_rx,
                work_tx,
                result_tx,
                shutdown_rx,
            );
        }));
    }

    // Drop the dispatcher-side handles so closing all slot copies signals queue shutdown.
    drop(work_rx);
    drop(result_tx);
    drop(shutdown_rx);

    // Seed the queue with the initial work.
    for (i, item) in items.iter().enumerate() {
        let job = DispatchJob {
            seq: i as u64,
            item: item.clone(),
            attempts: 0,
        };
        if work_tx.send(job).is_err() {
            return Err("cluster: all worker slots died before any work was sent".to_string());
        }
    }
    drop(work_tx); // close once initial enqueue is done; slot threads keep their own clones

    // Collect results in seq order. We allocate the full vector up-front and assign by
    // index so we don't depend on receive order — slot threads complete jobs in any order.
    let mut results: Vec<Option<Result<PerlValue, String>>> =
        (0..items.len()).map(|_| None).collect();
    let mut received = 0usize;
    while received < items.len() {
        match result_rx.recv() {
            Ok(r) => {
                let idx = r.seq as usize;
                if idx < results.len() && results[idx].is_none() {
                    results[idx] = Some(r.outcome);
                    received += 1;
                }
            }
            Err(_) => {
                // All slot threads dropped their senders before we got every result.
                break;
            }
        }
    }

    // All results (or terminal slot-death) are in. Signal slots to stop pulling new work
    // from the queue so they can run their SHUTDOWN handshake and exit cleanly. Without
    // this drop the slot `select!` below would park forever on `work_rx.recv()` because
    // every slot still holds its own `work_tx` clone for re-enqueue.
    drop(shutdown_tx);

    // Wait for slot threads to wind down.
    for h in handles {
        let _ = h.join();
    }

    // Stitch results back together; surface the first permanent failure if any.
    let mut out = Vec::with_capacity(items.len());
    for (i, slot_result) in results.into_iter().enumerate() {
        match slot_result {
            Some(Ok(v)) => out.push(v),
            Some(Err(e)) => {
                return Err(format!("cluster: job {i} failed permanently: {e}"));
            }
            None => {
                return Err(format!(
                    "cluster: job {i} never completed (all slots died?)"
                ));
            }
        }
    }
    Ok(out)
}

/// Per-slot worker thread: spawn ssh, do HELLO + SESSION_INIT, then loop pulling JOBs from
/// the shared queue. On any I/O failure the in-flight job is re-enqueued (or permanently
/// failed if it has exhausted its retry budget) and the slot exits.
#[allow(clippy::too_many_arguments)]
fn slot_worker_loop(
    slot_idx: usize,
    slot: RemoteSlot,
    init: Arc<SessionInit>,
    cluster: Arc<RemoteCluster>,
    work_rx: Receiver<DispatchJob>,
    work_tx: Sender<DispatchJob>,
    result_tx: Sender<DispatchResult>,
    shutdown_rx: Receiver<()>,
) {
    // Spawn the ssh child + initial handshake. Failures here mean this slot never makes
    // any progress; we exit and let other slots drain the queue.
    let mut session = match SlotSession::open(&slot, &init, &cluster) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "cluster: slot {slot_idx} ({}) failed to start: {e}",
                slot.host
            );
            return;
        }
    };

    loop {
        // Take one job, or bail out if the dispatcher has signalled shutdown. We can't rely
        // on `work_rx` closing by itself because every slot holds its own `work_tx` clone
        // for re-enqueue on transport failure — so the channel would stay open forever once
        // all initial jobs are drained. The shutdown channel is the explicit wakeup.
        let job = select! {
            recv(work_rx) -> r => match r {
                Ok(j) => j,
                Err(_) => {
                    // Queue fully closed (e.g. every slot dropped its `work_tx`) — done.
                    let _ = session.shutdown();
                    return;
                }
            },
            recv(shutdown_rx) -> _ => {
                // Dispatcher collected every result — clean SHUTDOWN frame + child wait.
                let _ = session.shutdown();
                return;
            },
        };

        match session.run_job(&job, cluster.job_timeout_ms) {
            Ok(resp) => {
                if resp.ok {
                    let pv = match crate::remote_wire::json_to_perl(&resp.result) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = result_tx.send(DispatchResult {
                                seq: job.seq,
                                outcome: Err(format!("decode result: {e}")),
                            });
                            continue;
                        }
                    };
                    let _ = result_tx.send(DispatchResult {
                        seq: job.seq,
                        outcome: Ok(pv),
                    });
                } else {
                    // Permanent in-script failure — no point retrying, the body is the
                    // same on every slot. Surface immediately.
                    let _ = result_tx.send(DispatchResult {
                        seq: job.seq,
                        outcome: Err(resp.err_msg),
                    });
                }
            }
            Err(SlotError::Transport(e)) => {
                // Wire-level failure — retry on a different slot if budget allows.
                eprintln!(
                    "cluster: slot {slot_idx} ({}) transport error: {e}; retrying job {}",
                    slot.host, job.seq
                );
                requeue_or_fail(&work_tx, &result_tx, &cluster, job);
                let _ = session.kill();
                return;
            }
            Err(SlotError::Timeout) => {
                eprintln!(
                    "cluster: slot {slot_idx} ({}) timed out on job {}; retrying",
                    slot.host, job.seq
                );
                requeue_or_fail(&work_tx, &result_tx, &cluster, job);
                let _ = session.kill();
                return;
            }
        }
    }
}

fn requeue_or_fail(
    work_tx: &Sender<DispatchJob>,
    result_tx: &Sender<DispatchResult>,
    cluster: &RemoteCluster,
    mut job: DispatchJob,
) {
    job.attempts += 1;
    if job.attempts >= cluster.max_attempts {
        let _ = result_tx.send(DispatchResult {
            seq: job.seq,
            outcome: Err(format!(
                "job exhausted retry budget after {} attempts",
                job.attempts
            )),
        });
        return;
    }
    if work_tx.send(job).is_err() {
        // No live slots left to take the work — the dispatcher will detect this when
        // result_rx closes with missing entries.
    }
}

/// One persistent ssh child + the framed I/O handles to talk to it. Holds a stderr
/// drainer thread so a verbose remote `pe` doesn't fill its pipe and deadlock.
struct SlotSession {
    child: Child,
    stdin: std::process::ChildStdin,
    /// Channel that receives one `JobRespMsg` per JOB, with a per-job timeout. Backed by a
    /// helper thread that loops on `read_typed_frame(stdout)` and forwards results.
    resp_rx: Receiver<Result<JobRespMsg, String>>,
}

#[derive(Debug)]
enum SlotError {
    Transport(String),
    Timeout,
}

impl SlotSession {
    fn open(
        slot: &RemoteSlot,
        init: &SessionInit,
        cluster: &RemoteCluster,
    ) -> Result<Self, String> {
        // ssh -o ConnectTimeout=N HOST PE_PATH --remote-worker
        let connect_timeout = (cluster.connect_timeout_ms / 1000).max(1);
        let mut child = Command::new("ssh")
            .arg("-o")
            .arg(format!("ConnectTimeout={connect_timeout}"))
            .arg("-o")
            .arg("BatchMode=yes")
            .arg(&slot.host)
            .arg(&slot.pe_path)
            .arg("--remote-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn ssh: {e}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "ssh stdin missing".to_string())?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "ssh stdout missing".to_string())?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| "ssh stderr missing".to_string())?;

        // Drain stderr in the background so a verbose worker can't deadlock its pipe.
        thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            // Forward to our own stderr prefixed for visibility — operators want to see
            // remote crashes when debugging cluster runs.
            if !buf.trim().is_empty() {
                eprintln!("[remote-worker] {}", buf.trim());
            }
        });

        // 1. HELLO. Direct stdin write (the helper-thread response loop hasn't started yet).
        let hello = HelloMsg {
            proto_version: PROTO_VERSION,
            pe_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        send_msg(&mut stdin, frame_kind::HELLO, &hello).map_err(|e| format!("send HELLO: {e}"))?;
        let (kind, body) =
            read_typed_frame(&mut stdout).map_err(|e| format!("read HELLO_ACK: {e}"))?;
        if kind != frame_kind::HELLO_ACK {
            return Err(format!("expected HELLO_ACK, got frame kind {kind:#04x}"));
        }
        let _: HelloAck =
            bincode::deserialize(&body).map_err(|e| format!("decode HELLO_ACK: {e}"))?;

        // 2. SESSION_INIT (`init` is `&SessionInit` via deref coercion from `&Arc<SessionInit>`).
        send_msg(&mut stdin, frame_kind::SESSION_INIT, init)
            .map_err(|e| format!("send SESSION_INIT: {e}"))?;
        let (kind, body) =
            read_typed_frame(&mut stdout).map_err(|e| format!("read SESSION_ACK: {e}"))?;
        if kind != frame_kind::SESSION_ACK {
            return Err(format!("expected SESSION_ACK, got frame kind {kind:#04x}"));
        }
        let ack: SessionAck =
            bincode::deserialize(&body).map_err(|e| format!("decode SESSION_ACK: {e}"))?;
        if !ack.ok {
            return Err(format!("worker rejected session: {}", ack.err_msg));
        }

        // 3. Spin up the response helper thread. Each iteration reads one frame and
        // forwards either the parsed JobRespMsg or an error string.
        let (resp_tx, resp_rx) = bounded::<Result<JobRespMsg, String>>(1);
        thread::spawn(move || loop {
            match read_typed_frame(&mut stdout) {
                Ok((kind, body)) if kind == frame_kind::JOB_RESP => {
                    match bincode::deserialize::<JobRespMsg>(&body) {
                        Ok(r) => {
                            if resp_tx.send(Ok(r)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = resp_tx.send(Err(format!("decode JOB_RESP: {e}")));
                            return;
                        }
                    }
                }
                Ok((other, _)) => {
                    let _ = resp_tx.send(Err(format!(
                        "unexpected frame kind {other:#04x} in resp loop"
                    )));
                    return;
                }
                Err(e) => {
                    let _ = resp_tx.send(Err(format!("read frame: {e}")));
                    return;
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            resp_rx,
        })
    }

    fn run_job(&mut self, job: &DispatchJob, timeout_ms: u64) -> Result<JobRespMsg, SlotError> {
        let msg = JobMsg {
            seq: job.seq,
            item: job.item.clone(),
        };
        send_msg(&mut self.stdin, frame_kind::JOB, &msg)
            .map_err(|e| SlotError::Transport(format!("send JOB: {e}")))?;
        match self.resp_rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(SlotError::Transport(e)),
            Err(RecvTimeoutError::Timeout) => Err(SlotError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                Err(SlotError::Transport("response channel closed".to_string()))
            }
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        // Best-effort SHUTDOWN frame; ignore errors because we're tearing down anyway.
        let _ = send_msg::<_, ()>(&mut self.stdin, frame_kind::SHUTDOWN, &());
        let _ = self.child.wait();
        Ok(())
    }

    fn kill(&mut self) -> Result<(), String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

/// Convenience: marshal a `Vec<PerlValue>` into the JSON values the dispatcher needs.
pub fn perl_items_to_json(items: &[PerlValue]) -> Result<Vec<serde_json::Value>, String> {
    items.iter().map(perl_to_json_value).collect()
}
