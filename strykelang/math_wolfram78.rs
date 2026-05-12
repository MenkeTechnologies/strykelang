// Batch 78 — Statsmodels: time series (ARIMA, GARCH, Kalman, state-space),
// stationarity tests, autocovariance, smoothing.

fn b78_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// `arima_fit` — AR(p) coefficient via Yule-Walker (return ρ_1 from autocov).
fn builtin_arima_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len();
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n as f64;
    let mut c0 = 0.0;
    let mut c1 = 0.0;
    for i in 0..n {
        let d = v[i] - mean;
        c0 += d * d;
        if i + 1 < n { c1 += d * (v[i + 1] - mean); }
    }
    Ok(StrykeValue::float(if c0 > 0.0 { c1 / c0 } else { 0.0 }))
}

/// `arima_forecast` — one-step forecast: ŷ = c + φ y_{t-1} + θ ε_{t-1}.
fn builtin_arima_forecast(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let phi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_prev = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let eps_prev = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(c + phi * y_prev + theta * eps_prev))
}

/// `arma_order_select` — Akaike Information Criterion = −2 ℓ + 2 k.
fn builtin_arma_order_select(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let log_lik = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(-2.0 * log_lik + 2.0 * k))
}

/// `sarimax_fit` — seasonal ARMA log-likelihood term.
fn builtin_sarimax_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let resid = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(-0.5 * (sigma2.ln() + resid * resid / sigma2)))
}

/// `garch_fit` — GARCH(1,1) variance update: σ²_t = ω + α ε² + β σ²_{t-1}.
fn builtin_garch_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let omega = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let eps_sq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.85);
    let sigma_sq_prev = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(omega + alpha * eps_sq + beta * sigma_sq_prev))
}

/// `ewma_smooth` — exponentially-weighted moving average y_t = α x + (1-α) y_{t-1}.
fn builtin_ewma_smooth(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y_prev = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.3).clamp(0.0, 1.0);
    Ok(StrykeValue::float(alpha * x + (1.0 - alpha) * y_prev))
}

/// `holt_winters_additive` — Holt-Winters: l_t = α(x − s) + (1-α)(l_{t-1}+b_{t-1}).
fn builtin_holt_winters_additive(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let s_prev = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l_prev = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b_prev = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(0.3).clamp(0.0, 1.0);
    Ok(StrykeValue::float(alpha * (x - s_prev) + (1.0 - alpha) * (l_prev + b_prev)))
}

/// `holt_winters_multiplicative` — multiplicative seasonality variant.
fn builtin_holt_winters_multiplicative(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let s_prev = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let l_prev = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b_prev = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(0.3).clamp(0.0, 1.0);
    Ok(StrykeValue::float(alpha * (x / s_prev) + (1.0 - alpha) * (l_prev + b_prev)))
}

/// `kalman_filter_step` — predict + update: x = x' + K (z − H x'); K = P' H' / (H P' H' + R).
fn builtin_kalman_filter_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_pred = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p_pred = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let k = p_pred / (p_pred + r);
    Ok(StrykeValue::float(x_pred + k * (z - x_pred)))
}

/// `kalman_smoother_step` — RTS smoother gain: A_k = P_k F' / P_{k+1|k}.
fn builtin_kalman_smoother_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_k = f1(args);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let p_k1_pred = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(p_k * f / p_k1_pred))
}

/// `var_fit` — VAR(1) coefficient via OLS on lagged regressor.
fn builtin_var_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = xs.len();
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 1..n {
        num += xs[i] * xs[i - 1];
        den += xs[i - 1] * xs[i - 1];
    }
    Ok(StrykeValue::float(if den > 0.0 { num / den } else { 0.0 }))
}

/// `vecm_fit` — error-correction term: λ (y − β x).
fn builtin_vecm_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(StrykeValue::float(lambda * (y - beta * x)))
}

/// `johansen_test` — trace statistic = −T Σ log(1 − λ_i).
fn builtin_johansen_test(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eigenvalues = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let s: f64 = eigenvalues.iter().filter(|&&l| l < 1.0).map(|l| (1.0 - l).ln()).sum();
    Ok(StrykeValue::float(-t * s))
}

/// `phillips_perron` — PP unit-root test: variance-correction multiplier.
fn builtin_phillips_perron(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s_e_sq = f1(args);
    let s_l_sq = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((s_e_sq / s_l_sq).sqrt() - 0.5 * (s_l_sq - s_e_sq) * n.sqrt() / s_l_sq))
}

/// `adfuller` — Augmented Dickey-Fuller test statistic = (φ̂ − 1) / SE(φ̂).
fn builtin_adfuller(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let phi_hat = f1(args);
    let se = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float((phi_hat - 1.0) / se))
}

/// `kpss_test` — KPSS LM statistic on partial-sum process.
fn builtin_kpss_test(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cum_sq_sum = f1(args);
    let s_lr_sq = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(cum_sq_sum / (n * n * s_lr_sq)))
}

/// `breusch_godfrey` — LM test for serial correlation: T R².
fn builtin_breusch_godfrey(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_sq = f1(args).clamp(0.0, 1.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(t * r_sq))
}

/// `ljung_box_q` — Ljung-Box Q = T(T+2) Σ ρ²/k for k=1..h.
fn builtin_ljung_box_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rho_sq = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let s: f64 = rho_sq.iter().enumerate()
        .map(|(i, r)| r / (i as f64 + 1.0)).sum();
    Ok(StrykeValue::float(t * (t + 2.0) * s))
}

/// `durbin_watson_d` — DW = Σ (e_t − e_{t-1})² / Σ e_t².
fn builtin_durbin_watson_d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let e = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if e.len() < 2 { return Ok(StrykeValue::float(0.0)); }
    let num: f64 = e.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
    let den: f64 = e.iter().map(|x| x * x).sum::<f64>().max(1e-300);
    Ok(StrykeValue::float(num / den))
}

/// `granger_causality` — F-statistic for restricted vs unrestricted models.
fn builtin_granger_causality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rss_r = f1(args);
    let rss_u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let n_k = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(((rss_r - rss_u) / q) / (rss_u / n_k)))
}

/// `cointegration_eg` — Engle-Granger 2-step: first regress y on x to get
/// residual u, then ADF test on u. Args: cov(x,y), var(x), residuals,
/// se(φ̂_resid). Returns EG test statistic.
fn builtin_cointegration_eg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cov_xy = f1(args);
    let var_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let phi_resid = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let se_resid = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let beta_hat = cov_xy / var_x;
    let _ = beta_hat;
    Ok(StrykeValue::float((phi_resid - 1.0) / se_resid))
}

/// `seasonal_decompose` — additive decomposition residual: x − trend − seasonal.
fn builtin_seasonal_decompose(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let trend = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let seasonal = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x - trend - seasonal))
}

/// `stl_decompose` — STL trend update via LOESS-style weighted local fit.
fn builtin_stl_decompose(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let weights = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let values = args.get(1).map(b78_to_floats).unwrap_or_default();
    let n = weights.len().min(values.len());
    let w_sum: f64 = weights.iter().take(n).sum();
    if w_sum <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| weights[i] * values[i]).sum();
    Ok(StrykeValue::float(s / w_sum))
}

/// `acf_basis` — sample autocorrelation at lag k.
fn builtin_acf_basis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = v.len();
    if n <= k { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n as f64;
    let mut c0 = 0.0;
    let mut ck = 0.0;
    for i in 0..n {
        c0 += (v[i] - mean).powi(2);
        if i + k < n { ck += (v[i] - mean) * (v[i + k] - mean); }
    }
    Ok(StrykeValue::float(if c0 > 0.0 { ck / c0 } else { 0.0 }))
}

/// `pacf_basis` — partial autocorrelation via Durbin-Levinson recursion.
fn builtin_pacf_basis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let acf = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    if k == 0 || k >= acf.len() { return Ok(StrykeValue::float(0.0)); }
    let mut phi = vec![vec![0.0; k + 1]; k + 1];
    phi[1][1] = acf.get(1).copied().unwrap_or(0.0);
    let mut sigma = vec![0.0; k + 1];
    sigma[1] = (1.0 - phi[1][1].powi(2)).max(1e-15);
    for i in 2..=k {
        let mut num = acf.get(i).copied().unwrap_or(0.0);
        for j in 1..i {
            num -= phi[i - 1][j] * acf.get(i - j).copied().unwrap_or(0.0);
        }
        phi[i][i] = num / sigma[i - 1].max(1e-15);
        for j in 1..i {
            phi[i][j] = phi[i - 1][j] - phi[i][i] * phi[i - 1][i - j];
        }
        sigma[i] = (sigma[i - 1] * (1.0 - phi[i][i].powi(2))).max(1e-15);
    }
    Ok(StrykeValue::float(phi[k][k]))
}

/// `moving_average_filter` — simple moving average value.
fn builtin_moving_average_filter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// `exp_smooth_simple` — fit α via SSE-min closed form: α* = Σ(x_t - y_{t-1})·
/// (x_t - x_{t-1}) / Σ(x_t - x_{t-1})². Args: residual_dot, residual_var.
fn builtin_exp_smooth_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let resid_dot = f1(args);
    let resid_var = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float((resid_dot / resid_var).clamp(0.0, 1.0)))
}

/// `exp_smooth_double` — Holt's two-parameter update: full state update of
/// (l_t, b_t) from x_t, returns trend-adjusted forecast l_t + h·b_t for
/// horizon h.
fn builtin_exp_smooth_double(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let l_prev = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b_prev = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.3).clamp(0.0, 1.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.1).clamp(0.0, 1.0);
    let h = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let l_new = alpha * x + (1.0 - alpha) * (l_prev + b_prev);
    let b_new = beta * (l_new - l_prev) + (1.0 - beta) * b_prev;
    Ok(StrykeValue::float(l_new + h * b_new))
}

/// `markov_switching_ar` — regime-switching transition step.
fn builtin_markov_switching_ar(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_stay = f1(args).clamp(0.0, 1.0);
    let p_switch = 1.0 - p_stay;
    let prior = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    Ok(StrykeValue::float(p_stay * prior + p_switch * (1.0 - prior)))
}

/// `markov_switching_mr` — mean-reverting variant: in regime k, x_{t+1} =
/// μ_k + κ_k (μ_k − x_t) + ε. Args: x_t, μ_k, κ_k. Returns one-step forecast.
fn builtin_markov_switching_mr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu_k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kappa_k = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(StrykeValue::float(mu_k + kappa_k * (mu_k - x)))
}

/// `arch_lm_test` — Engle's ARCH-LM: regress squared residuals on q lagged
/// squared residuals; LM = T·R² ~ χ²(q). Args: T, R² of aux regression.
fn builtin_arch_lm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let r_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 1.0);
    Ok(StrykeValue::float(t * r_sq))
}

/// `state_space_kalman` — log-likelihood contribution at step t.
fn builtin_state_space_kalman(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let resid = f1(args);
    let f_var = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(-0.5 * (f_var.ln() + resid * resid / f_var)))
}

/// `ucm_unobserved_components` — local-level + slope state update.
fn builtin_ucm_unobserved_components(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level = f1(args);
    let slope = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(level + slope))
}

/// `spectral_density_estimate` — Bartlett spectral density at frequency ω.
fn builtin_spectral_density_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let acf = b78_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s: f64 = acf.iter().enumerate().map(|(k, &r)| r * (omega * k as f64).cos()).sum();
    Ok(StrykeValue::float(s / std::f64::consts::PI))
}

/// `bayesian_step` — Bayesian update: posterior ∝ likelihood · prior.
fn builtin_bayesian_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prior = f1(args);
    let likelihood = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let evidence = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(prior * likelihood / evidence))
}

/// `pivoted_cholesky_var` — variance of OLS estimate via diag of (X'X)⁻¹.
fn builtin_pivoted_cholesky_var(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pivot = f1(args);
    let sigma_sq = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(sigma_sq / pivot.max(1e-15)))
}
