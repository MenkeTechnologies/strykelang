//! Typed message channels for parallel blocks (`pchannel`, `send`, `recv`, `pselect`).

use std::sync::Arc;
use std::time::Duration;

use crossbeam::channel::{self, Receiver, Select};

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

/// `pchannel()` — two-element list `(tx, rx)` for `my ($tx, $rx) = pchannel`.
pub fn create_pair() -> StrykeValue {
    let (tx, rx) = channel::unbounded();
    StrykeValue::array(vec![
        StrykeValue::channel_tx(Arc::new(tx)),
        StrykeValue::channel_rx(Arc::new(rx)),
    ])
}

/// `pchannel(N)` — bounded channel capacity `N`.
pub fn create_bounded_pair(capacity: usize) -> StrykeValue {
    let (tx, rx) = channel::bounded(capacity);
    StrykeValue::array(vec![
        StrykeValue::channel_tx(Arc::new(tx)),
        StrykeValue::channel_rx(Arc::new(rx)),
    ])
}

/// Multiplexed receive — `crossbeam_channel::Select` over several `pchannel` receivers.
/// Returns `(value, index)` where `index` is **0-based** (first argument is `0`), like Go's `select`.
pub fn pselect_recv(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime(
            "pselect() expects at least one pchannel receiver",
            line,
        ));
    }
    let mut rx_owned: Vec<Arc<Receiver<StrykeValue>>> = Vec::with_capacity(args.len());
    for v in args {
        if let Some(rx) = v.as_channel_rx() {
            rx_owned.push(rx);
        } else {
            return Err(StrykeError::runtime(
                "pselect() arguments must be pchannel receivers",
                line,
            ));
        }
    }
    let rx_refs: Vec<&Receiver<StrykeValue>> = rx_owned.iter().map(|a| a.as_ref()).collect();
    let mut sel = Select::new();
    for r in &rx_refs {
        sel.recv(r);
    }
    let oper = sel.select();
    let idx = oper.index();
    let val = match oper.recv(rx_refs[idx]) {
        Ok(v) => v,
        Err(_) => StrykeValue::UNDEF,
    };
    Ok(StrykeValue::array(vec![
        val,
        StrykeValue::integer(idx as i64),
    ]))
}

/// Like [`pselect_recv`], with optional overall timeout. On timeout returns `(undef, -1)`.
pub fn pselect_recv_with_optional_timeout(
    args: &[StrykeValue],
    timeout: Option<Duration>,
    line: usize,
) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime(
            "pselect() expects at least one pchannel receiver",
            line,
        ));
    }
    if timeout.is_none() {
        return pselect_recv(args, line);
    }
    let duration = timeout.unwrap();
    let mut rx_owned: Vec<Arc<Receiver<StrykeValue>>> = Vec::with_capacity(args.len());
    for v in args {
        if let Some(rx) = v.as_channel_rx() {
            rx_owned.push(rx);
        } else {
            return Err(StrykeError::runtime(
                "pselect() arguments must be pchannel receivers",
                line,
            ));
        }
    }
    let rx_refs: Vec<&Receiver<StrykeValue>> = rx_owned.iter().map(|a| a.as_ref()).collect();
    let mut sel = Select::new();
    for r in &rx_refs {
        sel.recv(r);
    }
    let oper = sel.select_timeout(duration);
    let Ok(oper) = oper else {
        return Ok(StrykeValue::array(vec![
            StrykeValue::UNDEF,
            StrykeValue::integer(-1),
        ]));
    };
    let idx = oper.index();
    let val = match oper.recv(rx_refs[idx]) {
        Ok(v) => v,
        Err(_) => StrykeValue::UNDEF,
    };
    Ok(StrykeValue::array(vec![
        val,
        StrykeValue::integer(idx as i64),
    ]))
}

/// `$tx->send($v)` and `$rx->recv` without package subs.
pub fn dispatch_method(
    receiver: &StrykeValue,
    method: &str,
    args: &[StrykeValue],
    line: usize,
) -> Option<StrykeResult<StrykeValue>> {
    if method == "send" {
        if let Some(tx) = receiver.as_channel_tx() {
            if args.len() != 1 {
                return Some(Err(StrykeError::runtime(
                    "send() on pchannel tx expects exactly one value",
                    line,
                )));
            }
            let ok = tx.send(args[0].clone()).is_ok();
            return Some(Ok(StrykeValue::integer(if ok { 1 } else { 0 })));
        }
    }
    if method == "recv" {
        if let Some(rx) = receiver.as_channel_rx() {
            if !args.is_empty() {
                return Some(Err(StrykeError::runtime(
                    "recv() on pchannel rx takes no arguments",
                    line,
                )));
            }
            return Some(Ok(match rx.recv() {
                Ok(v) => v,
                Err(_) => StrykeValue::UNDEF,
            }));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn pair_elems(pair: &StrykeValue) -> (StrykeValue, StrykeValue) {
        let v = pair.as_array_vec().expect("pchannel pair");
        assert_eq!(v.len(), 2);
        (v[0].clone(), v[1].clone())
    }

    #[test]
    fn create_pair_send_recv_roundtrip() {
        let pair = create_pair();
        let (tx, rx) = pair_elems(&pair);
        let sent = dispatch_method(&tx, "send", &[StrykeValue::integer(7)], 1)
            .expect("dispatch")
            .expect("send");
        assert_eq!(sent.to_int(), 1);
        let got = dispatch_method(&rx, "recv", &[], 1)
            .expect("dispatch")
            .expect("recv");
        assert_eq!(got.to_int(), 7);
    }

    #[test]
    fn create_bounded_pair_send_recv() {
        let pair = create_bounded_pair(2);
        let (tx, rx) = pair_elems(&pair);
        dispatch_method(&tx, "send", &[StrykeValue::integer(1)], 1)
            .unwrap()
            .unwrap();
        let v = dispatch_method(&rx, "recv", &[], 1).unwrap().unwrap();
        assert_eq!(v.to_int(), 1);
    }

    #[test]
    fn pselect_recv_empty_args_is_runtime_error() {
        let e = pselect_recv(&[], 1).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Runtime);
    }

    #[test]
    fn pselect_recv_rejects_non_receiver() {
        let e = pselect_recv(&[StrykeValue::integer(0)], 1).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Runtime);
    }

    #[test]
    fn pselect_recv_delivers_from_one_ready_channel() {
        let p = create_pair();
        let (tx, rx) = pair_elems(&p);
        dispatch_method(&tx, "send", &[StrykeValue::integer(99)], 1)
            .unwrap()
            .unwrap();
        let out = pselect_recv(&[rx], 1).expect("pselect");
        let row = out.as_array_vec().expect("result row");
        assert_eq!(row.len(), 2);
        assert_eq!(row[0].to_int(), 99);
        assert_eq!(row[1].to_int(), 0);
    }

    #[test]
    fn pselect_recv_with_timeout_times_out_when_empty() {
        let p = create_pair();
        let (_tx, rx) = pair_elems(&p);
        let out = pselect_recv_with_optional_timeout(&[rx], Some(Duration::from_millis(20)), 1)
            .expect("pselect");
        let row = out.as_array_vec().expect("result row");
        assert!(row[0].is_undef());
        assert_eq!(row[1].to_int(), -1);
    }

    #[test]
    fn dispatch_send_wrong_arity_is_error() {
        let pair = create_pair();
        let (tx, _rx) = pair_elems(&pair);
        let e = dispatch_method(&tx, "send", &[], 1)
            .expect("some")
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Runtime);
    }

    #[test]
    fn dispatch_recv_with_args_is_error() {
        let pair = create_pair();
        let (_tx, rx) = pair_elems(&pair);
        let e = dispatch_method(&rx, "recv", &[StrykeValue::integer(1)], 1)
            .expect("some")
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Runtime);
    }

    #[test]
    fn dispatch_unknown_method_returns_none() {
        let pair = create_pair();
        let (tx, _rx) = pair_elems(&pair);
        assert!(dispatch_method(&tx, "nope", &[], 1).is_none());
    }
}
