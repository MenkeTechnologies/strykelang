// R/SciPy distributions and tests: pdf/cdf/quantile/inverse for
// Normal, Student t, χ², F, plus rank-based and goodness-of-fit tests.

fn b53_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// Lanczos log-gamma (real argument). Used internally for t and χ² densities.
fn b53_lgamma(z: f64) -> f64 {
    if z < 0.5 {
        std::f64::consts::PI.ln() - (std::f64::consts::PI * z).sin().ln() - b53_lgamma(1.0 - z)
    } else {
        const G: f64 = 7.0;
        const C: [f64; 9] = [
            0.999_999_999_999_809_9,
            676.520_368_121_885_1,
            -1_259.139_216_722_402_8,
            771.323_428_777_653_1,
            -176.615_029_162_140_6,
            12.507_343_278_686_905,
            -0.138_571_095_265_720_12,
            9.984_369_578_019_572e-6,
            1.505_632_735_149_311_6e-7,
        ];
        let z = z - 1.0;
        let mut x = C[0];
        for (i, &c) in C.iter().enumerate().skip(1) { x += c / (z + i as f64); }
        let t = z + G + 0.5;
        0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + x.ln()
    }
}

/// Standard normal pdf φ(x) = (2π)^(-1/2) exp(-x²/2).
fn builtin_dnorm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let z = (x - mu) / sigma;
    Ok(StrykeValue::float((-z * z / 2.0).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt())))
}

/// Student t pdf with ν degrees of freedom.
fn builtin_dt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let nu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let log_norm = b53_lgamma((nu + 1.0) / 2.0) - b53_lgamma(nu / 2.0)
        - 0.5 * ((nu * std::f64::consts::PI).ln());
    let log_kernel = -((nu + 1.0) / 2.0) * (1.0 + x * x / nu).ln();
    Ok(StrykeValue::float((log_norm + log_kernel).exp()))
}

/// F distribution pdf with d1, d2 degrees of freedom.
fn builtin_df_dist(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let d1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let d2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    if x <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let log_b = b53_lgamma(d1 / 2.0) + b53_lgamma(d2 / 2.0) - b53_lgamma((d1 + d2) / 2.0);
    let log_p = (d1 / 2.0) * (d1 / d2).ln() + (d1 / 2.0 - 1.0) * x.ln()
        - ((d1 + d2) / 2.0) * (1.0 + d1 * x / d2).ln() - log_b;
    Ok(StrykeValue::float(log_p.exp()))
}

/// χ² pdf with k degrees of freedom.
fn builtin_dchisq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    if x <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let log_p = (k / 2.0 - 1.0) * x.ln() - x / 2.0
        - (k / 2.0) * 2.0_f64.ln() - b53_lgamma(k / 2.0);
    Ok(StrykeValue::float(log_p.exp()))
}

/// Generalized linear model log-likelihood for Normal: Σ log φ((y_i - μ_i)/σ).
/// Args: y, mu, sigma — equal-length arrays (or sigma scalar).
fn builtin_glm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mu = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let n = y.len().min(mu.len());
    let log_norm = -0.5 * (2.0 * std::f64::consts::PI).ln() - sigma.ln();
    let s: f64 = (0..n).map(|i| {
        let z = (y[i] - mu[i]) / sigma;
        log_norm - 0.5 * z * z
    }).sum();
    Ok(StrykeValue::float(s))
}

/// One-way ANOVA F statistic. Args: array of group sums of squares, group sizes,
/// total mean, group means flat.
fn builtin_aov(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let group_means = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let group_sizes = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let group_ss = b53_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let k = group_means.len();
    if k < 2 { return Ok(StrykeValue::float(0.0)); }
    let n_total: f64 = group_sizes.iter().sum();
    let grand: f64 = group_means.iter().zip(group_sizes.iter())
        .map(|(m, n)| m * n).sum::<f64>() / n_total.max(1.0);
    let ss_between: f64 = group_means.iter().zip(group_sizes.iter())
        .map(|(m, n)| n * (m - grand).powi(2)).sum();
    let ss_within: f64 = group_ss.iter().sum();
    let ms_between = ss_between / (k as f64 - 1.0).max(1.0);
    let ms_within = ss_within / (n_total - k as f64).max(1.0);
    if ms_within == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(ms_between / ms_within))
}

/// Shapiro-Wilk W statistic (small-n approximation, n ≤ 50). Uses Royston's
/// formula for a-coefficients. Args: sorted sample.
fn builtin_shapiro_wilk(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len();
    if n < 3 { return Ok(StrykeValue::float(1.0)); }
    let mean: f64 = x.iter().sum::<f64>() / n as f64;
    let ss: f64 = x.iter().map(|v| (v - mean).powi(2)).sum();
    if ss == 0.0 { return Ok(StrykeValue::float(1.0)); }
    let mut m = vec![0.0_f64; n];
    for i in 0..n {
        let p = (i as f64 + 1.0 - 0.375) / (n as f64 + 0.25);
        m[i] = b53_norm_inv(p);
    }
    let mm: f64 = m.iter().map(|v| v * v).sum();
    let mut a = vec![0.0_f64; n];
    let sqrt_mm = mm.sqrt();
    for i in 0..n { a[i] = m[i] / sqrt_mm; }
    let mut sorted = x.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let num: f64 = (0..n).map(|i| a[i] * sorted[i]).sum::<f64>().powi(2);
    Ok(StrykeValue::float((num / ss).clamp(0.0, 1.0)))
}

fn b53_norm_inv(p: f64) -> f64 {
    if p <= 0.0 || p >= 1.0 { return f64::NAN; }
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = q * q;
        q * ((((((-39.6968_30 * r + 220.9460_984) * r - 275.928_510_4) * r
            + 138.357_751_6) * r - 30.664_798_07) * r + 2.506_628_277_4)
            / (((((-54.476_098_8 * r + 161.585_836_9) * r - 155.698_979_2) * r
            + 66.801_311_88) * r - 13.280_681_55) * r + 1.0))
    } else {
        let r = if q < 0.0 { p } else { 1.0 - p };
        let r = (-r.ln()).sqrt();
        let v = if r <= 5.0 {
            let r = r - 1.6;
            (((((((0.000_077_454_501 * r + 0.022_723_844_3) * r + 0.241_780_725_2) * r
                + 1.270_456_56) * r + 3.647_848_06) * r + 5.769_497_22) * r
                + 4.630_337_85) * r + 1.423_437_11)
                / (((((((1.054_75e-9 * r + 5.475_938_8e-4) * r + 0.015_198_666_9) * r
                + 0.148_103_976_7) * r + 0.689_767_334_5) * r + 1.676_384_83) * r
                + 2.053_191_29) * r + 1.0)
        } else {
            let r = r - 5.0;
            ((((((2.010_334_4e-7 * r + 2.711_555_68e-5) * r + 0.001_242_660_95) * r
                + 0.026_532_189_5) * r + 0.296_560_571_8) * r + 1.784_826_54) * r
                + 5.463_054_97) / 1.0
        };
        if q < 0.0 { -v } else { v }
    }
}

/// Anderson-Darling A² (sorted sample of n from F): −n − (1/n) Σ (2i−1)
/// [ln F(x_i) + ln(1 − F(x_{n+1−i}))]. Works with normal F via z-scores.
fn builtin_anderson_darling(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len();
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mean: f64 = x.iter().sum::<f64>() / n as f64;
    let var: f64 = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    if var <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let sd = var.sqrt();
    let mut z: Vec<f64> = x.iter().map(|v| (v - mean) / sd).collect();
    z.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let cdf = |q: f64| 0.5 * (1.0 + libm::erf(q / std::f64::consts::SQRT_2));
    let mut s = 0.0_f64;
    for i in 0..n {
        let f_lo = cdf(z[i]).clamp(1e-300, 1.0 - 1e-300);
        let f_hi = (1.0 - cdf(z[n - 1 - i])).clamp(1e-300, 1.0 - 1e-300);
        s += (2 * i as i64 + 1) as f64 * (f_lo.ln() + f_hi.ln());
    }
    Ok(StrykeValue::float(-(n as f64) - s / n as f64))
}

/// Kolmogorov-Smirnov two-sample D = max |F1(x) − F2(x)| from sorted samples.
fn builtin_kolmogorov_smirnov(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut b = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    if a.is_empty() || b.is_empty() { return Ok(StrykeValue::float(0.0)); }
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let (mut i, mut j) = (0_usize, 0_usize);
    let (n, m) = (a.len(), b.len());
    let mut d = 0.0_f64;
    while i < n && j < m {
        if a[i] <= b[j] { i += 1; } else { j += 1; }
        let f1 = i as f64 / n as f64;
        let f2 = j as f64 / m as f64;
        d = d.max((f1 - f2).abs());
    }
    Ok(StrykeValue::float(d))
}

/// Spearman rank correlation: replace data with ranks, compute Pearson r.
fn builtin_spearmanr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len().min(y.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let rx = b53_ranks(&x[..n]);
    let ry = b53_ranks(&y[..n]);
    let mx = rx.iter().sum::<f64>() / n as f64;
    let my = ry.iter().sum::<f64>() / n as f64;
    let mut num = 0.0_f64;
    let mut dx = 0.0_f64;
    let mut dy = 0.0_f64;
    for i in 0..n {
        num += (rx[i] - mx) * (ry[i] - my);
        dx += (rx[i] - mx).powi(2);
        dy += (ry[i] - my).powi(2);
    }
    if dx <= 0.0 || dy <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(num / (dx * dy).sqrt()))
}

fn b53_ranks(v: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap_or(std::cmp::Ordering::Equal));
    let mut r = vec![0.0_f64; v.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && v[idx[j]] == v[idx[i]] { j += 1; }
        let avg_rank = (i + j - 1) as f64 / 2.0 + 1.0;
        for k in i..j { r[idx[k]] = avg_rank; }
        i = j;
    }
    r
}

/// Kendall τ (tau-b): (concordant - discordant) / sqrt((P-T_x)(P-T_y)).
fn builtin_kendalltau(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len().min(y.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let total_pairs = (n * (n - 1) / 2) as f64;
    let (mut conc, mut disc) = (0_i64, 0_i64);
    let (mut t_x, mut t_y) = (0_i64, 0_i64);
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = x[j] - x[i];
            let dy = y[j] - y[i];
            if dx == 0.0 { t_x += 1; }
            if dy == 0.0 { t_y += 1; }
            let prod = dx * dy;
            if prod > 0.0 { conc += 1; }
            else if prod < 0.0 { disc += 1; }
        }
    }
    let denom = ((total_pairs - t_x as f64) * (total_pairs - t_y as f64)).sqrt();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((conc - disc) as f64 / denom))
}

/// Pearson r, the canonical correlation.
fn builtin_pearsonr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = x.len().min(y.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = x.iter().take(n).sum::<f64>() / n as f64;
    let my = y.iter().take(n).sum::<f64>() / n as f64;
    let mut num = 0.0_f64;
    let mut dx = 0.0_f64;
    let mut dy = 0.0_f64;
    for i in 0..n {
        num += (x[i] - mx) * (y[i] - my);
        dx += (x[i] - mx).powi(2);
        dy += (y[i] - my).powi(2);
    }
    if dx <= 0.0 || dy <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(num / (dx * dy).sqrt()))
}

/// Mann-Whitney U: U_x = sum_of_ranks_x − n_x(n_x+1)/2.
fn builtin_mannwhitneyu(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = b53_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n_x = x.len();
    if n_x == 0 || y.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mut combined: Vec<(f64, u8)> = x.iter().map(|&v| (v, 0)).chain(y.iter().map(|&v| (v, 1))).collect();
    combined.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0_f64; combined.len()];
    let mut i = 0;
    while i < combined.len() {
        let mut j = i + 1;
        while j < combined.len() && combined[j].0 == combined[i].0 { j += 1; }
        let avg = (i + j - 1) as f64 / 2.0 + 1.0;
        for k in i..j { ranks[k] = avg; }
        i = j;
    }
    let r_x: f64 = combined.iter().zip(ranks.iter())
        .filter(|(c, _)| c.1 == 0).map(|(_, r)| r).sum();
    Ok(StrykeValue::float(r_x - n_x as f64 * (n_x as f64 + 1.0) / 2.0))
}

/// Wilcoxon signed-rank statistic W = sum of positive signed ranks.
fn builtin_wilcoxon(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = b53_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let pairs: Vec<(f64, f64)> = d.iter().filter(|&&x| x != 0.0)
        .map(|&x| (x.abs(), x.signum())).collect();
    if pairs.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mut idx: Vec<usize> = (0..pairs.len()).collect();
    idx.sort_by(|&a, &b| pairs[a].0.partial_cmp(&pairs[b].0).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0_f64; pairs.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && pairs[idx[j]].0 == pairs[idx[i]].0 { j += 1; }
        let avg = (i + j - 1) as f64 / 2.0 + 1.0;
        for k in i..j { ranks[idx[k]] = avg; }
        i = j;
    }
    let w_pos: f64 = (0..pairs.len()).filter(|&i| pairs[i].1 > 0.0)
        .map(|i| ranks[i]).sum();
    Ok(StrykeValue::float(w_pos))
}

/// Kruskal-Wallis H = (12 / N(N+1)) Σ T_g²/n_g − 3(N+1).
fn builtin_kruskal_h(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let groups = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let group_data: Vec<Vec<f64>> = groups.iter().map(b53_to_floats).collect();
    if group_data.len() < 2 { return Ok(StrykeValue::float(0.0)); }
    let mut combined: Vec<(f64, usize)> = Vec::new();
    for (gi, g) in group_data.iter().enumerate() {
        for &x in g { combined.push((x, gi)); }
    }
    let big_n = combined.len();
    if big_n < 2 { return Ok(StrykeValue::float(0.0)); }
    combined.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut ranks = vec![0.0_f64; big_n];
    let mut i = 0;
    while i < big_n {
        let mut j = i + 1;
        while j < big_n && combined[j].0 == combined[i].0 { j += 1; }
        let avg = (i + j - 1) as f64 / 2.0 + 1.0;
        for k in i..j { ranks[k] = avg; }
        i = j;
    }
    let mut group_t = vec![0.0_f64; group_data.len()];
    let mut group_n = vec![0_usize; group_data.len()];
    for (k, c) in combined.iter().enumerate() {
        group_t[c.1] += ranks[k];
        group_n[c.1] += 1;
    }
    let h: f64 = group_data.iter().enumerate().map(|(g, _)| {
        if group_n[g] == 0 { 0.0 } else { group_t[g].powi(2) / group_n[g] as f64 }
    }).sum::<f64>() * 12.0 / (big_n as f64 * (big_n as f64 + 1.0))
        - 3.0 * (big_n as f64 + 1.0);
    Ok(StrykeValue::float(h))
}
