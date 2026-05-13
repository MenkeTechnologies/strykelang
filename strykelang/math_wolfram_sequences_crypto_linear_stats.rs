// denser long-tail expansion: more sequences, more crypto helpers,
// more linear algebra atoms, more stats, more probability, more strings.

// ── Sequences ────────────────────────────────────────────────────────────────

fn builtin_lazy_caterer(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer((n * n + n + 2) / 2))
}
fn builtin_central_polygonal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer((n * n - n + 2) / 2))
}
fn builtin_centered_square(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * n + (n - 1) * (n - 1)))
}
fn builtin_centered_triangular(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer((3 * n * n - 3 * n + 2) / 2))
}
fn builtin_centered_pentagonal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer((5 * n * n - 5 * n + 2) / 2))
}
fn builtin_star_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(6 * n * (n - 1) + 1))
}
fn builtin_dodecahedral_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (3 * n - 1) * (3 * n - 2) / 2))
}
fn builtin_icosahedral_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (5 * n * n - 5 * n + 2) / 2))
}
fn builtin_pronic_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (n + 1)))
}
fn builtin_squared_triangular(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let t = n * (n + 1) / 2;
    Ok(StrykeValue::integer(t * t))
}
fn builtin_woodall_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (1_i64 << n) - 1))
}
fn builtin_cullen_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (1_i64 << n) + 1))
}
fn builtin_repunit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let s = "1".repeat(n as usize);
    Ok(StrykeValue::string(s))
}
fn builtin_repdigit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = args.first().map(|v| v.to_number() as i64).unwrap_or(0).rem_euclid(10);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let s: String = (0..n).map(|_| char::from_digit(d as u32, 10).unwrap_or('0')).collect();
    Ok(StrykeValue::string(s))
}
fn builtin_kaprekar_routine_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let s = format!("{:04}", n);
    let mut chars: Vec<char> = s.chars().collect();
    chars.sort_by(|a, b| b.cmp(a));
    let desc: String = chars.iter().collect();
    chars.sort();
    let asc: String = chars.iter().collect();
    let d: i64 = desc.parse().unwrap_or(0);
    let a: i64 = asc.parse().unwrap_or(0);
    Ok(StrykeValue::integer(d - a))
}
fn builtin_smith_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 2 || is_prime_check(n) {
        return Ok(StrykeValue::integer(0));
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
    Ok(StrykeValue::integer(if n_sum == factor_sum { 1 } else { 0 }))
}
fn builtin_keith_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 10 {
        return Ok(StrykeValue::integer(0));
    }
    let digits: Vec<i64> = n.to_string().chars().map(|c| c.to_digit(10).unwrap_or(0) as i64).collect();
    let k = digits.len();
    let mut window: Vec<i64> = digits;
    while *window.last().unwrap() < n {
        let s: i64 = window.iter().sum();
        window.remove(0);
        window.push(s);
        if s == n {
            return Ok(StrykeValue::integer(1));
        }
    }
    let _ = k;
    Ok(StrykeValue::integer(0))
}
fn builtin_armstrong_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(StrykeValue::integer(0));
    }
    let digits: Vec<i64> = n.to_string().chars().map(|c| c.to_digit(10).unwrap_or(0) as i64).collect();
    let k = digits.len() as u32;
    let s: i64 = digits.iter().map(|&d| d.pow(k)).sum();
    Ok(StrykeValue::integer(if s == n { 1 } else { 0 }))
}

// ── Crypto / hashing ────────────────────────────────────────────────────────

fn builtin_fnv1a_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(StrykeValue::integer(h as i64))
}
fn builtin_djb2_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    Ok(StrykeValue::integer(h as i64))
}
fn builtin_jenkins_one_at_a_time(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    Ok(StrykeValue::integer(h as i64))
}
fn builtin_murmurhash3_x32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    Ok(StrykeValue::integer(h as i64))
}
fn builtin_adler32_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in s.bytes() {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    Ok(StrykeValue::integer(((b << 16) | a) as i64))
}
fn builtin_crc16_ccitt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    Ok(StrykeValue::integer(crc as i64))
}

// ── Linear-algebra atoms ────────────────────────────────────────────────────

fn builtin_l1_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(StrykeValue::float(xs.iter().map(|v| v.abs()).sum()))
}
fn builtin_l2_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(StrykeValue::float(xs.iter().map(|v| v * v).sum::<f64>().sqrt()))
}
fn builtin_linf_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(StrykeValue::float(xs.iter().map(|v| v.abs()).fold(0.0_f64, f64::max)))
}
fn builtin_lp_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let s: f64 = xs.iter().map(|v| v.abs().powf(p)).sum();
    Ok(StrykeValue::float(s.powf(1.0 / p)))
}
fn builtin_unit_vector(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-30);
    Ok(StrykeValue::array(xs.into_iter().map(|v| StrykeValue::float(v / n)).collect()))
}
fn builtin_vector_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let bb: f64 = b.iter().map(|v| v * v).sum::<f64>().max(1e-30);
    let ab: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let scale = ab / bb;
    Ok(StrykeValue::array(b.into_iter().map(|v| StrykeValue::float(scale * v)).collect()))
}
fn builtin_vector_reject(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let bb: f64 = b.iter().map(|v| v * v).sum::<f64>().max(1e-30);
    let ab: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let scale = ab / bb;
    Ok(StrykeValue::array(
        a.iter().zip(b.iter()).map(|(x, y)| StrykeValue::float(x - scale * y)).collect(),
    ))
}
fn builtin_orthogonalize_vectors(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let raw = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
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
    Ok(StrykeValue::array(
        basis.into_iter()
            .map(|v| StrykeValue::array(v.into_iter().map(StrykeValue::float).collect()))
            .collect(),
    ))
}
fn builtin_outer_product(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let m: Vec<Vec<f64>> = a.iter().map(|&x| b.iter().map(|&y| x * y).collect()).collect();
    Ok(matrix_to_value(&m))
}
fn builtin_matrix_diagonal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    let diag: Vec<StrykeValue> = (0..n)
        .filter(|&i| i < m[i].len())
        .map(|i| StrykeValue::float(m[i][i]))
        .collect();
    Ok(StrykeValue::array(diag))
}
fn builtin_matrix_anti_diagonal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    let mut out = Vec::new();
    for i in 0..n {
        let j = n - 1 - i;
        if j < m[i].len() {
            out.push(StrykeValue::float(m[i][j]));
        }
    }
    Ok(StrykeValue::array(out))
}
fn builtin_matrix_symmetric_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return Ok(StrykeValue::integer(0));
    }
    for i in 0..n {
        for j in i + 1..n {
            if (m[i][j] - m[j][i]).abs() > 1e-12 {
                return Ok(StrykeValue::integer(0));
            }
        }
    }
    Ok(StrykeValue::integer(1))
}
fn builtin_matrix_orthogonal_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return Ok(StrykeValue::integer(0));
    }
    for i in 0..n {
        for j in i..n {
            let mut s = 0.0_f64;
            for k in 0..n {
                s += m[i][k] * m[j][k];
            }
            let target = if i == j { 1.0 } else { 0.0 };
            if (s - target).abs() > 1e-9 {
                return Ok(StrykeValue::integer(0));
            }
        }
    }
    Ok(StrykeValue::integer(1))
}

// ── Stats / probability ─────────────────────────────────────────────────────

fn builtin_geometric_mean_arr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let log_sum: f64 = xs.iter().filter(|&&x| x > 0.0).map(|x| x.ln()).sum();
    Ok(StrykeValue::float((log_sum / xs.len() as f64).exp()))
}
fn builtin_harmonic_mean_arr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64;
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = xs.iter().filter(|&&x| x.abs() > 1e-30).map(|x| 1.0 / x).sum();
    Ok(StrykeValue::float(n / s))
}
fn builtin_quadratic_mean_arr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len() as f64;
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = xs.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float((s / n).sqrt()))
}
fn builtin_lehmer_mean(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let num: f64 = xs.iter().map(|x| x.powf(p)).sum();
    let den: f64 = xs.iter().map(|x| x.powf(p - 1.0)).sum::<f64>().max(1e-30);
    Ok(StrykeValue::float(num / den))
}
fn builtin_running_mean(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut sum = 0.0_f64;
    let out: Vec<StrykeValue> = xs.iter().enumerate().map(|(i, &x)| {
        sum += x;
        StrykeValue::float(sum / (i + 1) as f64)
    }).collect();
    Ok(StrykeValue::array(out))
}
fn builtin_running_variance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut mean = 0.0_f64;
    let mut m2 = 0.0_f64;
    let mut out = Vec::with_capacity(xs.len());
    for (i, &x) in xs.iter().enumerate() {
        let n = (i + 1) as f64;
        let delta = x - mean;
        mean += delta / n;
        m2 += delta * (x - mean);
        out.push(StrykeValue::float(if i == 0 { 0.0 } else { m2 / i as f64 }));
    }
    Ok(StrykeValue::array(out))
}
fn builtin_outlier_iqr_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n < 4 { return Ok(StrykeValue::integer(0)); }
    let q1 = xs[n / 4];
    let q3 = xs[3 * n / 4];
    let iqr = q3 - q1;
    let outlier = target < q1 - 1.5 * iqr || target > q3 + 1.5 * iqr;
    Ok(StrykeValue::integer(if outlier { 1 } else { 0 }))
}
fn builtin_z_score_robust(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len();
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let median = xs[n / 2];
    let mut deviations: Vec<f64> = xs.iter().map(|x| (x - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = deviations[n / 2].max(1e-30);
    Ok(StrykeValue::float(0.6745 * (target - median) / mad))
}
fn builtin_geometric_sequence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut out = Vec::with_capacity(n);
    let mut x = a;
    for _ in 0..n {
        out.push(StrykeValue::float(x));
        x *= r;
    }
    Ok(StrykeValue::array(out))
}
fn builtin_arithmetic_sequence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(StrykeValue::array(
        (0..n).map(|i| StrykeValue::float(a + d * i as f64)).collect(),
    ))
}
fn builtin_log_sum_exp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    let m = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = xs.iter().map(|x| (x - m).exp()).sum();
    Ok(StrykeValue::float(m + s.ln()))
}
fn builtin_log_sigmoid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x >= 0.0 {
        -(-x).exp().ln_1p()
    } else {
        x - x.exp().ln_1p()
    }))
}
fn builtin_log1p_exp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x > 30.0 { x } else { x.exp().ln_1p() }))
}

// ── Strings ─────────────────────────────────────────────────────────────────

fn builtin_string_chars(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::array(s.chars().map(|c| StrykeValue::string(c.to_string())).collect()))
}
fn builtin_string_words_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::integer(s.split_whitespace().count() as i64))
}
fn builtin_string_lines_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::integer(s.lines().count() as i64))
}
fn builtin_string_intersperse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let sep = args.get(1).map(|v| v.to_string()).unwrap_or_else(|| " ".to_string());
    let chars: Vec<String> = s.chars().map(|c| c.to_string()).collect();
    Ok(StrykeValue::string(chars.join(&sep)))
}
fn builtin_string_replicate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(StrykeValue::string(s.repeat(n)))
}
fn builtin_string_uniq_chars(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for c in s.chars() {
        if seen.insert(c) {
            out.push(c);
        }
    }
    Ok(StrykeValue::string(out))
}
fn builtin_string_letter_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut counts: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    for c in s.chars().filter(|c| c.is_alphabetic()) {
        *counts.entry(c.to_ascii_lowercase()).or_insert(0) += 1;
    }
    let mut keys: Vec<char> = counts.keys().copied().collect();
    keys.sort();
    let pairs: Vec<StrykeValue> = keys.into_iter().map(|c| {
        StrykeValue::array(vec![
            StrykeValue::string(c.to_string()),
            StrykeValue::integer(counts[&c] as i64),
        ])
    }).collect();
    Ok(StrykeValue::array(pairs))
}
fn builtin_anagram_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let mut ca: Vec<char> = a.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    let mut cb: Vec<char> = b.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_lowercase()).collect();
    ca.sort();
    cb.sort();
    Ok(StrykeValue::integer(if ca == cb { 1 } else { 0 }))
}
fn builtin_string_take_while(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let target_set: std::collections::HashSet<char> = args
        .get(1).map(|v| v.to_string()).unwrap_or_default().chars().collect();
    let out: String = s.chars().take_while(|c| target_set.contains(c)).collect();
    Ok(StrykeValue::string(out))
}
fn builtin_string_drop_while(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let target_set: std::collections::HashSet<char> = args
        .get(1).map(|v| v.to_string()).unwrap_or_default().chars().collect();
    let out: String = s.chars().skip_while(|c| target_set.contains(c)).collect();
    Ok(StrykeValue::string(out))
}
fn builtin_string_split_at_first(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let sep = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if sep.is_empty() {
        return Ok(StrykeValue::array(vec![StrykeValue::string(s.clone()), StrykeValue::string(String::new())]));
    }
    if let Some(idx) = s.find(&sep) {
        Ok(StrykeValue::array(vec![
            StrykeValue::string(s[..idx].to_string()),
            StrykeValue::string(s[idx + sep.len()..].to_string()),
        ]))
    } else {
        Ok(StrykeValue::array(vec![
            StrykeValue::string(s),
            StrykeValue::string(String::new()),
        ]))
    }
}
fn builtin_string_partition_at_word(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let words: Vec<&str> = s.split_whitespace().collect();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let head = words.iter().take(n).copied().collect::<Vec<_>>().join(" ");
    let tail = words.iter().skip(n).copied().collect::<Vec<_>>().join(" ");
    Ok(StrykeValue::array(vec![StrykeValue::string(head), StrykeValue::string(tail)]))
}
