//! `par_pipeline` — two overloads:
//! - **List form** `par_pipeline(@list)` — same chaining as `pipeline(@list)`, but `->filter` / `->map`
//!   run in parallel on `collect()` (input order preserved).
//! - **Named form** `par_pipeline(source => …, stages => …, workers => …)` — multi-stage pipeline
//!   with **bounded channels** between stages (backpressure when a stage is slower than its upstream).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam::channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;

use crate::error::{PerlError, PerlResult};
use crate::interpreter::{Flow, FlowOrError, Interpreter};
use crate::scope::{AtomicArray, AtomicHash};
use crate::value::{PerlSub, PerlValue};

struct ParPipelineSpec {
    source: Arc<PerlSub>,
    stages: Vec<Arc<PerlSub>>,
    workers: Vec<usize>,
    buffer: usize,
}

fn list_from_value(v: &PerlValue) -> Vec<PerlValue> {
    if let Some(a) = v.as_array_vec() {
        return a;
    }
    if let Some(r) = v.as_array_ref() {
        return r.read().clone();
    }
    v.to_list()
}

/// `true` when args are the named `source => …, stages => …, workers => …` form (even length, all three keys present).
pub(crate) fn is_named_par_pipeline_args(args: &[PerlValue]) -> bool {
    if args.len() < 6 || !args.len().is_multiple_of(2) {
        return false;
    }
    let mut has_source = false;
    let mut has_stages = false;
    let mut has_workers = false;
    for chunk in args.chunks(2) {
        match chunk[0].to_string().as_str() {
            "source" => has_source = true,
            "stages" => has_stages = true,
            "workers" => has_workers = true,
            _ => {}
        }
    }
    has_source && has_stages && has_workers
}

fn parse_args(args: &[PerlValue]) -> Result<ParPipelineSpec, PerlError> {
    if args.len() < 6 || !args.len().is_multiple_of(2) {
        return Err(PerlError::runtime(
            "par_pipeline: expected pairs source => CODE, stages => [...], workers => [...], optional buffer => N",
            0,
        ));
    }
    let mut map: HashMap<String, PerlValue> = HashMap::new();
    for chunk in args.chunks(2) {
        let key = chunk[0].to_string();
        map.insert(key, chunk[1].clone());
    }
    let source = map
        .get("source")
        .and_then(|v| v.as_code_ref())
        .ok_or_else(|| PerlError::runtime("par_pipeline: source => CODE required", 0))?;
    let stages_val = map
        .get("stages")
        .ok_or_else(|| PerlError::runtime("par_pipeline: stages => ARRAY required", 0))?;
    let stages_items = list_from_value(stages_val);
    let mut stages: Vec<Arc<PerlSub>> = Vec::with_capacity(stages_items.len());
    for v in stages_items {
        let s = v
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("par_pipeline: each stage must be a CODE ref", 0))?;
        stages.push(s);
    }
    let workers_val = map
        .get("workers")
        .ok_or_else(|| PerlError::runtime("par_pipeline: workers => ARRAY required", 0))?;
    let workers_raw = list_from_value(workers_val);
    let workers: Vec<usize> = workers_raw
        .iter()
        .map(|v| v.to_int().max(1) as usize)
        .collect();
    if stages.is_empty() {
        return Err(PerlError::runtime(
            "par_pipeline: at least one stage required",
            0,
        ));
    }
    if workers.len() != stages.len() {
        return Err(PerlError::runtime(
            "par_pipeline: workers list must have the same length as stages",
            0,
        ));
    }
    let buffer = map
        .get("buffer")
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(256);
    Ok(ParPipelineSpec {
        source,
        stages,
        workers,
        buffer,
    })
}

fn flow_err_msg(e: FlowOrError) -> String {
    match e {
        FlowOrError::Error(pe) => pe.to_string(),
        FlowOrError::Flow(Flow::Return(_)) => "unexpected return in par_pipeline stage".into(),
        FlowOrError::Flow(f) => format!("unexpected control flow in par_pipeline: {:?}", f),
    }
}

#[allow(clippy::too_many_arguments)] // Thread entry: mirrors parallel stage wiring.
fn run_worker(
    sub: Arc<PerlSub>,
    subs: HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    rx: Receiver<PerlValue>,
    tx_out: Option<Sender<PerlValue>>,
    err: Arc<Mutex<Option<String>>>,
    last_stage_counter: Option<Arc<AtomicUsize>>,
) {
    while let Ok(item) = rx.recv() {
        if err.lock().is_some() {
            break;
        }
        let mut interp = Interpreter::new();
        interp.subs = subs.clone();
        interp.scope.restore_capture(&capture);
        interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
        if let Some(env) = sub.closure_env.as_ref() {
            interp.scope.restore_capture(env);
        }
        interp.enable_parallel_guard();
        let _ = interp.scope.set_scalar("_", item);
        interp.scope_push_hook();
        let out = match interp.exec_block_no_scope(&sub.body) {
            Ok(v) => v,
            Err(FlowOrError::Flow(Flow::Return(v))) => v,
            Err(e) => {
                interp.scope_pop_hook();
                let mut g = err.lock();
                if g.is_none() {
                    *g = Some(flow_err_msg(e));
                }
                break;
            }
        };
        interp.scope_pop_hook();
        if let Some(c) = &last_stage_counter {
            c.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(t) = &tx_out {
            if t.send(out).is_err() {
                let mut g = err.lock();
                if g.is_none() {
                    *g = Some("par_pipeline: downstream closed".into());
                }
                break;
            }
        }
    }
}

fn run_source(
    source: Arc<PerlSub>,
    subs: HashMap<String, Arc<PerlSub>>,
    capture: Vec<(String, PerlValue)>,
    atomic_arrays: Vec<(String, AtomicArray)>,
    atomic_hashes: Vec<(String, AtomicHash)>,
    tx: Sender<PerlValue>,
    err: Arc<Mutex<Option<String>>>,
) {
    let mut interp = Interpreter::new();
    interp.subs = subs.clone();
    interp.scope.restore_capture(&capture);
    interp.scope.restore_atomics(&atomic_arrays, &atomic_hashes);
    if let Some(env) = source.closure_env.as_ref() {
        interp.scope.restore_capture(env);
    }
    loop {
        if err.lock().is_some() {
            break;
        }
        // Like `ppool` workers: run the sub body with `exec_block_no_scope` so closure
        // state (e.g. `my $n` in `source => sub { ... }`) persists across pulls. `call_sub`
        // would push/pop frames each iteration and break that persistence.
        let v = match interp.exec_block_no_scope(&source.body) {
            Ok(v) => v,
            Err(FlowOrError::Flow(Flow::Return(v))) => v,
            Err(e) => {
                let mut g = err.lock();
                if g.is_none() {
                    *g = Some(flow_err_msg(e));
                }
                break;
            }
        };
        if v.is_undef() {
            break;
        }
        if tx.send(v).is_err() {
            let mut g = err.lock();
            if g.is_none() {
                *g = Some("par_pipeline: first stage stopped".into());
            }
            break;
        }
    }
}

/// Run a **batch** parallel pipeline: source generates all items, then each stage
/// processes the full batch via rayon before the next stage starts.
/// Returns the number of items processed by the **last** stage (scalar).
pub(crate) fn run_par_pipeline(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    use rayon::prelude::*;

    let spec = parse_args(args)?;
    let subs = interp.subs.clone();
    let (capture, atomic_arrays, atomic_hashes) = interp.scope.capture_with_atomics();

    // Phase 1: drain all items from source.
    let mut items = Vec::new();
    {
        let mut src_interp = Interpreter::new();
        src_interp.subs = subs.clone();
        src_interp.scope.restore_capture(&capture);
        src_interp
            .scope
            .restore_atomics(&atomic_arrays, &atomic_hashes);
        if let Some(env) = spec.source.closure_env.as_ref() {
            src_interp.scope.restore_capture(env);
        }
        loop {
            let v = match src_interp.exec_block_no_scope(&spec.source.body) {
                Ok(v) => v,
                Err(FlowOrError::Flow(Flow::Return(v))) => v,
                Err(e) => {
                    return Err(PerlError::runtime(flow_err_msg(e), line));
                }
            };
            if v.is_undef() {
                break;
            }
            items.push(v);
        }
    }

    // Phase 2: run each stage as a batch over all items.
    let mut err_msg: Option<String> = None;
    for stage_sub in &spec.stages {
        if err_msg.is_some() {
            break;
        }
        let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sub = Arc::clone(stage_sub);
        let subs_w = subs.clone();
        let cap_w = capture.clone();
        let aa_w = atomic_arrays.clone();
        let ah_w = atomic_hashes.clone();
        let err_w = Arc::clone(&first_err);
        items = items
            .into_par_iter()
            .map(|item| {
                if err_w.lock().is_some() {
                    return PerlValue::UNDEF;
                }
                let mut local_interp = Interpreter::new();
                local_interp.subs = subs_w.clone();
                local_interp.scope.restore_capture(&cap_w);
                local_interp.scope.restore_atomics(&aa_w, &ah_w);
                if let Some(env) = sub.closure_env.as_ref() {
                    local_interp.scope.restore_capture(env);
                }
                local_interp.enable_parallel_guard();
                let _ = local_interp.scope.set_scalar("_", item);
                local_interp.scope_push_hook();
                let out = match local_interp.exec_block_no_scope(&sub.body) {
                    Ok(v) => Ok(v),
                    Err(FlowOrError::Flow(Flow::Return(v))) => Ok(v),
                    Err(e) => Err(e),
                };
                local_interp.scope_pop_hook();
                match out {
                    Ok(v) => v,
                    Err(e) => {
                        let mut g = err_w.lock();
                        if g.is_none() {
                            *g = Some(flow_err_msg(e));
                        }
                        PerlValue::UNDEF
                    }
                }
            })
            .collect();
        err_msg = first_err.lock().take();
    }

    if let Some(msg) = err_msg {
        return Err(PerlError::runtime(msg, line));
    }
    Ok(PerlValue::integer(items.len() as i64))
}

/// Run a **streaming** parallel pipeline: items flow through bounded channels
/// between stages concurrently (order not preserved when a stage has multiple workers).
/// Returns the number of items processed by the **last** stage (scalar).
pub(crate) fn run_par_pipeline_streaming(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let spec = parse_args(args)?;
    let k = spec.stages.len();
    let cap = spec.buffer;
    let subs = interp.subs.clone();
    let (capture, atomic_arrays, atomic_hashes) = interp.scope.capture_with_atomics();

    let err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let processed = Arc::new(AtomicUsize::new(0));

    let mut txs: Vec<Sender<PerlValue>> = Vec::with_capacity(k);
    let mut rxs: Vec<Receiver<PerlValue>> = Vec::with_capacity(k);
    for _ in 0..k {
        let (tx, rx) = bounded(cap);
        txs.push(tx);
        rxs.push(rx);
    }

    let tx0 = txs.remove(0);
    let source = Arc::clone(&spec.source);
    let subs_s = subs.clone();
    let cap_s = capture.clone();
    let aa_s = atomic_arrays.clone();
    let ah_s = atomic_hashes.clone();
    let err_s = Arc::clone(&err);

    std::thread::scope(|scope| {
        scope.spawn(move || {
            run_source(source, subs_s, cap_s, aa_s, ah_s, tx0, err_s);
        });

        for (stage_idx, stage_sub) in spec.stages.iter().enumerate() {
            let wn = spec.workers[stage_idx];
            let rx = rxs[stage_idx].clone();
            let tx_out = if stage_idx + 1 < k {
                Some(txs[stage_idx].clone())
            } else {
                None
            };
            let last_ctr = if stage_idx + 1 == k {
                Some(Arc::clone(&processed))
            } else {
                None
            };
            let sub = Arc::clone(stage_sub);
            let subs_w = subs.clone();
            let cap_w = capture.clone();
            let aa_w = atomic_arrays.clone();
            let ah_w = atomic_hashes.clone();
            let err_w = Arc::clone(&err);
            for _ in 0..wn {
                let rx = rx.clone();
                let tx_out = tx_out.clone();
                let sub = Arc::clone(&sub);
                let subs_w = subs_w.clone();
                let cap_w = cap_w.clone();
                let aa_w = aa_w.clone();
                let ah_w = ah_w.clone();
                let err_w = Arc::clone(&err_w);
                let last_ctr = last_ctr.clone();
                scope.spawn(move || {
                    run_worker(sub, subs_w, cap_w, aa_w, ah_w, rx, tx_out, err_w, last_ctr);
                });
            }
        }
        txs.clear();
        rxs.clear();
    });

    if let Some(msg) = err.lock().take() {
        return Err(PerlError::runtime(msg, line));
    }
    let n = processed.load(Ordering::SeqCst);
    Ok(PerlValue::integer(n as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::PerlValue;
    use std::thread;

    /// Two-stage wiring must forward items (regression: multi-stage used to deadlock).
    #[test]
    fn two_stage_channel_forwarding() {
        let k = 2usize;
        let cap = 8usize;
        let mut txs: Vec<Sender<PerlValue>> = Vec::with_capacity(k);
        let mut rxs: Vec<Receiver<PerlValue>> = Vec::with_capacity(k);
        for _ in 0..k {
            let (tx, rx) = bounded(cap);
            txs.push(tx);
            rxs.push(rx);
        }
        let tx0 = txs.remove(0);
        let processed = Arc::new(AtomicUsize::new(0));
        let ctr = Arc::clone(&processed);

        thread::scope(|scope| {
            scope.spawn(move || {
                let _ = tx0.send(PerlValue::integer(7));
            });
            for stage_idx in 0..k {
                let rx = rxs[stage_idx].clone();
                let tx_out = if stage_idx + 1 < k {
                    Some(txs[stage_idx].clone())
                } else {
                    None
                };
                let last_ctr = if stage_idx + 1 == k {
                    Some(Arc::clone(&ctr))
                } else {
                    None
                };
                scope.spawn(move || {
                    while let Ok(item) = rx.recv() {
                        let out = item;
                        if let Some(c) = &last_ctr {
                            c.fetch_add(1, Ordering::SeqCst);
                        }
                        if let Some(t) = &tx_out {
                            let _ = t.send(out);
                        }
                    }
                });
            }
            txs.clear();
            rxs.clear();
        });

        assert_eq!(processed.load(Ordering::SeqCst), 1);
    }
}
