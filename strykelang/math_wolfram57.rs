// Batch 57 — spreadsheet (Excel/Sheets) lookups + bond / loan financial math.

fn b57_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b57_to_pairs(v: &PerlValue) -> Vec<(f64, f64)> {
    let flat = arg_to_vec(v);
    let n = flat.len() / 2;
    (0..n).map(|i| (flat[2 * i].to_number(), flat[2 * i + 1].to_number())).collect()
}

/// VLOOKUP exact: search column 0 of `table` for `key`, return col_index entry.
/// `table` is flat [k0, v0, w0, k1, v1, w1, ...] of n_cols-wide rows.
fn builtin_vlookup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let key = f1(args);
    let table = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n_cols = args.get(2).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let col = args.get(3).map(|v| v.to_number() as usize).unwrap_or(1);
    if col >= n_cols { return Ok(PerlValue::UNDEF); }
    let rows = table.len() / n_cols;
    for r in 0..rows {
        if (table[r * n_cols] - key).abs() < 1e-12 {
            return Ok(PerlValue::float(table[r * n_cols + col]));
        }
    }
    Ok(PerlValue::UNDEF)
}

/// HLOOKUP: search row 0, return entry from row_index.
fn builtin_hlookup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let key = f1(args);
    let table = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n_cols = args.get(2).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let row = args.get(3).map(|v| v.to_number() as usize).unwrap_or(1);
    let n_rows = table.len() / n_cols;
    if row >= n_rows { return Ok(PerlValue::UNDEF); }
    for c in 0..n_cols {
        if (table[c] - key).abs() < 1e-12 {
            return Ok(PerlValue::float(table[row * n_cols + c]));
        }
    }
    Ok(PerlValue::UNDEF)
}

/// XLOOKUP: like VLOOKUP but with explicit lookup and return arrays + default.
fn builtin_xlookup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let key = f1(args);
    let lookup = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let returns = b57_to_floats(args.get(2).unwrap_or(&PerlValue::array(vec![])));
    let default = args.get(3).map(|v| v.to_number()).unwrap_or(f64::NAN);
    for (i, &k) in lookup.iter().enumerate() {
        if (k - key).abs() < 1e-12 && i < returns.len() {
            return Ok(PerlValue::float(returns[i]));
        }
    }
    Ok(PerlValue::float(default))
}

/// INDEX(array, row, col) for a flat row-major matrix.
fn builtin_index_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let arr = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n_cols = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let row = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let col = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let pos = row * n_cols + col;
    if pos < arr.len() { Ok(PerlValue::float(arr[pos])) } else { Ok(PerlValue::UNDEF) }
}

/// INDIRECT: dispatch to a sub-array by string-ID lookup table. Args: id, table
/// of [id_n, value_n] pairs.
fn builtin_indirect(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let id = f1(args);
    let pairs = b57_to_pairs(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    for (k, v) in pairs {
        if (k - id).abs() < 1e-12 { return Ok(PerlValue::float(v)); }
    }
    Ok(PerlValue::UNDEF)
}

/// CHOOSE(index, list...) — pick by 1-based index from an array.
fn builtin_choose(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args).max(1) as usize - 1;
    let v = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    if i < v.len() { Ok(v[i].clone()) } else { Ok(PerlValue::UNDEF) }
}

/// OFFSET(start_index, rows, cols, n_cols) → linear index.
fn builtin_offset(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let start = i1(args);
    let rows = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let cols = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let n_cols = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(PerlValue::integer(start + rows * n_cols + cols))
}

/// SUMIF: sum values whose paired key matches predicate (>, <, =, !=, etc).
/// Args: key array, value array, target, op_id (0==, 1>, 2<, 3>=, 4<=, 5!=).
fn builtin_sumif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let keys = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let vals = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let target = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let op = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = keys.len().min(vals.len());
    let mut s = 0.0_f64;
    for i in 0..n {
        let pred = match op {
            0 => (keys[i] - target).abs() < 1e-12,
            1 => keys[i] > target,
            2 => keys[i] < target,
            3 => keys[i] >= target,
            4 => keys[i] <= target,
            5 => (keys[i] - target).abs() >= 1e-12,
            _ => false,
        };
        if pred { s += vals[i]; }
    }
    Ok(PerlValue::float(s))
}

/// COUNTIF: count entries satisfying predicate.
fn builtin_countif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let keys = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let op = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = keys.iter().filter(|&&k| match op {
        0 => (k - target).abs() < 1e-12,
        1 => k > target,
        2 => k < target,
        3 => k >= target,
        4 => k <= target,
        5 => (k - target).abs() >= 1e-12,
        _ => false,
    }).count();
    Ok(PerlValue::integer(n as i64))
}

/// AVERAGEIF
fn builtin_averageif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = builtin_sumif(args)?.to_number();
    let n = builtin_countif(&[args[0].clone(),
        args.get(2).cloned().unwrap_or(PerlValue::float(0.0)),
        args.get(3).cloned().unwrap_or(PerlValue::integer(0))])?.to_number();
    if n <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(s / n))
}

/// SUMIFS: sum where ALL of N predicates pass (op_array same length as
/// predicate_array, predicates are flattened triples (target, op)).
fn builtin_sumifs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let preds = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let mut s = 0.0_f64;
    for (i, v) in vals.iter().enumerate() {
        let mut ok = true;
        for grp in preds.chunks(3) {
            if grp.len() < 3 { continue; }
            let arr = b57_to_floats(&grp[0]);
            if i >= arr.len() { ok = false; break; }
            let target = grp[1].to_number();
            let op = grp[2].to_number() as i64;
            let pass = match op {
                0 => (arr[i] - target).abs() < 1e-12,
                1 => arr[i] > target,
                2 => arr[i] < target,
                3 => arr[i] >= target,
                4 => arr[i] <= target,
                5 => (arr[i] - target).abs() >= 1e-12,
                _ => false,
            };
            if !pass { ok = false; break; }
        }
        if ok { s += v; }
    }
    Ok(PerlValue::float(s))
}

/// COUNTIFS
fn builtin_countifs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let preds = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if preds.is_empty() { return Ok(PerlValue::integer(0)); }
    let len = b57_to_floats(&preds[0]).len();
    let mut count = 0_i64;
    for i in 0..len {
        let mut ok = true;
        for grp in preds.chunks(3) {
            if grp.len() < 3 { continue; }
            let arr = b57_to_floats(&grp[0]);
            if i >= arr.len() { ok = false; break; }
            let target = grp[1].to_number();
            let op = grp[2].to_number() as i64;
            let pass = match op {
                0 => (arr[i] - target).abs() < 1e-12,
                1 => arr[i] > target,
                2 => arr[i] < target,
                3 => arr[i] >= target,
                4 => arr[i] <= target,
                5 => (arr[i] - target).abs() >= 1e-12,
                _ => false,
            };
            if !pass { ok = false; break; }
        }
        if ok { count += 1; }
    }
    Ok(PerlValue::integer(count))
}

/// AVERAGEIFS
fn builtin_averageifs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = builtin_sumifs(args)?.to_number();
    let n = builtin_countifs(&[args.get(1).cloned().unwrap_or(PerlValue::array(vec![]))])?
        .to_number();
    if n <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(s / n))
}

/// SUMPRODUCT: Σ a_i · b_i.
fn builtin_sumproduct(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let b = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = a.len().min(b.len());
    Ok(PerlValue::float((0..n).map(|i| a[i] * b[i]).sum()))
}

/// RANK.EQ — descending rank with ties getting same lowest rank.
fn builtin_rank_eq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let arr = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let above = arr.iter().filter(|&&v| v > x).count();
    Ok(PerlValue::integer(above as i64 + 1))
}

/// RANK.AVG — average rank for ties.
fn builtin_rank_avg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let arr = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let above = arr.iter().filter(|&&v| v > x).count() as f64;
    let equal = arr.iter().filter(|&&v| (v - x).abs() < 1e-12).count() as f64;
    Ok(PerlValue::float(above + (equal + 1.0) / 2.0))
}

/// PERCENTRANK.INC: (k - 1) / (n - 1) where k = #{values ≤ x}.
fn builtin_percentrank(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let arr = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = arr.len();
    if n < 2 { return Ok(PerlValue::float(0.0)); }
    let k = arr.iter().filter(|&&v| v <= x).count() as f64;
    Ok(PerlValue::float((k - 1.0) / (n as f64 - 1.0)))
}

/// QUARTILE.INC (linear interpolation) and QUARTILE.EXC (n+1 method).
fn builtin_quartile_inc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    if arr.is_empty() { return Ok(PerlValue::float(0.0)); }
    arr.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = arr.len();
    let p = (q as f64) / 4.0;
    let h = (n as f64 - 1.0) * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - lo as f64;
    Ok(PerlValue::float(arr[lo] + frac * (arr[hi] - arr[lo])))
}

/// `quartile_exc`
fn builtin_quartile_exc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    if arr.is_empty() { return Ok(PerlValue::float(0.0)); }
    arr.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = arr.len() as f64;
    let p = (q as f64) / 4.0;
    let h = (n + 1.0) * p - 1.0;
    if h < 0.0 || h > n - 1.0 { return Ok(PerlValue::float(f64::NAN)); }
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(arr.len() - 1);
    let frac = h - lo as f64;
    Ok(PerlValue::float(arr[lo] + frac * (arr[hi] - arr[lo])))
}

/// XNPV: Σ cf_i / (1+r)^((d_i - d_0) / 365). Args: rate, cashflow array, days array.
fn builtin_xnpv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let cfs = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let days = b57_to_floats(args.get(2).unwrap_or(&PerlValue::array(vec![])));
    if cfs.is_empty() { return Ok(PerlValue::float(0.0)); }
    let d0 = days.first().copied().unwrap_or(0.0);
    let n = cfs.len().min(days.len());
    let s: f64 = (0..n).map(|i| cfs[i] / (1.0 + r).powf((days[i] - d0) / 365.0)).sum();
    Ok(PerlValue::float(s))
}

/// PPMT: principal portion of loan payment for period n (per-period rate).
fn builtin_ppmt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let nper = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pv = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if r == 0.0 { return Ok(PerlValue::float(-pv / nper)); }
    let pmt = -pv * r / (1.0 - (1.0 + r).powf(-nper));
    let bal = -pv * (1.0 + r).powf(n - 1.0)
        + pmt * ((1.0 + r).powf(n - 1.0) - 1.0) / r;
    let interest = bal * r;
    Ok(PerlValue::float(pmt - interest))
}

/// IPMT: interest portion of period n.
fn builtin_ipmt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let nper = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pv = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if r == 0.0 { return Ok(PerlValue::float(0.0)); }
    let pmt = -pv * r / (1.0 - (1.0 + r).powf(-nper));
    let bal = -pv * (1.0 + r).powf(n - 1.0)
        + pmt * ((1.0 + r).powf(n - 1.0) - 1.0) / r;
    Ok(PerlValue::float(bal * r))
}

/// RATE: Newton iteration on PV + PMT·a(rate) + FV·(1+rate)^(-n) = 0.
fn builtin_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let pmt = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let fv = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let mut r = 0.05_f64;
    for _ in 0..100 {
        let one_plus_r = 1.0 + r;
        let f = pv + pmt * (1.0 - one_plus_r.powf(-n)) / r + fv * one_plus_r.powf(-n);
        let dr = -pmt * (one_plus_r.powf(-n) * (n / r) - (1.0 - one_plus_r.powf(-n)) / (r * r))
            - fv * n * one_plus_r.powf(-n - 1.0);
        if dr.abs() < 1e-15 { break; }
        let new_r = r - f / dr;
        if (new_r - r).abs() < 1e-12 { return Ok(PerlValue::float(new_r)); }
        r = new_r;
    }
    Ok(PerlValue::float(r))
}

/// Macauley duration: Σ t·CF_t·(1+y)^(-t) / Σ CF_t·(1+y)^(-t).
fn builtin_macauley_duration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let cfs = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    if cfs.is_empty() { return Ok(PerlValue::float(0.0)); }
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for (i, c) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let disc = c / (1.0 + y).powf(t);
        num += t * disc;
        den += disc;
    }
    if den <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

/// Convexity: Σ t(t+1)·CF_t·(1+y)^(-t-2) / Σ CF_t·(1+y)^(-t).
fn builtin_convexity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let cfs = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    if cfs.is_empty() { return Ok(PerlValue::float(0.0)); }
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for (i, c) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        num += t * (t + 1.0) * c / (1.0 + y).powf(t + 2.0);
        den += c / (1.0 + y).powf(t);
    }
    if den <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

/// Yield to maturity: Newton on bond price. Args: price, face, coupon_rate
/// (annual), n_years, frequency.
fn builtin_yield_to_maturity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let price = f1(args);
    let face = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let cr = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(10.0);
    let freq = args.get(4).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let coupon = face * cr / freq;
    let nper = n * freq;
    let mut y = cr / freq;
    for _ in 0..100 {
        let mut p = 0.0_f64;
        let mut dp = 0.0_f64;
        for k in 1..=(nper as i64) {
            let t = k as f64;
            let disc = (1.0 + y).powf(-t);
            p += coupon * disc;
            dp -= t * coupon * disc / (1.0 + y);
        }
        p += face * (1.0 + y).powf(-nper);
        dp -= nper * face * (1.0 + y).powf(-nper - 1.0);
        let f = p - price;
        if dp.abs() < 1e-15 { break; }
        let new_y = y - f / dp;
        if (new_y - y).abs() < 1e-10 { return Ok(PerlValue::float(new_y * freq)); }
        y = new_y;
    }
    Ok(PerlValue::float(y * freq))
}

/// Accrued interest = face · coupon_rate · (days_since_last_coupon / 365).
fn builtin_accrued_interest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let face = f1(args);
    let cr = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let days_since = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let basis_days = args.get(3).map(|v| v.to_number()).unwrap_or(365.0).max(1.0);
    Ok(PerlValue::float(face * cr * days_since / basis_days))
}

/// Clean price = dirty price − accrued.
fn builtin_clean_price(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dirty = f1(args);
    let accrued = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dirty - accrued))
}

/// Dirty price = clean + accrued.
fn builtin_dirty_price(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let clean = f1(args);
    let accrued = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(clean + accrued))
}

/// Coupon count between settle and maturity at given frequency.
fn builtin_coupon_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let years = f1(args);
    let freq = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    Ok(PerlValue::integer((years * freq).ceil() as i64))
}

/// Brier score for binary forecasts: BS = (1/N) Σ (p_i − o_i)².
fn builtin_skill_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let o = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(o.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let bs: f64 = (0..n).map(|i| (p[i] - o[i]).powi(2)).sum::<f64>() / n as f64;
    let base_rate: f64 = o.iter().take(n).sum::<f64>() / n as f64;
    let bs_climo = base_rate * (1.0 - base_rate);
    if bs_climo <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - bs / bs_climo))
}

/// Reliability (calibration) discretized into 10 bins.
fn builtin_reliability_diagram(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b57_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let o = b57_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(o.len());
    let mut bin_count = [0_u64; 10];
    let mut bin_obs = [0_f64; 10];
    let mut bin_pred = [0_f64; 10];
    for i in 0..n {
        let b = ((p[i] * 10.0).floor() as usize).min(9);
        bin_count[b] += 1;
        bin_obs[b] += o[i];
        bin_pred[b] += p[i];
    }
    let mut rel = 0.0_f64;
    for k in 0..10 {
        if bin_count[k] == 0 { continue; }
        let mean_obs = bin_obs[k] / bin_count[k] as f64;
        let mean_pred = bin_pred[k] / bin_count[k] as f64;
        rel += bin_count[k] as f64 * (mean_pred - mean_obs).powi(2);
    }
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(rel / n as f64))
}

/// Taylor diagram score: combines correlation and stddev ratio. S = 4·(1+r) /
/// ((σ_r + 1/σ_r)² · (1+1)). Args: r (correlation), σ_norm = σ_model/σ_ref.
fn builtin_taylor_diagram_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args).clamp(-1.0, 1.0);
    let sigma_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let denom = (sigma_norm + 1.0 / sigma_norm).powi(2) * 2.0;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(4.0 * (1.0 + r) / denom))
}
