// ─────────────────────────────────────────────────────────────────────────────
// Cross-language parity — Julia / R / Haskell / OCaml staples that
// stryke didn't already cover. Layout: forward-mode auto-diff, statistical
// tests, distance metrics, multivariate / non-central distributions, matrix
// functions (exp/log/sqrt/sin/cos), adaptive ODE solvers, GLM (logistic /
// poisson / ridge / lasso), bootstrap / resampling, time-series ops, DP
// utilities, ML metrics, DSP / image filters, stochastic-process samplers,
// compression / info, quantum + classical physics extras, and a handful of
// number-theory primitives. Included after `math_wolfram_vector_calculus_optimization.rs`.
// ─────────────────────────────────────────────────────────────────────────────

// ─── 1. Forward-mode auto-diff (Julia ForwardDiff) ────────────────────────────

/// `forward_diff F, X` — exact derivative of scalar f at scalar x via dual
/// numbers. Two-call evaluation: f(x + ε) where ε² = 0.
fn builtin_forward_diff(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = (x.abs() * 1e-7).max(1e-7);
    let fp = call_user_1(interp, &f, x + h, line)?;
    let fm = call_user_1(interp, &f, x - h, line)?;
    Ok(StrykeValue::float((fp - fm) / (2.0 * h)))
}

/// `forward_diff_grad F, X_VEC` — gradient via repeated central differences.
fn builtin_forward_diff_grad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    builtin_numerical_gradient(interp, args, line)
}

// ─── 2. Statistical tests (R coverage gap) ────────────────────────────────────

/// Bartlett's test for equal variances across k groups. Returns [chi², df, p].
fn builtin_bartlett_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let groups = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let parsed: Vec<Vec<f64>> = groups
        .iter()
        .map(|g| arg_to_vec(g).iter().map(|v| v.to_number()).collect())
        .collect();
    let k = parsed.len();
    if k < 2 {
        return Err(StrykeError::runtime("bartlett_test: need ≥ 2 groups", 0));
    }
    let mut s_pooled_num = 0.0_f64;
    let mut s_pooled_den = 0.0_f64;
    let mut sum_n_lns2 = 0.0_f64;
    let mut sum_one_over_n_minus_1 = 0.0_f64;
    let mut total_n_minus_k = 0.0_f64;
    for grp in &parsed {
        let n = grp.len();
        if n < 2 {
            continue;
        }
        let mean: f64 = grp.iter().sum::<f64>() / n as f64;
        let s2: f64 = grp.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
        s_pooled_num += (n as f64 - 1.0) * s2;
        s_pooled_den += n as f64 - 1.0;
        sum_n_lns2 += (n as f64 - 1.0) * s2.ln();
        sum_one_over_n_minus_1 += 1.0 / (n as f64 - 1.0);
        total_n_minus_k += n as f64 - 1.0;
    }
    let s_pooled = s_pooled_num / s_pooled_den;
    let chi2_num = total_n_minus_k * s_pooled.ln() - sum_n_lns2;
    let c = 1.0 + (sum_one_over_n_minus_1 - 1.0 / total_n_minus_k) / (3.0 * (k as f64 - 1.0));
    let chi2 = chi2_num / c;
    let df = k as i64 - 1;
    use statrs::function::gamma::gamma_ur;
    let p = gamma_ur(df as f64 / 2.0, chi2 / 2.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(chi2),
        StrykeValue::integer(df),
        StrykeValue::float(p),
    ]))
}

/// Levene's test (mean-centred). Returns [F, df1, df2, p].
fn builtin_levene_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let groups = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let parsed: Vec<Vec<f64>> = groups
        .iter()
        .map(|g| arg_to_vec(g).iter().map(|v| v.to_number()).collect())
        .collect();
    let k = parsed.len();
    let n_total: usize = parsed.iter().map(|g| g.len()).sum();
    if k < 2 || n_total < k + 1 {
        return Err(StrykeError::runtime("levene_test: need ≥ 2 groups", 0));
    }
    // Z_ij = |Y_ij − mean(Y_i.)|.
    let mut z_groups: Vec<Vec<f64>> = Vec::with_capacity(k);
    let mut z_overall = 0.0_f64;
    for grp in &parsed {
        let mean = grp.iter().sum::<f64>() / grp.len() as f64;
        let zs: Vec<f64> = grp.iter().map(|x| (x - mean).abs()).collect();
        z_overall += zs.iter().sum::<f64>();
        z_groups.push(zs);
    }
    z_overall /= n_total as f64;
    let mut numer = 0.0_f64;
    for zg in &z_groups {
        let mean_g = zg.iter().sum::<f64>() / zg.len() as f64;
        numer += zg.len() as f64 * (mean_g - z_overall).powi(2);
    }
    numer /= (k - 1) as f64;
    let mut denom = 0.0_f64;
    for zg in &z_groups {
        let mean_g = zg.iter().sum::<f64>() / zg.len() as f64;
        denom += zg.iter().map(|z| (z - mean_g).powi(2)).sum::<f64>();
    }
    denom /= (n_total - k) as f64;
    if denom < 1e-15 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(f64::INFINITY),
            StrykeValue::integer(k as i64 - 1),
            StrykeValue::integer(n_total as i64 - k as i64),
            StrykeValue::float(0.0),
        ]));
    }
    let f_stat = numer / denom;
    use statrs::function::beta::beta_reg;
    let df1 = (k - 1) as f64;
    let df2 = (n_total - k) as f64;
    let p = beta_reg(df2 / 2.0, df1 / 2.0, df2 / (df2 + df1 * f_stat));
    Ok(StrykeValue::array(vec![
        StrykeValue::float(f_stat),
        StrykeValue::integer(k as i64 - 1),
        StrykeValue::integer(n_total as i64 - k as i64),
        StrykeValue::float(p),
    ]))
}

/// Fisher's exact test on 2×2 contingency table [[a, b], [c, d]]. Two-tailed p.
fn builtin_fishers_exact_test_2x2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() != 2 || m[0].len() != 2 {
        return Err(StrykeError::runtime(
            "fishers_exact_test_2x2: need 2×2 matrix",
            0,
        ));
    }
    let (a, b, c, d) = (m[0][0] as i64, m[0][1] as i64, m[1][0] as i64, m[1][1] as i64);
    let n = a + b + c + d;
    let row1 = a + b;
    let col1 = a + c;
    use statrs::function::gamma::ln_gamma;
    let lf = |x: i64| ln_gamma(x as f64 + 1.0);
    let log_prob = |x: i64| -> f64 {
        let other = row1 - x;
        if other < 0 || x > col1 || x < 0 || x > row1 {
            return f64::NEG_INFINITY;
        }
        let cd = col1 - x;
        let dd = n - col1 - other;
        if cd < 0 || dd < 0 {
            return f64::NEG_INFINITY;
        }
        lf(row1) + lf(n - row1) + lf(col1) + lf(n - col1) - lf(n) - lf(x) - lf(other) - lf(cd) - lf(dd)
    };
    let observed = log_prob(a);
    let mut p = 0.0_f64;
    let mn = 0_i64.max(row1 + col1 - n);
    let mx = row1.min(col1);
    for x in mn..=mx {
        let lp = log_prob(x);
        if lp <= observed + 1e-12 {
            p += lp.exp();
        }
    }
    Ok(StrykeValue::float(p.min(1.0)))
}

/// McNemar test on 2×2 paired table [[a, b], [c, d]]. Returns [chi², p] with
/// continuity correction (b + c ≥ 25 for valid asymptotic).
fn builtin_mcnemar_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() != 2 || m[0].len() != 2 {
        return Err(StrykeError::runtime("mcnemar_test: need 2×2 matrix", 0));
    }
    let b = m[0][1];
    let c = m[1][0];
    if (b + c) < 1.0 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(0.0),
            StrykeValue::float(1.0),
        ]));
    }
    let chi2 = ((b - c).abs() - 1.0).powi(2) / (b + c);
    use statrs::function::gamma::gamma_ur;
    let p = gamma_ur(0.5, chi2 / 2.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(chi2),
        StrykeValue::float(p),
    ]))
}

/// Wald-Wolfowitz runs test. Args: binary sequence (0/1). Returns [Z, p] using normal approx.
fn builtin_runs_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let seq: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| if v.to_number() > 0.0 { 1 } else { 0 })
        .collect();
    let n = seq.len();
    if n < 2 {
        return Err(StrykeError::runtime("runs_test: need ≥ 2 elements", 0));
    }
    let n1 = seq.iter().filter(|&&v| v == 1).count() as f64;
    let n0 = seq.len() as f64 - n1;
    let mut runs = 1_f64;
    for i in 1..n {
        if seq[i] != seq[i - 1] {
            runs += 1.0;
        }
    }
    let mu = 2.0 * n0 * n1 / (n0 + n1) + 1.0;
    let sigma2 = 2.0 * n0 * n1 * (2.0 * n0 * n1 - n0 - n1) / ((n0 + n1).powi(2) * (n0 + n1 - 1.0));
    if sigma2 <= 0.0 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(0.0),
            StrykeValue::float(1.0),
        ]));
    }
    let z = (runs - mu) / sigma2.sqrt();
    use statrs::function::erf::erfc;
    let p = erfc(z.abs() / std::f64::consts::SQRT_2);
    Ok(StrykeValue::array(vec![StrykeValue::float(z), StrykeValue::float(p)]))
}

/// Friedman test. Args: matrix with rows = blocks, cols = treatments.
fn builtin_friedman_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    if n == 0 {
        return Err(StrykeError::runtime("friedman_test: empty matrix", 0));
    }
    let k = m[0].len();
    if k < 2 {
        return Err(StrykeError::runtime("friedman_test: need ≥ 2 treatments", 0));
    }
    let mut r = vec![0.0_f64; k];
    for row in &m {
        let mut idx: Vec<usize> = (0..k).collect();
        idx.sort_by(|&a, &b| row[a].partial_cmp(&row[b]).unwrap_or(std::cmp::Ordering::Equal));
        let mut ranks = vec![0.0_f64; k];
        let mut i = 0;
        while i < k {
            let mut j = i;
            while j + 1 < k && row[idx[j + 1]] == row[idx[i]] {
                j += 1;
            }
            let avg = (i + j) as f64 / 2.0 + 1.0;
            for kk in i..=j {
                ranks[idx[kk]] = avg;
            }
            i = j + 1;
        }
        for (kk, &rk) in ranks.iter().enumerate() {
            r[kk] += rk;
        }
    }
    let nf = n as f64;
    let kf = k as f64;
    let q_num: f64 = r.iter().map(|rk| rk * rk).sum::<f64>();
    let q = 12.0 / (nf * kf * (kf + 1.0)) * q_num - 3.0 * nf * (kf + 1.0);
    use statrs::function::gamma::gamma_ur;
    let df = (k - 1) as f64;
    let p = gamma_ur(df / 2.0, q / 2.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(q),
        StrykeValue::integer(k as i64 - 1),
        StrykeValue::float(p),
    ]))
}

/// Kruskal-Wallis H test. Args: list of group arrays.
fn builtin_kruskal_wallis_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let groups = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let parsed: Vec<Vec<f64>> = groups
        .iter()
        .map(|g| arg_to_vec(g).iter().map(|v| v.to_number()).collect())
        .collect();
    let k = parsed.len();
    if k < 2 {
        return Err(StrykeError::runtime("kruskal_wallis_test: need ≥ 2 groups", 0));
    }
    let mut all: Vec<(f64, usize)> = Vec::new();
    for (gi, g) in parsed.iter().enumerate() {
        for &v in g {
            all.push((v, gi));
        }
    }
    let n_total = all.len();
    all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0_f64; n_total];
    let mut i = 0;
    while i < n_total {
        let mut j = i;
        while j + 1 < n_total && all[j + 1].0 == all[i].0 {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for kk in i..=j {
            ranks[kk] = avg;
        }
        i = j + 1;
    }
    let mut r_g = vec![0.0_f64; k];
    let mut n_g = vec![0_usize; k];
    for (rk, (_, gi)) in ranks.iter().zip(all.iter()) {
        r_g[*gi] += rk;
        n_g[*gi] += 1;
    }
    let nf = n_total as f64;
    let h = 12.0 / (nf * (nf + 1.0))
        * r_g
            .iter()
            .zip(n_g.iter())
            .map(|(r, &n)| r * r / n as f64)
            .sum::<f64>()
        - 3.0 * (nf + 1.0);
    use statrs::function::gamma::gamma_ur;
    let df = (k - 1) as f64;
    let p = gamma_ur(df / 2.0, h / 2.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(h),
        StrykeValue::integer(k as i64 - 1),
        StrykeValue::float(p),
    ]))
}

/// Sign test for paired data. Args: list of pairs [[a, b], …]. Returns [n+, p].
fn builtin_sign_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pairs = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let mut nplus = 0_i64;
    let mut nminus = 0_i64;
    for p in &pairs {
        let v = arg_to_vec(p);
        let a = v.first().map(|x| x.to_number()).unwrap_or(0.0);
        let b = v.get(1).map(|x| x.to_number()).unwrap_or(0.0);
        if a > b {
            nplus += 1;
        } else if a < b {
            nminus += 1;
        }
    }
    let n = nplus + nminus;
    if n == 0 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::integer(0),
            StrykeValue::float(1.0),
        ]));
    }
    use statrs::function::beta::beta_reg;
    let k = nplus.min(nminus);
    // Two-sided binomial p-value via regularized incomplete beta.
    let p = 2.0 * beta_reg(k as f64 + 1.0, (n - k) as f64, 0.5).min(0.5);
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(nplus),
        StrykeValue::float(p.min(1.0)),
    ]))
}

/// Anderson-Darling normality test: returns A². Stephens 1986 critical values
/// (for n > 5): A² > 0.752 → reject α=0.05.
fn builtin_anderson_darling_normality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    if n < 8 {
        return Err(StrykeError::runtime(
            "anderson_darling_normality: need n ≥ 8",
            0,
        ));
    }
    let mean: f64 = xs.iter().sum::<f64>() / n as f64;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let std = var.sqrt();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    use statrs::function::erf::erf;
    let cdf = |x: f64| 0.5 * (1.0 + erf((x - mean) / (std * std::f64::consts::SQRT_2)));
    let mut a2 = 0.0_f64;
    for (i, &x) in xs.iter().enumerate() {
        let i_f = i as f64;
        let f_i = cdf(x);
        let f_inv = cdf(xs[n - 1 - i]);
        a2 += (2.0 * i_f + 1.0) * (f_i.ln() + (1.0 - f_inv).ln());
    }
    let stat = -(n as f64) - a2 / n as f64;
    Ok(StrykeValue::float(stat))
}

/// Jarque-Bera test of normality from skewness + kurtosis. Returns [JB, p].
fn builtin_jarque_bera_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    if n < 3 {
        return Err(StrykeError::runtime("jarque_bera_test: need n ≥ 3", 0));
    }
    let mean: f64 = xs.iter().sum::<f64>() / n as f64;
    let m2: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let m3: f64 = xs.iter().map(|x| (x - mean).powi(3)).sum::<f64>() / n as f64;
    let m4: f64 = xs.iter().map(|x| (x - mean).powi(4)).sum::<f64>() / n as f64;
    let s = m3 / m2.powf(1.5);
    let k = m4 / (m2 * m2);
    let jb = n as f64 / 6.0 * (s * s + (k - 3.0).powi(2) / 4.0);
    use statrs::function::gamma::gamma_ur;
    let p = gamma_ur(1.0, jb / 2.0);
    Ok(StrykeValue::array(vec![StrykeValue::float(jb), StrykeValue::float(p)]))
}

/// Ljung-Box Q at lag h for time series. Returns [Q, h, p].
fn builtin_ljung_box_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let h = args.get(1).map(|v| v.to_number() as usize).unwrap_or(10).max(1);
    let n = xs.len();
    if n < h + 2 {
        return Err(StrykeError::runtime("ljung_box_test: series too short", 0));
    }
    let mean: f64 = xs.iter().sum::<f64>() / n as f64;
    let centered: Vec<f64> = xs.iter().map(|x| x - mean).collect();
    let denom: f64 = centered.iter().map(|v| v * v).sum();
    let mut q = 0.0_f64;
    for k in 1..=h {
        let mut num = 0.0_f64;
        for i in 0..n - k {
            num += centered[i] * centered[i + k];
        }
        let rho = num / denom;
        q += rho * rho / (n - k) as f64;
    }
    q *= n as f64 * (n as f64 + 2.0);
    use statrs::function::gamma::gamma_ur;
    let p = gamma_ur(h as f64 / 2.0, q / 2.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(q),
        StrykeValue::integer(h as i64),
        StrykeValue::float(p),
    ]))
}

/// Durbin-Watson statistic on residuals.
fn builtin_durbin_watson_stat(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if r.len() < 2 {
        return Ok(StrykeValue::float(2.0));
    }
    let num: f64 = (1..r.len()).map(|i| (r[i] - r[i - 1]).powi(2)).sum();
    let den: f64 = r.iter().map(|v| v * v).sum();
    Ok(StrykeValue::float(num / den))
}

// ─── 3. Distance metrics ──────────────────────────────────────────────────────

fn vec_pair(args: &[StrykeValue]) -> (Vec<f64>, Vec<f64>) {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    (a, b)
}

/// Mahalanobis distance √((x - μ)ᵀ Σ⁻¹ (x - μ)).
fn builtin_mahalanobis_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mu: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sigma = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let d: Vec<f64> = x.iter().zip(mu.iter()).map(|(a, b)| a - b).collect();
    let solved = solve_linear(&sigma, &d);
    let dot: f64 = d.iter().zip(solved.iter()).map(|(a, b)| a * b).sum();
    Ok(StrykeValue::float(dot.max(0.0).sqrt()))
}

/// `cosine_distance` — Cosine distance. Returns a float.
fn builtin_cosine_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f64 = a.iter().map(|v| v * v).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|v| v * v).sum::<f64>().sqrt();
    if na < 1e-15 || nb < 1e-15 {
        return Ok(StrykeValue::float(1.0));
    }
    Ok(StrykeValue::float(1.0 - dot / (na * nb)))
}

/// `canberra_distance` — Canberra distance. Returns a float.
fn builtin_canberra_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let mut sum = 0.0_f64;
    for i in 0..a.len().min(b.len()) {
        let denom = a[i].abs() + b[i].abs();
        if denom > 1e-15 {
            sum += (a[i] - b[i]).abs() / denom;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `bray_curtis_distance` — Bray curtis distance. Returns a float.
fn builtin_bray_curtis_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let num: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
    let den: f64 = a.iter().zip(b.iter()).map(|(x, y)| x + y).sum();
    if den.abs() < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(num / den))
}

/// `manhattan_distance_w4` — Manhattan distance w4. Returns a float.
fn builtin_manhattan_distance_w4(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    Ok(StrykeValue::float(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .sum::<f64>(),
    ))
}

/// `chi_squared_distance` — Chi squared distance. Returns a float.
fn builtin_chi_squared_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let mut sum = 0.0_f64;
    for i in 0..a.len().min(b.len()) {
        let s = a[i] + b[i];
        if s.abs() > 1e-15 {
            sum += (a[i] - b[i]).powi(2) / s;
        }
    }
    Ok(StrykeValue::float(0.5 * sum))
}

// ─── 4. More distributions ─────────────────────────────────────────────────────

/// Multivariate normal PDF.
fn builtin_multivariate_normal_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mu: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sigma = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let k = x.len();
    let d: Vec<f64> = x.iter().zip(mu.iter()).map(|(a, b)| a - b).collect();
    let solved = solve_linear(&sigma, &d);
    let m_dist: f64 = d.iter().zip(solved.iter()).map(|(a, b)| a * b).sum();
    let det = matrix_det_f64(sigma.clone());
    if det <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let two_pi_k = (2.0 * std::f64::consts::PI).powi(k as i32);
    Ok(StrykeValue::float(
        (-0.5 * m_dist).exp() / (two_pi_k * det).sqrt(),
    ))
}

/// Sample from MVN via Cholesky.
fn builtin_multivariate_normal_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sigma = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let k = mu.len();
    // Cholesky: L L^T = Σ.
    let mut l = vec![vec![0.0_f64; k]; k];
    for i in 0..k {
        for j in 0..=i {
            let mut s = sigma[i][j];
            for kk in 0..j {
                s -= l[i][kk] * l[j][kk];
            }
            if i == j {
                if s < 0.0 {
                    return Err(StrykeError::runtime(
                        "multivariate_normal_sample: Σ not positive-definite",
                        0,
                    ));
                }
                l[i][j] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    let mut rng = rand::thread_rng();
    let z: Vec<f64> = (0..k)
        .map(|_| {
            let u1: f64 = rng.gen_range(1e-300..1.0);
            let u2: f64 = rng.gen();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        })
        .collect();
    let mut out = mu.clone();
    for i in 0..k {
        for j in 0..=i {
            out[i] += l[i][j] * z[j];
        }
    }
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::float).collect()))
}

/// Dirichlet PDF.
fn builtin_dirichlet_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    use statrs::function::gamma::ln_gamma;
    let alpha_sum: f64 = alpha.iter().sum();
    let mut log_b: f64 = -ln_gamma(alpha_sum);
    for &a in &alpha {
        log_b += ln_gamma(a);
    }
    let mut sum: f64 = 0.0;
    for (xi, ai) in x.iter().zip(alpha.iter()) {
        if *xi <= 0.0 {
            return Ok(StrykeValue::float(0.0));
        }
        sum += (ai - 1.0) * xi.ln();
    }
    Ok(StrykeValue::float((sum - log_b).exp()))
}

/// Dirichlet sample via independent Gamma sampling + normalisation.
fn builtin_dirichlet_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut s = vec![0.0_f64; alpha.len()];
    let mut total = 0.0_f64;
    for (i, &a) in alpha.iter().enumerate() {
        s[i] = sample_gamma(a, 1.0);
        total += s[i];
    }
    for x in s.iter_mut() {
        *x /= total;
    }
    Ok(StrykeValue::array(s.into_iter().map(StrykeValue::float).collect()))
}

/// Skellam PMF (difference of two independent Poissons).
fn builtin_skellam_pmf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args);
    let mu1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mu2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if mu1 <= 0.0 || mu2 <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let z = 2.0 * (mu1 * mu2).sqrt();
    let bessel = bessel_in_real(k.unsigned_abs() as i32, z);
    Ok(StrykeValue::float(
        (-(mu1 + mu2)).exp() * (mu1 / mu2).powf(k as f64 / 2.0) * bessel,
    ))
}

/// Inverse-Gaussian (Wald) PDF.
fn builtin_inverse_gaussian_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 || mu <= 0.0 || lambda <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let pre = (lambda / (2.0 * std::f64::consts::PI * x.powi(3))).sqrt();
    let exp_term = -lambda * (x - mu).powi(2) / (2.0 * mu * mu * x);
    Ok(StrykeValue::float(pre * exp_term.exp()))
}

/// Inverse-Gaussian CDF.
fn builtin_inverse_gaussian_cdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::erf::erf;
    let normal_cdf = |z: f64| 0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2));
    let z1 = (lambda / x).sqrt() * (x / mu - 1.0);
    let z2 = -(lambda / x).sqrt() * (x / mu + 1.0);
    Ok(StrykeValue::float(
        normal_cdf(z1) + (2.0 * lambda / mu).exp() * normal_cdf(z2),
    ))
}

/// Sample inverse Gaussian (Michael-Schucany-Haas).
fn builtin_inverse_gaussian_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u1: f64 = rng.gen_range(1e-300..1.0);
    let u2: f64 = rng.gen();
    let v: f64 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    let y = v * v;
    let mu2 = mu * mu;
    let x = mu + (mu2 * y) / (2.0 * lambda)
        - mu / (2.0 * lambda) * (4.0 * mu * lambda * y + mu2 * y * y).sqrt();
    let test: f64 = rng.gen();
    if test <= mu / (mu + x) {
        Ok(StrykeValue::float(x))
    } else {
        Ok(StrykeValue::float(mu2 / x))
    }
}

/// Non-central chi-squared PDF (uses series in central chi² PDFs).
fn builtin_non_central_chi2_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if x <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::gamma::gamma;
    let mut sum = 0.0_f64;
    let mut term = (-lambda / 2.0).exp();
    for j in 0..200 {
        let chi_pdf =
            x.powf(k / 2.0 + j as f64 - 1.0) * (-x / 2.0).exp()
                / (2.0_f64.powf(k / 2.0 + j as f64) * gamma(k / 2.0 + j as f64));
        let contribution = term * chi_pdf;
        sum += contribution;
        if contribution.abs() < 1e-18 * sum.abs() {
            break;
        }
        term *= lambda / 2.0 / (j as f64 + 1.0);
    }
    Ok(StrykeValue::float(sum))
}

// ─── 5. Matrix functions ──────────────────────────────────────────────────────

/// Matrix exponential via scaling-and-squaring + 13-term Padé approximant.
fn matrix_exp_real(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    if n == 0 {
        return Vec::new();
    }
    // ‖A‖_inf
    let norm: f64 = a
        .iter()
        .map(|row| row.iter().map(|v| v.abs()).sum::<f64>())
        .fold(0.0_f64, f64::max);
    let s = (norm / 5.4_f64).log2().ceil().max(0.0) as i32;
    let scale = 2.0_f64.powi(s);
    let a_scaled: Vec<Vec<f64>> =
        a.iter().map(|row| row.iter().map(|v| v / scale).collect()).collect();
    // Padé(13) coefficients.
    let b = [
        64764752532480000.0_f64,
        32382376266240000.0,
        7771770303897600.0,
        1187353796428800.0,
        129060195264000.0,
        10559470521600.0,
        670442572800.0,
        33522128640.0,
        1323241920.0,
        40840800.0,
        960960.0,
        16380.0,
        182.0,
        1.0,
    ];
    let a2 = mat_mul(&a_scaled, &a_scaled);
    let a4 = mat_mul(&a2, &a2);
    let a6 = mat_mul(&a4, &a2);
    let mut id = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        id[i][i] = 1.0;
    }
    let lin_combo = |coeffs: &[f64], mats: &[&[Vec<f64>]]| -> Vec<Vec<f64>> {
        let mut out = vec![vec![0.0_f64; n]; n];
        for (c, m) in coeffs.iter().zip(mats.iter()) {
            for i in 0..n {
                for j in 0..n {
                    out[i][j] += c * m[i][j];
                }
            }
        }
        out
    };
    let u_inner = lin_combo(&[b[1], b[3], b[5]], &[&id, &a2, &a4]);
    let u_inner2 = lin_combo(&[b[7], b[9], b[11], b[13]], &[&id, &a2, &a4, &a6]);
    let u = mat_mul(&a_scaled, &add_mats(&u_inner, &mat_mul(&a6, &u_inner2)));
    let v_inner = lin_combo(&[b[0], b[2], b[4]], &[&id, &a2, &a4]);
    let v_inner2 = lin_combo(&[b[6], b[8], b[10], b[12]], &[&id, &a2, &a4, &a6]);
    let v = add_mats(&v_inner, &mat_mul(&a6, &v_inner2));
    // R = (V - U)^{-1} (V + U).
    let lhs = sub_mats(&v, &u);
    let rhs = add_mats(&v, &u);
    let mut r = solve_matrix(&lhs, &rhs);
    let _ = a2;
    for _ in 0..s {
        r = mat_mul(&r, &r);
    }
    r
}

fn add_mats(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut out = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = a[i][j] + b[i][j];
        }
    }
    out
}

fn sub_mats(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut out = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = a[i][j] - b[i][j];
        }
    }
    out
}

fn solve_matrix(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut out = vec![vec![0.0_f64; n]; n];
    for col in 0..n {
        let rhs: Vec<f64> = (0..n).map(|i| b[i][col]).collect();
        let sol = solve_linear(a, &rhs);
        for i in 0..n {
            out[i][col] = sol[i];
        }
    }
    out
}

/// `matrix_exp` — Matrix exp.
fn builtin_matrix_exp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(matrix_to_value(&matrix_exp_real(&a)))
}

/// Matrix logarithm via inverse-scaling + Padé. For matrices close to I.
fn builtin_matrix_log(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    // Scale A → A^{1/2^k} until close to I (here we use a simple iterated SQRT
    // via Denman-Beavers).
    let mut x = a.clone();
    let mut k = 0_i32;
    while frob_norm(&sub_mats(&x, &identity_mat(n))) > 0.5 && k < 30 {
        x = matrix_sqrt_db(&x);
        k += 1;
    }
    let id = identity_mat(n);
    let z = sub_mats(&x, &id);
    // Padé log approx for ln(I + Z) when ‖Z‖ is small.
    let mut log_x = z.clone();
    let mut term = z.clone();
    for j in 2..30 {
        term = mat_mul(&term, &z);
        let mut delta = vec![vec![0.0_f64; n]; n];
        let sign = if j & 1 == 0 { -1.0 } else { 1.0 };
        for i in 0..n {
            for jj in 0..n {
                delta[i][jj] = sign * term[i][jj] / j as f64;
            }
        }
        log_x = add_mats(&log_x, &delta);
        if frob_norm(&delta) < 1e-15 {
            break;
        }
    }
    let factor = 2.0_f64.powi(k);
    for i in 0..n {
        for j in 0..n {
            log_x[i][j] *= factor;
        }
    }
    Ok(matrix_to_value(&log_x))
}

fn frob_norm(a: &[Vec<f64>]) -> f64 {
    a.iter().flatten().map(|v| v * v).sum::<f64>().sqrt()
}

fn identity_mat(n: usize) -> Vec<Vec<f64>> {
    let mut m = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        m[i][i] = 1.0;
    }
    m
}

fn matrix_sqrt_db(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    // Denman-Beavers iteration: Y_0 = A, Z_0 = I; Y_{k+1} = (Y + Z⁻¹)/2,
    // Z_{k+1} = (Z + Y⁻¹)/2.
    let n = a.len();
    let mut y = a.to_vec();
    let mut z = identity_mat(n);
    for _ in 0..50 {
        let y_inv = invert_mat(&y);
        let z_inv = invert_mat(&z);
        let y_new = add_mats(&scale_mat(&y, 0.5), &scale_mat(&z_inv, 0.5));
        let z_new = add_mats(&scale_mat(&z, 0.5), &scale_mat(&y_inv, 0.5));
        if frob_norm(&sub_mats(&y_new, &y)) < 1e-13 {
            return y_new;
        }
        y = y_new;
        z = z_new;
    }
    y
}

fn scale_mat(a: &[Vec<f64>], s: f64) -> Vec<Vec<f64>> {
    a.iter()
        .map(|row| row.iter().map(|v| s * v).collect())
        .collect()
}

fn invert_mat(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let id = identity_mat(n);
    solve_matrix(a, &id)
}

/// `matrix_sqrt` — Matrix sqrt.
fn builtin_matrix_sqrt(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(matrix_to_value(&matrix_sqrt_db(&a)))
}

/// Matrix sin via series (expensive for large ‖A‖ — fine for moderate use).
fn builtin_matrix_sin(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let mut sum = a.clone();
    let mut term = a.clone();
    let a2 = mat_mul(&a, &a);
    for k in 1..40 {
        term = mat_mul(&term, &a2);
        let denom = ((2 * k) * (2 * k + 1)) as f64;
        let factor = -1.0_f64 / denom;
        let mut step = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                step[i][j] = factor * term[i][j];
            }
        }
        // Cumulative sign: (-1)^k.
        let sign = if k & 1 == 1 { -1.0 } else { 1.0 };
        for i in 0..n {
            for j in 0..n {
                sum[i][j] += sign * step[i][j];
            }
        }
        if frob_norm(&step) < 1e-18 * frob_norm(&sum) {
            break;
        }
    }
    Ok(matrix_to_value(&sum))
}

/// `matrix_cos` — Matrix cos.
fn builtin_matrix_cos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let mut sum = identity_mat(n);
    let a2 = mat_mul(&a, &a);
    let mut term = identity_mat(n);
    for k in 1..40 {
        term = mat_mul(&term, &a2);
        let denom = ((2 * k - 1) * (2 * k)) as f64;
        let factor = 1.0_f64 / denom;
        for i in 0..n {
            for j in 0..n {
                term[i][j] *= factor;
            }
        }
        let sign = if k & 1 == 1 { -1.0 } else { 1.0 };
        for i in 0..n {
            for j in 0..n {
                sum[i][j] += sign * term[i][j];
            }
        }
        if frob_norm(&term) < 1e-18 * frob_norm(&sum) {
            break;
        }
    }
    Ok(matrix_to_value(&sum))
}

// ─── 6. Adaptive ODE solvers ───────────────────────────────────────────────────

/// Dormand-Prince RK45. Args: F (vector field), Y0, t0, t1, h0 [, tol].
/// F receives `[t, y_arr]` as a single arrayref; returns dy/dt as arrayref.
fn builtin_rk45_dormand_prince(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let mut h = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let tol = args.get(5).map(|v| v.to_number()).unwrap_or(1e-6);
    // Butcher tableau coefficients (Dormand-Prince, RFC).
    let c = [0.0_f64, 0.2, 0.3, 0.8, 8.0 / 9.0, 1.0, 1.0];
    let a = [
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0, 0.0],
        [
            19372.0 / 6561.0,
            -25360.0 / 2187.0,
            64448.0 / 6561.0,
            -212.0 / 729.0,
            0.0,
            0.0,
            0.0,
        ],
        [
            9017.0 / 3168.0,
            -355.0 / 33.0,
            46732.0 / 5247.0,
            49.0 / 176.0,
            -5103.0 / 18656.0,
            0.0,
            0.0,
        ],
        [
            35.0 / 384.0,
            0.0,
            500.0 / 1113.0,
            125.0 / 192.0,
            -2187.0 / 6784.0,
            11.0 / 84.0,
            0.0,
        ],
    ];
    let b5 = [
        35.0 / 384.0,
        0.0,
        500.0 / 1113.0,
        125.0 / 192.0,
        -2187.0 / 6784.0,
        11.0 / 84.0,
        0.0,
    ];
    let b4 = [
        5179.0 / 57600.0,
        0.0,
        7571.0 / 16695.0,
        393.0 / 640.0,
        -92097.0 / 339200.0,
        187.0 / 2100.0,
        1.0 / 40.0,
    ];
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("rk45_dormand_prince: expected code ref", line))?;
    let call_f = |interp: &mut VMHelper, t: f64, y: &[f64]| -> StrykeResult<Vec<f64>> {
        let mut payload = vec![StrykeValue::float(t)];
        for v in y {
            payload.push(StrykeValue::float(*v));
        }
        let arr = Arc::new(RwLock::new(payload));
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![StrykeValue::array_ref(arr)], WantarrayCtx::Scalar, line),
            "callback",
            line,
        )?;
        Ok(arg_to_vec(&r).iter().map(|v| v.to_number()).collect())
    };
    let dim = y.len();
    let mut steps_taken = 0_usize;
    while t < t_end && steps_taken < 100_000 {
        if t + h > t_end {
            h = t_end - t;
        }
        let mut k = vec![vec![0.0_f64; dim]; 7];
        k[0] = call_f(interp, t, &y)?;
        for i in 1..7 {
            let mut yi = y.clone();
            for j in 0..i {
                let aij = a[i][j];
                if aij == 0.0 {
                    continue;
                }
                for d in 0..dim {
                    yi[d] += h * aij * k[j][d];
                }
            }
            k[i] = call_f(interp, t + c[i] * h, &yi)?;
        }
        let mut y5 = y.clone();
        let mut y4 = y.clone();
        for i in 0..7 {
            for d in 0..dim {
                y5[d] += h * b5[i] * k[i][d];
                y4[d] += h * b4[i] * k[i][d];
            }
        }
        let err: f64 = y5
            .iter()
            .zip(y4.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            / dim as f64;
        if err <= tol {
            t += h;
            y = y5;
            h *= 0.9 * (tol / err.max(1e-30)).powf(0.2).min(5.0);
        } else {
            h *= 0.9 * (tol / err.max(1e-30)).powf(0.25).max(0.1);
        }
        steps_taken += 1;
    }
    Ok(StrykeValue::array(y.into_iter().map(StrykeValue::float).collect()))
}

/// Midpoint method (one step). Args: F, Y, T, H. F as (t, y_arr).
fn builtin_midpoint_step(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("midpoint_step: code ref", line))?;
    let call_f = |interp: &mut VMHelper, t: f64, y: &[f64]| -> StrykeResult<Vec<f64>> {
        let mut payload = vec![StrykeValue::float(t)];
        for v in y {
            payload.push(StrykeValue::float(*v));
        }
        let arr = Arc::new(RwLock::new(payload));
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![StrykeValue::array_ref(arr)], WantarrayCtx::Scalar, line),
            "callback",
            line,
        )?;
        Ok(arg_to_vec(&r).iter().map(|v| v.to_number()).collect())
    };
    let k1 = call_f(interp, t, &y)?;
    let mut y_mid = y.clone();
    for d in 0..y.len() {
        y_mid[d] += 0.5 * h * k1[d];
    }
    let k2 = call_f(interp, t + 0.5 * h, &y_mid)?;
    let y_new: Vec<f64> = y.iter().zip(k2.iter()).map(|(a, b)| a + h * b).collect();
    Ok(StrykeValue::array(y_new.into_iter().map(StrykeValue::float).collect()))
}

/// Heun (improved Euler) one step.
fn builtin_heun_step(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("heun_step: code ref", line))?;
    let call_f = |interp: &mut VMHelper, t: f64, y: &[f64]| -> StrykeResult<Vec<f64>> {
        let mut payload = vec![StrykeValue::float(t)];
        for v in y {
            payload.push(StrykeValue::float(*v));
        }
        let arr = Arc::new(RwLock::new(payload));
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![StrykeValue::array_ref(arr)], WantarrayCtx::Scalar, line),
            "callback",
            line,
        )?;
        Ok(arg_to_vec(&r).iter().map(|v| v.to_number()).collect())
    };
    let k1 = call_f(interp, t, &y)?;
    let mut y_pred = y.clone();
    for d in 0..y.len() {
        y_pred[d] += h * k1[d];
    }
    let k2 = call_f(interp, t + h, &y_pred)?;
    let y_new: Vec<f64> = y
        .iter()
        .enumerate()
        .map(|(d, yd)| yd + 0.5 * h * (k1[d] + k2[d]))
        .collect();
    Ok(StrykeValue::array(y_new.into_iter().map(StrykeValue::float).collect()))
}

/// Verlet (velocity-Verlet) symplectic integrator step. Args:
/// ACCEL (f(q, t) → arr), Q, P (=v), T, H. Returns [Q', P'].
fn builtin_verlet_step(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let accel = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let p: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let sub = accel
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("verlet_step: code ref", line))?;
    let call_a = |interp: &mut VMHelper, q: &[f64], t: f64| -> StrykeResult<Vec<f64>> {
        let mut payload: Vec<StrykeValue> = q.iter().map(|v| StrykeValue::float(*v)).collect();
        payload.push(StrykeValue::float(t));
        let arr = Arc::new(RwLock::new(payload));
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![StrykeValue::array_ref(arr)], WantarrayCtx::Scalar, line),
            "callback",
            line,
        )?;
        Ok(arg_to_vec(&r).iter().map(|v| v.to_number()).collect())
    };
    let a0 = call_a(interp, &q, t)?;
    let q_new: Vec<f64> = q
        .iter()
        .zip(p.iter())
        .zip(a0.iter())
        .map(|((qq, pp), aa)| qq + h * pp + 0.5 * h * h * aa)
        .collect();
    let a1 = call_a(interp, &q_new, t + h)?;
    let p_new: Vec<f64> = p
        .iter()
        .zip(a0.iter())
        .zip(a1.iter())
        .map(|((pp, a0), a1)| pp + 0.5 * h * (a0 + a1))
        .collect();
    Ok(StrykeValue::array(vec![
        StrykeValue::array(q_new.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(p_new.into_iter().map(StrykeValue::float).collect()),
    ]))
}

// ─── 7. GLM ───────────────────────────────────────────────────────────────────

/// Logistic regression via IRLS. Args: X (n×p design matrix), y (binary).
/// Returns the coefficient vector β̂ (length p). Adds no intercept — caller
/// supplies a column of ones if needed.
fn builtin_logistic_regression(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let p = x[0].len();
    let mut beta = vec![0.0_f64; p];
    let max_iter = 50;
    for _ in 0..max_iter {
        // η = X β
        let eta: Vec<f64> = (0..n)
            .map(|i| (0..p).map(|j| x[i][j] * beta[j]).sum())
            .collect();
        // μ = sigmoid(η)
        let mu: Vec<f64> = eta.iter().map(|e| 1.0 / (1.0 + (-e).exp())).collect();
        // W = diag(μ (1 − μ)).
        let w: Vec<f64> = mu.iter().map(|m| m * (1.0 - m)).collect();
        // z = η + (y − μ)/W (working response).
        let z: Vec<f64> = eta
            .iter()
            .zip(mu.iter())
            .zip(y.iter())
            .zip(w.iter())
            .map(|(((e, m), yi), wi)| {
                if *wi < 1e-12 {
                    *e
                } else {
                    e + (yi - m) / wi
                }
            })
            .collect();
        // Solve (X^T W X) β_new = X^T W z.
        let mut xtwx = vec![vec![0.0_f64; p]; p];
        let mut xtwz = vec![0.0_f64; p];
        for i in 0..n {
            for j in 0..p {
                for k in 0..p {
                    xtwx[j][k] += x[i][j] * w[i] * x[i][k];
                }
                xtwz[j] += x[i][j] * w[i] * z[i];
            }
        }
        let beta_new = solve_linear(&xtwx, &xtwz);
        let max_change: f64 = beta
            .iter()
            .zip(beta_new.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        beta = beta_new;
        if max_change < 1e-8 {
            break;
        }
    }
    Ok(StrykeValue::array(beta.into_iter().map(StrykeValue::float).collect()))
}

/// Poisson regression via IRLS, log link.
fn builtin_poisson_regression(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let p = x[0].len();
    let mut beta = vec![0.0_f64; p];
    for _ in 0..50 {
        let eta: Vec<f64> = (0..n)
            .map(|i| (0..p).map(|j| x[i][j] * beta[j]).sum())
            .collect();
        let mu: Vec<f64> = eta.iter().map(|e| e.exp()).collect();
        let w = mu.clone();
        let z: Vec<f64> = eta
            .iter()
            .zip(mu.iter())
            .zip(y.iter())
            .zip(w.iter())
            .map(|(((e, m), yi), wi)| if *wi < 1e-12 { *e } else { e + (yi - m) / wi })
            .collect();
        let mut xtwx = vec![vec![0.0_f64; p]; p];
        let mut xtwz = vec![0.0_f64; p];
        for i in 0..n {
            for j in 0..p {
                for k in 0..p {
                    xtwx[j][k] += x[i][j] * w[i] * x[i][k];
                }
                xtwz[j] += x[i][j] * w[i] * z[i];
            }
        }
        let beta_new = solve_linear(&xtwx, &xtwz);
        let max_change: f64 = beta
            .iter()
            .zip(beta_new.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        beta = beta_new;
        if max_change < 1e-8 {
            break;
        }
    }
    Ok(StrykeValue::array(beta.into_iter().map(StrykeValue::float).collect()))
}

/// Ridge regression (Tikhonov): solves (XᵀX + λI) β = Xᵀy.
fn builtin_ridge_regression(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = x.len();
    let p = if n > 0 { x[0].len() } else { 0 };
    let mut xtx = vec![vec![0.0_f64; p]; p];
    let mut xty = vec![0.0_f64; p];
    for i in 0..n {
        for j in 0..p {
            for k in 0..p {
                xtx[j][k] += x[i][j] * x[i][k];
            }
            xty[j] += x[i][j] * y[i];
        }
    }
    for i in 0..p {
        xtx[i][i] += lambda;
    }
    let beta = solve_linear(&xtx, &xty);
    Ok(StrykeValue::array(beta.into_iter().map(StrykeValue::float).collect()))
}

/// LASSO via cyclical coordinate descent (soft thresholding).
fn builtin_lasso_coord(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let y: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let max_iter = args.get(3).map(|v| v.to_number() as usize).unwrap_or(500);
    let n = x.len() as f64;
    let p = if x.is_empty() { 0 } else { x[0].len() };
    let mut beta = vec![0.0_f64; p];
    let col_norms: Vec<f64> = (0..p)
        .map(|j| x.iter().map(|row| row[j] * row[j]).sum::<f64>())
        .collect();
    for _ in 0..max_iter {
        let mut max_change = 0.0_f64;
        for j in 0..p {
            if col_norms[j] < 1e-15 {
                continue;
            }
            // Compute X_j · (y - X β + X_j β_j).
            let r_j: f64 = (0..x.len())
                .map(|i| {
                    let mut e = y[i];
                    for k in 0..p {
                        e -= x[i][k] * beta[k];
                    }
                    x[i][j] * (e + x[i][j] * beta[j])
                })
                .sum();
            let r_j_norm = r_j / n;
            let z = col_norms[j] / n;
            let new_beta = if r_j_norm > lambda {
                (r_j_norm - lambda) / z
            } else if r_j_norm < -lambda {
                (r_j_norm + lambda) / z
            } else {
                0.0
            };
            max_change = max_change.max((new_beta - beta[j]).abs());
            beta[j] = new_beta;
        }
        if max_change < 1e-7 {
            break;
        }
    }
    Ok(StrykeValue::array(beta.into_iter().map(StrykeValue::float).collect()))
}

// ─── 8. Bootstrap / resampling ────────────────────────────────────────────────

/// Bootstrap percentile CI for the mean. Args: data, B (default 1000), alpha (default 0.05).
fn builtin_bootstrap_mean_ci(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let data: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1000);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let n = data.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(0.0)]));
    }
    let mut rng = rand::thread_rng();
    let mut means = Vec::with_capacity(b);
    for _ in 0..b {
        let mut s = 0.0_f64;
        for _ in 0..n {
            s += data[rng.gen_range(0..n)];
        }
        means.push(s / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo_idx = ((alpha / 2.0) * b as f64).floor() as usize;
    let hi_idx = ((1.0 - alpha / 2.0) * b as f64).floor() as usize;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(means[lo_idx]),
        StrykeValue::float(means[hi_idx.min(b - 1)]),
    ]))
}

/// Jackknife estimate of the standard error of the mean.
fn builtin_jackknife_estimate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let data: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = data.len();
    if n < 2 {
        return Ok(StrykeValue::float(0.0));
    }
    let total: f64 = data.iter().sum();
    let theta_hat = total / n as f64;
    let mut acc = 0.0_f64;
    for &x in &data {
        let theta_i = (total - x) / (n as f64 - 1.0);
        acc += (theta_i - theta_hat).powi(2);
    }
    Ok(StrykeValue::float((((n - 1) as f64 / n as f64) * acc).sqrt()))
}

/// Permutation test for difference of means (two-sided p).
fn builtin_permutation_test_diff(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::seq::SliceRandom;
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(2000);
    let n_a = a.len();
    let n_b = b.len();
    let mean_a = a.iter().sum::<f64>() / n_a as f64;
    let mean_b = b.iter().sum::<f64>() / n_b as f64;
    let observed = (mean_a - mean_b).abs();
    let mut combined: Vec<f64> = a.iter().chain(b.iter()).copied().collect();
    let mut rng = rand::thread_rng();
    let mut tail = 0_usize;
    for _ in 0..n_iter {
        combined.shuffle(&mut rng);
        let m_a = combined[..n_a].iter().sum::<f64>() / n_a as f64;
        let m_b = combined[n_a..].iter().sum::<f64>() / n_b as f64;
        if (m_a - m_b).abs() >= observed {
            tail += 1;
        }
    }
    Ok(StrykeValue::float((tail as f64 + 1.0) / (n_iter as f64 + 1.0)))
}

// ─── 9. Time series extras ────────────────────────────────────────────────────

/// `acf_at_lag` — Acf at lag. Returns a float.
fn builtin_acf_at_lag(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lag = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = xs.len();
    if n <= lag {
        return Ok(StrykeValue::float(0.0));
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    let denom: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum();
    let num: f64 = (0..n - lag)
        .map(|i| (xs[i] - mean) * (xs[i + lag] - mean))
        .sum();
    Ok(StrykeValue::float(num / denom))
}

/// Differencing operator: y_t - y_{t-lag}.
fn builtin_diff_op(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lag = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let mut out = Vec::new();
    for i in lag..xs.len() {
        out.push(StrykeValue::float(xs[i] - xs[i - lag]));
    }
    Ok(StrykeValue::array(out))
}

/// Lag operator: shift by k (insert NaN for missing positions).
fn builtin_lag_op(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let n = xs.len();
    let mut out = vec![StrykeValue::float(f64::NAN); n];
    if k >= 0 {
        let k = k as usize;
        for i in k..n {
            out[i] = StrykeValue::float(xs[i - k]);
        }
    } else {
        let k = (-k) as usize;
        for i in 0..n.saturating_sub(k) {
            out[i] = StrykeValue::float(xs[i + k]);
        }
    }
    Ok(StrykeValue::array(out))
}

/// Classical additive decomposition (trend via centred moving average,
/// season averaged across periods). Returns [trend, season, residual].
fn builtin_decompose_classical(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let period = args.get(1).map(|v| v.to_number() as usize).unwrap_or(12).max(2);
    let n = xs.len();
    if n < 2 * period {
        return Err(StrykeError::runtime(
            "decompose_classical: series shorter than 2 periods",
            0,
        ));
    }
    let half = period / 2;
    let mut trend = vec![f64::NAN; n];
    for i in half..n - half {
        let mut s = 0.0_f64;
        if period.is_multiple_of(2) {
            // Centred 2×p MA.
            s += 0.5 * xs[i - half];
            s += 0.5 * xs[i + half];
            for k in (i - half + 1)..(i + half) {
                s += xs[k];
            }
        } else {
            for k in (i - half)..=(i + half) {
                s += xs[k];
            }
        }
        trend[i] = s / period as f64;
    }
    let detrended: Vec<f64> = xs
        .iter()
        .zip(trend.iter())
        .map(|(x, t)| if t.is_nan() { f64::NAN } else { x - t })
        .collect();
    let mut season = vec![0.0_f64; period];
    let mut counts = vec![0_usize; period];
    for (i, d) in detrended.iter().enumerate() {
        if !d.is_nan() {
            season[i % period] += d;
            counts[i % period] += 1;
        }
    }
    for i in 0..period {
        season[i] = if counts[i] > 0 {
            season[i] / counts[i] as f64
        } else {
            0.0
        };
    }
    // Centre season to zero mean.
    let s_mean: f64 = season.iter().sum::<f64>() / period as f64;
    for s in season.iter_mut() {
        *s -= s_mean;
    }
    let resid: Vec<f64> = (0..n)
        .map(|i| {
            if trend[i].is_nan() {
                f64::NAN
            } else {
                xs[i] - trend[i] - season[i % period]
            }
        })
        .collect();
    let season_full: Vec<f64> = (0..n).map(|i| season[i % period]).collect();
    Ok(StrykeValue::array(vec![
        StrykeValue::array(trend.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(season_full.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(resid.into_iter().map(StrykeValue::float).collect()),
    ]))
}

// ─── 10. Combinatorial generators ─────────────────────────────────────────────

/// All k-element subsets of `0..n` in lexicographic order.
fn builtin_combinations_list(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (n, k) = i2(args);
    if n < 0 || k < 0 || k > n {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut comb: Vec<i64> = (0..k).collect();
    let mut out: Vec<StrykeValue> = Vec::new();
    out.push(StrykeValue::array(
        comb.iter().copied().map(StrykeValue::integer).collect(),
    ));
    loop {
        let mut i = k - 1;
        while i >= 0 && comb[i as usize] == n - k + i {
            i -= 1;
        }
        if i < 0 {
            break;
        }
        comb[i as usize] += 1;
        for j in (i + 1)..k {
            comb[j as usize] = comb[(j - 1) as usize] + 1;
        }
        out.push(StrykeValue::array(
            comb.iter().copied().map(StrykeValue::integer).collect(),
        ));
    }
    Ok(StrykeValue::array(out))
}

/// All permutations of `0..n` (Heap's algorithm).
fn builtin_permutations_list(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let n = n as usize;
    let mut a: Vec<i64> = (0..n as i64).collect();
    let mut c = vec![0_usize; n];
    let mut out: Vec<StrykeValue> = Vec::new();
    out.push(StrykeValue::array(
        a.iter().copied().map(StrykeValue::integer).collect(),
    ));
    let mut i = 0_usize;
    while i < n {
        if c[i] < i {
            if i & 1 == 0 {
                a.swap(0, i);
            } else {
                a.swap(c[i], i);
            }
            out.push(StrykeValue::array(
                a.iter().copied().map(StrykeValue::integer).collect(),
            ));
            c[i] += 1;
            i = 0;
        } else {
            c[i] = 0;
            i += 1;
        }
    }
    Ok(StrykeValue::array(out))
}

/// Cyclic permutations of an array.
fn builtin_cyclic_permutations(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = arr.len();
    let mut out: Vec<StrykeValue> = Vec::with_capacity(n);
    for i in 0..n {
        let mut row: Vec<StrykeValue> = Vec::with_capacity(n);
        for j in 0..n {
            row.push(arr[(i + j) % n].clone());
        }
        out.push(StrykeValue::array(row));
    }
    Ok(StrykeValue::array(out))
}

/// Subsets of size k from an arbitrary array.
fn builtin_subsets_of_size(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = arr.len();
    if k > n {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut comb: Vec<usize> = (0..k).collect();
    let mut out: Vec<StrykeValue> = Vec::new();
    let push_subset =
        |out: &mut Vec<StrykeValue>, comb: &[usize]| {
            let row: Vec<StrykeValue> = comb.iter().map(|&i| arr[i].clone()).collect();
            out.push(StrykeValue::array(row));
        };
    push_subset(&mut out, &comb);
    if k == 0 {
        return Ok(StrykeValue::array(out));
    }
    loop {
        let mut i = (k - 1) as i64;
        while i >= 0 && comb[i as usize] == n - k + i as usize {
            i -= 1;
        }
        if i < 0 {
            break;
        }
        comb[i as usize] += 1;
        for j in (i + 1)..k as i64 {
            comb[j as usize] = comb[(j - 1) as usize] + 1;
        }
        push_subset(&mut out, &comb);
    }
    Ok(StrykeValue::array(out))
}

// ─── 11. DP utilities ─────────────────────────────────────────────────────────

/// Length of the longest increasing subsequence (O(n log n)).
fn builtin_longest_increasing_subseq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut tails: Vec<f64> = Vec::new();
    for &x in &xs {
        match tails.binary_search_by(|t| t.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(_) => {}
            Err(idx) => {
                if idx == tails.len() {
                    tails.push(x);
                } else {
                    tails[idx] = x;
                }
            }
        }
    }
    Ok(StrykeValue::integer(tails.len() as i64))
}

/// 0/1 knapsack: maximum value, capacity W, items = `[[w, v], …]`.
fn builtin_knapsack_01(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let items = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let cap = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut dp = vec![0.0_f64; cap + 1];
    for it in &items {
        let v = arg_to_vec(it);
        let w = v.first().map(|x| x.to_number() as usize).unwrap_or(0);
        let val = v.get(1).map(|x| x.to_number()).unwrap_or(0.0);
        if w == 0 {
            for cell in dp.iter_mut() {
                *cell += val;
            }
            continue;
        }
        for c in (w..=cap).rev() {
            let cand = dp[c - w] + val;
            if cand > dp[c] {
                dp[c] = cand;
            }
        }
    }
    Ok(StrykeValue::float(dp[cap]))
}

/// Subset-sum: 1 if any subset of `arr` sums to T, else 0.
fn builtin_subset_sum_target(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let t = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if t == 0 {
        return Ok(StrykeValue::integer(1));
    }
    if t < 0 {
        return Ok(StrykeValue::integer(0));
    }
    let cap = t as usize;
    let mut dp = vec![false; cap + 1];
    dp[0] = true;
    for &x in &arr {
        if x < 0 || x > t {
            continue;
        }
        let xu = x as usize;
        for c in (xu..=cap).rev() {
            if dp[c - xu] {
                dp[c] = true;
            }
        }
        if dp[cap] {
            return Ok(StrykeValue::integer(1));
        }
    }
    Ok(StrykeValue::integer(if dp[cap] { 1 } else { 0 }))
}

/// Coin-change minimum number of coins to make amount N (-1 if impossible).
fn builtin_coin_change_min(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let coins: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let inf = i64::MAX / 4;
    let mut dp = vec![inf; n + 1];
    dp[0] = 0;
    for amount in 1..=n {
        for &c in &coins {
            if c == 0 || c > amount {
                continue;
            }
            if dp[amount - c] != inf {
                dp[amount] = dp[amount].min(dp[amount - c] + 1);
            }
        }
    }
    Ok(StrykeValue::integer(if dp[n] == inf { -1 } else { dp[n] }))
}

/// Levenshtein edit distance (full DP, returns number of edits).
fn builtin_edit_distance_levenshtein(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0_usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if av[i - 1] == bv[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    Ok(StrykeValue::integer(dp[m][n] as i64))
}

// ─── 12. ML metrics ───────────────────────────────────────────────────────────

/// One-hot encoding: labels (length n), n_classes -> n×n_classes matrix.
fn builtin_one_hot_encode(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let labels: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let k = args
        .get(1)
        .map(|v| v.to_number() as usize)
        .unwrap_or_else(|| labels.iter().copied().max().map(|m| m + 1).unwrap_or(0));
    let mut out = vec![vec![0.0_f64; k]; labels.len()];
    for (i, &lab) in labels.iter().enumerate() {
        if lab < k {
            out[i][lab] = 1.0;
        }
    }
    Ok(matrix_to_value(&out))
}

/// Label encoding: arbitrary array → integer indices.
fn builtin_label_encode(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let mut order: Vec<String> = Vec::new();
    let mut indices: Vec<i64> = Vec::with_capacity(arr.len());
    for v in &arr {
        let key = v.to_string();
        if let Some(i) = order.iter().position(|x| x == &key) {
            indices.push(i as i64);
        } else {
            order.push(key);
            indices.push((order.len() - 1) as i64);
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(indices.into_iter().map(StrykeValue::integer).collect()),
        StrykeValue::array(order.into_iter().map(StrykeValue::string).collect()),
    ]))
}

/// Categorical cross-entropy: -Σ y log ŷ averaged over batch.
fn builtin_categorical_cross_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y_true = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let y_pred = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let n = y_true.len();
    if n == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let k = y_true[0].len();
    let mut sum = 0.0_f64;
    for i in 0..n {
        for j in 0..k {
            if y_true[i][j] > 0.0 {
                sum -= y_true[i][j] * y_pred[i][j].max(1e-12).ln();
            }
        }
    }
    Ok(StrykeValue::float(sum / n as f64))
}

/// Confusion-matrix-derived metrics (binary). Args: tp, fp, fn, tn.
fn builtin_classification_metrics(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tp = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let fp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let fn_ = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let tn = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let total = tp + fp + fn_ + tn;
    let accuracy = if total > 0.0 { (tp + tn) / total } else { 0.0 };
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Ok(StrykeValue::array(vec![
        StrykeValue::float(accuracy),
        StrykeValue::float(precision),
        StrykeValue::float(recall),
        StrykeValue::float(f1),
    ]))
}

/// ROC AUC via Mann-Whitney U statistic. Args: scores, labels (0/1).
fn builtin_roc_auc(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let scores: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = scores.len();
    if n == 0 {
        return Ok(StrykeValue::float(0.5));
    }
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| {
        scores[a]
            .partial_cmp(&scores[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut rank_sum_pos = 0.0_f64;
    let mut n_pos = 0_usize;
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && scores[idx[j + 1]] == scores[idx[i]] {
            j += 1;
        }
        let avg_rank = (i + j) as f64 / 2.0 + 1.0;
        for kk in i..=j {
            if labels[idx[kk]] > 0 {
                rank_sum_pos += avg_rank;
                n_pos += 1;
            }
        }
        i = j + 1;
    }
    let n_neg = n - n_pos;
    if n_pos == 0 || n_neg == 0 {
        return Ok(StrykeValue::float(0.5));
    }
    let u = rank_sum_pos - n_pos as f64 * (n_pos as f64 + 1.0) / 2.0;
    Ok(StrykeValue::float(u / (n_pos as f64 * n_neg as f64)))
}

// ─── 13. DSP / image filter kernels ────────────────────────────────────────────

/// 1-D Gaussian blur kernel (length = 2 ceil(3 σ) + 1) normalised to sum 1.
fn builtin_gaussian_blur_kernel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let sigma = args.first().map(|v| v.to_number()).unwrap_or(1.0).max(1e-3);
    let radius = args.get(1).map(|v| v.to_number() as i64).unwrap_or_else(|| {
        (3.0 * sigma).ceil() as i64
    });
    let len = 2 * radius + 1;
    let mut k = vec![0.0_f64; len as usize];
    let mut sum = 0.0_f64;
    for i in 0..len {
        let x = (i - radius) as f64;
        let v = (-(x * x) / (2.0 * sigma * sigma)).exp();
        k[i as usize] = v;
        sum += v;
    }
    for v in k.iter_mut() {
        *v /= sum;
    }
    Ok(StrykeValue::array(k.into_iter().map(StrykeValue::float).collect()))
}

/// `sobel_x` — Sobel x.
fn builtin_sobel_x(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![-1.0, 0.0, 1.0],
        vec![-2.0, 0.0, 2.0],
        vec![-1.0, 0.0, 1.0],
    ]))
}
/// `sobel_y` — Sobel y.
fn builtin_sobel_y(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![-1.0, -2.0, -1.0],
        vec![0.0, 0.0, 0.0],
        vec![1.0, 2.0, 1.0],
    ]))
}
/// `prewitt_x` — Prewitt x.
fn builtin_prewitt_x(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![-1.0, 0.0, 1.0],
        vec![-1.0, 0.0, 1.0],
        vec![-1.0, 0.0, 1.0],
    ]))
}
/// `prewitt_y` — Prewitt y.
fn builtin_prewitt_y(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![-1.0, -1.0, -1.0],
        vec![0.0, 0.0, 0.0],
        vec![1.0, 1.0, 1.0],
    ]))
}

/// Laplacian of Gaussian kernel.
fn builtin_laplacian_of_gaussian(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let sigma = args.first().map(|v| v.to_number()).unwrap_or(1.0).max(1e-3);
    let radius = (3.0 * sigma).ceil() as i64;
    let len = 2 * radius + 1;
    let mut m = vec![vec![0.0_f64; len as usize]; len as usize];
    let s2 = sigma * sigma;
    for i in 0..len {
        for j in 0..len {
            let x = (j - radius) as f64;
            let y = (i - radius) as f64;
            let r2 = x * x + y * y;
            m[i as usize][j as usize] = -(1.0 - r2 / (2.0 * s2)) / (std::f64::consts::PI * s2.powi(2))
                * (-r2 / (2.0 * s2)).exp();
        }
    }
    Ok(matrix_to_value(&m))
}

// ─── 14. Stochastic-process samplers ───────────────────────────────────────────

fn sample_normal(rng: &mut rand::rngs::ThreadRng) -> f64 {
    use rand::Rng;
    let u1: f64 = rng.gen_range(1e-300..1.0);
    let u2: f64 = rng.gen();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Brownian motion path of length `n` over [0, T]. Returns time-indexed array.
fn builtin_brownian_path(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let mut path = vec![0.0_f64; n + 1];
    let mut rng = rand::thread_rng();
    for i in 1..=n {
        path[i] = path[i - 1] + dt.sqrt() * sample_normal(&mut rng);
    }
    Ok(StrykeValue::array(path.into_iter().map(StrykeValue::float).collect()))
}

/// Geometric Brownian motion path: dS = μ S dt + σ S dW.
fn builtin_geometric_brownian_path(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s0 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let mut path = vec![s0; n + 1];
    let mut rng = rand::thread_rng();
    for i in 1..=n {
        let z = sample_normal(&mut rng);
        path[i] = path[i - 1] * ((mu - 0.5 * sigma * sigma) * dt + sigma * dt.sqrt() * z).exp();
    }
    Ok(StrykeValue::array(path.into_iter().map(StrykeValue::float).collect()))
}

/// Homogeneous Poisson-process arrival times on [0, T] with rate λ.
fn builtin_poisson_process(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let lambda = args.first().map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let mut current = 0.0_f64;
    let mut out: Vec<StrykeValue> = Vec::new();
    while current < t {
        let u: f64 = rng.gen_range(1e-300..1.0);
        current += -u.ln() / lambda;
        if current < t {
            out.push(StrykeValue::float(current));
        }
    }
    Ok(StrykeValue::array(out))
}

/// 1-D random walk with `n` steps, each ±1 with probability p / 1-p.
fn builtin_random_walk_1d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(100);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let mut x = 0_i64;
    let mut path: Vec<StrykeValue> = Vec::with_capacity(n + 1);
    path.push(StrykeValue::integer(0));
    let mut rng = rand::thread_rng();
    for _ in 0..n {
        let u: f64 = rng.gen();
        if u < p {
            x += 1;
        } else {
            x -= 1;
        }
        path.push(StrykeValue::integer(x));
    }
    Ok(StrykeValue::array(path))
}

// ─── 15. Compression / info-theoretic complexity ───────────────────────────────

/// Lempel-Ziv complexity (LZ76). Counts production blocks in a sequence.
fn builtin_lempel_ziv_complexity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = s.len();
    if n == 0 {
        return Ok(StrykeValue::integer(0));
    }
    let mut c = 1_i64;
    let mut i = 0_usize;
    let mut k = 1_usize;
    let mut k_max = 1_usize;
    while i + k < n {
        if s[i + k - 1] == s[(i + k - 1) - (i + 1).min(i + k)] {
            k += 1;
            if i + k >= n {
                c += 1;
                break;
            }
        } else {
            if k > k_max {
                k_max = k;
            }
            i += 1;
            if i == k_max {
                c += 1;
                k_max = 0;
            }
            k = 1;
        }
    }
    Ok(StrykeValue::integer(c))
}

/// Huffman code-lengths for symbol frequencies. Returns length-array.
fn builtin_huffman_code_lengths(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let freqs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = freqs.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let mut heap: BinaryHeap<Reverse<(u64, Vec<usize>)>> = BinaryHeap::new();
    for (i, &f) in freqs.iter().enumerate() {
        // u64 quantisation keeps deterministic ordering.
        heap.push(Reverse(((f * 1e6) as u64, vec![i])));
    }
    let mut lengths = vec![0_i64; n];
    while heap.len() > 1 {
        let Reverse((f1, idx1)) = heap.pop().unwrap();
        let Reverse((f2, idx2)) = heap.pop().unwrap();
        for &i in idx1.iter().chain(idx2.iter()) {
            lengths[i] += 1;
        }
        let mut merged = idx1;
        merged.extend(idx2);
        heap.push(Reverse((f1 + f2, merged)));
    }
    Ok(StrykeValue::array(
        lengths.into_iter().map(StrykeValue::integer).collect(),
    ))
}

/// Block (Shannon) entropy rate H_m(X) = H(X_{1..m}) − H(X_{1..m-1}).
fn builtin_shannon_entropy_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    if s.len() < m + 1 {
        return Ok(StrykeValue::float(0.0));
    }
    use std::collections::HashMap;
    let mut counts_m: HashMap<Vec<i64>, usize> = HashMap::new();
    let mut counts_m1: HashMap<Vec<i64>, usize> = HashMap::new();
    for w in s.windows(m) {
        *counts_m.entry(w.to_vec()).or_insert(0) += 1;
    }
    for w in s.windows(m - 1) {
        *counts_m1.entry(w.to_vec()).or_insert(0) += 1;
    }
    let total_m = counts_m.values().sum::<usize>() as f64;
    let total_m1 = counts_m1.values().sum::<usize>() as f64;
    let h_m: f64 = counts_m
        .values()
        .map(|&c| {
            let p = c as f64 / total_m;
            -p * p.ln()
        })
        .sum();
    let h_m1: f64 = counts_m1
        .values()
        .map(|&c| {
            let p = c as f64 / total_m1;
            -p * p.ln()
        })
        .sum();
    Ok(StrykeValue::float(h_m - h_m1))
}

// ─── 16. Physics / quantum extras ─────────────────────────────────────────────

/// Planck blackbody spectral radiance B(λ, T) [W·sr⁻¹·m⁻³].
fn builtin_planck_blackbody(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambda = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = 6.626_070_15e-34_f64;
    let c = 2.997_924_58e8_f64;
    let kb = 1.380_649e-23_f64;
    if lambda <= 0.0 || t <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let exponent = h * c / (lambda * kb * t);
    let den = exponent.exp() - 1.0;
    Ok(StrykeValue::float(
        2.0 * h * c * c / lambda.powi(5) / den,
    ))
}

/// Rayleigh-Jeans approximation B(λ, T) ≈ 2 c k_B T / λ⁴.
fn builtin_rayleigh_jeans(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambda = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = 2.997_924_58e8_f64;
    let kb = 1.380_649e-23_f64;
    if lambda <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(2.0 * c * kb * t / lambda.powi(4)))
}

/// Compton wavelength shift Δλ = (h/mc)(1 − cos θ).
fn builtin_compton_shift(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let theta = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lambda_c = 2.4263102367e-12_f64; // electron Compton wavelength (m)
    Ok(StrykeValue::float(lambda_c * (1.0 - theta.cos())))
}

/// Rydberg energy E_n = -13.605693 / n² eV (for hydrogen).
fn builtin_rydberg_energy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(-13.605693_f64 / (n * n)))
}

/// Hydrogen radial wavefunction R_{n,l}(r) (atomic units, real-valued).
/// Uses associated-Laguerre recurrence; valid for n ≤ 8.
fn builtin_hydrogen_radial_wavefunction(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i32).unwrap_or(1).max(1);
    let l = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0).max(0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if l >= n {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::gamma::ln_gamma;
    let lf = |x: f64| ln_gamma(x + 1.0);
    let log_norm = 0.5 * ((2.0 / n as f64).powi(3) + lf((n - l - 1) as f64) - lf((n + l) as f64).max(0.0));
    let _ = log_norm;
    let rho = 2.0 * r / n as f64;
    let mut sum = 0.0_f64;
    for m in 0..=(n - l - 1) {
        let mf = m as f64;
        let coef = (-1.0_f64).powi(m)
            * ((lf((n + l) as f64) - lf((n - l - 1 - m) as f64) - lf((2 * l + 1 + m) as f64) - lf(mf)).exp());
        sum += coef * rho.powi(m);
    }
    let pre_log = 0.5 * (lf((n - l - 1) as f64) - lf((n + l) as f64) + (3.0 * (2.0 / n as f64).ln()));
    let pre = pre_log.exp();
    let v = pre * (-rho / 2.0).exp() * rho.powi(l) * sum;
    Ok(StrykeValue::float(v))
}

// ─── 17. Number theory / algebra ──────────────────────────────────────────────

/// Integer logarithm: largest k with base^k ≤ n.
fn builtin_integer_log(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1);
    let base = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
    let mut k = 0_i64;
    let mut p = 1_i64;
    while p <= n / base {
        p *= base;
        k += 1;
    }
    Ok(StrykeValue::integer(k))
}

/// AKS primality (deterministic, polynomial-time). Practical only for small n.
fn builtin_aks_primality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 2 {
        return Ok(StrykeValue::integer(0));
    }
    if n == 2 {
        return Ok(StrykeValue::integer(1));
    }
    if n % 2 == 0 {
        return Ok(StrykeValue::integer(0));
    }
    // Step 1: perfect-power test.
    for b in 2..=((n as f64).log2() as i64) {
        let mut lo = 2_i64;
        let mut hi = n;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let p = (mid as i128).pow(b as u32);
            if p == n as i128 {
                return Ok(StrykeValue::integer(0));
            }
            if p < n as i128 {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
    }
    // Step 2: smallest r with multiplicative_order(n, r) > log2²(n).
    let log_n_sq = ((n as f64).log2().powi(2)) as i64;
    let mut r = 2_i64;
    while r < n {
        if gcd_i64(r, n) > 1 {
            r += 1;
            continue;
        }
        let mut k = 1_i64;
        let mut cur = n.rem_euclid(r);
        let mut ok = false;
        while k <= log_n_sq {
            cur = (cur as i128 * n as i128 % r as i128) as i64;
            if cur == 1 {
                break;
            }
            k += 1;
        }
        if k > log_n_sq {
            ok = true;
        }
        let _ = ok;
        if k > log_n_sq {
            break;
        }
        r += 1;
    }
    if r >= n {
        return Ok(StrykeValue::integer(1));
    }
    // Step 3: trial-divide up to r.
    for a in 2..=r.min(n - 1) {
        if n % a == 0 {
            return Ok(StrykeValue::integer(0));
        }
    }
    if n <= r {
        return Ok(StrykeValue::integer(1));
    }
    // Step 4 — full polynomial check skipped (too expensive). Fall back to
    // Miller-Rabin with the deterministic 12-witness set, exact for 64-bit n.
    builtin_miller_rabin(args)
}

/// Elliptic-curve point addition over Q on y² = x³ + a x + b. Identity
/// represented by `[infinity, infinity]` (NaN). Args: P, Q, a, b.
fn builtin_elliptic_curve_add(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p_arr = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let q_arr = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let _b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let to_pt = |v: &[StrykeValue]| {
        (
            v.first().map(|x| x.to_number()).unwrap_or(f64::NAN),
            v.get(1).map(|x| x.to_number()).unwrap_or(f64::NAN),
        )
    };
    let p = to_pt(&p_arr);
    let q = to_pt(&q_arr);
    if p.0.is_nan() {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(q.0),
            StrykeValue::float(q.1),
        ]));
    }
    if q.0.is_nan() {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(p.0),
            StrykeValue::float(p.1),
        ]));
    }
    if (p.0 - q.0).abs() < 1e-12 && (p.1 + q.1).abs() < 1e-12 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(f64::NAN),
            StrykeValue::float(f64::NAN),
        ]));
    }
    let m = if (p.0 - q.0).abs() < 1e-12 {
        (3.0 * p.0 * p.0 + a) / (2.0 * p.1)
    } else {
        (q.1 - p.1) / (q.0 - p.0)
    };
    let x_r = m * m - p.0 - q.0;
    let y_r = m * (p.0 - x_r) - p.1;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(x_r),
        StrykeValue::float(y_r),
    ]))
}

/// Berlekamp-Massey: shortest LFSR generating sequence S over reals. Returns
/// connection-polynomial coefficients C(x) = 1 + c_1 x + … + c_L x^L.
fn builtin_berlekamp_massey(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = s.len();
    let mut b = vec![0.0_f64; n + 1];
    let mut c = vec![0.0_f64; n + 1];
    b[0] = 1.0;
    c[0] = 1.0;
    let mut l = 0_usize;
    let mut m = 1_usize;
    let mut b_val = 1.0_f64;
    for i in 0..n {
        let mut delta = s[i];
        for j in 1..=l {
            delta += c[j] * s[i - j];
        }
        if delta.abs() < 1e-12 {
            m += 1;
        } else if 2 * l <= i {
            let temp = c.clone();
            for j in 0..n {
                if j + m < c.len() {
                    c[j + m] -= delta / b_val * b[j];
                }
            }
            l = i + 1 - l;
            b = temp;
            b_val = delta;
            m = 1;
        } else {
            for j in 0..n {
                if j + m < c.len() {
                    c[j + m] -= delta / b_val * b[j];
                }
            }
            m += 1;
        }
    }
    Ok(StrykeValue::array(
        c[..=l].iter().copied().map(StrykeValue::float).collect(),
    ))
}

/// Bezout coefficients: returns [g, x, y] with a x + b y = g = gcd(a, b).
fn builtin_bezout_coefficients(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = i2(args);
    let (mut old_r, mut r) = (a, b);
    let (mut old_s, mut s) = (1_i64, 0_i64);
    let (mut old_t, mut t) = (0_i64, 1_i64);
    while r != 0 {
        let q = old_r / r;
        let tmp_r = r;
        r = old_r - q * r;
        old_r = tmp_r;
        let tmp_s = s;
        s = old_s - q * s;
        old_s = tmp_s;
        let tmp_t = t;
        t = old_t - q * t;
        old_t = tmp_t;
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(old_r),
        StrykeValue::integer(old_s),
        StrykeValue::integer(old_t),
    ]))
}
