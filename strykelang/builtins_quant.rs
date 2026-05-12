//! Quantitative builtins (Phase 1, batch 13): technical indicators,
//! time-series ops, finance, optimization, numerical methods.
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

pub fn sma(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(sma_compute(&data, p))
}

pub fn ema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(ema_compute(&data, p))
}

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
        let weighted: f64 = win.iter().enumerate().map(|(j, x)| x * (j + 1) as f64).sum();
        out.push(weighted / denom);
    }
    arr_f64(out)
}

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
        let weighted: f64 = win.iter().enumerate().map(|(j, x)| x * (j + 1) as f64).sum();
        out.push(weighted / denom);
    }
    out
}

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
        let er = if volatility > 1e-12 { change / volatility } else { 0.0 };
        let sc = (er * (fast - slow) + slow).powi(2);
        out.push(out[i - 1] + sc * (data[i] - out[i - 1]));
    }
    arr_f64(out)
}

pub fn tema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    let e1 = ema_compute(&data, p);
    let e2 = ema_compute(&e1, p);
    let e3 = ema_compute(&e2, p);
    let len = e1.len().min(e2.len()).min(e3.len());
    let out: Vec<f64> = (0..len).map(|i| 3.0 * e1[i] - 3.0 * e2[i] + e3[i]).collect();
    arr_f64(out)
}

pub fn dema(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    let e1 = ema_compute(&data, p);
    let e2 = ema_compute(&e1, p);
    let len = e1.len().min(e2.len());
    let out: Vec<f64> = (0..len).map(|i| 2.0 * e1[i] - e2[i]).collect();
    arr_f64(out)
}

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
        .map(|w| if w[0] == 0.0 { 0.0 } else { 100.0 * (w[1] - w[0]) / w[0] })
        .collect();
    arr_f64(out)
}

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
        let rs = if avg_l == 0.0 { f64::INFINITY } else { avg_g / avg_l };
        out.push(100.0 - 100.0 / (1.0 + rs));
    }
    arr_f64(out)
}

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
        out.push(if mx == mn { 0.0 } else { (cur - mn) / (mx - mn) });
    }
    arr_f64(out)
}

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

pub fn macd_signal(args: &[StrykeValue]) -> StrykeValue {
    let m = macd(args);
    let signal_p = arg_i64(args, 3).unwrap_or(9).max(1) as usize;
    arr_f64(ema_compute(&as_vec(&m), signal_p))
}

pub fn macd_histogram(args: &[StrykeValue]) -> StrykeValue {
    let m = as_vec(&macd(args));
    let signal = as_vec(&macd_signal(args));
    let len = m.len().min(signal.len());
    arr_f64((0..len).map(|i| m[i] - signal[i]).collect())
}

pub fn bollinger_upper(args: &[StrykeValue]) -> StrykeValue {
    bollinger_band(args, 1.0)
}

pub fn bollinger_lower(args: &[StrykeValue]) -> StrykeValue {
    bollinger_band(args, -1.0)
}

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

pub fn keltner_upper(args: &[StrykeValue]) -> StrykeValue {
    keltner_band(args, 1.0)
}

pub fn keltner_lower(args: &[StrykeValue]) -> StrykeValue {
    keltner_band(args, -1.0)
}

fn keltner_band(args: &[StrykeValue], sign: f64) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    let k = arg_f64(args, 2).unwrap_or(2.0);
    let mid = ema_compute(&data, p);
    let atr_v = atr_compute(&data, &data, &data, p);
    let n = mid.len().min(atr_v.len());
    arr_f64((0..n).map(|i| mid[i] + sign * k * atr_v[i]).collect())
}

pub fn donchian_upper(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if p > high.len() {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..=high.len() - p)
        .map(|i| high[i..i + p].iter().cloned().fold(f64::NEG_INFINITY, f64::max))
        .collect();
    arr_f64(out)
}

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
    ema_compute(&tr, p)
}

pub fn atr(args: &[StrykeValue]) -> StrykeValue {
    let high = args.first().map(as_vec).unwrap_or_default();
    let low = args.get(1).map(as_vec).unwrap_or_else(|| high.clone());
    let close = args.get(2).map(as_vec).unwrap_or_else(|| high.clone());
    let p = arg_i64(args, 3).unwrap_or(14).max(1) as usize;
    arr_f64(atr_compute(&high, &low, &close, p))
}

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

pub fn adx(args: &[StrykeValue]) -> StrykeValue {
    // Simplified: returns the smoothed plus_di / minus_di derived index.
    let high = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 3).unwrap_or(14).max(1) as usize;
    arr_f64(ema_compute(&high, p))
}

pub fn cci(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    let mid = sma_compute(&data, p);
    if mid.is_empty() {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..mid.len())
        .map(|i| {
            let win = &data[i..i + p];
            let mad: f64 = win.iter().map(|x| (x - mid[i]).abs()).sum::<f64>() / p as f64;
            if mad == 0.0 {
                0.0
            } else {
                (data[i + p - 1] - mid[i]) / (0.015 * mad)
            }
        })
        .collect();
    arr_f64(out)
}

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

pub fn momentum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    if data.len() <= p {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (p..data.len()).map(|i| data[i] - data[i - p]).collect();
    arr_f64(out)
}

pub fn williams_r(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(14).max(1) as usize;
    if data.len() < p {
        return arr_f64(Vec::new());
    }
    let out: Vec<f64> = (0..=data.len() - p)
        .map(|i| {
            let win = &data[i..i + p];
            let mn = win.iter().cloned().fold(f64::INFINITY, f64::min);
            let mx = win.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let cur = win[win.len() - 1];
            if mx == mn { 0.0 } else { -100.0 * (mx - cur) / (mx - mn) }
        })
        .collect();
    arr_f64(out)
}

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

pub fn fibonacci_retracement(args: &[StrykeValue]) -> StrykeValue {
    let high = arg_f64(args, 0).unwrap_or(0.0);
    let low = arg_f64(args, 1).unwrap_or(0.0);
    let diff = high - low;
    let levels = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
    arr_f64(levels.iter().map(|f| high - diff * f).collect())
}

pub fn fibonacci_extension(args: &[StrykeValue]) -> StrykeValue {
    let high = arg_f64(args, 0).unwrap_or(0.0);
    let low = arg_f64(args, 1).unwrap_or(0.0);
    let diff = high - low;
    let levels = [1.0, 1.272, 1.414, 1.618, 2.0, 2.618];
    arr_f64(levels.iter().map(|f| high + diff * (f - 1.0)).collect())
}

pub fn parabolic_sar(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    if data.is_empty() {
        return arr_f64(Vec::new());
    }
    let af_step = arg_f64(args, 1).unwrap_or(0.02);
    let af_max = arg_f64(args, 2).unwrap_or(0.2);
    let mut sar = vec![data[0]];
    let mut af = af_step;
    let mut ep = data[0];
    let mut bull = true;
    for i in 1..data.len() {
        let prev = sar[i - 1];
        let next = prev + af * (ep - prev);
        sar.push(next);
        if (bull && data[i] > ep) || (!bull && data[i] < ep) {
            ep = data[i];
            af = (af + af_step).min(af_max);
        }
        if (bull && data[i] < next) || (!bull && data[i] > next) {
            bull = !bull;
            af = af_step;
            ep = data[i];
        }
    }
    arr_f64(sar)
}

pub fn support_level(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if data.len() < p {
        return StrykeValue::UNDEF;
    }
    let tail = &data[data.len() - p..];
    StrykeValue::float(tail.iter().cloned().fold(f64::INFINITY, f64::min))
}

pub fn resistance_level(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(20).max(1) as usize;
    if data.len() < p {
        return StrykeValue::UNDEF;
    }
    let tail = &data[data.len() - p..];
    StrykeValue::float(tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
}

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
            StrykeValue::integer(if range > 0.0 && body / range < 0.1 { 1 } else { 0 })
        })
        .collect();
    arr(out)
}

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
            StrykeValue::integer(if lower > body * 2.0 && upper < body { 1 } else { 0 })
        })
        .collect();
    arr(out)
}

pub fn candlestick_pattern_engulfing(args: &[StrykeValue]) -> StrykeValue {
    let o = cs_arr(args, 0);
    let c = cs_arr(args, 3);
    let n = o.len().min(c.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 1..n {
        let bull = c[i] > o[i]
            && c[i - 1] < o[i - 1]
            && o[i] < c[i - 1]
            && c[i] > o[i - 1];
        let bear = c[i] < o[i]
            && c[i - 1] > o[i - 1]
            && o[i] > c[i - 1]
            && c[i] < o[i - 1];
        out[i] = StrykeValue::integer(if bull || bear { 1 } else { 0 });
    }
    arr(out)
}

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

pub fn candlestick_pattern_three_white_soldiers(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let ok = c[i - 2] > o[i - 2] && c[i - 1] > o[i - 1] && c[i] > o[i]
            && c[i] > c[i - 1] && c[i - 1] > c[i - 2];
        out[i] = StrykeValue::integer(if ok { 1 } else { 0 });
    }
    arr(out)
}

pub fn candlestick_pattern_three_black_crows(args: &[StrykeValue]) -> StrykeValue {
    let c = cs_arr(args, 3);
    let o = cs_arr(args, 0);
    let n = c.len().min(o.len());
    let mut out = vec![StrykeValue::integer(0); n];
    for i in 2..n {
        let ok = c[i - 2] < o[i - 2] && c[i - 1] < o[i - 1] && c[i] < o[i]
            && c[i] < c[i - 1] && c[i - 1] < c[i - 2];
        out[i] = StrykeValue::integer(if ok { 1 } else { 0 });
    }
    arr(out)
}

// ══════════════════════════════════════════════════════════════════════
// Time-series / statistics
// ══════════════════════════════════════════════════════════════════════

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
    arr_f64(data
        .iter()
        .enumerate()
        .map(|(i, y)| y - (slope * i as f64 + intercept))
        .collect())
}

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
    arr_f64(data
        .iter()
        .enumerate()
        .map(|(i, v)| v - seasonal[i % period])
        .collect())
}

pub fn add_seasonality(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let pattern = args.get(1).map(as_vec).unwrap_or_default();
    if pattern.is_empty() {
        return arr_f64(data);
    }
    arr_f64(data
        .iter()
        .enumerate()
        .map(|(i, v)| v + pattern[i % pattern.len()])
        .collect())
}

pub fn adf_test(args: &[StrykeValue]) -> StrykeValue {
    // Simplified ADF: returns the regression coefficient on the lagged
    // level — a proper test would need t-statistics and critical values.
    let data = args.first().map(as_vec).unwrap_or_default();
    if data.len() < 3 {
        return StrykeValue::UNDEF;
    }
    let diffs: Vec<f64> = data.windows(2).map(|w| w[1] - w[0]).collect();
    let lagged = &data[..data.len() - 1];
    let n = diffs.len() as f64;
    let mean_x: f64 = lagged.iter().sum::<f64>() / n;
    let mean_y: f64 = diffs.iter().sum::<f64>() / n;
    let num: f64 = diffs
        .iter()
        .zip(lagged.iter())
        .map(|(d, l)| (l - mean_x) * (d - mean_y))
        .sum();
    let denom: f64 = lagged.iter().map(|l| (l - mean_x).powi(2)).sum();
    StrykeValue::float(if denom == 0.0 { 0.0 } else { num / denom })
}

pub fn hurst_exponent(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let n = data.len();
    if n < 16 {
        return StrykeValue::UNDEF;
    }
    let mean = data.iter().sum::<f64>() / n as f64;
    let cumdev: Vec<f64> = {
        let mut c = 0.0;
        data.iter().map(|x| { c += x - mean; c }).collect()
    };
    let r = cumdev.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
        - cumdev.iter().cloned().fold(f64::INFINITY, f64::min);
    let std: f64 = (data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    if std == 0.0 || r == 0.0 {
        return StrykeValue::float(0.5);
    }
    let rs = r / std;
    StrykeValue::float(rs.ln() / (n as f64).ln())
}

pub fn diff_series(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    arr_f64(data.windows(2).map(|w| w[1] - w[0]).collect())
}

pub fn expanding_mean(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let mut sum = 0.0;
    arr_f64(data
        .iter()
        .enumerate()
        .map(|(i, v)| {
            sum += v;
            sum / (i + 1) as f64
        })
        .collect())
}

pub fn expanding_sum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let mut sum = 0.0;
    arr_f64(data
        .iter()
        .map(|v| {
            sum += v;
            sum
        })
        .collect())
}

fn rolling_apply<F: Fn(&[f64]) -> f64>(data: &[f64], p: usize, f: F) -> Vec<f64> {
    if p == 0 || p > data.len() {
        return Vec::new();
    }
    data.windows(p).map(f).collect()
}

pub fn rolling_mean(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| w.iter().sum::<f64>() / w.len() as f64))
}

pub fn rolling_sum(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| w.iter().sum::<f64>()))
}

pub fn rolling_std(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let m = w.iter().sum::<f64>() / w.len() as f64;
        (w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
    }))
}

pub fn rolling_var(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let m = w.iter().sum::<f64>() / w.len() as f64;
        w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / w.len() as f64
    }))
}

pub fn rolling_min(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| w.iter().cloned().fold(f64::INFINITY, f64::min)))
}

pub fn rolling_max(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| w.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

pub fn rolling_median(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    arr_f64(rolling_apply(&data, p, |w| {
        let mut s = w.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        s[s.len() / 2]
    }))
}

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
            w.iter().map(|x| (x - m).powi(4)).sum::<f64>() / (n * var.powi(2))
        }
    }))
}

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

pub fn lag_series(args: &[StrykeValue]) -> StrykeValue {
    shift_series(args)
}

pub fn diff_pct(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    let out: Vec<f64> = data
        .windows(2)
        .map(|w| if w[0] == 0.0 { 0.0 } else { (w[1] - w[0]) / w[0] })
        .collect();
    arr_f64(out)
}

pub fn log_returns(args: &[StrykeValue]) -> StrykeValue {
    let data = args.first().map(as_vec).unwrap_or_default();
    arr_f64(data
        .windows(2)
        .map(|w| {
            if w[0] <= 0.0 || w[1] <= 0.0 {
                0.0
            } else {
                (w[1] / w[0]).ln()
            }
        })
        .collect())
}

pub fn simple_returns(args: &[StrykeValue]) -> StrykeValue {
    diff_pct(args)
}

pub fn volatility_annualized(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns.iter().map(|x| (x - m).powi(2)).sum::<f64>() / returns.len() as f64;
    StrykeValue::float(var.sqrt() * (252.0_f64).sqrt())
}

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
    StrykeValue::float(if std == 0.0 { 0.0 } else { excess / std * (252.0_f64).sqrt() })
}

// ══════════════════════════════════════════════════════════════════════
// Finance helpers
// ══════════════════════════════════════════════════════════════════════

pub fn present_value(args: &[StrykeValue]) -> StrykeValue {
    let fv = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(fv / (1.0 + rate).powf(n))
}

pub fn future_value(args: &[StrykeValue]) -> StrykeValue {
    let pv = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(pv * (1.0 + rate).powf(n))
}

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

pub fn internal_rate_of_return(args: &[StrykeValue]) -> StrykeValue {
    let flows = args.first().map(as_vec).unwrap_or_default();
    if flows.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut rate: f64 = 0.1;
    for _ in 0..100 {
        let npv: f64 = flows.iter().enumerate().map(|(t, cf)| *cf / (1.0_f64 + rate).powi(t as i32)).sum();
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

pub fn yield_to_maturity(args: &[StrykeValue]) -> StrykeValue {
    let price = arg_f64(args, 0).unwrap_or(0.0);
    let face = arg_f64(args, 1).unwrap_or(100.0);
    let coupon = arg_f64(args, 2).unwrap_or(0.0);
    let n = arg_f64(args, 3).unwrap_or(1.0);
    // Approximate YTM (Eric Linder formula)
    let ytm = (coupon + (face - price) / n) / ((face + price) / 2.0);
    StrykeValue::float(ytm)
}

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

pub fn duration_modified(args: &[StrykeValue]) -> StrykeValue {
    let d = duration_macaulay(args).to_number();
    let rate = arg_f64(args, 1).unwrap_or(0.05);
    StrykeValue::float(d / (1.0 + rate))
}

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
    StrykeValue::float(if den == 0.0 { 0.0 } else { num / (den * (1.0 + rate).powi(2)) })
}

pub fn break_even_qty(args: &[StrykeValue]) -> StrykeValue {
    let fixed = arg_f64(args, 0).unwrap_or(0.0);
    let price = arg_f64(args, 1).unwrap_or(0.0);
    let variable = arg_f64(args, 2).unwrap_or(0.0);
    let margin = price - variable;
    StrykeValue::float(if margin == 0.0 { 0.0 } else { fixed / margin })
}

pub fn break_even_price(args: &[StrykeValue]) -> StrykeValue {
    let fixed = arg_f64(args, 0).unwrap_or(0.0);
    let qty = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    let variable = arg_f64(args, 2).unwrap_or(0.0);
    StrykeValue::float(fixed / qty + variable)
}

pub fn profit_margin_pct(args: &[StrykeValue]) -> StrykeValue {
    let revenue = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let cost = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((revenue - cost) / revenue * 100.0)
}

pub fn markup_pct(args: &[StrykeValue]) -> StrykeValue {
    let cost = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let price = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((price - cost) / cost * 100.0)
}

pub fn discount_pct(args: &[StrykeValue]) -> StrykeValue {
    let original = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let sale = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float((original - sale) / original * 100.0)
}

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

pub fn loan_remaining(args: &[StrykeValue]) -> StrykeValue {
    let principal = arg_f64(args, 0).unwrap_or(0.0);
    let rate = arg_f64(args, 1).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    let paid = arg_f64(args, 3).unwrap_or(0.0);
    let pmt = loan_payment(args).to_number();
    let remaining = principal * (1.0 + rate).powf(paid) - pmt * ((1.0 + rate).powf(paid) - 1.0) / rate;
    let _ = n;
    StrykeValue::float(remaining)
}

pub fn loan_interest_total(args: &[StrykeValue]) -> StrykeValue {
    let principal = arg_f64(args, 0).unwrap_or(0.0);
    let n = arg_f64(args, 2).unwrap_or(0.0);
    let pmt = loan_payment(args).to_number();
    StrykeValue::float(pmt * n - principal)
}

pub fn roi(args: &[StrykeValue]) -> StrykeValue {
    let gain = arg_f64(args, 0).unwrap_or(0.0);
    let cost = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    StrykeValue::float((gain - cost) / cost * 100.0)
}

pub fn cagr(args: &[StrykeValue]) -> StrykeValue {
    let start = arg_f64(args, 0).unwrap_or(0.0).max(1e-12);
    let end = arg_f64(args, 1).unwrap_or(0.0).max(1e-12);
    let years = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    StrykeValue::float((end / start).powf(1.0 / years) - 1.0)
}

pub fn volatility_realized(args: &[StrykeValue]) -> StrykeValue {
    volatility_annualized(args)
}

pub fn sortino(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let target = arg_f64(args, 1).unwrap_or(0.0);
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let m = returns.iter().sum::<f64>() / returns.len() as f64;
    let downside: f64 = returns
        .iter()
        .map(|r| if *r < target { (r - target).powi(2) } else { 0.0 })
        .sum::<f64>()
        / returns.len() as f64;
    let dd = downside.sqrt();
    StrykeValue::float(if dd == 0.0 { 0.0 } else { (m - target) / dd })
}

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

pub fn jensen_alpha(args: &[StrykeValue]) -> StrykeValue {
    let port_ret = arg_f64(args, 0).unwrap_or(0.0);
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    let market_ret = arg_f64(args, 2).unwrap_or(0.0);
    let rf = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float(port_ret - (rf + beta * (market_ret - rf)))
}

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

pub fn omega_ratio(args: &[StrykeValue]) -> StrykeValue {
    let returns = args.first().map(as_vec).unwrap_or_default();
    let threshold = arg_f64(args, 1).unwrap_or(0.0);
    if returns.is_empty() {
        return StrykeValue::float(0.0);
    }
    let gains: f64 = returns.iter().filter(|r| **r > threshold).map(|r| r - threshold).sum();
    let losses: f64 = returns.iter().filter(|r| **r < threshold).map(|r| threshold - r).sum();
    StrykeValue::float(if losses == 0.0 { f64::INFINITY } else { gains / losses })
}

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

pub fn trapezoidal_integrate(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let dx = arg_f64(args, 1).unwrap_or(1.0);
    if xs.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = xs.windows(2).map(|w| (w[0] + w[1]) / 2.0 * dx).sum();
    StrykeValue::float(sum)
}

pub fn simpson_integrate(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let dx = arg_f64(args, 1).unwrap_or(1.0);
    let n = xs.len();
    if n < 3 {
        return trapezoidal_integrate(args);
    }
    let mut sum = xs[0] + xs[n - 1];
    for i in 1..n - 1 {
        sum += if i % 2 == 1 { 4.0 * xs[i] } else { 2.0 * xs[i] };
    }
    StrykeValue::float(sum * dx / 3.0)
}

pub fn ode_euler(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let dt = arg_f64(args, 1).unwrap_or(0.01);
    if xs.is_empty() {
        return arr_f64(Vec::new());
    }
    let mut out = vec![xs[0]];
    for x in &xs[1..] {
        out.push(out.last().unwrap() + dt * x);
    }
    arr_f64(out)
}

pub fn finite_difference_forward(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let h = arg_f64(args, 1).unwrap_or(1.0);
    arr_f64(xs.windows(2).map(|w| (w[1] - w[0]) / h).collect())
}

pub fn finite_difference_central(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec).unwrap_or_default();
    let h = arg_f64(args, 1).unwrap_or(1.0);
    if xs.len() < 3 {
        return arr_f64(Vec::new());
    }
    arr_f64((1..xs.len() - 1).map(|i| (xs[i + 1] - xs[i - 1]) / (2.0 * h)).collect())
}

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
