// Batch 11 — denser long-tail expansion: more sequences, more crypto helpers,
// more linear algebra atoms, more stats, more probability, more strings.

// ── Sequences ────────────────────────────────────────────────────────────────

fn builtin_lazy_caterer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer((n * n + n + 2) / 2))
}
fn builtin_central_polygonal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer((n * n - n + 2) / 2))
}
fn builtin_centered_square(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * n + (n - 1) * (n - 1)))
}
fn builtin_centered_triangular(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer((3 * n * n - 3 * n + 2) / 2))
}
fn builtin_centered_pentagonal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer((5 * n * n - 5 * n + 2) / 2))
}
fn builtin_star_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(6 * n * (n - 1) + 1))
}
fn builtin_dodecahedral_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * (3 * n - 1) * (3 * n - 2) / 2))
}
fn builtin_icosahedral_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * (5 * n * n - 5 * n + 2) / 2))
}
fn builtin_pronic_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * (n + 1)))
}
fn builtin_squared_triangular(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let t = n * (n + 1) / 2;
    Ok(PerlValue::integer(t * t))
}
fn builtin_woodall_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * (1_i64 << n) - 1))
}
fn builtin_cullen_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * (1_i64 << n) + 1))
}
fn builtin_repunit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let s = "1".repeat(n as usize);
    Ok(PerlValue::string(s))
}
fn builtin_repdigit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = args.first().map(|v| v.to_number() as i64).unwrap_or(0).rem_euclid(10);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let s: String = (0..n).map(|_| char::from_digit(d as u32, 10).unwrap_or('0')).collect();
    Ok(PerlValue::string(s))
}
fn builtin_kaprekar_routine_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let s = format!("{:04}", n);
    let mut chars: Vec<char> = s.chars().collect();
    chars.sort_by(|a, b| b.cmp(a));
    let desc: String = chars.iter().collect();
    chars.sort();
    let asc: String = chars.iter().collect();
    let d: i64 = desc.parse().unwrap_or(0);
    let a: i64 = asc.parse().unwrap_or(0);
    Ok(PerlValue::integer(d - a))
}
fn builtin_smith_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 2 || is_prime_check(n) {
        return Ok(PerlValue::integer(0));
    }
    let digit_sum = |x: i64| -> i64 {
        let mut s = 0_i64;
        let mut y = x;
        while y > 0 {
            s += y % 10;
            y /= 10;
        }
        s
    };
    let n_sum = digit_sum(n);
    let factor_sum: i64 = prime_factorize(n).iter().map(|&p| digit_sum(p)).sum();
    Ok(PerlValue::integer(if n_sum == factor_sum { 1 } else { 0 }))
}
fn builtin_keith_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 10 {
        return Ok(PerlValue::integer(0));
    }
    let digits: Vec<i64> = n.to_string().chars().map(|c| c.to_digit(10).unwrap_or(0) as i64).collect();
    let k = digits.len();
    let mut window: Vec<i64> = digits;
    while *window.last().unwrap() < n {
        let s: i64 = window.iter().sum();
        window.remove(0);
        window.push(s);
        if s == n {
            return Ok(PerlValue::integer(1));
        }
    }
    let _ = k;
    Ok(PerlValue::integer(0))
}
fn builtin_armstrong_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let digits: Vec<i64> = n.to_string().chars().map(|c| c.to_digit(10).unwrap_or(0) as i64).collect();
    let k = digits.len() as u32;
    let s: i64 = digits.iter().map(|&d| d.pow(k)).sum();
    Ok(PerlValue::integer(if s == n { 1 } else { 0 }))
}

// ── Crypto / hashing ────────────────────────────────────────────────────────

fn builtin_fnv1a_hash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(PerlValue::integer(h as i64))
}
fn builtin_djb2_hash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    Ok(PerlValue::integer(h as i64))
}
fn builtin_jenkins_one_at_a_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h: u32 = 0;
    for b in s.bytes() {
        h = h.wrapping_add(b as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h = h.wrapping_add(h << 15);
    Ok(PerlValue::integer(h as i64))
}
fn builtin_murmurhash3_x32(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let seed = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut h: u32 = seed;
    let mut i = 0_usize;
    while i + 4 <= n {
        let mut k = u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        k = k.wrapping_mul(0xcc9e2d51);
        k = k.rotate_left(15);
        k = k.wrapping_mul(0x1b873593);
        h ^= k;
        h = h.rotate_left(13);
        h = h.wrapping_mul(5).wrapping_add(0xe6546b64);
        i += 4;
    }
    let mut tail: u32 = 0;
    let rem = n - i;
    if rem >= 3 {
        tail ^= (bytes[i + 2] as u32) << 16;
    }
    if rem >= 2 {
        tail ^= (bytes[i + 1] as u32) << 8;
    }
    if rem >= 1 {
        tail ^= bytes[i] as u32;
        tail = tail.wrapping_mul(0xcc9e2d51);
        tail = tail.rotate_left(15);
        tail = tail.wrapping_mul(0x1b873593);
        h ^= tail;
    }
    h ^= n as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    Ok(PerlValue::integer(h as i64))
}
fn builtin_adler32_hash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in s.bytes() {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    Ok(PerlValue::integer(((b << 16) | a) as i64))
}
fn builtin_crc16_ccitt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut crc: u16 = 0xFFFF;
    for byte in s.bytes() {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    Ok(PerlValue::integer(crc as i64))
}

// ── Linear-algebra atoms ────────────────────────────────────────────────────

fn builtin_l1_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(xs.iter().map(|v| v.abs()).sum()))
}
fn builtin_l2_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(xs.iter().map(|v| v * v).sum::<f64>().sqrt()))
}
fn builtin_linf_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(xs.iter().map(|v| v.abs()).fold(0.0_f64, f64::max)))
}
fn builtin_lp_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let s: f64 = xs.iter().map(|v| v.abs().powf(p)).sum();
    Ok(PerlValue::float(s.powf(1.0 / p)))
}
fn builtin_unit_vector(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-30);
    Ok(PerlValue::array(xs.into_iter().map(|v| PerlValue::float(v / n)).collect()))
}
fn builtin_vector_project(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let bb: f64 = b.iter().map(|v| v * v).sum::<f64>().max(1e-30);
    let ab: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let scale = ab / bb;
    Ok(PerlValue::array(b.into_iter().map(|v| PerlValue::float(scale * v)).collect()))
}
fn builtin_vector_reject(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let bb: f64 = b.iter().map(|v| v * v).sum::<f64>().max(1e-30);
    let ab: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let scale = ab / bb;
    Ok(PerlValue::array(
        a.iter().zip(b.iter()).map(|(x, y)| PerlValue::float(x - scale * y)).collect(),
    ))
}
fn builtin_orthogonalize_vectors(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let raw = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for v in &raw {
        let mut u: Vec<f64> = arg_to_vec(v).iter().map(|x| x.to_number()).collect();
        for w in &basis {
            let dot: f64 = u.iter().zip(w.iter()).map(|(x, y)| x * y).sum();
            for (xi, wi) in u.iter_mut().zip(w.iter()) {
                *xi -= dot * wi;
            }
        }
        let norm: f64 = u.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-12 {
            for x in u.iter_mut() {
                *x /= norm;
            }
            basis.push(u);
        }
    }
    Ok(PerlValue::array(
        basis.into_iter()
            .map(|v| PerlValue::array(v.into_iter().map(PerlValue::float).collect()))
            .collect(),
    ))
}
fn builtin_outer_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let m: Vec<Vec<f64>> = a.iter().map(|&x| b.iter().map(|&y| x * y).collect()).collect();
    Ok(matrix_to_value(&m))
}
fn builtin_matrix_diagonal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    let diag: Vec<PerlValue> = (0..n)
        .filter(|&i| i < m[i].len())
        .map(|i| PerlValue::float(m[i][i]))
        .collect();
    Ok(PerlValue::array(diag))
}
fn builtin_matrix_anti_diagonal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    let mut out = Vec::new();
    for i in 0..n {
        let j = n - 1 - i;
        if j < m[i].len() {
            out.push(PerlValue::float(m[i][j]));
        }
    }
    Ok(PerlValue::array(out))
}
fn builtin_matrix_symmetric_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return Ok(PerlValue::integer(0));
    }
    for i in 0..n {
        for j in i + 1..n {
            if (m[i][j] - m[j][i]).abs() > 1e-12 {
                return Ok(PerlValue::integer(0));
            }
        }
    }
    Ok(PerlValue::integer(1))
}
fn builtin_matrix_orthogonal_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return Ok(PerlValue::integer(0));
    }
    for i in 0..n {
        for j in i..n {
            let mut s = 0.0_f64;
            for k in 0..n {
                s += m[i][k] * m[j][k];
            }
            let target = if i == j { 1.0 } else { 0.0 };
            if (s - target).abs() > 1e-9 {
                return Ok(PerlValue::integer(0));
            }
        }
    }
    Ok(PerlValue::integer(1))
}

// ── Stats / probability ─────────────────────────────────────────────────────

fn builtin_geometric_mean_arr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(PerlValue::float(0.0)); }
    let log_sum: f64 = xs.iter().filter(|&&x| x > 0.0).map(|x| x.ln()).sum();
    Ok(PerlValue::float((log_sum / xs.len() as f64).exp()))
}
fn builtin_harmonic_mean_arr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64;
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = xs.iter().filter(|&&x| x.abs() > 1e-30).map(|x| 1.0 / x).sum();
    Ok(PerlValue::float(n / s))
}
fn builtin_quadratic_mean_arr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64;
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = xs.iter().map(|x| x * x).sum();
    Ok(PerlValue::float((s / n).sqrt()))
}
fn builtin_lehmer_mean(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let num: f64 = xs.iter().map(|x| x.powf(p)).sum();
    let den: f64 = xs.iter().map(|x| x.powf(p - 1.0)).sum::<f64>().max(1e-30);
    Ok(PerlValue::float(num / den))
}
fn builtin_running_mean(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut sum = 0.0_f64;
    let out: Vec<PerlValue> = xs.iter().enumerate().map(|(i, &x)| {
        sum += x;
        PerlValue::float(sum / (i + 1) as f64)
    }).collect();
    Ok(PerlValue::array(out))
}
fn builtin_running_variance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut mean = 0.0_f64;
    let mut m2 = 0.0_f64;
    let mut out = Vec::with_capacity(xs.len());
    for (i, &x) in xs.iter().enumerate() {
        let n = (i + 1) as f64;
        let delta = x - mean;
        mean += delta / n;
        m2 += delta * (x - mean);
        out.push(PerlValue::float(if i == 0 { 0.0 } else { m2 / i as f64 }));
    }
    Ok(PerlValue::array(out))
}
fn builtin_outlier_iqr_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n < 4 { return Ok(PerlValue::integer(0)); }
    let q1 = xs[n / 4];
    let q3 = xs[3 * n / 4];
    let iqr = q3 - q1;
    let outlier = target < q1 - 1.5 * iqr || target > q3 + 1.5 * iqr;
    Ok(PerlValue::integer(if outlier { 1 } else { 0 }))
}
fn builtin_z_score_robust(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let median = xs[n / 2];
    let mut deviations: Vec<f64> = xs.iter().map(|x| (x - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = deviations[n / 2].max(1e-30);
    Ok(PerlValue::float(0.6745 * (target - median) / mad))
}
fn builtin_geometric_sequence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut out = Vec::with_capacity(n);
    let mut x = a;
    for _ in 0..n {
        out.push(PerlValue::float(x));
        x *= r;
    }
    Ok(PerlValue::array(out))
}
fn builtin_arithmetic_sequence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(PerlValue::array(
        (0..n).map(|i| PerlValue::float(a + d * i as f64)).collect(),
    ))
}
fn builtin_log_sum_exp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    let m = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = xs.iter().map(|x| (x - m).exp()).sum();
    Ok(PerlValue::float(m + s.ln()))
}
fn builtin_log_sigmoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(if x >= 0.0 {
        -(-x).exp().ln_1p()
    } else {
        x - x.exp().ln_1p()
    }))
}
fn builtin_log1p_exp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(if x > 30.0 { x } else { x.exp().ln_1p() }))
}

// ── Strings ─────────────────────────────────────────────────────────────────

fn builtin_string_chars(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::array(s.chars().map(|c| PerlValue::string(c.to_string())).collect()))
}
fn builtin_string_words_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::integer(s.split_whitespace().count() as i64))
}
fn builtin_string_lines_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::integer(s.lines().count() as i64))
}
fn builtin_string_intersperse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let sep = args.get(1).map(|v| v.to_string()).unwrap_or_else(|| " ".to_string());
    let chars: Vec<String> = s.chars().map(|c| c.to_string()).collect();
    Ok(PerlValue::string(chars.join(&sep)))
}
fn builtin_string_replicate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(PerlValue::string(s.repeat(n)))
}
fn builtin_string_uniq_chars(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for c in s.chars() {
        if seen.insert(c) {
            out.push(c);
        }
    }
    Ok(PerlValue::string(out))
}
fn builtin_string_letter_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut counts: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    for c in s.chars().filter(|c| c.is_alphabetic()) {
        *counts.entry(c.to_ascii_lowercase()).or_insert(0) += 1;
    }
    let mut keys: Vec<char> = counts.keys().copied().collect();
    keys.sort();
    let pairs: Vec<PerlValue> = keys.into_iter().map(|c| {
        PerlValue::array(vec![
            PerlValue::string(c.to_string()),
            PerlValue::integer(counts[&c] as i64),
        ])
    }).collect();
    Ok(PerlValue::array(pairs))
}
fn builtin_anagram_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let mut ca: Vec<char> = a.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    let mut cb: Vec<char> = b.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    ca.sort();
    cb.sort();
    Ok(PerlValue::integer(if ca == cb { 1 } else { 0 }))
}
fn builtin_string_take_while(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let target_set: std::collections::HashSet<char> = args
        .get(1).map(|v| v.to_string()).unwrap_or_default().chars().collect();
    let out: String = s.chars().take_while(|c| target_set.contains(c)).collect();
    Ok(PerlValue::string(out))
}
fn builtin_string_drop_while(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let target_set: std::collections::HashSet<char> = args
        .get(1).map(|v| v.to_string()).unwrap_or_default().chars().collect();
    let out: String = s.chars().skip_while(|c| target_set.contains(c)).collect();
    Ok(PerlValue::string(out))
}
fn builtin_string_split_at_first(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let sep = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if sep.is_empty() {
        return Ok(PerlValue::array(vec![PerlValue::string(s.clone()), PerlValue::string(String::new())]));
    }
    if let Some(idx) = s.find(&sep) {
        Ok(PerlValue::array(vec![
            PerlValue::string(s[..idx].to_string()),
            PerlValue::string(s[idx + sep.len()..].to_string()),
        ]))
    } else {
        Ok(PerlValue::array(vec![
            PerlValue::string(s),
            PerlValue::string(String::new()),
        ]))
    }
}
fn builtin_string_partition_at_word(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let words: Vec<&str> = s.split_whitespace().collect();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let head = words.iter().take(n).copied().collect::<Vec<_>>().join(" ");
    let tail = words.iter().skip(n).copied().collect::<Vec<_>>().join(" ");
    Ok(PerlValue::array(vec![PerlValue::string(head), PerlValue::string(tail)]))
}
