// Batch 54 — APL/J/K array primitives: scalar/array reduce, scan, axis ops,
// base encoding, deal, permutation utilities.

fn b54_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// iota_n: APL ⍳N → [0, 1, ..., N-1].
fn builtin_iota_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let v: Vec<StrykeValue> = (0..n).map(StrykeValue::integer).collect();
    Ok(StrykeValue::array(v))
}

/// reduce_axis: f/array along last axis with op id (0=add, 1=mul, 2=max, 3=min, 4=or, 5=and).
fn builtin_reduce_axis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b54_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let op = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if v.is_empty() {
        return Ok(StrykeValue::float(match op { 1 => 1.0, 2 => f64::NEG_INFINITY, 3 => f64::INFINITY, _ => 0.0 }));
    }
    Ok(StrykeValue::float(match op {
        0 => v.iter().sum(),
        1 => v.iter().product(),
        2 => v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        3 => v.iter().cloned().fold(f64::INFINITY, f64::min),
        4 => if v.iter().any(|&x| x != 0.0) { 1.0 } else { 0.0 },
        5 => if v.iter().all(|&x| x != 0.0) { 1.0 } else { 0.0 },
        _ => v.iter().sum(),
    }))
}

/// scan_axis: f\array prefix scan (cumulative).
fn builtin_scan_axis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b54_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let op = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if v.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let mut out = Vec::with_capacity(v.len());
    let mut acc = match op { 1 => 1.0, 2 => f64::NEG_INFINITY, 3 => f64::INFINITY, _ => 0.0 };
    for &x in &v {
        acc = match op {
            0 => acc + x,
            1 => acc * x,
            2 => acc.max(x),
            3 => acc.min(x),
            _ => acc + x,
        };
        out.push(StrykeValue::float(acc));
    }
    Ok(StrykeValue::array(out))
}

/// fold_axis: like reduce but takes an explicit initial value.
fn builtin_fold_axis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let init = f1(args);
    let v = b54_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let op = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let acc = v.iter().fold(init, |a, &x| match op {
        0 => a + x,
        1 => a * x,
        2 => a.max(x),
        3 => a.min(x),
        _ => a + x,
    });
    Ok(StrykeValue::float(acc))
}

/// rotate_axis: APL φ — cyclically shift array by k (positive = left).
fn builtin_rotate_axis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len();
    if n == 0 { return Ok(StrykeValue::array(vec![])); }
    let k = (args.get(1).map(|v| v.to_number() as i64).unwrap_or(1)).rem_euclid(n as i64) as usize;
    let mut out: Vec<StrykeValue> = v[k..].to_vec();
    out.extend(v[..k].iter().cloned());
    Ok(StrykeValue::array(out))
}

/// transpose_axis: J |. — reverse the leading axis of a flat row-major matrix.
/// Args: matrix as flat array, n_rows.
fn builtin_transpose_axis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let rows = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let cols = v.len() / rows;
    if cols == 0 { return Ok(StrykeValue::array(v)); }
    let mut out = Vec::with_capacity(rows * cols);
    for c in 0..cols {
        for r in 0..rows {
            out.push(v[r * cols + c].clone());
        }
    }
    Ok(StrykeValue::array(out))
}

/// reshape_dim: APL ρ — reshape into grid of given size, padding from cycle.
fn builtin_reshape_dim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if v.is_empty() { return Ok(StrykeValue::array(vec![StrykeValue::integer(0); n])); }
    let out: Vec<StrykeValue> = (0..n).map(|i| v[i % v.len()].clone()).collect();
    Ok(StrykeValue::array(out))
}

/// encode_base: APL ⊤ → digits of n in mixed radix [b1, b2, ...]. Returns
/// digits little-endian.
fn builtin_encode_base(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args);
    let radix = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut out: Vec<StrykeValue> = Vec::with_capacity(radix.len());
    for r in &radix {
        let b = r.to_number() as i64;
        if b == 0 { out.push(StrykeValue::integer(n)); n = 0; }
        else { out.push(StrykeValue::integer(n.rem_euclid(b))); n = n.div_euclid(b); }
    }
    Ok(StrykeValue::array(out))
}

/// decode_base: APL ⊥ → fold digits in mixed radix back to integer (digits
/// little-endian).
fn builtin_decode_base(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let digits = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let radix = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut acc = 0_i64;
    let mut place = 1_i64;
    for (i, d) in digits.iter().enumerate() {
        acc = acc.saturating_add(d.to_number() as i64 * place);
        let r = radix.get(i).map(|v| v.to_number() as i64).unwrap_or(1);
        place = place.saturating_mul(r);
    }
    Ok(StrykeValue::integer(acc))
}

/// nub_list: APL ∪ — preserve first occurrences only.
fn builtin_nub_list(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(v.len());
    for x in v {
        let key = format!("{:?}", x);
        if seen.insert(key) { out.push(x); }
    }
    Ok(StrykeValue::array(out))
}

/// nub_count: |∪x — count of distinct elements.
fn builtin_nub_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut seen = std::collections::HashSet::new();
    for x in v { seen.insert(format!("{:?}", x)); }
    Ok(StrykeValue::integer(seen.len() as i64))
}

/// membership_idx: APL ∊ — element-wise membership of x in y, returns 0/1 array.
fn builtin_membership_idx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let yset: std::collections::HashSet<String> = y.iter().map(|v| format!("{:?}", v)).collect();
    let out: Vec<StrykeValue> = x.iter().map(|v| {
        StrykeValue::integer(if yset.contains(&format!("{:?}", v)) { 1 } else { 0 })
    }).collect();
    Ok(StrykeValue::array(out))
}

/// deal_n_k: APL k?n — random k-subset of [0..n) without replacement, given seed
/// for deterministic output (xorshift).
fn builtin_deal_n_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0).min(n);
    let mut seed = args.get(2).map(|v| v.to_number() as u64).unwrap_or(0xdeadbeef);
    let mut pool: Vec<i64> = (0..n as i64).collect();
    let mut out = Vec::with_capacity(k);
    for i in 0..k {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        let r = i + (seed as usize % (n - i));
        pool.swap(i, r);
        out.push(StrykeValue::integer(pool[i]));
    }
    Ok(StrykeValue::array(out))
}

/// roll_n: APL ?n — single uniform draw from [0, n) given seed.
fn builtin_roll_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(1);
    let mut seed = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xdeadbeef);
    seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
    Ok(StrykeValue::integer((seed % n as u64) as i64))
}

/// permute_idx: APL x⌷y — gather y at indices x.
fn builtin_permute_idx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let idx = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let src = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = src.len();
    let out: Vec<StrykeValue> = idx.iter().map(|i| {
        let k = i.to_number() as i64;
        if k < 0 || (k as usize) >= n { StrykeValue::UNDEF } else { src[k as usize].clone() }
    }).collect();
    Ok(StrykeValue::array(out))
}

/// invert_perm: given perm π, return σ with σ[π[i]] = i.
fn builtin_invert_perm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = p.len();
    let mut out = vec![StrykeValue::integer(0); n];
    for (i, v) in p.iter().enumerate() {
        let j = v.to_number() as usize;
        if j < n { out[j] = StrykeValue::integer(i as i64); }
    }
    Ok(StrykeValue::array(out))
}
