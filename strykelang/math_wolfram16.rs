// Batch 16 — combinatorics on words, statistics deep, network analysis.

fn builtin_bwt_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = s.len();
    let mut rotations: Vec<usize> = (0..n).collect();
    let bytes = s.as_bytes();
    rotations.sort_by(|&a, &b| {
        let ca: Vec<u8> = (0..n).map(|i| bytes[(a + i) % n]).collect();
        let cb: Vec<u8> = (0..n).map(|i| bytes[(b + i) % n]).collect();
        ca.cmp(&cb)
    });
    let mut out = String::new();
    let mut idx = 0;
    for (i, &r) in rotations.iter().enumerate() {
        out.push(bytes[(r + n - 1) % n] as char);
        if r == 0 { idx = i; }
    }
    Ok(PerlValue::array(vec![PerlValue::string(out), PerlValue::integer(idx as i64)]))
}
fn builtin_bwt_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let idx = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = s.len();
    if n == 0 { return Ok(PerlValue::string(String::new())); }
    let mut table: Vec<(u8, usize)> = s.bytes().enumerate().map(|(i, c)| (c, i)).collect();
    table.sort();
    let mut out = vec![0_u8; n];
    let mut t = idx;
    for i in 0..n {
        out[i] = table[t].0;
        t = table[t].1;
    }
    Ok(PerlValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_mtf_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut alphabet: Vec<u8> = (0..=255).collect();
    let mut out = Vec::new();
    for c in s.bytes() {
        let pos = alphabet.iter().position(|&x| x == c).unwrap_or(0);
        out.push(pos as i64);
        alphabet.remove(pos);
        alphabet.insert(0, c);
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::integer).collect()))
}
fn builtin_mtf_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let codes: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as usize).collect();
    let mut alphabet: Vec<u8> = (0..=255).collect();
    let mut out = Vec::new();
    for &p in &codes {
        let c = alphabet[p];
        out.push(c);
        alphabet.remove(p);
        alphabet.insert(0, c);
    }
    Ok(PerlValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_run_length_encode_str_b16(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut out: Vec<PerlValue> = Vec::new();
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        let mut cur = first; let mut cnt = 1_i64;
        for c in chars {
            if c == cur { cnt += 1; } else {
                out.push(PerlValue::array(vec![PerlValue::string(cur.to_string()), PerlValue::integer(cnt)]));
                cur = c; cnt = 1;
            }
        }
        out.push(PerlValue::array(vec![PerlValue::string(cur.to_string()), PerlValue::integer(cnt)]));
    }
    Ok(PerlValue::array(out))
}
fn builtin_lyndon_factorize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut out: Vec<PerlValue> = Vec::new();
    while i < n {
        let mut j = i + 1; let mut k = i;
        while j < n && bytes[k] <= bytes[j] {
            if bytes[k] < bytes[j] { k = i; } else { k += 1; }
            j += 1;
        }
        while i <= k {
            out.push(PerlValue::string(s[i..i + j - k].to_string()));
            i += j - k;
        }
    }
    Ok(PerlValue::array(out))
}
fn builtin_christoffel_word(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = i1(args).max(0); let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let mut out = String::new();
    let mut x = 0_i64;
    for _ in 0..(p + q) {
        x += p;
        if x >= q { out.push('1'); x -= q; } else { out.push('0'); }
    }
    Ok(PerlValue::string(out))
}
fn builtin_sturmian_word(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(20);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut out = String::new();
    for i in 0..n {
        let v = ((i as f64 + 1.0) * alpha + beta).floor() - (i as f64 * alpha + beta).floor();
        out.push(if v > 0.5 { '1' } else { '0' });
    }
    Ok(PerlValue::string(out))
}
fn builtin_z_function_alt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut z = vec![0_i64; n];
    if n == 0 { return Ok(PerlValue::array(vec![])); }
    z[0] = n as i64;
    let mut l = 0_usize; let mut r = 0_usize;
    for i in 1..n {
        if i < r { z[i] = (r - i).min(z[i - l] as usize) as i64; }
        while i + (z[i] as usize) < n && bytes[z[i] as usize] == bytes[i + (z[i] as usize)] { z[i] += 1; }
        if i + z[i] as usize > r { l = i; r = i + z[i] as usize; }
    }
    Ok(PerlValue::array(z.into_iter().map(PerlValue::integer).collect()))
}
fn builtin_period_of_string(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes(); let n = bytes.len();
    for p in 1..=n {
        if n % p == 0 && (0..n - p).all(|i| bytes[i] == bytes[i + p]) {
            return Ok(PerlValue::integer(p as i64));
        }
    }
    Ok(PerlValue::integer(n as i64))
}
fn builtin_borders_of_string(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes(); let n = bytes.len();
    let mut out: Vec<PerlValue> = Vec::new();
    for k in 1..n {
        if bytes[..k] == bytes[n - k..] { out.push(PerlValue::integer(k as i64)); }
    }
    Ok(PerlValue::array(out))
}
fn builtin_thue_morse_string(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0) as usize;
    let mut s = "0".to_string();
    while s.len() < n {
        let inv: String = s.chars().map(|c| if c == '0' { '1' } else { '0' }).collect();
        s.push_str(&inv);
    }
    Ok(PerlValue::string(s.chars().take(n).collect()))
}
fn builtin_fibonacci_word(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0) as usize;
    let mut a = "0".to_string(); let mut b = "01".to_string();
    while a.len() < n {
        let next = format!("{}{}", b, a);
        a = b;
        b = next;
    }
    Ok(PerlValue::string(a.chars().take(n).collect()))
}

// Statistics deep
fn builtin_mann_kendall_tau(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len(); if n < 2 { return Ok(PerlValue::float(0.0)); }
    let mut s = 0_i64;
    for i in 0..n - 1 { for j in i + 1..n {
        s += (xs[j] - xs[i]).signum() as i64;
    }}
    Ok(PerlValue::float(s as f64 * 2.0 / (n * (n - 1)) as f64))
}
fn builtin_theil_sen_slope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len().min(ys.len());
    let mut slopes: Vec<f64> = Vec::new();
    for i in 0..n - 1 { for j in i + 1..n {
        let dx = xs[j] - xs[i];
        if dx.abs() > 1e-30 { slopes.push((ys[j] - ys[i]) / dx); }
    }}
    slopes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = slopes.len();
    if m == 0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(if m % 2 == 1 { slopes[m / 2] } else { 0.5 * (slopes[m / 2 - 1] + slopes[m / 2]) }))
}
fn builtin_hodges_lehmann(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len(); if n == 0 { return Ok(PerlValue::float(0.0)); }
    let mut walsh: Vec<f64> = Vec::new();
    for i in 0..n { for j in i..n { walsh.push(0.5 * (xs[i] + xs[j])); }}
    walsh.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = walsh.len();
    Ok(PerlValue::float(if m % 2 == 1 { walsh[m / 2] } else { 0.5 * (walsh[m / 2 - 1] + walsh[m / 2]) }))
}
fn builtin_huber_m_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.345);
    let n = xs.len() as f64; if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    let mut mu: f64 = xs.iter().sum::<f64>() / n;
    for _ in 0..50 {
        let median: f64 = { let mut s = xs.clone(); s.sort_by(|a,b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)); s[s.len() / 2] };
        let mut deviations: Vec<f64> = xs.iter().map(|x| (x - median).abs()).collect();
        deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mad = deviations[deviations.len() / 2].max(1e-9) / 0.6745;
        let weights: Vec<f64> = xs.iter().map(|x| {
            let r = (x - mu) / mad;
            if r.abs() <= k { 1.0 } else { k / r.abs() }
        }).collect();
        let new_mu = xs.iter().zip(weights.iter()).map(|(x, w)| w * x).sum::<f64>()
            / weights.iter().sum::<f64>().max(1e-30);
        if (new_mu - mu).abs() < 1e-9 { mu = new_mu; break; }
        mu = new_mu;
    }
    Ok(PerlValue::float(mu))
}
fn builtin_winsorized_variance_arr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let frac = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    let k = (n as f64 * frac) as usize;
    if k > 0 && k * 2 < n {
        for i in 0..k { xs[i] = xs[k]; xs[n - 1 - i] = xs[n - 1 - k]; }
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let s: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum();
    Ok(PerlValue::float(s / n as f64))
}
fn builtin_bowley_skewness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len(); if n < 4 { return Ok(PerlValue::float(0.0)); }
    let q1 = xs[n / 4]; let q2 = xs[n / 2]; let q3 = xs[3 * n / 4];
    let denom = (q3 - q1).abs().max(1e-30);
    Ok(PerlValue::float((q1 + q3 - 2.0 * q2) / denom))
}
fn builtin_pearson_skewness_2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64; if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    let mean = xs.iter().sum::<f64>() / n;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let std = var.sqrt().max(1e-30);
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    Ok(PerlValue::float(3.0 * (mean - median) / std))
}
fn builtin_concordance_correlation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n; let my = ys.iter().sum::<f64>() / n;
    let vx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>() / n;
    let vy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>() / n;
    let cov: f64 = xs.iter().zip(ys.iter()).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / n;
    Ok(PerlValue::float(2.0 * cov / (vx + vy + (mx - my).powi(2)).max(1e-30)))
}
fn builtin_quantile_p(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len(); if n == 0 { return Ok(PerlValue::float(0.0)); }
    let idx = (p * (n as f64 - 1.0)).round() as usize;
    Ok(PerlValue::float(xs[idx.min(n - 1)]))
}

// Network analysis
fn builtin_label_propagation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = adj.len();
    let mut new_labels = labels.clone();
    for u in 0..n {
        let mut counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();
        for &v in &adj[u] { if v < n { *counts.entry(labels[v]).or_insert(0) += 1; } }
        if let Some((&l, _)) = counts.iter().max_by_key(|(_, c)| **c) { new_labels[u] = l; }
    }
    Ok(PerlValue::array(new_labels.into_iter().map(PerlValue::integer).collect()))
}
fn builtin_modularity_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = adj.len();
    let degrees: Vec<usize> = adj.iter().map(|nbrs| nbrs.len()).collect();
    let m = degrees.iter().sum::<usize>() as f64 / 2.0;
    if m < 1.0 { return Ok(PerlValue::float(0.0)); }
    let mut q = 0.0_f64;
    for u in 0..n { for v in 0..n {
        if labels[u] == labels[v] {
            let a_uv = if adj[u].contains(&v) { 1.0 } else { 0.0 };
            q += a_uv - degrees[u] as f64 * degrees[v] as f64 / (2.0 * m);
        }
    }}
    Ok(PerlValue::float(q / (2.0 * m)))
}
fn builtin_clique_count_3(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|n| n.iter().copied().collect()).collect();
    let mut count = 0_i64;
    for i in 0..n { for &j in &adj[i] { if j > i {
        for &k in &adj[j] { if k > j && sets[i].contains(&k) { count += 1; } }
    }}}
    Ok(PerlValue::integer(count))
}
fn builtin_local_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut total = 0.0_f64;
    let mut count = 0_usize;
    for u in 0..n {
        let nbrs = &adj[u];
        if nbrs.len() < 2 { continue; }
        let nbr_set: std::collections::HashSet<usize> = nbrs.iter().copied().collect();
        let mut e = 0.0_f64;
        for &i in nbrs { for &j in nbrs {
            if i < j {
                let nbrs_i: std::collections::HashSet<usize> = adj[i].iter().copied().collect();
                if nbrs_i.contains(&j) { e += 1.0; }
            }
        }}
        let _ = nbr_set;
        let nn = nbrs.len() as f64;
        e /= nn * (nn - 1.0) / 2.0;
        total += e;
        count += 1;
    }
    Ok(PerlValue::float(if count == 0 { 0.0 } else { total / count as f64 }))
}
fn builtin_global_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len(); if n < 2 { return Ok(PerlValue::float(0.0)); }
    let mut total = 0.0_f64;
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        let mut q = std::collections::VecDeque::new();
        dist[s] = 0; q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] { if v < n && dist[v] == -1 { dist[v] = dist[u] + 1; q.push_back(v); } }
        }
        for v in 0..n { if v != s && dist[v] > 0 { total += 1.0 / dist[v] as f64; } }
    }
    Ok(PerlValue::float(total / (n * (n - 1)) as f64))
}
fn builtin_diameter_unweighted(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut max_d = 0_i64;
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        let mut q = std::collections::VecDeque::new();
        dist[s] = 0; q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] { if v < n && dist[v] == -1 { dist[v] = dist[u] + 1; q.push_back(v); max_d = max_d.max(dist[v]); } }
        }
    }
    Ok(PerlValue::integer(max_d))
}

// Numerics acceleration
fn builtin_aitken_delta_squared(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len(); if n < 3 { return Ok(PerlValue::array(vec![])); }
    let out: Vec<PerlValue> = (0..n - 2).map(|i| {
        let denom = xs[i + 2] - 2.0 * xs[i + 1] + xs[i];
        if denom.abs() < 1e-30 { PerlValue::float(xs[i + 2]) }
        else { PerlValue::float(xs[i] - (xs[i + 1] - xs[i]).powi(2) / denom) }
    }).collect();
    Ok(PerlValue::array(out))
}
fn builtin_wynn_epsilon(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len(); if n == 0 { return Ok(PerlValue::float(0.0)); }
    let mut e = vec![vec![0.0_f64; n + 1]; n + 1];
    for i in 0..n { e[i][1] = xs[i]; }
    for k in 2..=n { for i in 0..n + 1 - k {
        let denom = e[i + 1][k - 1] - e[i][k - 1];
        if denom.abs() < 1e-30 { e[i][k] = f64::INFINITY; } else { e[i][k] = e.get(i + 1).and_then(|r| r.get(k - 2)).copied().unwrap_or(0.0) + 1.0 / denom; }
    }}
    Ok(PerlValue::float(e[0][n]))
}
fn builtin_shanks_transform(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_aitken_delta_squared(args)
}
fn builtin_levin_t_transform(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len(); if n < 3 { return Ok(PerlValue::float(if n > 0 { xs[n - 1] } else { 0.0 })); }
    let omega: Vec<f64> = (0..n - 1).map(|i| xs[i + 1] - xs[i]).collect();
    let mut numer = 0.0_f64; let mut denom = 0.0_f64;
    for j in 0..omega.len() {
        let w = if j == 0 { 0.0 } else { (j + 1) as f64 / omega[j].abs().max(1e-30) };
        numer += w * xs[j + 1];
        denom += w;
    }
    Ok(PerlValue::float(if denom.abs() < 1e-30 { xs[n - 1] } else { numer / denom }))
}
fn builtin_harmonic_seq_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let mut s = 0.0_f64;
    for k in 1..=n { s += 1.0 / k as f64; }
    Ok(PerlValue::float(s))
}
fn builtin_alternating_seq_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s: f64 = xs.iter().enumerate().map(|(i, x)| if i & 1 == 0 { *x } else { -x }).sum();
    Ok(PerlValue::float(s))
}
