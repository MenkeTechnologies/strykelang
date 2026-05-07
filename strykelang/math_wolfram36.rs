// Batch 36 — econometrics: regression diagnostics, time-series tests, panel data, MLE.

// Helpers local to this batch
fn b36_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}
fn b36_mean(xs: &[f64]) -> f64 {
    if xs.is_empty() { 0.0 } else { xs.iter().sum::<f64>() / xs.len() as f64 }
}
fn b36_var(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 { return 0.0; }
    let m = b36_mean(xs);
    xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64
}
fn b36_cov(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len().min(ys.len());
    if n < 2 { return 0.0; }
    let mx = b36_mean(&xs[..n]);
    let my = b36_mean(&ys[..n]);
    xs.iter().zip(ys.iter()).take(n).map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / (n - 1) as f64
}

// ARCH LM test statistic for residuals (squared resid AR(1) R² × n)
fn builtin_arch_lm_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(b36_to_floats).unwrap_or_default();
    let n = r.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let r2: Vec<f64> = r.iter().map(|x| x * x).collect();
    let y: Vec<f64> = r2[1..].to_vec();
    let x: Vec<f64> = r2[..n - 1].to_vec();
    let cv = b36_cov(&x, &y);
    let vx = b36_var(&x);
    let vy = b36_var(&y);
    if vx <= 0.0 || vy <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let r_sq = (cv * cv) / (vx * vy);
    Ok(PerlValue::float(r_sq * (n - 1) as f64))
}

// Breusch-Pagan test (resid² regressed on x; LM = n·R²)
fn builtin_breusch_pagan_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = r.len().min(x.len());
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let r2: Vec<f64> = r.iter().take(n).map(|v| v * v).collect();
    let cv = b36_cov(&x[..n], &r2);
    let vx = b36_var(&x[..n]);
    let vr = b36_var(&r2);
    if vx <= 0.0 || vr <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let r_sq = (cv * cv) / (vx * vr);
    Ok(PerlValue::float(n as f64 * r_sq))
}

// White heteroskedasticity-robust SE: σ²_HC0 = (X'X)⁻¹ X' diag(e²) X (X'X)⁻¹ — scalar 1-D form
fn builtin_white_robust_se(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(b36_to_floats).unwrap_or_default();
    let e = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = x.len().min(e.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let xx: f64 = x.iter().take(n).map(|v| v * v).sum();
    if xx == 0.0 { return Ok(PerlValue::float(0.0)); }
    let mid: f64 = x.iter().zip(e.iter()).take(n).map(|(xi, ei)| xi * xi * ei * ei).sum();
    Ok(PerlValue::float((mid / (xx * xx)).sqrt()))
}

// Newey-West HAC SE with given lag truncation L
fn builtin_newey_west_se(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(b36_to_floats).unwrap_or_default();
    let e = args.get(1).map(b36_to_floats).unwrap_or_default();
    let lag = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = x.len().min(e.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let xx: f64 = x.iter().take(n).map(|v| v * v).sum();
    if xx == 0.0 { return Ok(PerlValue::float(0.0)); }
    let g: Vec<f64> = (0..n).map(|i| x[i] * e[i]).collect();
    let mut s: f64 = g.iter().map(|v| v * v).sum();
    for l in 1..=lag {
        let w = 1.0 - l as f64 / (lag as f64 + 1.0);
        let mut acc = 0.0;
        for t in l..n { acc += g[t] * g[t - l]; }
        s += 2.0 * w * acc;
    }
    Ok(PerlValue::float((s / (xx * xx)).sqrt()))
}

// Hansen J statistic = N · g'·W·g (already-formed moment vector and weight)
fn builtin_hansen_j_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = args.first().map(b36_to_floats).unwrap_or_default();
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(g.len() as f64);
    let q: f64 = g.iter().map(|v| v * v).sum();
    Ok(PerlValue::float(n * w * q))
}

// GMM moment condition: E[Z'(y - Xβ)] sample mean
fn builtin_gmm_moment_condition(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = args.first().map(b36_to_floats).unwrap_or_default();
    let y = args.get(1).map(b36_to_floats).unwrap_or_default();
    let x = args.get(2).map(b36_to_floats).unwrap_or_default();
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let n = z.len().min(y.len()).min(x.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| z[i] * (y[i] - x[i] * beta)).sum();
    Ok(PerlValue::float(s / n as f64))
}

// Hausman test statistic |β_FE - β_RE|² / (Var_FE - Var_RE)
fn builtin_hausman_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b_fe = f1(args);
    let b_re = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_fe = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let v_re = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let dv = v_fe - v_re;
    if dv.abs() < 1e-12 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((b_fe - b_re).powi(2) / dv))
}

// Breusch-Godfrey LM test for serial correlation: n·R²(e on lagged e)
fn builtin_breusch_godfrey_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e = args.first().map(b36_to_floats).unwrap_or_default();
    let n = e.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let y: Vec<f64> = e[1..].to_vec();
    let x: Vec<f64> = e[..n - 1].to_vec();
    let cv = b36_cov(&x, &y);
    let vx = b36_var(&x);
    let vy = b36_var(&y);
    if vx <= 0.0 || vy <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let r_sq = (cv * cv) / (vx * vy);
    Ok(PerlValue::float(n as f64 * r_sq))
}

// Box-Pierce test Q = n·Σρₖ²
fn builtin_box_pierce_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let acf = args.first().map(b36_to_floats).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let q: f64 = acf.iter().map(|r| r * r).sum();
    Ok(PerlValue::float(n * q))
}

// Augmented Dickey-Fuller test statistic (γ̂ / SE(γ̂)) on Δyₜ = γyₜ₋₁ + ε
fn builtin_adf_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let n = y.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let dy: Vec<f64> = (1..n).map(|i| y[i] - y[i - 1]).collect();
    let yl: Vec<f64> = y[..n - 1].to_vec();
    let cv = b36_cov(&yl, &dy);
    let vyl = b36_var(&yl);
    if vyl <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let gamma = cv / vyl;
    let resid: Vec<f64> = (0..dy.len()).map(|i| dy[i] - gamma * yl[i]).collect();
    let sigma2 = resid.iter().map(|x| x * x).sum::<f64>() / (resid.len() - 1).max(1) as f64;
    let se = (sigma2 / (vyl * (n - 1) as f64)).sqrt();
    if se == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(gamma / se))
}

// Phillips-Perron test stat (ADF stat with Newey-West correction approx)
fn builtin_pp_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let lag = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = y.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let adf = builtin_adf_test_stat(&[PerlValue::array(y.iter().copied().map(PerlValue::float).collect())])?.to_number();
    let dy: Vec<f64> = (1..n).map(|i| y[i] - y[i - 1]).collect();
    let mut s2 = b36_var(&dy);
    for l in 1..=lag {
        let w = 1.0 - l as f64 / (lag as f64 + 1.0);
        let mut acc = 0.0;
        for t in l..dy.len() { acc += dy[t] * dy[t - l]; }
        s2 += 2.0 * w * acc / dy.len() as f64;
    }
    let v = b36_var(&dy);
    if v <= 0.0 { return Ok(PerlValue::float(adf)); }
    Ok(PerlValue::float(adf * (v / s2).sqrt()))
}

// KPSS test statistic: η = (1/n²)·Σ Sₜ² / σ̂²
fn builtin_kpss_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let n = y.len();
    if n < 2 { return Ok(PerlValue::float(0.0)); }
    let m = b36_mean(&y);
    let e: Vec<f64> = y.iter().map(|v| v - m).collect();
    let mut s = vec![0.0; n];
    let mut acc = 0.0;
    for i in 0..n { acc += e[i]; s[i] = acc; }
    let num: f64 = s.iter().map(|v| v * v).sum::<f64>() / (n * n) as f64;
    let denom: f64 = e.iter().map(|v| v * v).sum::<f64>() / n as f64;
    if denom <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / denom))
}

// Dickey-Fuller critical value at 5% level for sample size n
fn builtin_dickey_fuller_critical(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    Ok(PerlValue::float(-2.86 - 2.738 / n - 8.36 / (n * n)))
}

// Engle-Granger cointegration step: ADF on residuals of y ~ x
fn builtin_engle_granger_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = y.len().min(x.len());
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let cv = b36_cov(&x[..n], &y[..n]);
    let vx = b36_var(&x[..n]);
    if vx <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let beta = cv / vx;
    let alpha = b36_mean(&y[..n]) - beta * b36_mean(&x[..n]);
    let resid: Vec<f64> = (0..n).map(|i| y[i] - alpha - beta * x[i]).collect();
    builtin_adf_test_stat(&[PerlValue::array(resid.into_iter().map(PerlValue::float).collect())])
}

// Johansen trace step: λ_max from canonical correlation between Δy and y_lag
fn builtin_johansen_trace_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let n = y.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let dy: Vec<f64> = (1..n).map(|i| y[i] - y[i - 1]).collect();
    let yl: Vec<f64> = y[..n - 1].to_vec();
    let cv = b36_cov(&yl, &dy);
    let vy = b36_var(&yl);
    let vd = b36_var(&dy);
    if vy <= 0.0 || vd <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let r2 = cv * cv / (vy * vd);
    Ok(PerlValue::float(-(n as f64) * (1.0 - r2).max(1e-12).ln()))
}

// VECM α·β decomposition: π = αβ' approximation as scalar product
fn builtin_vecm_alpha_beta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pi = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if alpha == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(pi / alpha))
}

// Panel within-group estimator: demeaned OLS slope
fn builtin_panel_within_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = y.len().min(x.len());
    if n < 2 { return Ok(PerlValue::float(0.0)); }
    let my = b36_mean(&y[..n]);
    let mx = b36_mean(&x[..n]);
    let yd: Vec<f64> = (0..n).map(|i| y[i] - my).collect();
    let xd: Vec<f64> = (0..n).map(|i| x[i] - mx).collect();
    let num: f64 = (0..n).map(|i| xd[i] * yd[i]).sum();
    let den: f64 = xd.iter().map(|v| v * v).sum();
    if den == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// Panel between-group estimator: group-mean OLS
fn builtin_panel_between_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let cv = b36_cov(&x, &y);
    let vx = b36_var(&x);
    if vx == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(cv / vx))
}

// Panel random-effects θ-transform coefficient θ = 1 - σ_ε/√(σ_ε² + Tσ_u²)
fn builtin_panel_random_effects(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s_eps = f1(args);
    let s_u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let denom = (s_eps * s_eps + t * s_u * s_u).sqrt();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - s_eps / denom))
}

// Arellano-Bond GMM step: first-difference IV estimate
fn builtin_arellano_bond_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let n = y.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let dy: Vec<f64> = (1..n).map(|i| y[i] - y[i - 1]).collect();
    let z: Vec<f64> = y[..n - 2].to_vec();
    let dyl: Vec<f64> = dy[..dy.len() - 1].to_vec();
    let dyc: Vec<f64> = dy[1..].to_vec();
    let num: f64 = (0..z.len()).map(|i| z[i] * dyc[i]).sum();
    let den: f64 = (0..z.len()).map(|i| z[i] * dyl[i]).sum();
    if den == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// OLS estimator β̂ = Σxy / Σx²
fn builtin_ols_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(b36_to_floats).unwrap_or_default();
    let y = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = x.len().min(y.len());
    let num: f64 = (0..n).map(|i| x[i] * y[i]).sum();
    let den: f64 = x.iter().take(n).map(|v| v * v).sum();
    if den == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// OLS residual variance σ̂² = SSE / (n - k)
fn builtin_ols_residual_variance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(b36_to_floats).unwrap_or_default();
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = r.len();
    if n <= k { return Ok(PerlValue::float(0.0)); }
    let sse: f64 = r.iter().map(|v| v * v).sum();
    Ok(PerlValue::float(sse / (n - k) as f64))
}

// OLS R² = 1 - SSR/SST
fn builtin_ols_r_squared(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let yhat = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = y.len().min(yhat.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let m = b36_mean(&y[..n]);
    let sst: f64 = y.iter().take(n).map(|v| (v - m).powi(2)).sum();
    let ssr: f64 = (0..n).map(|i| (y[i] - yhat[i]).powi(2)).sum();
    if sst == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - ssr / sst))
}

// OLS adjusted R²
fn builtin_ols_adjusted_r2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r2 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n - k - 1.0 <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - (1.0 - r2) * (n - 1.0) / (n - k - 1.0)))
}

// Akaike Information Criterion AIC = 2k - 2ln L
fn builtin_akaike_info_crit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ll = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(2.0 * k - 2.0 * ll))
}

// Bayesian Information Criterion BIC = k·ln n - 2 ln L
fn builtin_bayesian_info_crit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ll = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    Ok(PerlValue::float(k * n.ln() - 2.0 * ll))
}

// Hannan-Quinn IC = 2k·ln(ln n) - 2 ln L
fn builtin_hannan_quinn_ic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ll = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    Ok(PerlValue::float(2.0 * k * n.ln().ln() - 2.0 * ll))
}

// F statistic for pooled regression: ((SSR_R - SSR_U)/q) / (SSR_U/(n-k))
fn builtin_f_statistic_pooled(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ssr_r = f1(args);
    let ssr_u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(4).map(|v| v.to_number()).unwrap_or(2.0);
    if ssr_u <= 0.0 || (n - k) <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(((ssr_r - ssr_u) / q) / (ssr_u / (n - k))))
}

// Breusch-Pagan LM (alternative formula via auxiliary regression)
fn builtin_breusch_pagan_lm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ess = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma2 == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(0.5 * ess / sigma2.powi(2)))
}

// Ramsey RESET test (powers of fitted values added to model — F approx)
fn builtin_ramsey_reset_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r2_u = f1(args);
    let r2_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let q = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(4).map(|v| v.to_number()).unwrap_or(2.0);
    if 1.0 - r2_u <= 0.0 || n - k <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(((r2_u - r2_r) / q) / ((1.0 - r2_u) / (n - k))))
}

// Chow test for structural break: F = ((SSR - SSR1 - SSR2)/k) / ((SSR1+SSR2)/(n-2k))
fn builtin_chow_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ssr = f1(args);
    let ssr1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ssr2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(4).map(|v| v.to_number()).unwrap_or(2.0);
    let den = (ssr1 + ssr2) / (n - 2.0 * k);
    if den <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(((ssr - ssr1 - ssr2) / k) / den))
}

// White general test: LM = n·R² of resid² on x, x²
fn builtin_white_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let n = r.len().min(x.len());
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let r2: Vec<f64> = r.iter().take(n).map(|v| v * v).collect();
    let x2: Vec<f64> = x.iter().take(n).map(|v| v * v).collect();
    let cv1 = b36_cov(&x[..n], &r2);
    let cv2 = b36_cov(&x2, &r2);
    let vr = b36_var(&r2);
    if vr <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let r_sq = (cv1 * cv1 + cv2 * cv2) / (vr * (b36_var(&x[..n]) + b36_var(&x2)).max(1e-12));
    Ok(PerlValue::float(n as f64 * r_sq))
}

// Goldfeld-Quandt test: F = SSR2/SSR1 with central observations dropped
fn builtin_goldfeld_quandt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ssr1 = f1(args);
    let ssr2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ssr1 == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(ssr2 / ssr1))
}

// Wald test statistic: (Rβ̂ - r)' [R V R']⁻¹ (Rβ̂ - r) — scalar form
fn builtin_wald_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let diff = f1(args);
    let var = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if var <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(diff * diff / var))
}

// Score (Lagrange Multiplier) test statistic
fn builtin_score_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let info = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if info == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(s * s / info))
}

// Likelihood ratio test: LR = -2(ln L_R - ln L_U)
fn builtin_likelihood_ratio_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ll_r = f1(args);
    let ll_u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(-2.0 * (ll_r - ll_u)))
}

// Two-stage least squares (2SLS / IV) coefficient
fn builtin_two_sls_iv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = args.first().map(b36_to_floats).unwrap_or_default();
    let x = args.get(1).map(b36_to_floats).unwrap_or_default();
    let y = args.get(2).map(b36_to_floats).unwrap_or_default();
    let n = z.len().min(x.len()).min(y.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let zx: f64 = (0..n).map(|i| z[i] * x[i]).sum();
    let zy: f64 = (0..n).map(|i| z[i] * y[i]).sum();
    if zx == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(zy / zx))
}

// IV estimator: β̂ = (Z'X)⁻¹ Z'y — same as 2SLS for just-identified case
fn builtin_iv_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_two_sls_iv(args)
}

// MLE log-likelihood for normal: -n/2 ln(2π σ²) - SSE/(2σ²)
fn builtin_mle_normal_log_lik(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sse = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    if sigma2 <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-0.5 * n * (2.0 * std::f64::consts::PI * sigma2).ln() - sse / (2.0 * sigma2)))
}

// MLE log-likelihood for exponential: n·ln λ - λ·Σx
fn builtin_mle_exponential_log_lik(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let sum = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if lambda <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(n * lambda.ln() - lambda * sum))
}

// MLE log-likelihood for poisson: Σ(xᵢ ln λ - λ - ln xᵢ!)
fn builtin_mle_poisson_log_lik(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let sum_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let sum_lnxfact = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if lambda <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(sum_x * lambda.ln() - n * lambda - sum_lnxfact))
}

// GMM moment function g(θ) = E[m(x, θ)] — sample mean of m
fn builtin_gmm_moment_function(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(b36_to_floats).unwrap_or_default();
    if m.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(m.iter().sum::<f64>() / m.len() as f64))
}

// Pooling test (Pesaran CD or simple): F-style (SSR_R - SSR_U)/SSR_U·df
fn builtin_pooling_test_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ssr_r = f1(args);
    let ssr_u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let df = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if ssr_u <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((ssr_r - ssr_u) / ssr_u * df))
}

// Heteroskedasticity test (general n·R²)
fn builtin_heteroskedasticity_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r2 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    Ok(PerlValue::float(n * r2))
}

// Robust (Huber-White) standard error
fn builtin_robust_se_huber_white(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_white_robust_se(args)
}

// Bootstrap SE estimate from B replicates
fn builtin_bootstrap_se_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let reps = args.first().map(b36_to_floats).unwrap_or_default();
    if reps.len() < 2 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(b36_var(&reps).sqrt()))
}

// Heckman correction (inverse Mills ratio λ = φ(z)/Φ(z))
fn builtin_heckman_correction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let phi = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let big_phi = 0.5 * (1.0 + libm::erf(z / std::f64::consts::SQRT_2));
    if big_phi == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(phi / big_phi))
}

// Tobit log-likelihood: censored regression at 0
fn builtin_tobit_log_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xb = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if sigma <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    if y > 0.0 {
        let z = (y - xb) / sigma;
        Ok(PerlValue::float(-0.5 * z * z - sigma.ln() - 0.5 * (2.0 * std::f64::consts::PI).ln()))
    } else {
        let z = -xb / sigma;
        let big_phi = 0.5 * (1.0 + libm::erf(z / std::f64::consts::SQRT_2));
        Ok(PerlValue::float(big_phi.max(1e-300).ln()))
    }
}

// Probit log-likelihood for one observation
fn builtin_probit_log_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xb = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let big_phi = 0.5 * (1.0 + libm::erf(xb / std::f64::consts::SQRT_2));
    let p = if y > 0.5 { big_phi } else { 1.0 - big_phi };
    Ok(PerlValue::float(p.max(1e-300).ln()))
}

// Logit log-likelihood for one observation
fn builtin_logit_log_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xb = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p = 1.0 / (1.0 + (-xb).exp());
    let pp = if y > 0.5 { p } else { 1.0 - p };
    Ok(PerlValue::float(pp.max(1e-300).ln()))
}

// Multinomial logit probability for class j: exp(xβⱼ) / Σ exp(xβₖ)
fn builtin_multinomial_logit_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xb = args.first().map(b36_to_floats).unwrap_or_default();
    let j = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if j >= xb.len() { return Ok(PerlValue::float(0.0)); }
    let max = xb.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = xb.iter().map(|v| (v - max).exp()).sum();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((xb[j] - max).exp() / denom))
}

// Ordered probit threshold μⱼ for category j (cumulative form)
fn builtin_ordered_probit_threshold(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xb = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = (mu - xb) / std::f64::consts::SQRT_2;
    Ok(PerlValue::float(0.5 * (1.0 + libm::erf(z))))
}

// Panel VAR step: OLS on lagged y across panels
fn builtin_panel_var_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(b36_to_floats).unwrap_or_default();
    let n = y.len();
    if n < 2 { return Ok(PerlValue::float(0.0)); }
    let yl: Vec<f64> = y[..n - 1].to_vec();
    let yc: Vec<f64> = y[1..].to_vec();
    let cv = b36_cov(&yl, &yc);
    let v = b36_var(&yl);
    if v == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(cv / v))
}

// Impulse response step: φₕ = ψ^h
fn builtin_impulse_response_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(psi.powf(h)))
}

// Variance decomposition: share of variance from shock j at horizon h
fn builtin_variance_decomposition(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi = args.first().map(b36_to_floats).unwrap_or_default();
    let total: f64 = psi.iter().map(|v| v * v).sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let j = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if j >= psi.len() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(psi[j] * psi[j] / total))
}

// Granger causality χ² statistic
fn builtin_granger_causality_chi2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ssr_r = f1(args);
    let ssr_u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    if ssr_u <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n * (ssr_r - ssr_u) / ssr_u))
}

// Cointegration residual: y - β·x
fn builtin_cointegration_residual(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(y - beta * x))
}

// Error-correction step Δyₜ = α(yₜ₋₁ - βxₜ₋₁) + ε
fn builtin_error_correction_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_lag = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x_lag = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(-0.1);
    Ok(PerlValue::float(alpha * (y_lag - beta * x_lag)))
}

// Random walk innovation: εₜ = yₜ - yₜ₋₁
fn builtin_random_walk_innovation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let y_lag = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(y - y_lag))
}

// Random walk with drift step: yₜ = yₜ₋₁ + μ + ε
fn builtin_random_walk_drift_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_lag = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(y_lag + mu + eps))
}

// AR(p) model log-likelihood (gaussian errors)
fn builtin_ar_model_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(b36_to_floats).unwrap_or_default();
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = r.len() as f64;
    if sigma2 <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    let sse: f64 = r.iter().map(|v| v * v).sum();
    Ok(PerlValue::float(-0.5 * n * (2.0 * std::f64::consts::PI * sigma2).ln() - sse / (2.0 * sigma2)))
}

// MA(q) model log-likelihood (same Gaussian form)
fn builtin_ma_model_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ar_model_likelihood(args)
}

// ARMA innovation step: εₜ = yₜ - φyₜ₋₁ - θεₜ₋₁
fn builtin_arma_model_innovation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let phi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_lag = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let eps_lag = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(y - phi * y_lag - theta * eps_lag))
}
