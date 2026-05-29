//! Quantitative builtins: technical indicators, time-series ops,
//! finance, optimization, numerical methods.
//! Pure functions; deliberately compact implementations.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arr(vs: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(vs)))
}

fn as_vec(v: &StrykeValue) -> Vec<f64> {
    if let Some(a) = v.as_array_ref() {
        return a.read().iter().map(|x| x.to_number()).collect();
    }
    if let Some(a) = v.as_array_vec() {
        return a.iter().map(|x| x.to_number()).collect();
    }
    Vec::new()
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    arr(v.into_iter().map(StrykeValue::float).collect())
}

// ══════════════════════════════════════════════════════════════════════
// Moving averages & technical indicators
// ══════════════════════════════════════════════════════════════════════

fn sma_compute(data: &[f64], period: usize) -> Vec<f64> {
    if period == 0 || period > data.len() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(data.len() - period + 1);
    let mut sum: f64 = data[..period].iter().sum();
    out.push(sum / period as f64);
    for i in period..data.len() {
        sum += data[i] - data[i - period];
        out.push(sum / period as f64);
    }
    out
}

fn ema_compute(data: &[f64], period: usize) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]);
    for i in 1..data.len() {
        out.push(alpha * data[i] + (1.0 - alpha) * out[i - 1]);
    }
    out
}
/// `sma` — see implementation.

pub fn sma(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(sma_compute(&data, p))
}
/// `ema` — see implementation.

pub fn ema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(ema_compute(&data, p))
}
/// `wma` — see implementation.

pub fn wma(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    if p > data.len() {
        return arr_f64(Vec::new());
    }
    let denom: f64 = (1..=p).sum::<usize>() as f64;
    let mut out = Vec::with_capacity(data.len() - p + 1);
    for i in 0..=data.len() - p {
        let win = &data[i..i + p];
        let weighted: f64 = win
            .iter()
            .enumerate()
            .map(|(j, x)| x * (j + 1) as f64)
            .sum();
        out.push(weighted / denom);
    }
    arr_f64(out)
}
/// `hma` — see implementation.

pub fn hma(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(2) as usize;
    let half = p / 2;
    let wma_half = wma(&[arr_f64(data.clone()), StrykeValue::integer(half as i64)]);
    let wma_full = wma(&[arr_f64(data.clone()), StrykeValue::integer(p as i64)]);
    let h = as_vec(&wma_half);
    let f = as_vec(&wma_full);
    let len = h.len().min(f.len());
    let diff: Vec<f64> = (0..len).map(|i| 2.0 * h[i] - f[i]).collect();
    let sqp = (p as f64).sqrt().round() as usize;
    arr_f64(wma_compute_raw(&diff, sqp))
}

fn wma_compute_raw(data: &[f64], p: usize) -> Vec<f64> {
    if p == 0 || p > data.len() {
        return Vec::new();
    }
    let denom: f64 = (1..=p).sum::<usize>() as f64;
    let mut out = Vec::with_capacity(data.len() - p + 1);
    for i in 0..=data.len() - p {
        let win = &data[i..i + p];
        let weighted: f64 = win
            .iter()
            .enumerate()
            .map(|(j, x)| x * (j + 1) as f64)
            .sum();
        out.push(weighted / denom);
    }
    out
}
/// `kama` — see implementation.

pub fn kama(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    if data.len() < p + 1 {
        return arr_f64(Vec::new());
    }
    let fast = 2.0 / 3.0;
    let slow = 2.0 / 31.0;
    let mut out = vec![data[0]];
    for i in 1..data.len() {
        let start = i.saturating_sub(p);
        let change = (data[i] - data[start]).abs();
        let volatility: f64 = (start + 1..=i).map(|j| (data[j] - data[j - 1]).abs()).sum();
        let er = if volatility > 1e-12 {
            change / volatility
        } else {
            0.0
        };
        let sc = (er * (fast - slow) + slow).powi(2);
        out.push(out[i - 1] + sc * (data[i] - out[i - 1]));
    }
    arr_f64(out)
}
/// `tema` — see implementation.

pub fn tema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    let e1 = ema_compute(&data, p);
    let e2 = ema_compute(&e1, p);
    let e3 = ema_compute(&e2, p);
    let len = e1.len().min(e2.len()).min(e3.len());
    let out: Vec<f64> = (0..len)
        .map(|i| 3.0 * e1[i] - 3.0 * e2[i] + e3[i])
        .collect();
    arr_f64(out)
}
/// `dema` — see implementation.

pub fn dema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    let e1 = ema_compute(&data, p);
    let e2 = ema_compute(&e1, p);
    let len = e1.len().min(e2.len());
    let out: Vec<f64> = (0..len).map(|i| 2.0 * e1[i] - e2[i]).collect();
    arr_f64(out)
}
/// `trix` — see implementation.

pub fn trix(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(15).max(1) as usize;
    let e1 = ema_compute(&data, p);
    let e2 = ema_compute(&e1, p);
    let e3 = ema_compute(&e2, p);
    if e3.len() < 2 {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = e3
        .windows(2)
        .map(|w| {
            if w[0] == 0.0 {
                0.0
            } else {
                100.0 * (w[1] - w[0]) / w[0]
            }
        })
        .collect();
    arr_f64(out)
}
/// `rsi` — see implementation.

pub fn rsi(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(14).max(1) as usize;
    if data.len() < p + 1 {
        return arr_f64(Vec::new());
    }
    let mut gains = Vec::with_capacity(data.len());
    let mut losses = Vec::with_capacity(data.len());
    gains.push(0.0);
    losses.push(0.0);
    for i in 1..data.len() {
        let d = data[i] - data[i - 1];
        gains.push(d.max(0.0));
        losses.push((-d).max(0.0));
    }
    let mut avg_g: f64 = gains[1..=p].iter().sum::<f64>() / p as f64;
    let mut avg_l: f64 = losses[1..=p].iter().sum::<f64>() / p as f64;
    let mut out = Vec::new();
    for i in p..data.len() {
        if i > p {
            avg_g = (avg_g * (p as f64 - 1.0) + gains[i]) / p as f64;
            avg_l = (avg_l * (p as f64 - 1.0) + losses[i]) / p as f64;
        }
        let rs = if avg_l == 0.0 {
            f64::INFINITY
        } else {
            avg_g / avg_l
        };
        out.push(100.0 - 100.0 / (1.0 + rs));
    }
    arr_f64(out)
}
/// `stoch_rsi` — see implementation.

pub fn stoch_rsi(args: &[StrykeValue]) -> StrykeValue {
    let r = rsi(args);
    let v = as_vec(&r);
    let p = arg_i64(args, 1).unwrap_or(14).max(1) as usize;
    if v.len() < p {
        return arr_f64(Vec::new());
    }
    let mut out = Vec::with_capacity(v.len() - p + 1);
    for i in 0..=v.len() - p {
        let win = &v[i..i + p];
        let mn = win.iter().cloned().fold(f64::INFINITY, f64::min);
        let mx = win.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let cur = win[win.len() - 1];
        out.push(if mx == mn {
            0.0
        } else {
            (cur - mn) / (mx - mn)
        });
    }
    arr_f64(out)
}
/// `macd` — see implementation.

pub fn macd(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let fast = arg_i64(args, 1).unwrap_or(12).max(1) as usize;
    let slow = arg_i64(args, 2).unwrap_or(26).max(1) as usize;
    let ef = ema_compute(&data, fast);
    let es = ema_compute(&data, slow);
    let len = ef.len().min(es.len());
    let macd_line: Vec<f64> = (0..len).map(|i| ef[i] - es[i]).collect();
    arr_f64(macd_line)
}
/// `macd_signal` — see implementation.

pub fn macd_signal(args: &[StrykeValue]) -> StrykeValue {
    let m = macd(args);
    let signal_p = arg_i64(args, 3).unwrap_or(9).max(1) as usize;
    arr_f64(ema_compute(&as_vec(&m), signal_p))
}
/// `macd_histogram` — see implementation.

pub fn macd_histogram(args: &[StrykeValue]) -> StrykeValue {
    let m = as_vec(&macd(args));
    let signal = as_vec(&macd_signal(args));
    let len = m.len().min(signal.len());
    arr_f64((0..len).map(|i| m[i] - signal[i]).collect())
}
/// `bollinger_upper` — see implementation.

pub fn bollinger_upper(args: &[StrykeValue]) -> StrykeValue {
    bollinger_band(args, 1.0)
}
/// `bollinger_lower` — see implementation.

pub fn bollinger_lower(args: &[StrykeValue]) -> StrykeValue {
    bollinger_band(args, -1.0)
}
/// `bollinger_middle` — see implementation.

pub fn bollinger_middle(args: &[StrykeValue]) -> StrykeValue {
    sma(args)
}

fn bollinger_band(args: &[StrykeValue], sign: f64) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    let k = arg_f64(args, 2).unwrap_or(2.0);
    let mid = sma_compute(&data, p);
    if mid.is_empty() {
        return arr_f64(Vec::new());
    }
    let mut out = Vec::with_capacity(mid.len());
    for (i, m) in mid.iter().enumerate() {
        let win = &data[i..i + p];
        let var = win.iter().map(|x| (x - m).powi(2)).sum::<f64>() / p as f64;
        let std = var.sqrt();
        out.push(m + sign * k * std);
    }
    arr_f64(out)
}
/// `keltner_upper` — see implementation.

pub fn keltner_upper(args: &[StrykeValue]) -> StrykeValue {
    keltner_band(args, 1.0)
}
/// `keltner_lower` — see implementation.

pub fn keltner_lower(args: &[StrykeValue]) -> StrykeValue {
    keltner_band(args, -1.0)
}

fn keltner_band(args: &[StrykeValue], sign: f64) -> StrykeValue {
    let highs = args.first().map(as_vec).unwrap_or_default();
    let lows = args.get(1).map(as_vec).unwrap_or_default();
    let closes = args.get(2).map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 3).unwrap_or(20).max(1) as usize;
    let k = arg_f64(args, 4).unwrap_or(2.0);
    let mid = ema_compute(&closes, p);
    let atr_v = atr_compute(&highs, &lows, &closes, p);
    let n = mid.len().min(atr_v.len());
    arr_f64((0..n).map(|i| mid[i] + sign * k * atr_v[i]).collect())
}
/// `donchian_upper` — see implementation.

pub fn donchian_upper(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if p > high.len() {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..=high.len() - p)
        .map(|i| {
            high[i..i + p]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max)
        })
        .collect();
    arr_f64(out)
}
/// `donchian_lower` — see implementation.

pub fn donchian_lower(args: &[StrykeValue]) -> StrykeValue {
    let low = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if p > low.len() {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..=low.len() - p)
        .map(|i| low[i..i + p].iter().cloned().fold(f64::INFINITY, f64::min))
        .collect();
    arr_f64(out)
}

fn atr_compute(high: &[f64], low: &[f64], close: &[f64], p: usize) -> Vec<f64> {
    let n = high.len().min(low.len()).min(close.len());
    if n < 2 || p == 0 {
        return Vec::new();
    }
    let mut tr = Vec::with_capacity(n);
    tr.push(high[0] - low[0]);
    for i in 1..n {
        let a = high[i] - low[i];
        let b = (high[i] - close[i - 1]).abs();
        let c = (low[i] - close[i - 1]).abs();
        tr.push(a.max(b).max(c));
    }
    // Wilder smoothing: seed = mean(TR[0..p]), then ATR[i] = (ATR[i-1]·(p-1) + TR[i]) / p.
    if n < p {
        return Vec::new();
    }
    let mut out = vec![0.0_f64; n];
    let seed: f64 = tr[..p].iter().sum::<f64>() / p as f64;
    out[p - 1] = seed;
    let pf = p as f64;
    for i in p..n {
        out[i] = (out[i - 1] * (pf - 1.0) + tr[i]) / pf;
    }
    out
}
/// `atr` — see implementation.

pub fn atr(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let low = args.get(1).map(as_vec).unwrap_or_else(|| high.clone());
    let close = args.get(2).map(as_vec).unwrap_or_else(|| high.clone());
    let p = arg_i64(args, 3).unwrap_or(14).max(1) as usize;
    arr_f64(atr_compute(&high, &low, &close, p))
}
/// `true_range` — see implementation.

pub fn true_range(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let low = args.get(1).map(as_vec).unwrap_or_else(|| high.clone());
    let close = args.get(2).map(as_vec).unwrap_or_else(|| high.clone());
    let n = high.len().min(low.len()).min(close.len());
    if n == 0 {
        return arr_f64(Vec::new());
    }
    let mut tr = Vec::with_capacity(n);
    tr.push(high[0] - low[0]);
    for i in 1..n {
        let a = high[i] - low[i];
        let b = (high[i] - close[i - 1]).abs();
        let c = (low[i] - close[i - 1]).abs();
        tr.push(a.max(b).max(c));
    }
    arr_f64(tr)
}

/// Wilder's Average Directional Index. Inputs: high, low, close arrays,
/// period (default 14). Computes +DM/-DM/TR per bar, applies Wilder
/// smoothing, derives ±DI, then DX = 100·|+DI − -DI|/(+DI + -DI), and
/// finally ADX = Wilder smoothing of DX. Returns one ADX value per bar
/// from index `p` onward; the warm-up period is zero-filled.
pub fn adx(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let low = args.get(1).map(as_vec).unwrap_or_default();
    let close = args.get(2).map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 3).unwrap_or(14).max(1) as usize;
    let n = high.len().min(low.len()).min(close.len());
    if n < p + 1 {
        return arr_f64(Vec::new());
    }
    let mut plus_dm = Vec::with_capacity(n);
    let mut minus_dm = Vec::with_capacity(n);
    let mut tr = Vec::with_capacity(n);
    plus_dm.push(0.0);
    minus_dm.push(0.0);
    tr.push(high[0] - low[0]);
    for i in 1..n {
        let up = high[i] - high[i - 1];
        let dn = low[i - 1] - low[i];
        plus_dm.push(if up > dn && up > 0.0 { up } else { 0.0 });
        minus_dm.push(if dn > up && dn > 0.0 { dn } else { 0.0 });
        tr.push(
            (high[i] - low[i])
                .max((high[i] - close[i - 1]).abs())
                .max((low[i] - close[i - 1]).abs()),
        );
    }
    // Wilder smoothing: seed = sum of first p, then x[t] = x[t-1] - x[t-1]/p + raw[t].
    let wilder = |raw: &[f64]| -> Vec<f64> {
        let mut out = vec![0.0_f64; raw.len()];
        if raw.len() <= p {
            return out;
        }
        let seed: f64 = raw[1..=p].iter().sum();
        out[p] = seed;
        for i in p + 1..raw.len() {
            out[i] = out[i - 1] - out[i - 1] / p as f64 + raw[i];
        }
        out
    };
    let sm_plus = wilder(&plus_dm);
    let sm_minus = wilder(&minus_dm);
    let sm_tr = wilder(&tr);
    let mut dx = vec![0.0_f64; n];
    for i in p..n {
        if sm_tr[i] > 1e-12 {
            let plus_di = 100.0 * sm_plus[i] / sm_tr[i];
            let minus_di = 100.0 * sm_minus[i] / sm_tr[i];
            let sum = plus_di + minus_di;
            if sum > 1e-12 {
                dx[i] = 100.0 * (plus_di - minus_di).abs() / sum;
            }
        }
    }
    let mut adx_out = vec![0.0_f64; n];
    if n >= 2 * p {
        let dx_seed: f64 = dx[p..2 * p].iter().sum::<f64>() / p as f64;
        adx_out[2 * p - 1] = dx_seed;
        for i in 2 * p..n {
            adx_out[i] = (adx_out[i - 1] * (p as f64 - 1.0) + dx[i]) / p as f64;
        }
    }
    arr_f64(adx_out[p..].to_vec())
}

/// Lambert's Commodity Channel Index. Inputs: highs, lows, closes,
/// period=20. Uses typical price `TP = (H + L + C) / 3` then
/// `CCI = (TP − SMA_p(TP)) / (0.015 · mean_dev_p)`.
pub fn cci(args: &[StrykeValue]) -> StrykeValue {
    let highs = args.first().map(as_vec).unwrap_or_default();
    let lows = args.get(1).map(as_vec).unwrap_or_default();
    let closes = args.get(2).map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 3).unwrap_or(20).max(1) as usize;
    let n = highs.len().min(lows.len()).min(closes.len());
    if n < p {
        return arr_f64(Vec::new());
    }
    let tp: Vec<f64> = (0..n)
        .map(|i| (highs[i] + lows[i] + closes[i]) / 3.0)
        .collect();
    let mid = sma_compute(&tp, p);
    if mid.is_empty() {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..mid.len())
        .map(|i| {
            let win = &tp[i..i + p];
            let mad: f64 = win.iter().map(|x| (x - mid[i]).abs()).sum::<f64>() / p as f64;
            if mad == 0.0 {
                0.0
            } else {
                (tp[i + p - 1] - mid[i]) / (0.015 * mad)
            }
        })
        .collect();
    arr_f64(out)
}
/// `roc` — see implementation.

pub fn roc(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    if data.len() <= p {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (p..data.len())
        .map(|i| {
            if data[i - p] == 0.0 {
                0.0
            } else {
                100.0 * (data[i] - data[i - p]) / data[i - p]
            }
        })
        .collect();
    arr_f64(out)
}
/// `momentum` — see implementation.

pub fn momentum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    if data.len() <= p {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (p..data.len()).map(|i| data[i] - data[i - p]).collect();
    arr_f64(out)
}
/// `williams_r` — see implementation.

pub fn williams_r(args: &[StrykeValue]) -> StrykeValue {
    let highs = args.first().map(as_vec).unwrap_or_default();
    let lows = args.get(1).map(as_vec).unwrap_or_default();
    let closes = args.get(2).map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 3).unwrap_or(14).max(1) as usize;
    let n = highs.len().min(lows.len()).min(closes.len());
    if n < p {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..=n - p)
        .map(|i| {
            let hi = highs[i..i + p]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);
            let lo = lows[i..i + p].iter().cloned().fold(f64::INFINITY, f64::min);
            let close = closes[i + p - 1];
            if hi == lo {
                0.0
            } else {
                -100.0 * (hi - close) / (hi - lo)
            }
        })
        .collect();
    arr_f64(out)
}
/// `obv` — see implementation.

pub fn obv(args: &[StrykeValue]) -> StrykeValue {
    let prices = args.first().map(as_vec).unwrap_or_default();
    let volumes = args.get(1).map(as_vec).unwrap_or_default();
    let n = prices.len().min(volumes.len());
    if n == 0 {
        return arr_f64(Vec::new());
    }
    let mut out = Vec::with_capacity(n);
    out.push(volumes[0]);
    for i in 1..n {
        let prev = out[i - 1];
        let v = if prices[i] > prices[i - 1] {
            prev + volumes[i]
        } else if prices[i] < prices[i - 1] {
            prev - volumes[i]
        } else {
            prev
        };
        out.push(v);
    }
    arr_f64(out)
}
/// `vwap` — see implementation.

pub fn vwap(args: &[StrykeValue]) -> StrykeValue {
    let prices = args.first().map(as_vec).unwrap_or_default();
    let volumes = args.get(1).map(as_vec).unwrap_or_default();
    let n = prices.len().min(volumes.len());
    if n == 0 {
        return arr_f64(Vec::new());
    }
    let mut out = Vec::with_capacity(n);
    let mut sum_pv = 0.0;
    let mut sum_v = 0.0;
    for i in 0..n {
        sum_pv += prices[i] * volumes[i];
        sum_v += volumes[i];
        out.push(if sum_v == 0.0 { 0.0 } else { sum_pv / sum_v });
    }
    arr_f64(out)
}
/// `twap` — see implementation.

pub fn twap(args: &[StrykeValue]) -> StrykeValue {
    let prices = args.first().map(as_vec).unwrap_or_default();
    if prices.is_empty() {
        return arr_f64(Vec::new());
    }
    let mut out = Vec::with_capacity(prices.len());
    let mut sum = 0.0;
    for (i, p) in prices.iter().enumerate() {
        sum += p;
        out.push(sum / (i + 1) as f64);
    }
    arr_f64(out)
}
/// `pivot_points` — see implementation.

pub fn pivot_points(args: &[StrykeValue]) -> StrykeValue {
    let high = arg_f64(args, 0).unwrap_or(0.0);
    let low = arg_f64(args, 1).unwrap_or(0.0);
    let close = arg_f64(args, 2).unwrap_or(0.0);
    let pp = (high + low + close) / 3.0;
    let r1 = 2.0 * pp - low;
    let s1 = 2.0 * pp - high;
    let r2 = pp + (high - low);
    let s2 = pp - (high - low);
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("pp".to_string(), StrykeValue::float(pp));
    h.insert("r1".to_string(), StrykeValue::float(r1));
    h.insert("s1".to_string(), StrykeValue::float(s1));
    h.insert("r2".to_string(), StrykeValue::float(r2));
    h.insert("s2".to_string(), StrykeValue::float(s2));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}
/// `fibonacci_retracement` — see implementation.

pub fn fibonacci_retracement(args: &[StrykeValue]) -> StrykeValue {
    let high = arg_f64(args, 0).unwrap_or(0.0);
    let low = arg_f64(args, 1).unwrap_or(0.0);
    let diff = high - low;
    let levels = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
    arr_f64(levels.iter().map(|f| high - diff * f).collect())
}
/// `fibonacci_extension` — see implementation.

pub fn fibonacci_extension(args: &[StrykeValue]) -> StrykeValue {
    let high = arg_f64(args, 0).unwrap_or(0.0);
    let low = arg_f64(args, 1).unwrap_or(0.0);
    let diff = high - low;
    let levels = [1.0, 1.272, 1.414, 1.618, 2.0, 2.618];
    arr_f64(levels.iter().map(|f| high + diff * (f - 1.0)).collect())
}

/// Wilder's Parabolic SAR (stop-and-reverse). Inputs:
/// `highs`, `lows`, `af_step=0.02`, `af_max=0.2`.
///
/// In an uptrend the extreme-point EP tracks the highest high since trend
/// onset; in a downtrend EP tracks the lowest low. SAR is updated by
/// `prev + af · (EP − prev)`, AF increments by `af_step` (capped at `af_max`)
/// on each new extreme, and the trend reverses when price crosses SAR.
pub fn parabolic_sar(args: &[StrykeValue]) -> StrykeValue {
    let highs = args.first().map(as_vec).unwrap_or_default();
    let lows = args.get(1).map(as_vec).unwrap_or_default();
    let n = highs.len().min(lows.len());
    if n < 2 {
        return arr_f64(Vec::new());
    }
    let af_step = arg_f64(args, 2).unwrap_or(0.02);
    let af_max = arg_f64(args, 3).unwrap_or(0.2);
    let mut sar = Vec::with_capacity(n);
    // Wilder seed: assume initial uptrend; SAR starts at first low, EP at first high.
    let mut bull = true;
    let mut sar_cur = lows[0];
    let mut ep = highs[0];
    let mut af = af_step;
    sar.push(sar_cur);
    for i in 1..n {
        let next_sar = sar_cur + af * (ep - sar_cur);
        // Clamp the new SAR so it cannot enter the previous two bars' range.
        let new_sar = if bull {
            let lo = lows[i - 1].min(if i >= 2 { lows[i - 2] } else { lows[i - 1] });
            next_sar.min(lo)
        } else {
            let hi = highs[i - 1].max(if i >= 2 { highs[i - 2] } else { highs[i - 1] });
            next_sar.max(hi)
        };
        // Update extreme + acceleration on new high (bull) / low (bear).
        let mut reversed = false;
        if bull {
            if highs[i] > ep {
                ep = highs[i];
                af = (af + af_step).min(af_max);
            }
            if lows[i] < new_sar {
                // Reverse to bear.
                bull = false;
                sar_cur = ep;
                ep = lows[i];
                af = af_step;
                reversed = true;
            }
        } else {
            if lows[i] < ep {
                ep = lows[i];
                af = (af + af_step).min(af_max);
            }
            if highs[i] > new_sar {
                bull = true;
                sar_cur = ep;
                ep = highs[i];
                af = af_step;
                reversed = true;
            }
        }
        if !reversed {
            sar_cur = new_sar;
        }
        sar.push(sar_cur);
    }
    arr_f64(sar)
}
/// `support_level` — see implementation.

pub fn support_level(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if data.len() < p {
        return StrykeValue::UNDEF;
    }
    let tail = &data[data.len() - p..];
    StrykeValue::float(tail.iter().cloned().fold(f64::INFINITY, f64::min))
}
/// `resistance_level` — see implementation.

pub fn resistance_level(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if data.len() < p {
        return StrykeValue::UNDEF;
    }
    let tail = &data[data.len() - p..];
    StrykeValue::float(tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
}
/// `trend_line` — see implementation.

pub fn trend_line(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let n = data.len();
    if n < 2 {
        return StrykeValue::UNDEF;
    }
    let sum_x = (0..n).map(|i| i as f64).sum::<f64>();
    let sum_y: f64 = data.iter().sum();
    let sum_xy: f64 = data.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
    let sum_xx = (0..n).map(|i| (i as f64).powi(2)).sum::<f64>();
    let n_f = n as f64;
    let slope = (n_f * sum_xy - sum_x * sum_y) / (n_f * sum_xx - sum_x.powi(2));
    let intercept = (sum_y - slope * sum_x) / n_f;
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("slope".to_string(), StrykeValue::float(slope));
    h.insert("intercept".to_string(), StrykeValue::float(intercept));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

// Candlestick patterns — each takes OHLC arrays and returns boolean per bar
fn cs_arr(args: &[StrykeValue], idx: usize) -> Vec<f64> {
    args.get(idx).map(as_vec).unwrap_or_default()
}
/// `candlestick_pattern_doji` — see implementation.

pub fn candlestick_pattern_doji(args: &[StrykeValue]) -> StrykeValue {
    let o = cs_arr(args, 0);
    let h = cs_arr(args, 1);
    let l = cs_arr(args, 2);
    let c = cs_arr(args, 3);
    let n = o.len().min(h.len()).min(l.len()).min(c.len());
    let out: Vec<StrykeValue> = (0..n)
        .map(|i| {
            let body = (c[i] - o[i]).abs();
            let range = h[i] - l[i];
            StrykeValue::integer(if range > 0.0 && body / range < 0.1 {
                1
            } else {
                0
            })
        })
        .collect();
    arr(out)
}
/// `candlestick_pattern_hammer` — see implementation.

pub fn candlestick_pattern_hammer(args: &[StrykeValue]) -> StrykeValue {
    let o = cs_arr(args, 0);
    let h = cs_arr(args, 1);
    let l = cs_arr(args, 2);
    let c = cs_arr(args, 3);
    let n = o.len().min(h.len()).min(l.len()).min(c.len());
    let out: Vec<StrykeValue> = (0..n)
        .map(|i| {
            let body = (c[i] - o[i]).abs();
            let lower = o[i].min(c[i]) - l[i];
            let upper = h[i] - o[i].max(c[i]);
            StrykeValue::integer(if lower > body * 2.0 && upper < body {
                1
            } else {
                0
            })
        })
        .collect();
    arr(out)
}
/// `candlestick_pattern_engulfing` — see implementation.

pub fn candlestick_pattern_engulfing(args: &[StrykeValue]) -> StrykeValue {
    let o = cs_arr(args, 0);
    let c = cs_arr(args, 3);
    let n = o.len().min(c.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 1..n {
        let bull = c[i] > o[i] && c[i - 1] < o[i - 1] && o[i] < c[i - 1] && c[i] > o[i - 1];
        let bear = c[i] < o[i] && c[i - 1] > o[i - 1] && o[i] > c[i - 1] && c[i] < o[i - 1];
        out[i] = StrykeValue::integer(if bull || bear { 1 } else { 0 });
    }
    arr(out)
}
/// `candlestick_pattern_morning_star` — see implementation.

pub fn candlestick_pattern_morning_star(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let down = c[i - 2] < o[i - 2];
        let star = (c[i - 1] - o[i - 1]).abs() < (o[i - 2] - c[i - 2]).abs() / 3.0;
        let up = c[i] > o[i] && c[i] > (o[i - 2] + c[i - 2]) / 2.0;
        out[i] = StrykeValue::integer(if down && star && up { 1 } else { 0 });
    }
    arr(out)
}
/// `candlestick_pattern_evening_star` — see implementation.

pub fn candlestick_pattern_evening_star(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let up = c[i - 2] > o[i - 2];
        let star = (c[i - 1] - o[i - 1]).abs() < (c[i - 2] - o[i - 2]).abs() / 3.0;
        let down = c[i] < o[i] && c[i] < (o[i - 2] + c[i - 2]) / 2.0;
        out[i] = StrykeValue::integer(if up && star && down { 1 } else { 0 });
    }
    arr(out)
}
/// `candlestick_pattern_three_white_soldiers` — see implementation.

pub fn candlestick_pattern_three_white_soldiers(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let ok = c[i - 2] > o[i - 2]
            && c[i - 1] > o[i - 1]
            && c[i] > o[i]
            && c[i] > c[i - 1]
            && c[i - 1] > c[i - 2];
        out[i] = StrykeValue::integer(if ok { 1 } else { 0 });
    }
    arr(out)
}
/// `candlestick_pattern_three_black_crows` — see implementation.

pub fn candlestick_pattern_three_black_crows(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let ok = c[i - 2] < o[i - 2]
            && c[i - 1] < o[i - 1]
            && c[i] < o[i]
            && c[i] < c[i - 1]
            && c[i - 1] < c[i - 2];
        out[i] = StrykeValue::integer(if ok { 1 } else { 0 });
    }
    arr(out)
}

// ══════════════════════════════════════════════════════════════════════
// Time-series / statistics
// ══════════════════════════════════════════════════════════════════════
/// `acf` — see implementation.

pub fn acf(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let max_lag = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    let n = data.len();
    if n < 2 {
        return arr_f64(Vec::new());
    }
    let mean = data.iter().sum::<f64>() / n as f64;
    let var: f64 = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let mut out = Vec::with_capacity(max_lag);
    for lag in 0..max_lag.min(n) {
        let cov: f64 = (0..n - lag)
            .map(|i| (data[i] - mean) * (data[i + lag] - mean))
            .sum::<f64>()
            / n as f64;
        out.push(if var == 0.0 { 0.0 } else { cov / var });
    }
    arr_f64(out)
}
/// `exponential_smoothing` — see implementation.

pub fn exponential_smoothing(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let alpha = arg_f64(args, 1).unwrap_or(0.3).clamp(0.0, 1.0);
    if data.is_empty() {
        return arr_f64(Vec::new());
    }
    let mut out = vec![data[0]];
    for i in 1..data.len() {
        out.push(alpha * data[i] + (1.0 - alpha) * out[i - 1]);
    }
    arr_f64(out)
}
/// `double_exponential_smoothing` — see implementation.

pub fn double_exponential_smoothing(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let alpha = arg_f64(args, 1).unwrap_or(0.3).clamp(0.0, 1.0);
    let beta = arg_f64(args, 2).unwrap_or(0.1).clamp(0.0, 1.0);
    if data.len() < 2 {
        return arr_f64(data);
    }
    let mut s = data[0];
    let mut b = data[1] - data[0];
    let mut out = vec![s];
    for i in 1..data.len() {
        let s_prev = s;
        s = alpha * data[i] + (1.0 - alpha) * (s + b);
        b = beta * (s - s_prev) + (1.0 - beta) * b;
        out.push(s);
    }
    arr_f64(out)
}
/// `detrend_linear` — see implementation.

pub fn detrend_linear(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let n = data.len();
    if n < 2 {
        return arr_f64(data);
    }
    let n_f = n as f64;
    let sum_x = (0..n).map(|i| i as f64).sum::<f64>();
    let sum_y: f64 = data.iter().sum();
    let sum_xy: f64 = data.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
    let sum_xx = (0..n).map(|i| (i as f64).powi(2)).sum::<f64>();
    let slope = (n_f * sum_xy - sum_x * sum_y) / (n_f * sum_xx - sum_x.powi(2));
    let intercept = (sum_y - slope * sum_x) / n_f;
    arr_f64(
        data.iter()
            .enumerate()
            .map(|(i, y)| y - (slope * i as f64 + intercept))
            .collect(),
    )
}
/// `remove_seasonality` — see implementation.

pub fn remove_seasonality(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let period = arg_i64(args, 1).unwrap_or(12).max(1) as usize;
    if data.len() < period {
        return arr_f64(data);
    }
    let mut seasonal = vec![0.0; period];
    let mut counts = vec![0u32; period];
    for (i, v) in data.iter().enumerate() {
        seasonal[i % period] += v;
        counts[i % period] += 1;
    }
    for i in 0..period {
        if counts[i] > 0 {
            seasonal[i] /= counts[i] as f64;
        }
    }
    arr_f64(
        data.iter()
            .enumerate()
            .map(|(i, v)| v - seasonal[i % period])
            .collect(),
    )
}
/// `add_seasonality` — see implementation.

pub fn add_seasonality(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let pattern = args.get(1).map(as_vec).unwrap_or_default();
    if pattern.is_empty() {
        return arr_f64(data);
    }
    arr_f64(
        data.iter()
            .enumerate()
            .map(|(i, v)| v + pattern[i % pattern.len()])
            .collect(),
    )
}
/// `hurst_exponent` — see implementation.

pub fn hurst_exponent(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let n = data.len();
    if n < 16 {
        return StrykeValue::UNDEF;
    }
    let rs_for_chunk = |chunk: &[f64]| -> Option<f64> {
        let m = chunk.len();
        if m < 2 {
            return None;
        }
        let mean = chunk.iter().sum::<f64>() / m as f64;
        let mut c = 0.0_f64;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for x in chunk {
            c += x - mean;
            if c < lo {
                lo = c;
            }
            if c > hi {
                hi = c;
            }
        }
        let range = hi - lo;
        let var = chunk.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / m as f64;
        let sd = var.sqrt();
        if sd <= 0.0 || range <= 0.0 {
            return None;
        }
        Some(range / sd)
    };
    let mut sizes: Vec<usize> = Vec::new();
    let mut s = 8_usize;
    while s <= n / 2 {
        sizes.push(s);
        s *= 2;
    }
    if sizes.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut log_n: Vec<f64> = Vec::new();
    let mut log_rs: Vec<f64> = Vec::new();
    for &size in &sizes {
        let chunks = n / size;
        if chunks == 0 {
            continue;
        }
        let mut sum = 0.0_f64;
        let mut k = 0_usize;
        for i in 0..chunks {
            let slice = &data[i * size..(i + 1) * size];
            if let Some(rs) = rs_for_chunk(slice) {
                sum += rs;
                k += 1;
            }
        }
        if k > 0 {
            log_n.push((size as f64).ln());
            log_rs.push((sum / k as f64).ln());
        }
    }
    let kn = log_n.len();
    if kn < 2 {
        return StrykeValue::UNDEF;
    }
    let mean_x = log_n.iter().sum::<f64>() / kn as f64;
    let mean_y = log_rs.iter().sum::<f64>() / kn as f64;
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for i in 0..kn {
        let dx = log_n[i] - mean_x;
        num += dx * (log_rs[i] - mean_y);
        den += dx * dx;
    }
    if den == 0.0 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(num / den)
}
/// `diff_series` — see implementation.

pub fn diff_series(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    arr_f64(data.windows(2).map(|w| w[1] - w[0]).collect())
}
/// `expanding_mean` — see implementation.

pub fn expanding_mean(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let mut sum = 0.0;
    arr_f64(
        data.iter()
            .enumerate()
            .map(|(i, v)| {
                sum += v;
                sum / (i + 1) as f64
            })
            .collect(),
    )
}
/// `expanding_sum` — see implementation.

pub fn expanding_sum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let mut sum = 0.0;
    arr_f64(
        data.iter()
            .map(|v| {
                sum += v;
                sum
            })
            .collect(),
    )
}

fn rolling_apply<F: Fn(&[f64]) -> f64>(data: &[f64], p: usize, f: F) -> Vec<f64> {
    if p == 0 || p > data.len() {
        return Vec::new();
    }
    data.windows(p).map(f).collect()
}
/// `rolling_mean` — see implementation.

pub fn rolling_mean(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        w.iter().sum::<f64>() / w.len() as f64
    }))
}
/// `rolling_sum` — see implementation.

pub fn rolling_sum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| w.iter().sum::<f64>()))
}
/// `rolling_std` — see implementation.

pub fn rolling_std(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let m = w.iter().sum::<f64>() / w.len() as f64;
        (w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
    }))
}
/// `rolling_var` — see implementation.

pub fn rolling_var(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let m = w.iter().sum::<f64>() / w.len() as f64;
        w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / w.len() as f64
    }))
}
/// `rolling_min` — see implementation.

pub fn rolling_min(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        w.iter().cloned().fold(f64::INFINITY, f64::min)
    }))
}
/// `rolling_max` — see implementation.

pub fn rolling_max(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        w.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }))
}
/// `rolling_median` — see implementation.

pub fn rolling_median(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let mut s = w.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        s[s.len() / 2]
    }))
}
/// `rolling_skew` — see implementation.

pub fn rolling_skew(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let n = w.len() as f64;
        let m = w.iter().sum::<f64>() / n;
        let var = w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n;
        let std = var.sqrt();
        if std == 0.0 {
            0.0
        } else {
            w.iter().map(|x| ((x - m) / std).powi(3)).sum::<f64>() / n
        }
    }))
}
/// `rolling_kurtosis` — see implementation.

pub fn rolling_kurtosis(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let n = w.len() as f64;
        let m = w.iter().sum::<f64>() / n;
        let var = w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n;
        if var == 0.0 {
            0.0
        } else {
            // Excess kurtosis: raw fourth-moment ratio minus 3 (Normal = 0).
            w.iter().map(|x| (x - m).powi(4)).sum::<f64>() / (n * var.powi(2)) - 3.0
        }
    }))
}
/// `shift_series` — see implementation.

pub fn shift_series(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let n = arg_i64(args, 1).unwrap_or(1);
    let len = data.len();
    if n.unsigned_abs() as usize >= len {
        return arr_f64(vec![0.0; len]);
    }
    let mut out = vec![0.0; len];
    if n > 0 {
        for i in n as usize..len {
            out[i] = data[i - n as usize];
        }
    } else {
        let n = (-n) as usize;
        out[..(len - n)].copy_from_slice(&data[n..]);
    }
    arr_f64(out)
}
/// `diff_pct` — see implementation.

pub fn diff_pct(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let out: Vec<f64> = data
        .windows(2)
        .map(|w| {
            if w[0] == 0.0 {
                0.0
            } else {
                (w[1] - w[0]) / w[0]
            }
        })
        .collect();
    arr_f64(out)
}
/// `log_returns` — see implementation.

pub fn log_returns(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    arr_f64(
        data.windows(2)
            .map(|w| {
                if w[0] <= 0.0 || w[1] <= 0.0 {
                    0.0
                } else {
                    (w[1] / w[0]).ln()
                }
            })
            .collect(),
    )
}
/// `volatility_annualized` — see implementation.

pub fn volatility_annualized(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|x| (x - m).powi(2)).sum::<f64>() / returns.len() as f64;
    StrykeValue::float(var.sqrt() * (252.0_f64).sqrt())
}
/// `sharpe` — see implementation.

pub fn sharpe(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let rf = arg_f64(args, 1).unwrap_or(0.0);
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    let excess = m - rf;
    let var = returns.iter().map(|x| (x - m).powi(2)).sum::<f64>() / returns.len() as f64;
    let std = var.sqrt();
    StrykeValue::float(if std == 0.0 {
        0.0
    } else {
        excess / std * (252.0_f64).sqrt()
    })
}

// ══════════════════════════════════════════════════════════════════════
// Finance helpers
// ══════════════════════════════════════════════════════════════════════
/// `present_value` — see implementation.

pub fn present_value(args: &[StrykeValue]) -> StrykeValue {
    let fv = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(fv / (1.0 + rate).powf(n))
}
/// `future_value` — see implementation.

pub fn future_value(args: &[StrykeValue]) -> StrykeValue {
    let pv = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(pv * (1.0 + rate).powf(n))
}
/// `net_present_value` — see implementation.

pub fn net_present_value(args: &[StrykeValue]) -> StrykeValue {
    let flows = args.first().map(as_vec).unwrap_or_default();
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let npv: f64 = flows
        .iter()
        .enumerate()
        .map(|(t, cf)| *cf / (1.0_f64 + rate).powi(t as i32))
        .sum();
    StrykeValue::float(npv)
}
/// `internal_rate_of_return` — see implementation.

pub fn internal_rate_of_return(args: &[StrykeValue]) -> StrykeValue {
    let flows = args.first().map(as_vec).unwrap_or_default();
    if flows.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut rate: f64 = 0.1;
    for _ in 0..100 {
        let npv: f64 = flows
            .iter()
            .enumerate()
            .map(|(t, cf)| *cf / (1.0_f64 + rate).powi(t as i32))
            .sum();
        let dnpv: f64 = flows
            .iter()
            .enumerate()
            .map(|(t, cf)| -(t as f64) * *cf / (1.0_f64 + rate).powi(t as i32 + 1))
            .sum();
        if dnpv.abs() < 1e-12 {
            break;
        }
        let new_rate = rate - npv / dnpv;
        if (new_rate - rate).abs() < 1e-9 {
            return StrykeValue::float(new_rate);
        }
        rate = new_rate;
    }
    StrykeValue::float(rate)
}
/// `yield_to_maturity` — see implementation.

pub fn yield_to_maturity(args: &[StrykeValue]) -> StrykeValue {
    // Solve P = Σ C/(1+r)^t + F/(1+r)^n for r via Newton-Raphson.
    // Seeded with the Linder approximation; bails to bisection if Newton diverges.
    let price = arg_f64(args, 0).unwrap_or(0.0);
    let face = arg_f64(args, 1).unwrap_or(100.0);
    let coupon = arg_f64(args, 2).unwrap_or(0.0);
    let n = arg_f64(args, 3).unwrap_or(1.0);
    if price <= 0.0 || n <= 0.0 {
        return StrykeValue::UNDEF;
    }
    let pv = |r: f64| -> f64 {
        let one_plus_r = 1.0 + r;
        if one_plus_r <= 0.0 {
            return f64::INFINITY;
        }
        let mut s = 0.0_f64;
        let mut t = 1.0_f64;
        let n_int = n as i64;
        let mut k = 0_i64;
        while k < n_int {
            t *= one_plus_r;
            s += coupon / t;
            k += 1;
        }
        s += face / t;
        s
    };
    let dpv = |r: f64| -> f64 {
        let one_plus_r = 1.0 + r;
        if one_plus_r <= 0.0 {
            return 0.0;
        }
        let mut s = 0.0_f64;
        let n_int = n as i64;
        for k in 1..=n_int {
            let t = one_plus_r.powi(k as i32);
            s -= (k as f64) * coupon / (t * one_plus_r);
        }
        let tn = one_plus_r.powi(n_int as i32);
        s -= n * face / (tn * one_plus_r);
        s
    };
    let mut r = (coupon + (face - price) / n) / ((face + price) / 2.0);
    if !r.is_finite() {
        r = 0.05;
    }
    for _ in 0..50 {
        let f = pv(r) - price;
        let fp = dpv(r);
        if fp.abs() < 1e-12 {
            break;
        }
        let step = f / fp;
        let new_r = r - step;
        if !new_r.is_finite() {
            break;
        }
        r = new_r.clamp(-0.99, 10.0);
        if step.abs() < 1e-10 {
            return StrykeValue::float(r);
        }
    }
    let mut lo = -0.99_f64;
    let mut hi = 10.0_f64;
    let f_lo = pv(lo) - price;
    if f_lo.is_nan() {
        return StrykeValue::float(r);
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fm = pv(mid) - price;
        if fm.abs() < 1e-10 || (hi - lo) < 1e-12 {
            return StrykeValue::float(mid);
        }
        if (fm > 0.0) == (f_lo > 0.0) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    StrykeValue::float((lo + hi) / 2.0)
}
/// `duration_macaulay` — see implementation.

pub fn duration_macaulay(args: &[StrykeValue]) -> StrykeValue {
    let flows = args.first().map(as_vec).unwrap_or_default();
    let rate = arg_f64(args, 1).unwrap_or(0.05);
    let mut num = 0.0;
    let mut den = 0.0;
    for (t, cf) in flows.iter().enumerate() {
        let disc = cf / (1.0 + rate).powi(t as i32 + 1);
        num += (t as f64 + 1.0) * disc;
        den += disc;
    }
    StrykeValue::float(if den == 0.0 { 0.0 } else { num / den })
}
/// `duration_modified` — see implementation.

pub fn duration_modified(args: &[StrykeValue]) -> StrykeValue {
    let d = duration_macaulay(args).to_number();
    let rate = arg_f64(args, 1).unwrap_or(0.05);
    StrykeValue::float(d / (1.0 + rate))
}
/// `convexity` — see implementation.

pub fn convexity(args: &[StrykeValue]) -> StrykeValue {
    let flows = args.first().map(as_vec).unwrap_or_default();
    let rate = arg_f64(args, 1).unwrap_or(0.05);
    let mut num = 0.0;
    let mut den = 0.0;
    for (t, cf) in flows.iter().enumerate() {
        let t_f = t as f64 + 1.0;
        let disc = cf / (1.0 + rate).powi(t as i32 + 1);
        num += t_f * (t_f + 1.0) * disc;
        den += disc;
    }
    StrykeValue::float(if den == 0.0 {
        0.0
    } else {
        num / (den * (1.0 + rate).powi(2))
    })
}
/// `break_even_qty` — see implementation.

pub fn break_even_qty(args: &[StrykeValue]) -> StrykeValue {
    let fixed = arg_f64(args, 0).unwrap_or(0.0);
    let price = arg_f64(args, 1).unwrap_or(0.0);
    let variable = arg_f64(args, 2).unwrap_or(0.0);
    let margin = price - variable;
    StrykeValue::float(if margin == 0.0 { 0.0 } else { fixed / margin })
}
/// `break_even_price` — see implementation.

pub fn break_even_price(args: &[StrykeValue]) -> StrykeValue {
    let fixed = arg_f64(args, 0).unwrap_or(0.0);
    let qty = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    let variable = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(fixed / qty + variable)
}
/// `profit_margin_pct` — see implementation.

pub fn profit_margin_pct(args: &[StrykeValue]) -> StrykeValue {
    let revenue = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let cost = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((revenue - cost) / revenue * 100.0)
}
/// `markup_pct` — see implementation.

pub fn markup_pct(args: &[StrykeValue]) -> StrykeValue {
    let cost = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let price = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((price - cost) / cost * 100.0)
}
/// `discount_pct` — see implementation.

pub fn discount_pct(args: &[StrykeValue]) -> StrykeValue {
    let original = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let sale = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((original - sale) / original * 100.0)
}
/// `loan_payment` — see implementation.

pub fn loan_payment(args: &[StrykeValue]) -> StrykeValue {
    let principal = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    if rate == 0.0 || n == 0.0 {
        return StrykeValue::float(if n > 0.0 { principal / n } else { 0.0 });
    }
    let pmt = principal * rate / (1.0 - (1.0 + rate).powf(-n));
    StrykeValue::float(pmt)
}
/// `loan_remaining` — see implementation.

pub fn loan_remaining(args: &[StrykeValue]) -> StrykeValue {
    let principal = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    let paid = arg_f64(args, 3).unwrap_or(0.0);
    let pmt = loan_payment(args).to_number();
    let remaining =
        principal * (1.0 + rate).powf(paid) - pmt * ((1.0 + rate).powf(paid) - 1.0) / rate;
    let _ = n;
    StrykeValue::float(remaining)
}
/// `loan_interest_total` — see implementation.

pub fn loan_interest_total(args: &[StrykeValue]) -> StrykeValue {
    let principal = arg_f64(args, 0).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    let pmt = loan_payment(args).to_number();
    StrykeValue::float(pmt * n - principal)
}
/// `roi` — see implementation.

pub fn roi(args: &[StrykeValue]) -> StrykeValue {
    let gain = arg_f64(args, 0).unwrap_or(0.0);
    let cost = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    StrykeValue::float((gain - cost) / cost * 100.0)
}
/// `cagr` — see implementation.

pub fn cagr(args: &[StrykeValue]) -> StrykeValue {
    let start = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let end = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    let years = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    StrykeValue::float((end / start).powf(1.0 / years) - 1.0)
}
/// `sortino` — see implementation.

pub fn sortino(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let target = arg_f64(args, 1).unwrap_or(0.0);
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    let downside: f64 = returns
        .iter()
        .map(|r| {
            if *r < target {
                (r - target).powi(2)
            } else {
                0.0
            }
        })
        .sum::<f64>()
        / returns.len() as f64;
    let dd = downside.sqrt();
    StrykeValue::float(if dd == 0.0 { 0.0 } else { (m - target) / dd })
}
/// `treynor` — see implementation.

pub fn treynor(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    let rf = arg_f64(args, 2).unwrap_or(0.0);
    if returns.is_empty() || beta == 0.0 {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    StrykeValue::float((m - rf) / beta)
}
/// `jensen_alpha` — see implementation.

pub fn jensen_alpha(args: &[StrykeValue]) -> StrykeValue {
    let port_ret = arg_f64(args, 0).unwrap_or(0.0);
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    let market_ret = arg_f64(args, 2).unwrap_or(0.0);
    let rf = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float(port_ret - (rf + beta * (market_ret - rf)))
}
/// `information_ratio` — see implementation.

pub fn information_ratio(args: &[StrykeValue]) -> StrykeValue {
    let port = args.first().map(as_vec).unwrap_or_default();
    let bench = args.get(1).map(as_vec).unwrap_or_default();
    let n = port.len().min(bench.len());
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let active: Vec<f64> = (0..n).map(|i| port[i] - bench[i]).collect();
    let m = active.iter().sum::<f64>() / n as f64;
    let var = active.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n as f64;
    let std = var.sqrt();
    StrykeValue::float(if std == 0.0 { 0.0 } else { m / std })
}
/// `calmar_ratio` — see implementation.

pub fn calmar_ratio(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let total: f64 = returns.iter().sum();
    // Max drawdown
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0f64;
    let mut cum = 0.0;
    for r in &returns {
        cum += r;
        if cum > peak {
            peak = cum;
        }
        let dd = peak - cum;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    StrykeValue::float(if max_dd == 0.0 { 0.0 } else { total / max_dd })
}
/// `omega_ratio` — see implementation.

pub fn omega_ratio(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let threshold = arg_f64(args, 1).unwrap_or(0.0);
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let gains: f64 = returns
        .iter()
        .filter(|r| **r > threshold)
        .map(|r| r - threshold)
        .sum();
    let losses: f64 = returns
        .iter()
        .filter(|r| **r < threshold)
        .map(|r| threshold - r)
        .sum();
    StrykeValue::float(if losses == 0.0 {
        f64::INFINITY
    } else {
        gains / losses
    })
}
/// `ulcer_index` — see implementation.

pub fn ulcer_index(args: &[StrykeValue]) -> StrykeValue {
    let prices = args.first().map(as_vec).unwrap_or_default();
    if prices.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let mut peak = prices[0];
    let mut sum_sq = 0.0;
    for p in &prices {
        peak = peak.max(*p);
        let dd_pct = (p - peak) / peak * 100.0;
        sum_sq += dd_pct.powi(2);
    }
    StrykeValue::float((sum_sq / prices.len() as f64).sqrt())
}

// ══════════════════════════════════════════════════════════════════════
// Optimization / numerical methods
// ══════════════════════════════════════════════════════════════════════
/// `trapezoidal_integrate` — see implementation.

pub fn trapezoidal_integrate(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let dx = arg_f64(args, 1).unwrap_or(1.0);
    if xs.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = xs.windows(2).map(|w| (w[0] + w[1]) / 2.0 * dx).sum();
    StrykeValue::float(sum)
}
/// `simpson_integrate` — see implementation.

pub fn simpson_integrate(args: &[StrykeValue]) -> StrykeValue {
    // Composite Simpson's 1/3 over even intervals; if (n−1) is odd, apply
    // Simpson's 3/8 to the last three intervals so the rule is exact for
    // any number of points ≥ 3 (not just odd n).
    let xs = args.first().map(as_vec).unwrap_or_default();
    let dx = arg_f64(args, 1).unwrap_or(1.0);
    let n = xs.len();
    if n < 3 {
        return trapezoidal_integrate(args);
    }
    let intervals = n - 1;
    let end_idx = if intervals.is_multiple_of(2) {
        n
    } else {
        n - 3
    };
    let mut sum = xs[0] + xs[end_idx - 1];
    for i in 1..end_idx - 1 {
        sum += if i % 2 == 1 { 4.0 * xs[i] } else { 2.0 * xs[i] };
    }
    let mut result = sum * dx / 3.0;
    if intervals % 2 == 1 {
        // Simpson's 3/8 on the trailing 3 intervals [n-4 .. n-1].
        let a = xs[n - 4];
        let b = xs[n - 3];
        let c = xs[n - 2];
        let d = xs[n - 1];
        result += 3.0 * dx / 8.0 * (a + 3.0 * b + 3.0 * c + d);
    }
    StrykeValue::float(result)
}
/// `ode_euler` — see implementation.

pub fn ode_euler(args: &[StrykeValue]) -> StrykeValue {
    // Explicit Euler integration over a pre-sampled derivative array `f`:
    //   y[0] = y0
    //   y[i+1] = y[i] + dt · f[i]
    // Returns y[0..=len(f)].
    let derivs = args.first().map(as_vec).unwrap_or_default();
    let dt = arg_f64(args, 1).unwrap_or(0.01);
    let y0 = arg_f64(args, 2).unwrap_or(0.0);
    if derivs.is_empty() {
        return arr_f64(vec![y0]);
    }
    let mut out = Vec::with_capacity(derivs.len() + 1);
    out.push(y0);
    for f in &derivs {
        out.push(out.last().unwrap() + dt * f);
    }
    arr_f64(out)
}
/// `finite_difference_forward` — see implementation.

pub fn finite_difference_forward(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let h = arg_f64(args, 1).unwrap_or(1.0);
    arr_f64(xs.windows(2).map(|w| (w[1] - w[0]) / h).collect())
}
/// `finite_difference_central` — see implementation.

pub fn finite_difference_central(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let h = arg_f64(args, 1).unwrap_or(1.0);
    if xs.len() < 3 {
        return arr_f64(Vec::new());
    }
    arr_f64(
        (1..xs.len() - 1)
            .map(|i| (xs[i + 1] - xs[i - 1]) / (2.0 * h))
            .collect(),
    )
}
/// `interp_linear` — see implementation.

pub fn interp_linear(args: &[StrykeValue]) -> StrykeValue {
    let x_vals = args.first().map(as_vec).unwrap_or_default();
    let y_vals = args.get(1).map(as_vec).unwrap_or_default();
    let x = arg_f64(args, 2).unwrap_or(0.0);
    let n = x_vals.len().min(y_vals.len());
    if n < 2 {
        return StrykeValue::UNDEF;
    }
    if x <= x_vals[0] {
        return StrykeValue::float(y_vals[0]);
    }
    if x >= x_vals[n - 1] {
        return StrykeValue::float(y_vals[n - 1]);
    }
    for i in 0..n - 1 {
        if x >= x_vals[i] && x <= x_vals[i + 1] {
            let t = (x - x_vals[i]) / (x_vals[i + 1] - x_vals[i]);
            return StrykeValue::float(y_vals[i] + t * (y_vals[i + 1] - y_vals[i]));
        }
    }
    StrykeValue::UNDEF
}
/// `interp_lagrange` — see implementation.

pub fn interp_lagrange(args: &[StrykeValue]) -> StrykeValue {
    let x_vals = args.first().map(as_vec).unwrap_or_default();
    let y_vals = args.get(1).map(as_vec).unwrap_or_default();
    let x = arg_f64(args, 2).unwrap_or(0.0);
    let n = x_vals.len().min(y_vals.len());
    if n == 0 {
        return StrykeValue::UNDEF;
    }
    let mut result = 0.0;
    for i in 0..n {
        let mut term = y_vals[i];
        for j in 0..n {
            if i != j {
                term *= (x - x_vals[j]) / (x_vals[i] - x_vals[j]);
            }
        }
        result += term;
    }
    StrykeValue::float(result)
}
/// `fit_curve_least_squares` — see implementation.

pub fn fit_curve_least_squares(args: &[StrykeValue]) -> StrykeValue {
    // Fit y = a + b*x via OLS, returns {a, b}
    let x_vals = args.first().map(as_vec).unwrap_or_default();
    let y_vals = args.get(1).map(as_vec).unwrap_or_default();
    let n = x_vals.len().min(y_vals.len());
    if n < 2 {
        return StrykeValue::UNDEF;
    }
    let n_f = n as f64;
    let sum_x: f64 = x_vals.iter().take(n).sum();
    let sum_y: f64 = y_vals.iter().take(n).sum();
    let sum_xy: f64 = (0..n).map(|i| x_vals[i] * y_vals[i]).sum();
    let sum_xx: f64 = x_vals.iter().take(n).map(|x| x * x).sum();
    let b = (n_f * sum_xy - sum_x * sum_y) / (n_f * sum_xx - sum_x.powi(2));
    let a = (sum_y - b * sum_x) / n_f;
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("a".to_string(), StrykeValue::float(a));
    h.insert("b".to_string(), StrykeValue::float(b));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    fn data(v: &[f64]) -> StrykeValue {
        arr_f64(v.to_vec())
    }

    fn i(n: i64) -> StrykeValue {
        StrykeValue::integer(n)
    }

    // ─── sma_compute: simple moving average ────────────────────────────

    #[test]
    fn sma_period_one_returns_input_unchanged() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(sma_compute(&data, 1), data);
    }

    #[test]
    fn sma_constant_input_returns_constant_output() {
        let out = sma_compute(&[5.0; 10], 3);
        for v in &out {
            assert!(approx(*v, 5.0, 1e-9));
        }
    }

    #[test]
    fn sma_known_window_average() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        // period=3: windows [1,2,3]=2, [2,3,4]=3, [3,4,5]=4.
        let out = sma_compute(&data, 3);
        assert_eq!(out, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn sma_zero_period_returns_empty() {
        assert!(sma_compute(&[1.0, 2.0], 0).is_empty());
    }

    #[test]
    fn sma_period_larger_than_data_returns_empty() {
        assert!(sma_compute(&[1.0, 2.0], 5).is_empty());
    }

    #[test]
    fn sma_output_length_is_data_minus_period_plus_one() {
        let data: Vec<f64> = (1..=20).map(|i| i as f64).collect();
        assert_eq!(sma_compute(&data, 5).len(), 16);
        assert_eq!(sma_compute(&data, 1).len(), 20);
        assert_eq!(sma_compute(&data, 20).len(), 1);
    }

    // ─── ema_compute: exponential moving average ───────────────────────

    #[test]
    fn ema_first_value_equals_first_input() {
        let out = ema_compute(&[10.0, 20.0, 30.0], 5);
        assert_eq!(out[0], 10.0);
    }

    #[test]
    fn ema_constant_input_returns_constant_output() {
        let out = ema_compute(&[7.0; 20], 10);
        for v in &out {
            assert!(approx(*v, 7.0, 1e-12));
        }
    }

    #[test]
    fn ema_output_length_matches_input_length() {
        let data: Vec<f64> = (1..=50).map(|i| i as f64).collect();
        assert_eq!(ema_compute(&data, 10).len(), 50);
    }

    #[test]
    fn ema_responds_more_to_recent_values_than_sma() {
        // After a step jump, EMA tracks faster than SMA in the next bar.
        let mut data = vec![100.0; 10];
        data.push(200.0); // big jump on bar 11
        let ema = ema_compute(&data, 5);
        let sma = sma_compute(&data, 5);
        // EMA at the jump bar absorbs more of the new value than SMA does.
        assert!(ema[10] > sma[*&sma.len() - 1]);
    }

    #[test]
    fn ema_empty_input_returns_empty() {
        assert!(ema_compute(&[], 10).is_empty());
    }

    // ─── wma_compute_raw: weighted moving average ──────────────────────

    #[test]
    fn wma_period_one_returns_input_unchanged() {
        let data = vec![1.0, 2.0, 3.0];
        assert_eq!(wma_compute_raw(&data, 1), data);
    }

    #[test]
    fn wma_zero_period_returns_empty() {
        assert!(wma_compute_raw(&[1.0, 2.0], 0).is_empty());
    }

    #[test]
    fn wma_period_too_large_returns_empty() {
        assert!(wma_compute_raw(&[1.0, 2.0], 5).is_empty());
    }

    #[test]
    fn wma_known_value() {
        // Period 3: weights = 1, 2, 3. Denominator = 6.
        // Window [1, 2, 3] → (1*1 + 2*2 + 3*3) / 6 = (1+4+9)/6 = 14/6 ≈ 2.333.
        let out = wma_compute_raw(&[1.0, 2.0, 3.0], 3);
        assert_eq!(out.len(), 1);
        assert!(approx(out[0], 14.0 / 6.0, 1e-9));
    }

    // ─── Public sma/ema builtin wrappers ──────────────────────────────

    #[test]
    fn sma_builtin_with_default_period() {
        // Default period is 10 — too small a dataset → empty array.
        let v = sma(&[data(&[1.0, 2.0, 3.0])]);
        let out = as_vec(&v);
        assert!(out.is_empty(), "default period 10 > data len 3");
    }

    #[test]
    fn sma_builtin_with_explicit_period_three() {
        let v = sma(&[data(&[1.0, 2.0, 3.0, 4.0, 5.0]), i(3)]);
        let out = as_vec(&v);
        assert_eq!(out, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn sma_builtin_clamps_period_zero_to_one() {
        // .max(1) in the wrapper protects from period=0.
        let v = sma(&[data(&[1.0, 2.0, 3.0]), i(0)]);
        let out = as_vec(&v);
        assert_eq!(out, vec![1.0, 2.0, 3.0], "period 0 is clamped to 1");
    }

    #[test]
    fn ema_builtin_returns_input_length_array() {
        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let v = ema(&[data(&input), i(2)]);
        assert_eq!(as_vec(&v).len(), input.len());
    }

    // ─── trix: triple-EMA momentum oscillator ──────────────────────────

    #[test]
    fn trix_constant_input_is_all_zero() {
        // EMA of constant = constant → consecutive deltas = 0 → TRIX = 0.
        let v = trix(&[data(&[100.0; 30]), i(5)]);
        let out = as_vec(&v);
        for x in &out {
            assert!(
                approx(*x, 0.0, 1e-9),
                "TRIX of constant should be 0, got {x}"
            );
        }
    }

    #[test]
    fn trix_strong_uptrend_eventually_positive() {
        let input: Vec<f64> = (1..=50).map(|i| 100.0 + i as f64).collect();
        let v = trix(&[data(&input), i(5)]);
        let out = as_vec(&v);
        assert!(out[out.len() - 1] > 0.0, "uptrend → TRIX positive at end");
    }

    // ─── rsi: relative strength index in [0, 100] ──────────────────────

    #[test]
    fn rsi_strict_uptrend_approaches_100() {
        let input: Vec<f64> = (1..=30).map(|i| 100.0 + i as f64).collect();
        let v = rsi(&[data(&input), i(14)]);
        let out = as_vec(&v);
        // Strict uptrend → no losses → RSI = 100.
        for x in &out {
            assert!(approx(*x, 100.0, 1e-9));
        }
    }

    #[test]
    fn rsi_strict_downtrend_approaches_zero() {
        let input: Vec<f64> = (1..=30).map(|i| 200.0 - i as f64).collect();
        let v = rsi(&[data(&input), i(14)]);
        let out = as_vec(&v);
        // Strict downtrend → no gains → RSI = 0.
        for x in &out {
            assert!(approx(*x, 0.0, 1e-9), "downtrend → RSI ~ 0, got {x}");
        }
    }

    #[test]
    fn rsi_short_data_returns_empty() {
        let v = rsi(&[data(&[1.0, 2.0]), i(14)]);
        assert!(as_vec(&v).is_empty());
    }

    // ─── macd: MACD line as EMA(fast) - EMA(slow) ─────────────────────

    #[test]
    fn macd_length_matches_short_ema() {
        let input: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let v = macd(&[data(&input), i(12), i(26)]);
        // EMAs are full-length over the input; macd is their elementwise
        // difference → also full input length.
        assert_eq!(as_vec(&v).len(), input.len());
    }

    #[test]
    fn macd_constant_input_yields_zero_line() {
        let v = macd(&[data(&[50.0; 100]), i(12), i(26)]);
        let out = as_vec(&v);
        for x in &out {
            assert!(approx(*x, 0.0, 1e-9));
        }
    }

    // ─── dema/tema: weighted EMA composites ───────────────────────────

    #[test]
    fn dema_constant_input_returns_constant() {
        let v = dema(&[data(&[7.0; 50]), i(10)]);
        let out = as_vec(&v);
        for x in &out {
            assert!(approx(*x, 7.0, 1e-9));
        }
    }

    #[test]
    fn tema_constant_input_returns_constant() {
        let v = tema(&[data(&[3.0; 50]), i(10)]);
        let out = as_vec(&v);
        for x in &out {
            assert!(approx(*x, 3.0, 1e-9));
        }
    }

    // ─── atr / true_range ─────────────────────────────────────────────

    #[test]
    fn true_range_first_bar_is_high_minus_low() {
        let v = true_range(&[data(&[10.0]), data(&[5.0]), data(&[8.0])]);
        let out = as_vec(&v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], 5.0);
    }

    #[test]
    fn true_range_uses_max_of_three_formulas() {
        // Bar 2: H=20, L=18, prev_close=10 → TR = max(2, 10, 8) = 10.
        let h = vec![10.0, 20.0];
        let l = vec![5.0, 18.0];
        let c = vec![10.0, 19.0];
        let v = true_range(&[data(&h), data(&l), data(&c)]);
        let out = as_vec(&v);
        assert_eq!(out, vec![5.0, 10.0]);
    }

    #[test]
    fn atr_too_short_returns_empty() {
        // p=5 but only 3 bars → returns empty array.
        let v = atr(&[
            data(&[1.0, 2.0, 3.0]),
            data(&[1.0, 2.0, 3.0]),
            data(&[1.0, 2.0, 3.0]),
            i(5),
        ]);
        assert!(as_vec(&v).is_empty());
    }

    #[test]
    fn atr_constant_bars_zero() {
        // Identical OHLC each bar → TR = 0 → ATR = 0.
        let bars = vec![100.0; 20];
        let v = atr(&[data(&bars), data(&bars), data(&bars), i(14)]);
        let out = as_vec(&v);
        // Warm-up is zero-filled; the seeded value at index p-1 is also 0.
        for x in &out[13..] {
            assert!(
                approx(*x, 0.0, 1e-9),
                "ATR of constant should be 0, got {x}"
            );
        }
    }

    // ─── bollinger / keltner / donchian ───────────────────────────────

    #[test]
    fn bollinger_middle_equals_sma() {
        let input: Vec<f64> = (1..=30).map(|i| i as f64).collect();
        let mid = as_vec(&bollinger_middle(&[data(&input), i(5)]));
        let sma_v = as_vec(&sma(&[data(&input), i(5)]));
        assert_eq!(mid, sma_v);
    }

    #[test]
    fn bollinger_upper_above_middle_above_lower() {
        let input: Vec<f64> = (0..30).map(|i| (i as f64).sin() * 10.0 + 50.0).collect();
        let mid = as_vec(&bollinger_middle(&[data(&input), i(10)]));
        let up = as_vec(&bollinger_upper(&[data(&input), i(10)]));
        let lo = as_vec(&bollinger_lower(&[data(&input), i(10)]));
        for i in 0..mid.len() {
            assert!(up[i] >= mid[i], "upper >= middle at {i}");
            assert!(mid[i] >= lo[i], "middle >= lower at {i}");
        }
    }

    #[test]
    fn bollinger_constant_input_zero_width() {
        // Stddev of constant series = 0 → upper = middle = lower.
        let input = vec![50.0; 20];
        let mid = as_vec(&bollinger_middle(&[data(&input), i(10)]));
        let up = as_vec(&bollinger_upper(&[data(&input), i(10)]));
        let lo = as_vec(&bollinger_lower(&[data(&input), i(10)]));
        for i in 0..mid.len() {
            assert!(approx(up[i], mid[i], 1e-9));
            assert!(approx(lo[i], mid[i], 1e-9));
        }
    }

    #[test]
    fn donchian_upper_picks_window_max() {
        // Period 3 over [1,5,3,7,2] → windows [1,5,3]=5, [5,3,7]=7, [3,7,2]=7.
        let v = donchian_upper(&[data(&[1.0, 5.0, 3.0, 7.0, 2.0]), i(3)]);
        assert_eq!(as_vec(&v), vec![5.0, 7.0, 7.0]);
    }

    #[test]
    fn donchian_lower_picks_window_min() {
        // Period 3 over [5,1,3,2,7] → windows [5,1,3]=1, [1,3,2]=1, [3,2,7]=2.
        let v = donchian_lower(&[data(&[5.0, 1.0, 3.0, 2.0, 7.0]), i(3)]);
        assert_eq!(as_vec(&v), vec![1.0, 1.0, 2.0]);
    }

    #[test]
    fn donchian_period_larger_than_data_returns_empty() {
        let v = donchian_upper(&[data(&[1.0, 2.0]), i(5)]);
        assert!(as_vec(&v).is_empty());
    }

    // ─── adx ──────────────────────────────────────────────────────────

    #[test]
    fn adx_too_short_returns_empty() {
        let v = adx(&[
            data(&[1.0, 2.0, 3.0]),
            data(&[1.0, 2.0, 3.0]),
            data(&[1.0, 2.0, 3.0]),
            i(14),
        ]);
        assert!(as_vec(&v).is_empty());
    }

    #[test]
    fn adx_constant_bars_zero_directional_strength() {
        // No direction → ADX should stay near 0 across the series.
        let bars = vec![100.0; 60];
        let v = adx(&[data(&bars), data(&bars), data(&bars), i(14)]);
        let out = as_vec(&v);
        // The first 14 + warm-up will be zero-filled; tail must be zero too.
        if let Some(last) = out.last() {
            assert!(
                approx(*last, 0.0, 1e-6),
                "ADX of constant bars must be 0, got {last}"
            );
        }
    }

    // ─── cci ──────────────────────────────────────────────────────────

    #[test]
    fn cci_constant_typical_price_returns_zeros() {
        // All H=L=C → TP constant → SMA(TP) = TP → numerator = 0 → CCI = 0.
        let bars = vec![100.0; 30];
        let v = cci(&[data(&bars), data(&bars), data(&bars), i(20)]);
        let out = as_vec(&v);
        for x in &out {
            assert!(approx(*x, 0.0, 1e-9));
        }
    }

    #[test]
    fn cci_too_short_returns_empty() {
        let v = cci(&[data(&[1.0; 5]), data(&[1.0; 5]), data(&[1.0; 5]), i(20)]);
        assert!(as_vec(&v).is_empty());
    }

    // ─── roc / momentum ───────────────────────────────────────────────

    #[test]
    fn roc_basic_percent_change() {
        // (100 → 110) → 10%.
        let v = roc(&[data(&[100.0, 105.0, 110.0]), i(2)]);
        let out = as_vec(&v);
        assert_eq!(out.len(), 1);
        assert!(approx(out[0], 10.0, 1e-9));
    }

    #[test]
    fn roc_division_by_zero_returns_zero() {
        let v = roc(&[data(&[0.0, 5.0, 10.0]), i(2)]);
        let out = as_vec(&v);
        assert_eq!(out, vec![0.0]);
    }

    #[test]
    fn roc_too_short_returns_empty() {
        // len == p is too short — need len > p.
        let v = roc(&[data(&[1.0, 2.0]), i(2)]);
        assert!(as_vec(&v).is_empty());
    }

    #[test]
    fn momentum_is_simple_difference() {
        // momentum_p[i] = data[i] - data[i - p].
        let v = momentum(&[data(&[1.0, 2.0, 5.0, 8.0]), i(2)]);
        let out = as_vec(&v);
        // i=2: 5-1=4. i=3: 8-2=6.
        assert_eq!(out, vec![4.0, 6.0]);
    }

    // ─── williams_r ───────────────────────────────────────────────────

    #[test]
    fn williams_r_at_window_low_is_neg_100() {
        // close = lowest in window → -100·(hi-close)/(hi-lo) = -100.
        let highs = vec![10.0, 10.0, 10.0];
        let lows = vec![5.0, 5.0, 5.0];
        let closes = vec![10.0, 10.0, 5.0]; // last close = window low
        let v = williams_r(&[data(&highs), data(&lows), data(&closes), i(3)]);
        let out = as_vec(&v);
        assert_eq!(out.len(), 1);
        assert!(approx(out[0], -100.0, 1e-9));
    }

    #[test]
    fn williams_r_at_window_high_is_zero() {
        let highs = vec![10.0, 10.0, 10.0];
        let lows = vec![5.0, 5.0, 5.0];
        let closes = vec![5.0, 5.0, 10.0]; // last close = window high
        let v = williams_r(&[data(&highs), data(&lows), data(&closes), i(3)]);
        let out = as_vec(&v);
        assert!(approx(out[0], 0.0, 1e-9));
    }

    #[test]
    fn williams_r_zero_range_returns_zero() {
        // hi == lo → guarded → 0.
        let v = williams_r(&[
            data(&[10.0, 10.0, 10.0]),
            data(&[10.0, 10.0, 10.0]),
            data(&[10.0, 10.0, 10.0]),
            i(3),
        ]);
        assert_eq!(as_vec(&v), vec![0.0]);
    }

    // ─── obv ──────────────────────────────────────────────────────────

    #[test]
    fn obv_empty_returns_empty() {
        let v = obv(&[data(&[]), data(&[])]);
        assert!(as_vec(&v).is_empty());
    }

    #[test]
    fn obv_first_value_is_first_volume() {
        let v = obv(&[data(&[100.0, 101.0]), data(&[1000.0, 500.0])]);
        let out = as_vec(&v);
        assert_eq!(out[0], 1000.0);
    }
}
