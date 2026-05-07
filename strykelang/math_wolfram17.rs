// Batch 17 — sparse linear algebra, advanced geometry, more distributions.

fn builtin_sparse_csr_build(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut values: Vec<f64> = Vec::new();
    let mut col_idx: Vec<i64> = Vec::new();
    let mut row_ptr: Vec<i64> = vec![0];
    for row in &m {
        for (j, &v) in row.iter().enumerate() {
            if v.abs() > 1e-30 { values.push(v); col_idx.push(j as i64); }
        }
        row_ptr.push(values.len() as i64);
    }
    Ok(PerlValue::array(vec![
        PerlValue::array(values.into_iter().map(PerlValue::float).collect()),
        PerlValue::array(col_idx.into_iter().map(PerlValue::integer).collect()),
        PerlValue::array(row_ptr.into_iter().map(PerlValue::integer).collect()),
    ]))
}
fn builtin_sparse_csr_mul_vec(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let csr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let values: Vec<f64> = arg_to_vec(&csr.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let col_idx: Vec<usize> = arg_to_vec(&csr.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as usize).collect();
    let row_ptr: Vec<usize> = arg_to_vec(&csr.get(2).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as usize).collect();
    let x: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = row_ptr.len() - 1;
    let mut y = vec![0.0_f64; n];
    for i in 0..n {
        for k in row_ptr[i]..row_ptr[i + 1] {
            if col_idx[k] < x.len() { y[i] += values[k] * x[col_idx[k]]; }
        }
    }
    Ok(PerlValue::array(y.into_iter().map(PerlValue::float).collect()))
}
fn builtin_sparse_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let total: usize = m.iter().map(|r| r.len()).sum();
    if total == 0 { return Ok(PerlValue::float(0.0)); }
    let nz: usize = m.iter().flat_map(|r| r.iter()).filter(|&&v| v.abs() > 1e-30).count();
    Ok(PerlValue::float(nz as f64 / total as f64))
}
fn builtin_lower_triangular_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    for i in 0..n { for j in i + 1..m[i].len() {
        if m[i][j].abs() > 1e-12 { return Ok(PerlValue::integer(0)); }
    }}
    Ok(PerlValue::integer(1))
}
fn builtin_upper_triangular_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    for i in 0..n { for j in 0..i.min(m[i].len()) {
        if m[i][j].abs() > 1e-12 { return Ok(PerlValue::integer(0)); }
    }}
    Ok(PerlValue::integer(1))
}
fn builtin_diagonal_dominance_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    for i in 0..n {
        let off_sum: f64 = (0..m[i].len()).filter(|&j| j != i).map(|j| m[i][j].abs()).sum();
        if m[i][i].abs() <= off_sum { return Ok(PerlValue::integer(0)); }
    }
    Ok(PerlValue::integer(1))
}
fn builtin_matrix_zero_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    Ok(PerlValue::integer(if m.iter().flatten().all(|v| v.abs() < 1e-12) { 1 } else { 0 }))
}
fn builtin_matrix_identity_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = m.len();
    if n == 0 { return Ok(PerlValue::integer(1)); }
    if m[0].len() != n { return Ok(PerlValue::integer(0)); }
    for i in 0..n { for j in 0..n {
        let target = if i == j { 1.0 } else { 0.0 };
        if (m[i][j] - target).abs() > 1e-12 { return Ok(PerlValue::integer(0)); }
    }}
    Ok(PerlValue::integer(1))
}
fn builtin_matrix_random_uniform(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use rand::Rng;
    let r = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let c = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let lo = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let hi = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let m: Vec<Vec<f64>> = (0..r).map(|_| (0..c).map(|_| rng.gen_range(lo..hi)).collect()).collect();
    Ok(matrix_to_value(&m))
}
fn builtin_matrix_random_normal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use rand::Rng;
    let r = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let c = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let m: Vec<Vec<f64>> = (0..r).map(|_| (0..c).map(|_| {
        let u1: f64 = rng.gen_range(1e-300..1.0);
        let u2: f64 = rng.gen();
        mu + sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }).collect()).collect();
    Ok(matrix_to_value(&m))
}

// Geometry advanced
fn builtin_andrew_monotone_chain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(
        a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 { lower.pop(); }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 { upper.pop(); }
        upper.push(p);
    }
    lower.pop(); upper.pop();
    lower.extend(upper);
    Ok(PerlValue::array(lower.into_iter().map(|(x, y)| {
        PerlValue::array(vec![PerlValue::float(x), PerlValue::float(y)])
    }).collect()))
}
fn builtin_polygon_area_signed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let n = pts.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let mut s = 0.0_f64;
    for i in 0..n { let j = (i + 1) % n; s += pts[i].0 * pts[j].1 - pts[j].0 * pts[i].1; }
    Ok(PerlValue::float(s / 2.0))
}
fn builtin_polygon_perimeter_b17(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let n = pts.len(); if n < 2 { return Ok(PerlValue::float(0.0)); }
    let mut s = 0.0_f64;
    for i in 0..n { let j = (i + 1) % n;
        s += ((pts[j].0 - pts[i].0).powi(2) + (pts[j].1 - pts[i].1).powi(2)).sqrt();
    }
    Ok(PerlValue::float(s))
}
fn builtin_polygon_convex_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let n = pts.len(); if n < 3 { return Ok(PerlValue::integer(0)); }
    let mut sign = 0_i32;
    for i in 0..n {
        let a = pts[i]; let b = pts[(i + 1) % n]; let c = pts[(i + 2) % n];
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        let s = if cross > 0.0 { 1 } else if cross < 0.0 { -1 } else { 0 };
        if s != 0 {
            if sign == 0 { sign = s; } else if sign != s { return Ok(PerlValue::integer(0)); }
        }
    }
    Ok(PerlValue::integer(1))
}
fn builtin_iou_2d_axis_aligned(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let (ax1, ay1, ax2, ay2) = (
        a.first().map(|v| v.to_number()).unwrap_or(0.0),
        a.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        a.get(2).map(|v| v.to_number()).unwrap_or(0.0),
        a.get(3).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let (bx1, by1, bx2, by2) = (
        b.first().map(|v| v.to_number()).unwrap_or(0.0),
        b.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        b.get(2).map(|v| v.to_number()).unwrap_or(0.0),
        b.get(3).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let ix1 = ax1.max(bx1); let iy1 = ay1.max(by1);
    let ix2 = ax2.min(bx2); let iy2 = ay2.min(by2);
    let inter = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
    let area_a = (ax2 - ax1) * (ay2 - ay1);
    let area_b = (bx2 - bx1) * (by2 - by1);
    let union = area_a + area_b - inter;
    Ok(PerlValue::float(if union.abs() < 1e-30 { 0.0 } else { inter / union }))
}
fn builtin_hausdorff_distance_2d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let b: Vec<(f64, f64)> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|p| { let v = arg_to_vec(p); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let dist = |p: (f64, f64), q: (f64, f64)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
    let max_min = |x: &[(f64, f64)], y: &[(f64, f64)]| {
        let mut m = 0.0_f64;
        for &p in x {
            let mn = y.iter().map(|&q| dist(p, q)).fold(f64::INFINITY, f64::min);
            if mn > m { m = mn; }
        }
        m
    };
    Ok(PerlValue::float(max_min(&a, &b).max(max_min(&b, &a))))
}
fn builtin_minkowski_sum_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut out: Vec<PerlValue> = Vec::new();
    for p in &a { for q in &b {
        let pv = arg_to_vec(p); let qv = arg_to_vec(q);
        let x = pv.first().map(|v| v.to_number()).unwrap_or(0.0) + qv.first().map(|v| v.to_number()).unwrap_or(0.0);
        let y = pv.get(1).map(|v| v.to_number()).unwrap_or(0.0) + qv.get(1).map(|v| v.to_number()).unwrap_or(0.0);
        out.push(PerlValue::array(vec![PerlValue::float(x), PerlValue::float(y)]));
    }}
    Ok(PerlValue::array(out))
}
fn builtin_circle_3_points(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let to_pair = |v: &PerlValue| { let xs = arg_to_vec(v); (
        xs.first().map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
    )};
    let p1 = to_pair(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p2 = to_pair(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let p3 = to_pair(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF));
    let ax = p2.0 - p1.0; let ay = p2.1 - p1.1;
    let bx = p3.0 - p1.0; let by = p3.1 - p1.1;
    let d = 2.0 * (ax * by - ay * bx);
    if d.abs() < 1e-30 { return Ok(PerlValue::array(vec![])); }
    let u = (ax * ax + ay * ay) / d;
    let v = (bx * bx + by * by) / d;
    let cx = p1.0 + by * u - ay * v;
    let cy = p1.1 + ax * v - bx * u;
    let r = ((p1.0 - cx).powi(2) + (p1.1 - cy).powi(2)).sqrt();
    Ok(PerlValue::array(vec![PerlValue::float(cx), PerlValue::float(cy), PerlValue::float(r)]))
}
fn builtin_polygon_winding_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|q| { let v = arg_to_vec(q); (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )}).collect();
    let px = p.first().map(|v| v.to_number()).unwrap_or(0.0);
    let py = p.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = pts.len();
    let mut wn = 0_i64;
    for i in 0..n {
        let a = pts[i]; let b = pts[(i + 1) % n];
        if a.1 <= py {
            if b.1 > py {
                let cross = (b.0 - a.0) * (py - a.1) - (px - a.0) * (b.1 - a.1);
                if cross > 0.0 { wn += 1; }
            }
        } else if b.1 <= py {
            let cross = (b.0 - a.0) * (py - a.1) - (px - a.0) * (b.1 - a.1);
            if cross < 0.0 { wn -= 1; }
        }
    }
    Ok(PerlValue::integer(wn))
}
fn builtin_segment_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let to_pair = |v: &PerlValue| { let xs = arg_to_vec(v); (
        xs.first().map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
    )};
    let a = to_pair(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = to_pair(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    Ok(PerlValue::float(((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()))
}
fn builtin_segments_parallel_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let to_pair = |v: &PerlValue| { let xs = arg_to_vec(v); (
        xs.first().map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
    )};
    let p1 = to_pair(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p2 = to_pair(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let p3 = to_pair(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF));
    let p4 = to_pair(&args.get(3).cloned().unwrap_or(PerlValue::UNDEF));
    let cross = (p2.0 - p1.0) * (p4.1 - p3.1) - (p2.1 - p1.1) * (p4.0 - p3.0);
    Ok(PerlValue::integer(if cross.abs() < 1e-12 { 1 } else { 0 }))
}
fn builtin_segments_perpendicular_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let to_pair = |v: &PerlValue| { let xs = arg_to_vec(v); (
        xs.first().map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
    )};
    let p1 = to_pair(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p2 = to_pair(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let p3 = to_pair(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF));
    let p4 = to_pair(&args.get(3).cloned().unwrap_or(PerlValue::UNDEF));
    let dot = (p2.0 - p1.0) * (p4.0 - p3.0) + (p2.1 - p1.1) * (p4.1 - p3.1);
    Ok(PerlValue::integer(if dot.abs() < 1e-12 { 1 } else { 0 }))
}

// More distributions
fn builtin_burr_xii_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(c * k * x.powf(c - 1.0) / (1.0 + x.powf(c)).powf(k + 1.0)))
}
fn builtin_burr_xii_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - (1.0 + x.powf(c)).powf(-k)))
}
fn builtin_dagum_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let z = x / b;
    Ok(PerlValue::float(a * p / x * z.powf(a * p) / (1.0 + z.powf(a)).powf(p + 1.0)))
}
fn builtin_lomax_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(alpha / lambda * (1.0 + x / lambda).powf(-(alpha + 1.0))))
}
fn builtin_birnbaum_saunders_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let z = ((x / beta).sqrt() - (beta / x).sqrt()) / alpha;
    let phi = (-z * z / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let der = ((x / beta).sqrt() + (beta / x).sqrt()) / (2.0 * alpha * x);
    Ok(PerlValue::float(phi * der))
}
fn builtin_tukey_lambda_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args); let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if lambda.abs() < 1e-12 {
        return Ok(PerlValue::float((p / (1.0 - p).max(1e-30)).ln()));
    }
    Ok(PerlValue::float((p.powf(lambda) - (1.0 - p).powf(lambda)) / lambda))
}
fn builtin_half_cauchy_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let scale = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 / (std::f64::consts::PI * scale * (1.0 + (x / scale).powi(2)))))
}
fn builtin_half_logistic_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); if x < 0.0 { return Ok(PerlValue::float(0.0)); }
    let e = (-x).exp();
    Ok(PerlValue::float(2.0 * e / (1.0 + e).powi(2)))
}
fn builtin_reciprocal_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if x < a || x > b || x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / (x * (b / a).ln())))
}
fn builtin_levy_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= mu { return Ok(PerlValue::float(0.0)); }
    let d = x - mu;
    Ok(PerlValue::float((c / (2.0 * std::f64::consts::PI)).sqrt() / d.powf(1.5) * (-c / (2.0 * d)).exp()))
}
fn builtin_voigt_profile_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let g = (-x * x / (2.0 * sigma * sigma)).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt());
    let l = gamma / (std::f64::consts::PI * (x * x + gamma * gamma));
    Ok(PerlValue::float(0.5 * (g + l)))
}
fn builtin_gompertz_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let eta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(b * eta * (eta + b * x - eta * (b * x).exp()).exp()))
}
fn builtin_inverse_weibull_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(alpha * beta.powf(alpha) * x.powf(-alpha - 1.0) * (-(beta / x).powf(alpha)).exp()))
}
fn builtin_log_gamma_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    use statrs::function::gamma::ln_gamma;
    Ok(PerlValue::float(ln_gamma(x)))
}
fn builtin_inverse_chi2_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args); let nu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 { return Ok(PerlValue::float(0.0)); }
    use statrs::function::gamma::gamma;
    let pre = 2.0_f64.powf(-nu / 2.0) / gamma(nu / 2.0);
    Ok(PerlValue::float(pre * x.powf(-nu / 2.0 - 1.0) * (-1.0 / (2.0 * x)).exp()))
}
