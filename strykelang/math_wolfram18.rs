// Batch 18 — more crypto, more time series, more graph theory.

// Poly1305 one-block step per RFC 8439 §2.5.1: acc = ((acc + block) * r) mod (2^130 - 5).
// Uses 5×26-bit radix limbs so each schoolbook product fits in u64. r is clamped
// per RFC (clear bits 24,25,26,27,28,29,30,31 within each 32-bit word).
fn builtin_poly1305_block_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let acc_in = i1(args) as u128;
    let block_in = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u128;
    let r_in = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) as u128;
    let r = r_in & 0x0ffffffc_0ffffffc_0ffffffc_0fffffffu128;
    fn limbs(x: u128) -> [u64; 5] {
        [
            (x & 0x3ffffff) as u64,
            ((x >> 26) & 0x3ffffff) as u64,
            ((x >> 52) & 0x3ffffff) as u64,
            ((x >> 78) & 0x3ffffff) as u64,
            ((x >> 104) & 0x3ffffff) as u64,
        ]
    }
    let mut h = limbs(acc_in.wrapping_add(block_in));
    let r_l = limbs(r);
    let s1 = r_l[1].wrapping_mul(5);
    let s2 = r_l[2].wrapping_mul(5);
    let s3 = r_l[3].wrapping_mul(5);
    let s4 = r_l[4].wrapping_mul(5);
    let d0 = (h[0] as u128) * (r_l[0] as u128) + (h[1] as u128) * (s4 as u128)
           + (h[2] as u128) * (s3 as u128) + (h[3] as u128) * (s2 as u128) + (h[4] as u128) * (s1 as u128);
    let d1 = (h[0] as u128) * (r_l[1] as u128) + (h[1] as u128) * (r_l[0] as u128)
           + (h[2] as u128) * (s4 as u128) + (h[3] as u128) * (s3 as u128) + (h[4] as u128) * (s2 as u128);
    let d2 = (h[0] as u128) * (r_l[2] as u128) + (h[1] as u128) * (r_l[1] as u128)
           + (h[2] as u128) * (r_l[0] as u128) + (h[3] as u128) * (s4 as u128) + (h[4] as u128) * (s3 as u128);
    let d3 = (h[0] as u128) * (r_l[3] as u128) + (h[1] as u128) * (r_l[2] as u128)
           + (h[2] as u128) * (r_l[1] as u128) + (h[3] as u128) * (r_l[0] as u128) + (h[4] as u128) * (s4 as u128);
    let d4 = (h[0] as u128) * (r_l[4] as u128) + (h[1] as u128) * (r_l[3] as u128)
           + (h[2] as u128) * (r_l[2] as u128) + (h[3] as u128) * (r_l[1] as u128) + (h[4] as u128) * (r_l[0] as u128);
    let mut c;
    h[0] = (d0 & 0x3ffffff) as u64; c = d0 >> 26;
    let d1c = d1 + c; h[1] = (d1c & 0x3ffffff) as u64; c = d1c >> 26;
    let d2c = d2 + c; h[2] = (d2c & 0x3ffffff) as u64; c = d2c >> 26;
    let d3c = d3 + c; h[3] = (d3c & 0x3ffffff) as u64; c = d3c >> 26;
    let d4c = d4 + c; h[4] = (d4c & 0x3ffffff) as u64; c = d4c >> 26;
    h[0] = h[0].wrapping_add((c as u64).wrapping_mul(5));
    let extra = h[0] >> 26; h[0] &= 0x3ffffff;
    h[1] = h[1].wrapping_add(extra);
    let acc = (h[0] as u128) | ((h[1] as u128) << 26) | ((h[2] as u128) << 52)
            | ((h[3] as u128) << 78) | ((h[4] as u128) << 104);
    Ok(StrykeValue::integer(acc as i64))
}
// X25519 field multiplication mod 2²⁵⁵-19 using 5×51-bit radix limbs (DJB form).
// Schoolbook multiply produces 9 limb products, each 102-bit; reduce by replacing
// limb_i (i ≥ 5) with 19·limb_{i-5} per the Curve25519 prime identity 2²⁵⁵ ≡ 19.
fn builtin_x25519_field_mul(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_lo = i1(args) as u128;
    let b_lo = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u128;
    fn limbs51(x: u128) -> [u64; 5] {
        let m = (1u128 << 51) - 1;
        [
            (x & m) as u64,
            ((x >> 51) & m) as u64,
            ((x >> 102) & m) as u64,
            (((x >> 102) >> 51) & m) as u64,
            0,
        ]
    }
    let a = limbs51(a_lo);
    let b = limbs51(b_lo);
    let m51 = (1u64 << 51) - 1;
    let mut t = [0u128; 5];
    let s1 = (b[1] as u128) * 19;
    let s2 = (b[2] as u128) * 19;
    let s3 = (b[3] as u128) * 19;
    let s4 = (b[4] as u128) * 19;
    t[0] = (a[0] as u128) * (b[0] as u128) + (a[1] as u128) * s4
         + (a[2] as u128) * s3 + (a[3] as u128) * s2 + (a[4] as u128) * s1;
    t[1] = (a[0] as u128) * (b[1] as u128) + (a[1] as u128) * (b[0] as u128)
         + (a[2] as u128) * s4 + (a[3] as u128) * s3 + (a[4] as u128) * s2;
    t[2] = (a[0] as u128) * (b[2] as u128) + (a[1] as u128) * (b[1] as u128)
         + (a[2] as u128) * (b[0] as u128) + (a[3] as u128) * s4 + (a[4] as u128) * s3;
    t[3] = (a[0] as u128) * (b[3] as u128) + (a[1] as u128) * (b[2] as u128)
         + (a[2] as u128) * (b[1] as u128) + (a[3] as u128) * (b[0] as u128) + (a[4] as u128) * s4;
    t[4] = (a[0] as u128) * (b[4] as u128) + (a[1] as u128) * (b[3] as u128)
         + (a[2] as u128) * (b[2] as u128) + (a[3] as u128) * (b[1] as u128) + (a[4] as u128) * (b[0] as u128);
    let mut h = [0u64; 5];
    let mut c;
    h[0] = (t[0] & m51 as u128) as u64; c = t[0] >> 51;
    let t1 = t[1] + c; h[1] = (t1 & m51 as u128) as u64; c = t1 >> 51;
    let t2 = t[2] + c; h[2] = (t2 & m51 as u128) as u64; c = t2 >> 51;
    let t3 = t[3] + c; h[3] = (t3 & m51 as u128) as u64; c = t3 >> 51;
    let t4 = t[4] + c; h[4] = (t4 & m51 as u128) as u64; c = t4 >> 51;
    h[0] = h[0].wrapping_add((c as u64).wrapping_mul(19));
    let cc = h[0] >> 51; h[0] &= m51;
    h[1] = h[1].wrapping_add(cc);
    let lower = (h[0] as u128) | ((h[1] as u128) << 51);
    Ok(StrykeValue::integer(lower as i64))
}
fn builtin_curve25519_mul_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_x25519_field_mul(args)
}
fn builtin_secp256k1_y_recover(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y2 = x.powi(3) + 7.0;
    Ok(StrykeValue::float(y2.max(0.0).sqrt()))
}
fn builtin_hmac_step_xor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_default();
    let pad = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0x36);
    let out: Vec<u8> = key.bytes().map(|b| b ^ pad).collect();
    Ok(StrykeValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_pkcs7_pad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let block = args.get(1).map(|v| v.to_number() as usize).unwrap_or(16);
    let pad_len = block - (s.len() % block);
    let mut out = s.into_bytes();
    for _ in 0..pad_len { out.push(pad_len as u8); }
    Ok(StrykeValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_pkcs7_unpad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    if n == 0 { return Ok(StrykeValue::string(s)); }
    let pad = bytes[n - 1] as usize;
    if pad <= n { Ok(StrykeValue::string(String::from_utf8_lossy(&bytes[..n - pad]).into_owned())) }
    else { Ok(StrykeValue::string(s)) }
}
fn builtin_xor_byte_string(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let bv = b.as_bytes(); if bv.is_empty() { return Ok(StrykeValue::string(a)); }
    let out: Vec<u8> = a.bytes().enumerate().map(|(i, c)| c ^ bv[i % bv.len()]).collect();
    Ok(StrykeValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_atbash_cipher(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() { (b'Z' - (c as u8 - b'A')) as char }
        else if c.is_ascii_lowercase() { (b'z' - (c as u8 - b'a')) as char }
        else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}
fn builtin_vigenere_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let kbytes: Vec<u8> = key.bytes().filter(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_uppercase() - b'A').collect();
    if kbytes.is_empty() { return Ok(StrykeValue::string(s)); }
    let mut k = 0_usize;
    let out: String = s.chars().map(|c| {
        if c.is_ascii_alphabetic() {
            let base = if c.is_ascii_uppercase() { b'A' } else { b'a' };
            let shift = kbytes[k % kbytes.len()];
            k += 1;
            ((c as u8 - base + shift) % 26 + base) as char
        } else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}
fn builtin_vigenere_decrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let kbytes: Vec<u8> = key.bytes().filter(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_uppercase() - b'A').collect();
    if kbytes.is_empty() { return Ok(StrykeValue::string(s)); }
    let mut k = 0_usize;
    let out: String = s.chars().map(|c| {
        if c.is_ascii_alphabetic() {
            let base = if c.is_ascii_uppercase() { b'A' } else { b'a' };
            let shift = kbytes[k % kbytes.len()];
            k += 1;
            ((c as u8 - base + 26 - shift) % 26 + base) as char
        } else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}
fn builtin_xor_brute_keylen(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut best_len = 1; let mut best_score = f64::INFINITY;
    for keylen in 1..=20.min(n / 2) {
        let mut hd = 0_f64;
        let mut count = 0_f64;
        for i in 0..(n - keylen).min(keylen * 4) {
            hd += (bytes[i] ^ bytes[i + keylen]).count_ones() as f64;
            count += 1.0;
        }
        let score = if count > 0.0 { hd / count / keylen as f64 } else { f64::INFINITY };
        if score < best_score { best_score = score; best_len = keylen; }
    }
    Ok(StrykeValue::integer(best_len as i64))
}

// Time series advanced
fn builtin_arima_diff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let d = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let mut cur = xs.clone();
    for _ in 0..d {
        cur = (1..cur.len()).map(|i| cur[i] - cur[i - 1]).collect();
    }
    Ok(StrykeValue::array(cur.into_iter().map(StrykeValue::float).collect()))
}
fn builtin_seasonal_diff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(12);
    let out: Vec<StrykeValue> = (s..xs.len()).map(|i| StrykeValue::float(xs[i] - xs[i - s])).collect();
    Ok(StrykeValue::array(out))
}
fn builtin_garch_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let omega = f1(args); let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_var = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_ret = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(omega + alpha * prev_ret * prev_ret + beta * prev_var))
}
fn builtin_egarch_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let omega = f1(args); let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_log_var = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_z = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(omega + alpha * (prev_z.abs() - (2.0 / std::f64::consts::PI).sqrt()) + gamma * prev_z + beta * prev_log_var))
}
fn builtin_realized_volatility(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s: f64 = returns.iter().map(|r| r * r).sum();
    Ok(StrykeValue::float(s.sqrt()))
}
fn builtin_max_drawdown_arr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut peak = xs.first().copied().unwrap_or(0.0);
    let mut mdd = 0.0_f64;
    for &x in &xs {
        if x > peak { peak = x; }
        let dd = if peak.abs() > 1e-30 { (peak - x) / peak } else { 0.0 };
        if dd > mdd { mdd = dd; }
    }
    Ok(StrykeValue::float(mdd))
}
fn builtin_calmar_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cagr = f1(args); let mdd = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float(cagr / mdd))
}
fn builtin_omega_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pos: f64 = returns.iter().filter(|&&r| r > threshold).map(|r| r - threshold).sum();
    let neg: f64 = returns.iter().filter(|&&r| r < threshold).map(|r| threshold - r).sum::<f64>().max(1e-30);
    Ok(StrykeValue::float(pos / neg))
}
fn builtin_kelly_criterion(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args); let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((p * (b + 1.0) - 1.0) / b))
}
fn builtin_var_historical(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (alpha * returns.len() as f64) as usize;
    Ok(StrykeValue::float(-returns.get(idx).copied().unwrap_or(0.0)))
}
fn builtin_cvar_historical(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = (alpha * returns.len() as f64) as usize;
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let cvar: f64 = returns[..n].iter().sum::<f64>() / n as f64;
    Ok(StrykeValue::float(-cvar))
}

// Graph extras
fn builtin_graph_degree_distribution(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for nbrs in &adj { *counts.entry(nbrs.len()).or_insert(0) += 1; }
    let mut keys: Vec<usize> = counts.keys().copied().collect();
    keys.sort();
    let pairs: Vec<StrykeValue> = keys.into_iter().map(|d| StrykeValue::array(vec![
        StrykeValue::integer(d as i64), StrykeValue::integer(counts[&d] as i64),
    ])).collect();
    Ok(StrykeValue::array(pairs))
}
fn builtin_graph_count_edges(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let total: usize = adj.iter().map(|nbrs| nbrs.len()).sum();
    Ok(StrykeValue::integer((total / 2) as i64))
}
fn builtin_graph_bipartite_match_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut matched = vec![-1_i64; n];
    let mut count = 0_i64;
    for u in 0..n {
        if matched[u] != -1 { continue; }
        for &v in &adj[u] {
            if v < n && matched[v] == -1 {
                matched[u] = v as i64; matched[v] = u as i64;
                count += 1; break;
            }
        }
    }
    Ok(StrykeValue::integer(count))
}
fn builtin_graph_count_triangles(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut t = 0_i64;
    for i in 0..n { for &j in &adj[i] { if j > i {
        for &k in &adj[j] { if k > j && sets[i].contains(&k) { t += 1; } }
    }}}
    Ok(StrykeValue::integer(t))
}
fn builtin_graph_avg_clustering(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut total = 0.0_f64;
    let mut count = 0_usize;
    for i in 0..n {
        let k = adj[i].len();
        if k < 2 { continue; }
        let mut tri = 0_usize;
        for &u in &adj[i] { for &v in &adj[i] {
            if u < v && sets[u].contains(&v) { tri += 1; }
        }}
        total += 2.0 * tri as f64 / (k * (k - 1)) as f64;
        count += 1;
    }
    Ok(StrykeValue::float(if count == 0 { 0.0 } else { total / count as f64 }))
}
fn builtin_graph_transitivity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut tri = 0_i64; let mut tris = 0_i64;
    for i in 0..n {
        let k = adj[i].len() as i64;
        tris += k * (k - 1) / 2;
        for &u in &adj[i] { for &v in &adj[i] {
            if u < v && sets[u].contains(&v) { tri += 1; }
        }}
    }
    if tris == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(3.0 * tri as f64 / tris as f64))
}
fn builtin_graph_max_clique_brute(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    if n > 20 { return Ok(StrykeValue::integer(0)); }
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut best = 0_i64;
    for mask in 0u64..(1u64 << n) {
        let bits = mask.count_ones() as i64;
        if bits <= best { continue; }
        let vertices: Vec<usize> = (0..n).filter(|i| (mask >> i) & 1 == 1).collect();
        let mut clique = true;
        'outer: for i in 0..vertices.len() {
            for j in i + 1..vertices.len() {
                if !sets[vertices[i]].contains(&vertices[j]) { clique = false; break 'outer; }
            }
        }
        if clique { best = bits; }
    }
    Ok(StrykeValue::integer(best))
}
fn builtin_graph_independent_set_brute(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    if n > 20 { return Ok(StrykeValue::integer(0)); }
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut best = 0_i64;
    for mask in 0u64..(1u64 << n) {
        let bits = mask.count_ones() as i64;
        if bits <= best { continue; }
        let vertices: Vec<usize> = (0..n).filter(|i| (mask >> i) & 1 == 1).collect();
        let mut indep = true;
        'outer: for i in 0..vertices.len() {
            for j in i + 1..vertices.len() {
                if sets[vertices[i]].contains(&vertices[j]) { indep = false; break 'outer; }
            }
        }
        if indep { best = bits; }
    }
    Ok(StrykeValue::integer(best))
}
fn builtin_graph_count_paths_length_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    let n = adj.len();
    let mut a = vec![vec![0_i64; n]; n];
    for i in 0..n { for &j in &adj[i] { if j < n { a[i][j] = 1; } } }
    let mut b = a.clone();
    for _ in 1..k {
        let mut next = vec![vec![0_i64; n]; n];
        for i in 0..n { for j in 0..n { for kk in 0..n {
            next[i][j] += b[i][kk] * a[kk][j];
        }}}
        b = next;
    }
    let total: i64 = b.iter().flatten().sum();
    Ok(StrykeValue::integer(total))
}
fn builtin_graph_pagerank_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let damping = args.get(1).map(|v| v.to_number()).unwrap_or(0.85);
    let n = adj.len();
    if n == 0 { return Ok(StrykeValue::array(vec![])); }
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..100 {
        let mut nr = vec![(1.0 - damping) / n as f64; n];
        for u in 0..n {
            let out_deg = adj[u].len().max(1);
            let share = damping * rank[u] / out_deg as f64;
            for &v in &adj[u] { if v < n { nr[v] += share; } }
        }
        rank = nr;
    }
    Ok(StrykeValue::array(rank.into_iter().map(StrykeValue::float).collect()))
}
