//! Persistent thread pool (`ppool`) — workers pull jobs from a shared queue and run
//! each task on a **fresh** [`Interpreter`] on an **existing** OS thread (no rayon task
//! spawn per item; threads stay alive between jobs).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crossbeam::channel::{unbounded, Receiver, Sender};

use crate::error::{PerlError, PerlResult};
use crate::interpreter::{Flow, FlowOrError, Interpreter};
use crate::scope::{AtomicArray, AtomicHash};
use crate::value::{PerlPpool, PerlSub, PerlValue};

/// Shared pool state (jobs in, results out-of-order; `PerlPpool::collect` reorders).
pub struct PpoolInner {
    /// `None` after the pool is shut down.
    pub(crate) job_tx: Mutex<Option<Sender<PoolJob>>>,
    result_rx: Mutex<Receiver<(u64, PerlValue)>>,
    pending: Mutex<VecDeque<(u64, PerlValue)>>,
    pub(crate) next_order: AtomicU64,
    collect_from: AtomicU64,
    workers: Mutex<Option<Vec<JoinHandle<()>>>>,
}

pub(crate) struct PoolJob {
    order: u64,
    sub: Arc<PerlSub>,
    arg: PerlValue,
    subs: HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
}

impl PerlPpool {
    pub(crate) fn submit(
        &self,
        interp: &mut Interpreter,
        args: &[PerlValue],
        line: usize,
    ) -> PerlResult<PerlValue> {
        if args.is_empty() {
            return Err(PerlError::runtime(
                "submit() expects a code reference and optional argument for $_",
                line,
            ));
        }
        let Some(sub) = args[0].as_code_ref() else {
            return Err(PerlError::runtime(
                "submit() first argument must be a CODE ref",
                line,
            ));
        };
        // One-arg form: bind worker `$_` from the caller's `$_` at submit time (postfix `for @tasks`
        // sets `$_` each iteration). Two-arg form: explicit binding (may be `undef`).
        let arg = if args.len() >= 2 {
            args[1].clone()
        } else {
            interp.scope.get_scalar("_").clone()
        };
        let order = self.0.next_order.fetch_add(1, Ordering::SeqCst);
        let subs = interp.subs.clone();
        let (capture, atomic_arrays, atomic_hashes) = interp.scope.capture_with_atomics();
        let job = PoolJob {
            order,
            sub: Arc::clone(&sub),
            arg,
            subs,
            capture,
            atomic_arrays,
            atomic_hashes,
        };
        let tx = self
            .0
            .job_tx
            .lock()
            .map_err(|_| PerlError::runtime("ppool: job queue poisoned", line))?;
        let Some(sender) = tx.as_ref() else {
            return Err(PerlError::runtime("ppool: pool shut down", line));
        };
        sender
            .send(job)
            .map_err(|_| PerlError::runtime("ppool: submit failed (pool shut down)", line))?;
        Ok(PerlValue::UNDEF)
    }

    pub(crate) fn collect(&self, line: usize) -> PerlResult<PerlValue> {
        let start = self.0.collect_from.load(Ordering::SeqCst);
        let end = self.0.next_order.load(Ordering::SeqCst);
        let n = (end - start) as usize;
        if n == 0 {
            return Ok(PerlValue::array(vec![]));
        }

        let mut slots: Vec<Option<PerlValue>> = vec![None; n];
        let mut count = 0usize;

        {
            let mut pending = self
                .0
                .pending
                .lock()
                .map_err(|_| PerlError::runtime("ppool: pending buffer poisoned", line))?;
            let mut keep = VecDeque::new();
            for (o, v) in pending.drain(..) {
                if o >= start && o < end {
                    let idx = (o - start) as usize;
                    if slots[idx].is_none() {
                        slots[idx] = Some(v);
                        count += 1;
                    }
                } else {
                    keep.push_back((o, v));
                }
            }
            *pending = keep;
        }

        let rx = self
            .0
            .result_rx
            .lock()
            .map_err(|_| PerlError::runtime("ppool: collect lock poisoned", line))?;

        while count < n {
            let (o, v) = rx.recv().map_err(|_| {
                PerlError::runtime("ppool: result channel closed (workers stopped)", line)
            })?;
            if o < start {
                continue;
            }
            if o >= end {
                self.0
                    .pending
                    .lock()
                    .map_err(|_| PerlError::runtime("ppool: pending buffer poisoned", line))?
                    .push_back((o, v));
                continue;
            }
            let idx = (o - start) as usize;
            if slots[idx].is_none() {
                slots[idx] = Some(v);
                count += 1;
            }
        }

        self.0.collect_from.store(end, Ordering::SeqCst);
        let out: Vec<PerlValue> = slots
            .into_iter()
            .map(|s| s.unwrap_or(PerlValue::UNDEF))
            .collect();
        Ok(PerlValue::array(out))
    }
}

impl Drop for PpoolInner {
    fn drop(&mut self) {
        if let Ok(mut g) = self.job_tx.lock() {
            let _ = g.take();
        }
        if let Ok(mut g) = self.workers.lock() {
            if let Some(handles) = g.take() {
                for h in handles {
                    let _ = h.join();
                }
            }
        }
    }
}

fn worker_loop(job_rx: Receiver<PoolJob>, result_tx: Sender<(u64, PerlValue)>) {
    while let Ok(job) = job_rx.recv() {
        let mut interp = Interpreter::new();
        interp.subs = job.subs;
        interp.scope.restore_capture(&job.capture);
        interp
            .scope
            .restore_atomics(&job.atomic_arrays, &job.atomic_hashes);
        if let Some(env) = job.sub.closure_env.as_ref() {
            interp.scope.restore_capture(env);
        }
        interp.enable_parallel_guard();
        interp.scope.set_topic(job.arg);
        interp.scope_push_hook();
        let val = match interp.exec_block_no_scope(&job.sub.body) {
            Ok(v) => v,
            Err(FlowOrError::Flow(Flow::Return(v))) => v,
            Err(_) => PerlValue::UNDEF,
        };
        interp.scope_pop_hook();
        let _ = result_tx.send((job.order, val));
    }
}

/// Create a pool with `workers` OS threads (clamped to 1..=256). Each thread runs jobs
/// sequentially; new [`Interpreter`] values are constructed per job (cheap vs thread spawn).
pub fn create_pool(workers: usize) -> PerlResult<PerlValue> {
    let workers = workers.clamp(1, 256);
    let (job_tx, job_rx): (Sender<PoolJob>, Receiver<PoolJob>) = unbounded();
    type ResultMsg = (u64, PerlValue);
    let (result_tx, result_rx): (Sender<ResultMsg>, Receiver<ResultMsg>) = unbounded();

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let jrx = job_rx.clone();
        let rtx = result_tx.clone();
        handles.push(thread::spawn(move || worker_loop(jrx, rtx)));
    }
    drop(job_rx);
    drop(result_tx);

    let inner = Arc::new(PpoolInner {
        job_tx: Mutex::new(Some(job_tx)),
        result_rx: Mutex::new(result_rx),
        pending: Mutex::new(VecDeque::new()),
        next_order: AtomicU64::new(0),
        collect_from: AtomicU64::new(0),
        workers: Mutex::new(Some(handles)),
    });

    Ok(PerlValue::ppool(PerlPpool(inner)))
}
