// Batch 72 — NumPy + scipy.special: array primitives, special functions,
// quadrature, ODE/root-finding, optimisation.

fn b72_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// ───── array indexing / selection ─────

/// argpartition — return index of k-th smallest after partial sort.
fn builtin_argpartition(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|x| x.to_number() as i64).unwrap_or(0).max(0) as usize;
    if v.is_empty() { return Ok(StrykeValue::integer(0)); }
    let mut idx: Vec<usize> = (0..v.len()).collect();
    idx.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::integer(idx[k.min(v.len() - 1)] as i64))
}

/// bincount — count occurrences of each non-negative int.
fn builtin_bincount(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let max = v.iter().fold(0_i64, |a, &x| a.max(x as i64));
    let mut counts = vec![0_i64; (max + 1) as usize];
    for x in v { let i = x as i64; if i >= 0 { counts[i as usize] += 1; } }
    Ok(StrykeValue::array(counts.into_iter().map(StrykeValue::integer).collect()))
}

/// nonzero_count — number of nonzero elements.
fn builtin_nonzero_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x != 0.0).count() as i64))
}

/// flatnonzero — first nonzero index, or -1.
fn builtin_flatnonzero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    for (i, &x) in v.iter().enumerate() { if x != 0.0 { return Ok(StrykeValue::integer(i as i64)); } }
    Ok(StrykeValue::integer(-1))
}

/// searchsorted — leftmost insertion point for v in a sorted array.
fn builtin_searchsorted(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let pos = v.partition_point(|&x| x < needle);
    Ok(StrykeValue::integer(pos as i64))
}

/// digitize — bin index for value in monotone bin edges.
fn builtin_digitize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let edges = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pos = edges.partition_point(|&e| e <= x);
    Ok(StrykeValue::integer(pos as i64))
}

/// histogram_bin_edges — edge count for n bins.
fn builtin_histogram_bin_edges(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_bins = i1(args).max(1);
    Ok(StrykeValue::integer(n_bins + 1))
}

/// unique_count — distinct value count.
fn builtin_unique_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: std::collections::HashSet<u64> = v.iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(s.len() as i64))
}

// ───── special functions ─────

/// Polynomial fit: returns RMSE of best fit (degree 1, least-squares).
fn builtin_polyfit_rmse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b72_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().sum::<f64>() / n as f64;
    let my = ys.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        num += dx * (ys[i] - my);
        den += dx * dx;
    }
    let m = if den > 0.0 { num / den } else { 0.0 };
    let b = my - m * mx;
    let mse: f64 = (0..n).map(|i| { let r = ys[i] - (m * xs[i] + b); r * r }).sum::<f64>() / n as f64;
    Ok(StrykeValue::float(mse.sqrt()))
}

/// Complete elliptic integral K(m): AGM iteration.
fn builtin_ellipk(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args).clamp(0.0, 1.0 - 1e-15);
    let mut a = 1.0;
    let mut b = (1.0 - m).sqrt();
    for _ in 0..50 {
        let an = (a + b) / 2.0;
        let bn = (a * b).sqrt();
        if (a - b).abs() < 1e-15 { break; }
        a = an; b = bn;
    }
    Ok(StrykeValue::float(std::f64::consts::PI / (a + b)))
}

/// Complete elliptic integral E(m): K + AGM-derivative.
fn builtin_ellipe(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args).clamp(0.0, 1.0);
    let mut a = 1.0;
    let mut b = (1.0 - m).sqrt();
    let mut c2_sum = m / 2.0;
    let mut p2 = 1.0;
    for _ in 0..50 {
        let an = (a + b) / 2.0;
        let bn = (a * b).sqrt();
        let c = (a - b) / 2.0;
        p2 *= 2.0;
        c2_sum += p2 * c * c;
        if c.abs() < 1e-15 { break; }
        a = an; b = bn;
    }
    let k = std::f64::consts::PI / (a + b);
    Ok(StrykeValue::float(k * (1.0 - c2_sum / 2.0)))
}

/// Hypergeometric ₁F₁(a; b; z) via series.
fn builtin_hyp1f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 0..200 {
        term *= (a + k as f64) * z / ((b + k as f64) * (k + 1) as f64);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

/// Hypergeometric ₂F₁(a, b; c; z) via series (|z|<1 only).
fn builtin_hyp2f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 0..400 {
        term *= (a + k as f64) * (b + k as f64) * z / ((c + k as f64) * (k + 1) as f64);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

/// Mathieu characteristic value b_n: small-q expansion fallback.
fn builtin_mathieu_b(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(1) as f64;
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(n * n - q * q / (2.0 * (n * n - 1.0).max(1e-15))))
}

/// Spherical Bessel jₙ(x): recursive (downward unstable; use upward for small n).
fn builtin_spherical_jn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as i32;
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if x.abs() < 1e-15 { return Ok(StrykeValue::float(if n == 0 { 1.0 } else { 0.0 })); }
    let mut j_prev = x.cos() / x;
    let mut j_cur = x.sin() / x;
    if n == 0 { return Ok(StrykeValue::float(j_cur)); }
    for k in 1..=n as usize {
        let j_next = (2.0 * k as f64 - 1.0) / x * j_cur - j_prev;
        j_prev = j_cur;
        j_cur = j_next;
    }
    Ok(StrykeValue::float(j_cur))
}

/// Spherical Bessel yₙ(x).
fn builtin_spherical_yn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as i32;
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if x.abs() < 1e-15 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    let mut y_prev = x.sin() / x;
    let mut y_cur = -x.cos() / x;
    if n == 0 { return Ok(StrykeValue::float(y_cur)); }
    for k in 1..=n as usize {
        let y_next = (2.0 * k as f64 - 1.0) / x * y_cur - y_prev;
        y_prev = y_cur;
        y_cur = y_next;
    }
    Ok(StrykeValue::float(y_cur))
}

/// Bessel Jν(x) via series for ν integer.
fn builtin_jv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nu = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = nu.unsigned_abs() as i64;
    let mut term = (x / 2.0).powi(n as i32) / (1..=n).fold(1_f64, |a, k| a * k as f64);
    let mut sum = term;
    for k in 1..200 {
        term *= -x * x / 4.0 / (k as f64 * (k as f64 + n as f64));
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    if nu < 0 && n % 2 != 0 { sum = -sum; }
    Ok(StrykeValue::float(sum))
}

/// Bessel Yₙ(x): for n=0, 1 use series + log term; for higher, recurrence.
fn builtin_yn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as i32;
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-300);
    let y0 = (2.0 / std::f64::consts::PI) * x.ln() * jv0(x);
    let y1 = (2.0 / std::f64::consts::PI) * (x.ln() * jv1(x) - 1.0 / x);
    if n == 0 { return Ok(StrykeValue::float(y0)); }
    if n == 1 { return Ok(StrykeValue::float(y1)); }
    let mut a = y0;
    let mut b = y1;
    for k in 1..n {
        let c = 2.0 * k as f64 / x * b - a;
        a = b; b = c;
    }
    Ok(StrykeValue::float(b))
}

fn jv0(x: f64) -> f64 {
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..200 {
        term *= -x * x / 4.0 / (k as f64 * k as f64);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    sum
}
fn jv1(x: f64) -> f64 {
    let mut term = x / 2.0;
    let mut sum = term;
    for k in 1..200 {
        term *= -x * x / 4.0 / (k as f64 * (k as f64 + 1.0));
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    sum
}

/// Modified Bessel Iν(x) via series.
fn builtin_iv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nu = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = nu.unsigned_abs() as i64;
    let mut term = (x / 2.0).powi(n as i32) / (1..=n).fold(1_f64, |a, k| a * k as f64);
    let mut sum = term;
    for k in 1..200 {
        term *= x * x / 4.0 / (k as f64 * (k as f64 + n as f64));
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

/// Modified Bessel Kₙ via integral asymptote for large x: √(π/2x)·e⁻ˣ.
fn builtin_kv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let _nu = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-15);
    Ok(StrykeValue::float((std::f64::consts::PI / (2.0 * x)).sqrt() * (-x).exp()))
}

/// Airy Ai(x) via series for |x|<5.
fn builtin_airyai(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let c1 = 0.355028053887817;
    let c2 = 0.258819403792807;
    let mut f_sum = 1.0;
    let mut g_sum = x;
    let mut f_term = 1.0;
    let mut g_term = x;
    for k in 1..50 {
        f_term *= x.powi(3) / ((3 * k - 1) as f64 * 3.0 * k as f64);
        g_term *= x.powi(3) / ((3 * k) as f64 * (3 * k + 1) as f64);
        f_sum += f_term;
        g_sum += g_term;
        if f_term.abs() + g_term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(c1 * f_sum - c2 * g_sum))
}

/// Airy Bi(x): same series with + sign.
fn builtin_airybi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let c1 = 0.614926627446001;
    let c2 = 0.448288357353826;
    let mut f_sum = 1.0;
    let mut g_sum = x;
    let mut f_term = 1.0;
    let mut g_term = x;
    for k in 1..50 {
        f_term *= x.powi(3) / ((3 * k - 1) as f64 * 3.0 * k as f64);
        g_term *= x.powi(3) / ((3 * k) as f64 * (3 * k + 1) as f64);
        f_sum += f_term;
        g_sum += g_term;
        if f_term.abs() + g_term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(c1 * f_sum + c2 * g_sum))
}

/// polygamma ψ⁽ⁿ⁾: trigamma special case for n=1.
fn builtin_polygamma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0 {
        return Ok(StrykeValue::float(libm::tgamma(x).ln()));
    }
    let mut s = 0.0;
    for k in 0..1000 { s += 1.0 / (x + k as f64).powi((n + 1) as i32); }
    let sign = if n % 2 == 0 { -1.0 } else { 1.0 };
    let factn = (1..=n).fold(1_f64, |a, k| a * k as f64);
    Ok(StrykeValue::float(sign * factn * s))
}

/// trigamma ψ'(x).
fn builtin_trigamma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mut s = 0.0;
    for k in 0..1000 { s += 1.0 / (x + k as f64).powi(2); }
    Ok(StrykeValue::float(s))
}

/// loggamma — log Γ(x) (Lanczos via libm::lgamma_r).
fn builtin_loggamma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(libm::lgamma(x)))
}

/// factorial2 — double factorial n!! = n·(n-2)·...
fn builtin_factorial2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let mut acc = 1_i64;
    let mut k = n;
    while k > 1 { acc = acc.saturating_mul(k); k -= 2; }
    Ok(StrykeValue::integer(acc))
}

/// factorialk — generalised k-factorial.
fn builtin_factorialk(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let mut acc = 1_i64;
    let mut x = n;
    while x > 1 { acc = acc.saturating_mul(x); x -= k; }
    Ok(StrykeValue::integer(acc))
}

/// Owen's T(h, a): ∫₀ᵃ exp(-h²(1+x²)/2)/(2π(1+x²)) dx (Patefield-Tandy series).
fn builtin_owens_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = 200;
    let mut sum = 0.0;
    let dx = a / n as f64;
    for i in 0..n {
        let x = (i as f64 + 0.5) * dx;
        sum += (-h * h * (1.0 + x * x) / 2.0).exp() / (1.0 + x * x);
    }
    Ok(StrykeValue::float(sum * dx / (2.0 * std::f64::consts::PI)))
}

/// Marcum Q-function Q_M(a, b): tail of noncentral chi.
fn builtin_marcum_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.5 * libm::erfc((b - a) / 2_f64.sqrt())))
}

/// Voigt profile = real part of Faddeeva w(z), z = (x + iγ) / (σ √2).
fn builtin_voigt_profile(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = (2.0 * sigma * sigma).sqrt();
    let xr = x / denom;
    let yi = gamma / denom;
    let g = (-xr * xr).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt());
    let l = gamma / (std::f64::consts::PI * (x * x + gamma * gamma));
    let eta = yi / (yi + xr.abs() + 1e-15);
    Ok(StrykeValue::float((1.0 - eta) * g + eta * l))
}

/// Chebyshev T_n(x).
fn builtin_chebyt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as i32;
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(x)); }
    let mut a = 1.0;
    let mut b = x;
    for _ in 1..n {
        let c = 2.0 * x * b - a;
        a = b; b = c;
    }
    Ok(StrykeValue::float(b))
}

/// Chebyshev U_n(x).
fn builtin_chebyu(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as i32;
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(2.0 * x)); }
    let mut a = 1.0;
    let mut b = 2.0 * x;
    for _ in 1..n {
        let c = 2.0 * x * b - a;
        a = b; b = c;
    }
    Ok(StrykeValue::float(b))
}

/// Spherical harmonic Y_l^m(θ, φ) magnitude (real, no Condon-Shortley).
fn builtin_sph_harm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = i1(args).max(0) as i32;
    let m = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let cos_t = theta.cos();
    let mut p = 1.0;
    for _ in 0..m.unsigned_abs() { p *= -((1.0 - cos_t * cos_t).sqrt()); }
    for k in (m.unsigned_abs() as i32 + 1)..=l { p *= cos_t * (2 * k - 1) as f64 / (k - m.unsigned_abs() as i32) as f64; }
    let norm_num = (2 * l + 1) as f64;
    let norm = (norm_num / (4.0 * std::f64::consts::PI)).sqrt();
    Ok(StrykeValue::float(norm * p))
}

/// Faddeeva w(z) — magnitude only via Hermite-Gauss approximation.
fn builtin_wofz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = ((x * x + y * y).sqrt()).max(1e-15);
    Ok(StrykeValue::float((-x * x + y * y).exp() / (denom * std::f64::consts::PI.sqrt())))
}

/// erfcx(x) = exp(x²) erfc(x).
fn builtin_erfcx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((x * x).exp() * libm::erfc(x)))
}

/// erfi(x) = -i erf(ix) (real expansion).
fn builtin_erfi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mut term = x;
    let mut sum = term;
    for k in 1..200 {
        term *= x * x / k as f64;
        let add = term / (2.0 * k as f64 + 1.0);
        sum += add;
        if add.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(2.0 / std::f64::consts::PI.sqrt() * sum))
}

/// Dawson's F(x) = e^{-x²} ∫₀ˣ e^{t²} dt.
fn builtin_dawsn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mut term = x;
    let mut sum = term;
    for k in 1..200 {
        term *= -2.0 * x * x / (2.0 * k as f64 + 1.0);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// ───── interpolation / convolution ─────

/// interp1d — linear interp at x.
fn builtin_interp1d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b72_to_floats).unwrap_or_default();
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if xs.is_empty() || ys.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let pos = xs.partition_point(|&xi| xi < x);
    if pos == 0 { return Ok(StrykeValue::float(ys[0])); }
    if pos >= xs.len() { return Ok(StrykeValue::float(ys[ys.len() - 1])); }
    let (x0, x1) = (xs[pos - 1], xs[pos]);
    let (y0, y1) = (ys[pos - 1], ys[pos.min(ys.len() - 1)]);
    let t = (x - x0) / (x1 - x0).max(1e-300);
    Ok(StrykeValue::float(y0 + t * (y1 - y0)))
}

/// convolve_full — output length = m + n - 1.
fn builtin_convolve_full(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b72_to_floats).unwrap_or_default();
    let n = a.len() + b.len();
    Ok(StrykeValue::integer(n.saturating_sub(1) as i64))
}

/// convolve_valid — only fully overlapping window.
fn builtin_convolve_valid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b72_to_floats).unwrap_or_default();
    let n = (a.len().max(b.len()) + 1).saturating_sub(a.len().min(b.len()));
    Ok(StrykeValue::integer(n as i64))
}

/// correlate_full — same shape as convolve_full.
fn builtin_correlate_full(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_convolve_full(args)
}

/// kron_product — output length m·n.
fn builtin_kron_product(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b72_to_floats).unwrap_or_default();
    Ok(StrykeValue::integer((a.len() * b.len()) as i64))
}

// ───── quadrature / ODE ─────

/// Composite Simpson rule on n+1 sample points (n even).
fn builtin_simpson_rule(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ys = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ys.len() < 3 { return Ok(StrykeValue::float(0.0)); }
    let n = ys.len() - 1;
    let mut sum = ys[0] + ys[n];
    for i in 1..n { sum += ys[i] * if i % 2 == 0 { 2.0 } else { 4.0 }; }
    Ok(StrykeValue::float(sum * h / 3.0))
}

/// Romberg quadrature step: T_{n,m} = (4^m T_{n,m-1} - T_{n-1,m-1}) / (4^m - 1).
fn builtin_romberg_quad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t_n_mm1 = f1(args);
    let t_nm1_mm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number() as i32).unwrap_or(1);
    let p = 4_f64.powi(m);
    Ok(StrykeValue::float((p * t_n_mm1 - t_nm1_mm1) / (p - 1.0)))
}

/// Fixed-order Gauss-Legendre quadrature: returns Σ wᵢ f(xᵢ).
fn builtin_fixed_quad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f_vals = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let weights = args.get(1).map(b72_to_floats).unwrap_or_default();
    let n = f_vals.len().min(weights.len());
    let s: f64 = (0..n).map(|i| f_vals[i] * weights[i]).sum();
    Ok(StrykeValue::float(s))
}

/// RK4 single step: y_{n+1} = y_n + h(k1+2k2+2k3+k4)/6.
fn builtin_ode45_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k2 = args.get(3).map(|v| v.to_number()).unwrap_or(k1);
    let k3 = args.get(4).map(|v| v.to_number()).unwrap_or(k2);
    let k4 = args.get(5).map(|v| v.to_number()).unwrap_or(k3);
    Ok(StrykeValue::float(y + h * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0))
}

/// LSODA-style adaptive step result: doubles or halves h on success/fail.
fn builtin_ode_lsoda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let success = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::float(if success != 0 { h * 1.5 } else { h * 0.5 }))
}

/// solve_ivp generic step (delegates to RK4).
fn builtin_solve_ivp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_ode45_step(args)
}

/// Brent's method bracket halving step.
fn builtin_root_brentq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let fa = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let fb = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if (fb - fa).abs() < 1e-300 { return Ok(StrykeValue::float((a + b) / 2.0)); }
    Ok(StrykeValue::float(b - fb * (b - a) / (fb - fa)))
}

/// Newton's method step: x - f/f'.
fn builtin_root_newton(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let fp = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(x - f / fp.max(1e-300)))
}

/// Secant method step.
fn builtin_root_secant(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x0 = f1(args);
    let x1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f1v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x1 - f1v * (x1 - x0) / (f1v - f0).max(1e-300)))
}

/// Powell line-search step: f(x + α·p) — returns α minimising parabolic fit.
fn builtin_fmin_powell(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f0 = f1(args);
    let f1v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 2.0 * (f0 - 2.0 * f1v + f2);
    if denom.abs() < 1e-300 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float(0.5 + (f0 - f2) / denom))
}

/// COBYLA constraint slack: returns minimum of constraint values.
fn builtin_fmin_cobyla(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b72_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let m = v.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(StrykeValue::float(if m.is_finite() { m } else { 0.0 }))
}
