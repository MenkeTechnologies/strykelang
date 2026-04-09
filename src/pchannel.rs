//! Typed message channels for parallel blocks (`pchannel`, `send`, `recv`, `pselect`).

use std::sync::Arc;

use crossbeam::channel::{self, Receiver, Select};

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

/// `pchannel()` — two-element list `(tx, rx)` for `my ($tx, $rx) = pchannel`.
pub fn create_pair() -> PerlValue {
    let (tx, rx) = channel::unbounded();
    PerlValue::Array(vec![
        PerlValue::ChannelTx(Arc::new(tx)),
        PerlValue::ChannelRx(Arc::new(rx)),
    ])
}

/// Multiplexed receive — [`crossbeam_channel::Select`] over several `pchannel` receivers.
/// Returns `(value, index)` where `index` is **0-based** (first argument is `0`), like Go's `select`.
pub fn pselect_recv(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "pselect() expects at least one pchannel receiver",
            line,
        ));
    }
    let mut rx_refs: Vec<&Receiver<PerlValue>> = Vec::with_capacity(args.len());
    for v in args {
        match v {
            PerlValue::ChannelRx(rx) => rx_refs.push(rx.as_ref()),
            _ => {
                return Err(PerlError::runtime(
                    "pselect() arguments must be pchannel receivers",
                    line,
                ));
            }
        }
    }
    let mut sel = Select::new();
    for r in &rx_refs {
        sel.recv(r);
    }
    let oper = sel.select();
    let idx = oper.index();
    let val = match oper.recv(rx_refs[idx]) {
        Ok(v) => v,
        Err(_) => PerlValue::Undef,
    };
    Ok(PerlValue::Array(vec![val, PerlValue::Integer(idx as i64)]))
}

/// `$tx->send($v)` and `$rx->recv` without package subs.
pub fn dispatch_method(
    receiver: &PerlValue,
    method: &str,
    args: &[PerlValue],
    line: usize,
) -> Option<PerlResult<PerlValue>> {
    match (receiver, method) {
        (PerlValue::ChannelTx(tx), "send") => {
            if args.len() != 1 {
                return Some(Err(PerlError::runtime(
                    "send() on pchannel tx expects exactly one value",
                    line,
                )));
            }
            let ok = tx.send(args[0].clone()).is_ok();
            Some(Ok(PerlValue::Integer(if ok { 1 } else { 0 })))
        }
        (PerlValue::ChannelRx(rx), "recv") => {
            if !args.is_empty() {
                return Some(Err(PerlError::runtime(
                    "recv() on pchannel rx takes no arguments",
                    line,
                )));
            }
            Some(Ok(match rx.recv() {
                Ok(v) => v,
                Err(_) => PerlValue::Undef,
            }))
        }
        _ => None,
    }
}
