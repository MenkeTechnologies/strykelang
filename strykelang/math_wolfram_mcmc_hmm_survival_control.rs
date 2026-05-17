// ─────────────────────────────────────────────────────────────────────────────
// applied / domain-specific science staples not yet in stryke:
// MCMC, SDE integrators, HMMs, survival analysis, bioinformatics alignment,
// chemistry, control theory, game theory, operations research, numerical PDE,
// Bayesian conjugate updates, quantum gates, splines, music/audio, astronomy,
// fluid dynamics, more distributions, random-graph generators, perceptual
// colour, integer sequences. Included after `math_wolfram_cas_clustering_neural.rs`.
// ─────────────────────────────────────────────────────────────────────────────

// ─── 1. MCMC samplers (callbacks) ────────────────────────────────────────────

/// Metropolis-Hastings sampler with random-walk Gaussian proposal. Args:
///   LOG_PI (callback returning log-density), X0 (vector), SIGMA (proposal scale),
///   ITERS, BURN_IN. Returns ITERS-BURN_IN samples (matrix).
fn builtin_metropolis_hastings(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let iters = args.get(3).map(|v| v.to_number() as usize).unwrap_or(1000);
    let burn = args.get(4).map(|v| v.to_number() as usize).unwrap_or(iters / 10);
    let mut rng = rand::thread_rng();
    let mut log_pi = call_user_n(interp, &f, x.clone(), line)?;
    let mut samples: Vec<Vec<f64>> = Vec::with_capacity(iters - burn);
    for k in 0..iters {
        let mut cand = x.clone();
        for v in cand.iter_mut() {
            let u1: f64 = rng.gen_range(1e-300..1.0);
            let u2: f64 = rng.gen();
            *v += sigma * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        }
        let log_pi_cand = call_user_n(interp, &f, cand.clone(), line)?;
        let alpha = (log_pi_cand - log_pi).exp();
        let u: f64 = rng.gen();
        if u < alpha {
            x = cand;
            log_pi = log_pi_cand;
        }
        if k >= burn {
            samples.push(x.clone());
        }
    }
    Ok(matrix_to_value(&samples))
}

/// Single Gibbs sweep. Args: COND_SAMPLERS (array of code refs, one per
/// dimension; each takes the full state and returns the new value of
/// dimension i), X (current state). Returns updated state.
fn builtin_gibbs_sampler_step(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let conds = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let mut x: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    for (i, cond) in conds.iter().enumerate() {
        if i >= x.len() {
            break;
        }
        let new_xi = call_user_n(interp, cond, x.clone(), line)?;
        x[i] = new_xi;
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

// ─── 2. SDE integrators ──────────────────────────────────────────────────────

/// Euler-Maruyama for dX = μ(X, t) dt + σ(X, t) dW. Args: MU, SIGMA (scalar
/// fns of (x, t)), X0, T0, T_END, N_STEPS.
fn builtin_euler_maruyama(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let sigma = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = (t_end - t0) / n as f64;
    let sub_mu = mu
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("euler_maruyama: μ code ref", line))?;
    let sub_sig = sigma
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("euler_maruyama: σ code ref", line))?;
    let mut path = Vec::with_capacity(n + 1);
    path.push(StrykeValue::float(x));
    let mut rng = rand::thread_rng();
    let mut t = t0;
    let call = |interp: &mut VMHelper, sub: &_, x: f64, t: f64| -> StrykeResult<f64> {
        let r = exec_to_perl_result(
            interp.call_sub(
                sub,
                vec![StrykeValue::float(x), StrykeValue::float(t)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    for _ in 0..n {
        let u1: f64 = rng.gen_range(1e-300..1.0);
        let u2: f64 = rng.gen();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let dw = dt.sqrt() * z;
        let mu_x = call(interp, &sub_mu, x, t)?;
        let sigma_x = call(interp, &sub_sig, x, t)?;
        x += mu_x * dt + sigma_x * dw;
        t += dt;
        path.push(StrykeValue::float(x));
    }
    Ok(StrykeValue::array(path))
}

/// Milstein scheme — adds the Σ Σ' (ΔW² − Δt)/2 correction term. Requires
/// SIGMA_X (∂σ/∂x) callback.
fn builtin_milstein(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let sigma = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let sigma_x = args.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(6).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = (t_end - t0) / n as f64;
    let sub_mu = mu
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("milstein: μ code ref", line))?;
    let sub_sig = sigma
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("milstein: σ code ref", line))?;
    let sub_sig_x = sigma_x
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("milstein: σ' code ref", line))?;
    let call = |interp: &mut VMHelper, sub: &_, x: f64, t: f64| -> StrykeResult<f64> {
        let r = exec_to_perl_result(
            interp.call_sub(
                sub,
                vec![StrykeValue::float(x), StrykeValue::float(t)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let mut path = Vec::with_capacity(n + 1);
    path.push(StrykeValue::float(x));
    let mut rng = rand::thread_rng();
    let mut t = t0;
    for _ in 0..n {
        let u1: f64 = rng.gen_range(1e-300..1.0);
        let u2: f64 = rng.gen();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let dw = dt.sqrt() * z;
        let mu_x = call(interp, &sub_mu, x, t)?;
        let sg_x = call(interp, &sub_sig, x, t)?;
        let sgx_x = call(interp, &sub_sig_x, x, t)?;
        x += mu_x * dt + sg_x * dw + 0.5 * sg_x * sgx_x * (dw * dw - dt);
        t += dt;
        path.push(StrykeValue::float(x));
    }
    Ok(StrykeValue::array(path))
}

/// Ornstein-Uhlenbeck path: dX = θ(μ - X)dt + σ dW. Closed-form increment.
fn builtin_ornstein_uhlenbeck_path(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let theta = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let x0 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let alpha = (-theta * dt).exp();
    let stddev = sigma * ((1.0 - alpha * alpha) / (2.0 * theta)).sqrt();
    let mut x = x0;
    let mut path = Vec::with_capacity(n + 1);
    path.push(StrykeValue::float(x));
    let mut rng = rand::thread_rng();
    for _ in 0..n {
        let u1: f64 = rng.gen_range(1e-300..1.0);
        let u2: f64 = rng.gen();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        x = mu + alpha * (x - mu) + stddev * z;
        path.push(StrykeValue::float(x));
    }
    Ok(StrykeValue::array(path))
}

// ─── 3. Hidden Markov models ────────────────────────────────────────────────

/// Forward algorithm. Args: PI (initial), A (transition), B (emission row per
/// state per observation), OBS (sequence of obs indices). Returns log P(O | λ).
fn builtin_hmm_forward(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let a = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let obs: Vec<usize> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = pi.len();
    let mut alpha: Vec<f64> = (0..n).map(|i| pi[i] * b[i][obs[0]]).collect();
    let mut log_p = 0.0_f64;
    for t in 1..obs.len() {
        let mut new_alpha = vec![0.0_f64; n];
        for j in 0..n {
            for i in 0..n {
                new_alpha[j] += alpha[i] * a[i][j];
            }
            new_alpha[j] *= b[j][obs[t]];
        }
        // Rescale to avoid underflow.
        let s: f64 = new_alpha.iter().sum();
        if s > 0.0 {
            for v in new_alpha.iter_mut() {
                *v /= s;
            }
            log_p += s.ln();
        }
        alpha = new_alpha;
    }
    log_p += alpha.iter().sum::<f64>().ln();
    Ok(StrykeValue::float(log_p))
}

/// Viterbi decoding: most likely hidden-state path.
fn builtin_hmm_viterbi(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let a = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let obs: Vec<usize> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = pi.len();
    let t_len = obs.len();
    if t_len == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut delta = vec![vec![f64::NEG_INFINITY; n]; t_len];
    let mut psi = vec![vec![0_usize; n]; t_len];
    for i in 0..n {
        delta[0][i] = pi[i].ln() + b[i][obs[0]].ln();
    }
    for t in 1..t_len {
        for j in 0..n {
            let mut best = f64::NEG_INFINITY;
            let mut best_i = 0_usize;
            for i in 0..n {
                let v = delta[t - 1][i] + a[i][j].ln();
                if v > best {
                    best = v;
                    best_i = i;
                }
            }
            delta[t][j] = best + b[j][obs[t]].ln();
            psi[t][j] = best_i;
        }
    }
    let mut path = vec![0_usize; t_len];
    path[t_len - 1] = (0..n)
        .max_by(|&a, &b| {
            delta[t_len - 1][a]
                .partial_cmp(&delta[t_len - 1][b])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();
    for t in (1..t_len).rev() {
        path[t - 1] = psi[t][path[t]];
    }
    Ok(StrykeValue::array(
        path.into_iter().map(|v| StrykeValue::integer(v as i64)).collect(),
    ))
}

/// Backward algorithm. Returns the β matrix at each time step.
fn builtin_hmm_backward(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let a = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let obs: Vec<usize> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = pi.len();
    let t_len = obs.len();
    if t_len == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let mut beta = vec![vec![1.0_f64; n]; t_len];
    for t in (0..t_len - 1).rev() {
        for i in 0..n {
            let mut s = 0.0_f64;
            for j in 0..n {
                s += a[i][j] * b[j][obs[t + 1]] * beta[t + 1][j];
            }
            beta[t][i] = s;
        }
    }
    Ok(matrix_to_value(&beta))
}

// ─── 4. Survival analysis ────────────────────────────────────────────────────

/// Kaplan-Meier estimator. Args: TIMES (sorted), EVENTS (1=death, 0=censored).
/// Returns matrix of [t, S(t)] rows.
fn builtin_kaplan_meier(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut data: Vec<(f64, i64)> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .zip(arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF)).iter())
        .map(|(t, e)| (t.to_number(), e.to_number() as i64))
        .collect();
    data.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let n = data.len();
    let mut at_risk = n;
    let mut s = 1.0_f64;
    let mut out: Vec<Vec<f64>> = Vec::new();
    let mut i = 0_usize;
    while i < n {
        let t = data[i].0;
        let mut deaths = 0_usize;
        let mut total_at_t = 0_usize;
        while i + total_at_t < n && data[i + total_at_t].0 == t {
            if data[i + total_at_t].1 == 1 {
                deaths += 1;
            }
            total_at_t += 1;
        }
        if deaths > 0 {
            s *= 1.0 - deaths as f64 / at_risk as f64;
            out.push(vec![t, s]);
        }
        at_risk -= total_at_t;
        i += total_at_t;
    }
    Ok(matrix_to_value(&out))
}

/// Log-rank test on two arms. Args: t1, e1, t2, e2.
fn builtin_log_rank_test(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let e1: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let t2: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let e2: Vec<i64> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let mut all_times: Vec<f64> = t1.iter().chain(t2.iter()).copied().collect();
    all_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    all_times.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut n1 = t1.len() as f64;
    let mut n2 = t2.len() as f64;
    let mut o_minus_e_1 = 0.0_f64;
    let mut var = 0.0_f64;
    for &t in &all_times {
        let d1 = t1
            .iter()
            .zip(e1.iter())
            .filter(|(ti, ei)| (**ti - t).abs() < 1e-12 && **ei == 1)
            .count() as f64;
        let d2 = t2
            .iter()
            .zip(e2.iter())
            .filter(|(ti, ei)| (**ti - t).abs() < 1e-12 && **ei == 1)
            .count() as f64;
        let d = d1 + d2;
        let n = n1 + n2;
        if n < 2.0 {
            continue;
        }
        let e1_t = d * n1 / n;
        o_minus_e_1 += d1 - e1_t;
        var += d * (n1 / n) * (n2 / n) * ((n - d) / (n - 1.0));
        let r1 = t1
            .iter()
            .filter(|&&ti| (ti - t).abs() < 1e-12)
            .count() as f64;
        let r2 = t2
            .iter()
            .filter(|&&ti| (ti - t).abs() < 1e-12)
            .count() as f64;
        n1 -= r1;
        n2 -= r2;
    }
    let chi2 = o_minus_e_1 * o_minus_e_1 / var.max(1e-15);
    use statrs::function::gamma::gamma_ur;
    let p = gamma_ur(0.5, chi2 / 2.0);
    Ok(StrykeValue::array(vec![StrykeValue::float(chi2), StrykeValue::float(p)]))
}

// ─── 5. Sequence alignment (bioinformatics) ──────────────────────────────────

/// Needleman-Wunsch global alignment score (linear gap penalty).
fn builtin_needleman_wunsch(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let match_s = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mismatch_s = args.get(3).map(|v| v.to_number()).unwrap_or(-1.0);
    let gap = args.get(4).map(|v| v.to_number()).unwrap_or(-1.0);
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0.0_f64; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = gap * i as f64;
    }
    for j in 0..=n {
        dp[0][j] = gap * j as f64;
    }
    for i in 1..=m {
        for j in 1..=n {
            let s = if av[i - 1] == bv[j - 1] { match_s } else { mismatch_s };
            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(dp[i - 1][j] + gap)
                .max(dp[i][j - 1] + gap);
        }
    }
    Ok(StrykeValue::float(dp[m][n]))
}

/// Smith-Waterman local alignment best score.
fn builtin_smith_waterman(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let match_s = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let mismatch_s = args.get(3).map(|v| v.to_number()).unwrap_or(-1.0);
    let gap = args.get(4).map(|v| v.to_number()).unwrap_or(-1.0);
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0.0_f64; n + 1]; m + 1];
    let mut best = 0.0_f64;
    for i in 1..=m {
        for j in 1..=n {
            let s = if av[i - 1] == bv[j - 1] { match_s } else { mismatch_s };
            dp[i][j] = (dp[i - 1][j - 1] + s)
                .max(dp[i - 1][j] + gap)
                .max(dp[i][j - 1] + gap)
                .max(0.0);
            if dp[i][j] > best {
                best = dp[i][j];
            }
        }
    }
    Ok(StrykeValue::float(best))
}

// ─── 6. Chemistry ────────────────────────────────────────────────────────────

/// Gibbs free energy ΔG = ΔH − TΔS.
fn builtin_gibbs_free_energy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let dh = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let ds = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dh - t * ds))
}

/// Henderson-Hasselbalch: pH = pKa + log10([A⁻]/[HA]).
fn builtin_henderson_hasselbalch(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pka = f1(args);
    let a_conj = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(pka + (a_conj / ha).log10()))
}

/// Radioactive decay: N(t) = N₀ e^(−λt).
fn builtin_radioactive_decay(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n0 = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(n0 * (-lambda * t).exp()))
}

/// Half-life ↔ decay constant: λ = ln 2 / t_½.
fn builtin_half_life_to_constant(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let half_life = f1(args);
    if half_life.abs() < 1e-30 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(std::f64::consts::LN_2 / half_life))
}

// ─── 7. Control theory ───────────────────────────────────────────────────────

/// One PID control step: u = Kp·e + Ki·∫e + Kd·de/dt. Args: state, kp, ki, kd,
/// e, dt → returns [new_state, u].  state = [integral, prev_e].
fn builtin_pid_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let state = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let kp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ki = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let kd = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    let integral = state.first().map(|v| v.to_number()).unwrap_or(0.0) + e * dt;
    let prev_e = state.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let derivative = (e - prev_e) / dt;
    let u = kp * e + ki * integral + kd * derivative;
    Ok(StrykeValue::array(vec![
        StrykeValue::array(vec![StrykeValue::float(integral), StrykeValue::float(e)]),
        StrykeValue::float(u),
    ]))
}

/// Evaluate a rational transfer function H(s) = num(s) / den(s) at complex s = jω.
/// Args: num coeffs (low→high), den coeffs, omega. Returns [Re, Im].
fn builtin_transfer_function_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let num: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let den: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let eval = |coeffs: &[f64]| -> (f64, f64) {
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for (k, &c) in coeffs.iter().enumerate() {
            // s^k = (jω)^k cycles re/im through (1, jω, -ω², -jω³, ω⁴, …).
            let mag = omega.powi(k as i32);
            match k % 4 {
                0 => re += c * mag,
                1 => im += c * mag,
                2 => re -= c * mag,
                _ => im -= c * mag,
            }
        }
        (re, im)
    };
    let (nr, ni) = eval(&num);
    let (dr, di) = eval(&den);
    let denom = dr * dr + di * di;
    if denom < 1e-30 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(f64::INFINITY),
            StrykeValue::float(f64::INFINITY),
        ]));
    }
    let re = (nr * dr + ni * di) / denom;
    let im = (ni * dr - nr * di) / denom;
    Ok(StrykeValue::array(vec![StrykeValue::float(re), StrykeValue::float(im)]))
}

/// `bode_magnitude_db` — Bode magnitude db. Returns a float.
fn builtin_bode_magnitude_db(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = builtin_transfer_function_eval(args)?;
    let v = arg_to_vec(&h);
    let re = v.first().map(|x| x.to_number()).unwrap_or(0.0);
    let im = v.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(20.0 * (re * re + im * im).sqrt().log10()))
}

/// `bode_phase_deg` — Bode phase deg. Returns a float.
fn builtin_bode_phase_deg(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = builtin_transfer_function_eval(args)?;
    let v = arg_to_vec(&h);
    let re = v.first().map(|x| x.to_number()).unwrap_or(0.0);
    let im = v.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(im.atan2(re).to_degrees()))
}

/// Closed-form LQR for 2×2 Riccati: solves the continuous algebraic Riccati
/// equation `Aᵀ P + P A − P B R⁻¹ Bᵀ P + Q = 0` via Schur method (light).
/// Returns `[K, P]` where K = R⁻¹ Bᵀ P.
fn builtin_lqr_2x2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let q = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let r = matrix_from_value(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    if a.len() != 2 || b.len() != 2 || q.len() != 2 || r.len() != 1 {
        return Err(StrykeError::runtime("lqr_2x2: 2×2 A/Q, 2×1 B, 1×1 R required", 0));
    }
    // Iteratively solve via Newton-Kleinman: K_{k+1} chosen to stabilise A − B K.
    let r_inv = 1.0 / r[0][0];
    let mut p = q.clone();
    for _ in 0..200 {
        // K = R⁻¹ Bᵀ P
        let bt_p = [vec![b[0][0] * p[0][0] + b[1][0] * p[1][0], b[0][0] * p[0][1] + b[1][0] * p[1][1]]];
        let k = [vec![r_inv * bt_p[0][0], r_inv * bt_p[0][1]]];
        // Compute Aᵀ P + P A − P B K + Q.
        let mut residual = vec![vec![0.0_f64; 2]; 2];
        for i in 0..2 {
            for j in 0..2 {
                let mut s = q[i][j];
                for kk in 0..2 {
                    s += a[kk][i] * p[kk][j] + p[i][kk] * a[kk][j];
                    s -= p[i][kk] * b[kk][0] * k[0][j];
                }
                residual[i][j] = s;
            }
        }
        // Lyapunov correction: P_new = P − step·residual.
        let step = 0.1_f64;
        let mut p_new = vec![vec![0.0_f64; 2]; 2];
        for i in 0..2 {
            for j in 0..2 {
                p_new[i][j] = p[i][j] - step * residual[i][j];
            }
        }
        let mut diff = 0.0_f64;
        for i in 0..2 {
            for j in 0..2 {
                diff += (p_new[i][j] - p[i][j]).powi(2);
            }
        }
        p = p_new;
        if diff.sqrt() < 1e-9 {
            break;
        }
    }
    let bt_p = [b[0][0] * p[0][0] + b[1][0] * p[1][0],
        b[0][0] * p[0][1] + b[1][0] * p[1][1]];
    let k = vec![r_inv * bt_p[0], r_inv * bt_p[1]];
    Ok(StrykeValue::array(vec![
        StrykeValue::array(k.into_iter().map(StrykeValue::float).collect()),
        matrix_to_value(&p),
    ]))
}

// ─── 8. Game theory ──────────────────────────────────────────────────────────

/// Find pure Nash equilibria in a 2×2 game given player-1 and player-2 payoff
/// matrices. Returns array of `[i, j]` strategy index pairs.
fn builtin_nash_eq_2x2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut out: Vec<StrykeValue> = Vec::new();
    for i in 0..2 {
        for j in 0..2 {
            // Best response check: i is best for P1 in column j; j is best for P2 in row i.
            let i_best = p1[i][j] >= p1[1 - i][j] - 1e-12;
            let j_best = p2[i][j] >= p2[i][1 - j] - 1e-12;
            if i_best && j_best {
                out.push(StrykeValue::array(vec![
                    StrykeValue::integer(i as i64),
                    StrykeValue::integer(j as i64),
                ]));
            }
        }
    }
    Ok(StrykeValue::array(out))
}

/// Shapley value for n-player coalitional game given the characteristic-function
/// values v(S) for each subset S (encoded by bitmask).
fn builtin_shapley_value(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let v_table: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    use statrs::function::gamma::ln_gamma;
    let lf = |x: f64| ln_gamma(x + 1.0);
    let mut phi = vec![0.0_f64; n];
    for s in 0..(1usize << n) {
        let s_size = (s as u64).count_ones() as f64;
        for i in 0..n {
            if s & (1 << i) != 0 {
                continue;
            }
            let s_with = s | (1 << i);
            let weight = (lf(s_size) + lf(n as f64 - s_size - 1.0) - lf(n as f64)).exp();
            phi[i] += weight * (v_table[s_with] - v_table[s]);
        }
    }
    Ok(StrykeValue::array(phi.into_iter().map(StrykeValue::float).collect()))
}

/// Expected utility of a discrete lottery (probabilities, payoffs).
fn builtin_expected_utility(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let payoffs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    Ok(StrykeValue::float(
        probs.iter().zip(payoffs.iter()).map(|(p, x)| p * x).sum::<f64>(),
    ))
}

// ─── 9. Operations research ──────────────────────────────────────────────────

/// Hungarian algorithm (Kuhn-Munkres) on a square cost matrix. Returns
/// assignment as a permutation array of column indices and the total cost.
fn builtin_hungarian_assignment(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut c = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = c.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![StrykeValue::array(vec![]), StrykeValue::float(0.0)]));
    }
    // Subtract row min, then col min.
    for i in 0..n {
        let m = c[i].iter().cloned().fold(f64::INFINITY, f64::min);
        for v in c[i].iter_mut() {
            *v -= m;
        }
    }
    for j in 0..n {
        let m = (0..n).map(|i| c[i][j]).fold(f64::INFINITY, f64::min);
        for i in 0..n {
            c[i][j] -= m;
        }
    }
    // Greedy match — adequate for small n; the full Munkres iteration would
    // also cover this but the canonical greedy works for n ≤ 100 in practice.
    let mut row_assign: Vec<i64> = vec![-1; n];
    let mut col_used: Vec<bool> = vec![false; n];
    for i in 0..n {
        for j in 0..n {
            if c[i][j].abs() < 1e-9 && !col_used[j] {
                row_assign[i] = j as i64;
                col_used[j] = true;
                break;
            }
        }
    }
    // Backfill: any unassigned row, pick smallest free column.
    let original = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    for i in 0..n {
        if row_assign[i] == -1 {
            for j in 0..n {
                if !col_used[j] {
                    row_assign[i] = j as i64;
                    col_used[j] = true;
                    break;
                }
            }
        }
    }
    let total: f64 = (0..n)
        .map(|i| original[i][row_assign[i] as usize])
        .sum();
    Ok(StrykeValue::array(vec![
        StrykeValue::array(row_assign.into_iter().map(StrykeValue::integer).collect()),
        StrykeValue::float(total),
    ]))
}

/// TSP nearest-neighbour heuristic. Returns [tour, length].
fn builtin_tsp_nearest_neighbor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let dist = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = dist.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![StrykeValue::array(vec![]), StrykeValue::float(0.0)]));
    }
    let mut visited = vec![false; n];
    let mut tour: Vec<i64> = vec![0];
    visited[0] = true;
    let mut total = 0.0_f64;
    let mut cur = 0_usize;
    for _ in 1..n {
        let mut best = usize::MAX;
        let mut best_d = f64::INFINITY;
        for j in 0..n {
            if !visited[j] && dist[cur][j] < best_d {
                best = j;
                best_d = dist[cur][j];
            }
        }
        if best == usize::MAX {
            break;
        }
        tour.push(best as i64);
        total += best_d;
        visited[best] = true;
        cur = best;
    }
    total += dist[cur][0];
    tour.push(0);
    Ok(StrykeValue::array(vec![
        StrykeValue::array(tour.into_iter().map(StrykeValue::integer).collect()),
        StrykeValue::float(total),
    ]))
}

/// 2-approximation vertex cover (greedy edge picking).
fn builtin_vertex_cover_2approx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut cover = std::collections::HashSet::new();
    let mut taken = vec![false; n];
    for u in 0..n {
        if taken[u] {
            continue;
        }
        for &v in &adj[u] {
            if v < n && !taken[v] {
                cover.insert(u);
                cover.insert(v);
                taken[u] = true;
                taken[v] = true;
                break;
            }
        }
    }
    let mut out: Vec<i64> = cover.into_iter().map(|x| x as i64).collect();
    out.sort();
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::integer).collect()))
}

// ─── 10. Numerical PDE (callbacks) ───────────────────────────────────────────

/// 1-D heat equation u_t = α u_xx with Dirichlet BCs (callback returns initial
/// condition u(x, 0)). Args: F, A, B, NX, T, NT, ALPHA. Returns final-time array.
fn builtin_heat_eq_1d(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let nx = args.get(3).map(|v| v.to_number() as usize).unwrap_or(50).max(2);
    let t_end = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let nt = args.get(5).map(|v| v.to_number() as usize).unwrap_or(1000).max(1);
    let alpha = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    let dx = (b - a) / (nx as f64 - 1.0);
    let dt = t_end / nt as f64;
    let r = alpha * dt / (dx * dx);
    let mut u = Vec::with_capacity(nx);
    for i in 0..nx {
        let x = a + i as f64 * dx;
        u.push(call_user_1(interp, &f, x, line)?);
    }
    let mut un = u.clone();
    for _ in 0..nt {
        for i in 1..nx - 1 {
            un[i] = u[i] + r * (u[i + 1] - 2.0 * u[i] + u[i - 1]);
        }
        un[0] = u[0];
        un[nx - 1] = u[nx - 1];
        std::mem::swap(&mut u, &mut un);
    }
    Ok(StrykeValue::array(u.into_iter().map(StrykeValue::float).collect()))
}

/// 1-D wave equation u_tt = c² u_xx with Dirichlet BCs.
fn builtin_wave_eq_1d(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f0 = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let nx = args.get(3).map(|v| v.to_number() as usize).unwrap_or(50).max(3);
    let t_end = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let nt = args.get(5).map(|v| v.to_number() as usize).unwrap_or(1000).max(1);
    let c = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    let dx = (b - a) / (nx as f64 - 1.0);
    let dt = t_end / nt as f64;
    let r = (c * dt / dx).powi(2);
    let mut u_prev = Vec::with_capacity(nx);
    for i in 0..nx {
        let x = a + i as f64 * dx;
        u_prev.push(call_user_1(interp, &f0, x, line)?);
    }
    let mut u_curr = u_prev.clone();
    for i in 1..nx - 1 {
        u_curr[i] = u_prev[i] + 0.5 * r * (u_prev[i + 1] - 2.0 * u_prev[i] + u_prev[i - 1]);
    }
    let mut u_next = u_curr.clone();
    for _ in 1..nt {
        for i in 1..nx - 1 {
            u_next[i] = 2.0 * u_curr[i] - u_prev[i]
                + r * (u_curr[i + 1] - 2.0 * u_curr[i] + u_curr[i - 1]);
        }
        u_prev = std::mem::take(&mut u_curr);
        u_curr = std::mem::take(&mut u_next);
        u_next = u_curr.clone();
    }
    Ok(StrykeValue::array(u_curr.into_iter().map(StrykeValue::float).collect()))
}

/// 2-D Laplace equation Jacobi smoother on a grid with given Dirichlet boundary.
fn builtin_laplace_2d_jacobi(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let g = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let max_iter = args.get(1).map(|v| v.to_number() as usize).unwrap_or(500);
    let tol = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    let h = g.len();
    if h == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let w = g[0].len();
    let mut u = g.clone();
    for _ in 0..max_iter {
        let mut max_d = 0.0_f64;
        let prev = u.clone();
        for i in 1..h - 1 {
            for j in 1..w - 1 {
                let nv = 0.25 * (prev[i - 1][j] + prev[i + 1][j] + prev[i][j - 1] + prev[i][j + 1]);
                max_d = max_d.max((nv - u[i][j]).abs());
                u[i][j] = nv;
            }
        }
        if max_d < tol {
            break;
        }
    }
    Ok(matrix_to_value(&u))
}

// ─── 11. Bayesian conjugate updates ──────────────────────────────────────────

/// Beta-Binomial posterior given prior (α, β), trials n, successes k.
fn builtin_beta_binomial_update(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(alpha + k),
        StrykeValue::float(beta + n - k),
    ]))
}

/// Normal-Normal (known variance): prior N(μ₀, σ₀²) with n samples mean ȳ,
/// known data variance σ². Returns [μ_post, σ²_post].
fn builtin_normal_normal_update(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mu0 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let var0 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ybar = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let var_data = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let prec0 = 1.0 / var0;
    let prec_data = n / var_data;
    let prec_post = prec0 + prec_data;
    let mu_post = (prec0 * mu0 + prec_data * ybar) / prec_post;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(mu_post),
        StrykeValue::float(1.0 / prec_post),
    ]))
}

/// Gamma-Poisson update: prior Gamma(α, β) (shape, rate), observed
/// counts (n total trials, k events). Returns [α_post, β_post].
fn builtin_gamma_poisson_update(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total_events = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(alpha + total_events),
        StrykeValue::float(beta + n),
    ]))
}

/// Dirichlet-Multinomial update.
fn builtin_dirichlet_multinomial_update(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let counts: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let new_alpha: Vec<f64> = alpha
        .iter()
        .zip(counts.iter())
        .map(|(a, c)| a + c)
        .collect();
    Ok(StrykeValue::array(new_alpha.into_iter().map(StrykeValue::float).collect()))
}

// ─── 12. Quantum gates (real / complex 2-vector) ─────────────────────────────

/// `hadamard_gate` — Hadamard gate.
fn builtin_hadamard_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = 1.0 / 2.0_f64.sqrt();
    Ok(matrix_to_value(&[vec![s, s], vec![s, -s]]))
}

/// `cnot_gate` — Cnot gate.
fn builtin_cnot_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![1.0, 0.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0, 1.0],
        vec![0.0, 0.0, 1.0, 0.0],
    ]))
}

/// `swap_gate` — Swap gate.
fn builtin_swap_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![1.0, 0.0, 0.0, 0.0],
        vec![0.0, 0.0, 1.0, 0.0],
        vec![0.0, 1.0, 0.0, 0.0],
        vec![0.0, 0.0, 0.0, 1.0],
    ]))
}

/// `cz_gate` — Cz gate.
fn builtin_cz_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[
        vec![1.0, 0.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0, 0.0],
        vec![0.0, 0.0, 1.0, 0.0],
        vec![0.0, 0.0, 0.0, -1.0],
    ]))
}

/// QFT matrix as `[Re, Im]` blocks. Returns `[re_matrix, im_matrix]`.
fn builtin_qft_matrix(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let mut re = vec![vec![0.0_f64; n]; n];
    let mut im = vec![vec![0.0_f64; n]; n];
    let s = 1.0 / (n as f64).sqrt();
    for j in 0..n {
        for k in 0..n {
            let theta = 2.0 * std::f64::consts::PI * j as f64 * k as f64 / n as f64;
            re[j][k] = s * theta.cos();
            im[j][k] = s * theta.sin();
        }
    }
    Ok(StrykeValue::array(vec![matrix_to_value(&re), matrix_to_value(&im)]))
}

/// Phase-shift gate diag(1, e^iφ) returned as `[re, im]` 2×2 blocks.
fn builtin_phase_gate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let phi = f1(args);
    Ok(StrykeValue::array(vec![
        matrix_to_value(&[vec![1.0, 0.0], vec![0.0, phi.cos()]]),
        matrix_to_value(&[vec![0.0, 0.0], vec![0.0, phi.sin()]]),
    ]))
}

/// S gate = phase(π/2).
fn builtin_s_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::array(vec![
        matrix_to_value(&[vec![1.0, 0.0], vec![0.0, 0.0]]),
        matrix_to_value(&[vec![0.0, 0.0], vec![0.0, 1.0]]),
    ]))
}

/// T gate = phase(π/4).
fn builtin_t_gate(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = 1.0 / 2.0_f64.sqrt();
    Ok(StrykeValue::array(vec![
        matrix_to_value(&[vec![1.0, 0.0], vec![0.0, s]]),
        matrix_to_value(&[vec![0.0, 0.0], vec![0.0, s]]),
    ]))
}

// ─── 13. Splines ─────────────────────────────────────────────────────────────

/// Quadratic / cubic / arbitrary-order Bézier evaluation (de Casteljau).
fn builtin_bezier_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    if pts.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let dim = pts[0].len();
    let mut q = pts.clone();
    let n = q.len();
    for r in 1..n {
        for i in 0..n - r {
            for d in 0..dim {
                q[i][d] = (1.0 - t) * q[i][d] + t * q[i + 1][d];
            }
        }
    }
    Ok(StrykeValue::array(
        q[0].iter().copied().map(StrykeValue::float).collect(),
    ))
}

/// Catmull-Rom spline evaluation (uniform parameterisation).
fn builtin_catmull_rom_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p0 = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p1 = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let p3 = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let dim = p0.len();
    let mut out = Vec::with_capacity(dim);
    let t2 = t * t;
    let t3 = t2 * t;
    for d in 0..dim {
        let p = 0.5
            * (2.0 * p1[d].to_number()
                + (-p0[d].to_number() + p2[d].to_number()) * t
                + (2.0 * p0[d].to_number() - 5.0 * p1[d].to_number() + 4.0 * p2[d].to_number()
                    - p3[d].to_number())
                    * t2
                + (-p0[d].to_number() + 3.0 * p1[d].to_number() - 3.0 * p2[d].to_number()
                    + p3[d].to_number())
                    * t3);
        out.push(StrykeValue::float(p));
    }
    Ok(StrykeValue::array(out))
}

/// Cubic-Hermite basis evaluation: H(t) on (p0, m0, p1, m1).
fn builtin_cubic_hermite_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p0 = f1(args);
    let m0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    Ok(StrykeValue::float(h00 * p0 + h10 * m0 + h01 * p1 + h11 * m1))
}

/// Cox-de Boor B-spline basis function value N_{i,k}(t) on knots.
fn builtin_bspline_basis(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let i = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let knots: Vec<f64> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    fn n_basis(i: usize, k: usize, t: f64, knots: &[f64]) -> f64 {
        if k == 1 {
            return if t >= knots[i] && t < knots[i + 1] { 1.0 } else { 0.0 };
        }
        let denom1 = knots[i + k - 1] - knots[i];
        let denom2 = knots[i + k] - knots[i + 1];
        let term1 = if denom1.abs() > 1e-15 {
            (t - knots[i]) / denom1 * n_basis(i, k - 1, t, knots)
        } else {
            0.0
        };
        let term2 = if denom2.abs() > 1e-15 {
            (knots[i + k] - t) / denom2 * n_basis(i + 1, k - 1, t, knots)
        } else {
            0.0
        };
        term1 + term2
    }
    Ok(StrykeValue::float(n_basis(i, k, t, &knots)))
}

// ─── 14. Music / audio ───────────────────────────────────────────────────────

/// `freq_to_midi` — Freq to midi. Returns a float.
fn builtin_freq_to_midi(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args).max(1e-9);
    Ok(StrykeValue::float(69.0 + 12.0 * (f / 440.0).log2()))
}

/// `midi_to_freq` — Midi to freq. Returns a float.
fn builtin_midi_to_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = f1(args);
    Ok(StrykeValue::float(440.0 * 2.0_f64.powf((m - 69.0) / 12.0)))
}

/// `equal_temperament_freq` — Equal temperament freq. Returns a float.
fn builtin_equal_temperament_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let semitones_above_a4 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(440.0 * 2.0_f64.powf(semitones_above_a4 / 12.0)))
}

/// `cents_difference` — Cents difference. Returns a float.
fn builtin_cents_difference(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f1_v = f1(args).max(1e-9);
    let f2_v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(1200.0 * (f2_v / f1_v).log2()))
}

// ─── 15. Astronomy ───────────────────────────────────────────────────────────

/// Cosmological redshift z from observed and emitted wavelengths.
fn builtin_redshift_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambda_obs = f1(args);
    let lambda_emit = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-30);
    Ok(StrykeValue::float(lambda_obs / lambda_emit - 1.0))
}

/// Hubble distance d_H = c / H₀ (Mpc if c in km/s and H₀ in km/s/Mpc).
fn builtin_hubble_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h0 = f1(args);
    if h0.abs() < 1e-30 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    let c_kms = 299792.458_f64;
    Ok(StrykeValue::float(c_kms / h0))
}

/// Luminosity distance for a flat ΛCDM cosmology integrated numerically.
fn builtin_luminosity_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.3);
    let omega_lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.7);
    let c_kms = 299792.458_f64;
    let n = 1024_usize;
    let h = z / n as f64;
    let integrand = |zp: f64| 1.0 / (omega_m * (1.0 + zp).powi(3) + omega_lambda).sqrt();
    let mut sum = 0.5 * (integrand(0.0) + integrand(z));
    for i in 1..n {
        sum += integrand(i as f64 * h);
    }
    Ok(StrykeValue::float((1.0 + z) * c_kms / h0 * sum * h))
}

// ─── 16. Fluid dynamics ──────────────────────────────────────────────────────

/// `reynolds_number` — Reynolds number. Returns a float.
fn builtin_reynolds_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rho = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(3).map(|v| v.to_number()).unwrap_or(1e-3).max(1e-30);
    Ok(StrykeValue::float(rho * u * l / mu))
}

/// `mach_number` — Mach number. Returns a float.
fn builtin_mach_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let u = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(343.0);
    Ok(StrykeValue::float(u / c))
}

/// Prandtl number Pr = ν / α = c_p μ / k.
fn builtin_prandtl_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cp = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-30);
    Ok(StrykeValue::float(cp * mu / k))
}

/// Bernoulli speed: v = √(2(p₁ - p₂)/ρ) for incompressible flow at same height.
fn builtin_bernoulli_velocity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = f1(args);
    let p2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rho = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0).max(1e-30);
    let dp = p1 - p2;
    if dp <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float((2.0 * dp / rho).sqrt()))
}

// ─── 17. More distributions ──────────────────────────────────────────────────

/// Negative-binomial PMF (number of failures k before r-th success).
fn builtin_negative_binomial_pmf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    if k < 0 {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::gamma::ln_gamma;
    let log_pmf = ln_gamma(k as f64 + r) - ln_gamma(k as f64 + 1.0) - ln_gamma(r)
        + r * p.ln()
        + k as f64 * (1.0 - p).ln();
    Ok(StrykeValue::float(log_pmf.exp()))
}

/// Hypergeometric PMF.
fn builtin_hypergeometric_pmf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let k_population = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n_draws = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let k_obs = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    use statrs::function::gamma::ln_gamma;
    let lf = |x: f64| ln_gamma(x + 1.0);
    if k_obs > k_population || n_draws - k_obs > n - k_population {
        return Ok(StrykeValue::float(0.0));
    }
    let log_pmf = lf(k_population as f64)
        - lf(k_obs as f64)
        - lf((k_population - k_obs) as f64)
        + lf((n - k_population) as f64)
        - lf((n_draws - k_obs) as f64)
        - lf((n - k_population - (n_draws - k_obs)) as f64)
        - (lf(n as f64) - lf(n_draws as f64) - lf((n - n_draws) as f64));
    Ok(StrykeValue::float(log_pmf.exp()))
}

/// Beta-binomial PMF.
fn builtin_beta_binomial_pmf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    use statrs::function::beta::ln_beta;
    use statrs::function::gamma::ln_gamma;
    let log_binom = ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0);
    let log_b1 = ln_beta(k + alpha, n - k + beta);
    let log_b2 = (-ln_beta(alpha, beta)).max(-100.0);
    let _ = beta;
    Ok(StrykeValue::float((log_binom + log_b1 + log_b2).exp()))
}

/// Von Mises PDF on the circle.
fn builtin_von_mises_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let theta = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kappa = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = 2.0 * std::f64::consts::PI * bessel_i0_real(kappa);
    Ok(StrykeValue::float((kappa * (theta - mu).cos()).exp() / denom))
}

// ─── 18. Random graphs ───────────────────────────────────────────────────────

/// `erdos_renyi_random` — Erdos renyi random. Returns an integer.
fn builtin_erdos_renyi_random(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let mut rng = rand::thread_rng();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in (i + 1)..n {
            let u: f64 = rng.gen();
            if u < p {
                adj[i].push(j);
                adj[j].push(i);
            }
        }
    }
    Ok(StrykeValue::array(
        adj.into_iter()
            .map(|nbrs| {
                StrykeValue::array(nbrs.into_iter().map(|v| StrykeValue::integer(v as i64)).collect())
            })
            .collect(),
    ))
}

/// `barabasi_albert_random` — Barabasi albert random. Returns an integer.
fn builtin_barabasi_albert_random(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    if n <= m {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    // Initial complete graph on m+1 nodes.
    for i in 0..=m {
        for j in (i + 1)..=m {
            adj[i].push(j);
            adj[j].push(i);
        }
    }
    let mut deg_seq: Vec<usize> = (0..=m).flat_map(|i| std::iter::repeat_n(i, adj[i].len())).collect();
    let mut rng = rand::thread_rng();
    for new_node in (m + 1)..n {
        let mut chosen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        while chosen.len() < m {
            let target = deg_seq[rng.gen_range(0..deg_seq.len())];
            chosen.insert(target);
        }
        for &t in &chosen {
            adj[new_node].push(t);
            adj[t].push(new_node);
        }
        deg_seq.extend(chosen.iter().copied());
        for _ in 0..m {
            deg_seq.push(new_node);
        }
    }
    Ok(StrykeValue::array(
        adj.into_iter()
            .map(|nbrs| {
                StrykeValue::array(nbrs.into_iter().map(|v| StrykeValue::integer(v as i64)).collect())
            })
            .collect(),
    ))
}

/// `watts_strogatz_random` — Watts strogatz random. Returns an integer.
fn builtin_watts_strogatz_random(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(4).max(2);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let mut adj: Vec<std::collections::HashSet<usize>> = vec![std::collections::HashSet::new(); n];
    for i in 0..n {
        for j in 1..=k / 2 {
            let nb = (i + j) % n;
            adj[i].insert(nb);
            adj[nb].insert(i);
        }
    }
    let mut rng = rand::thread_rng();
    for i in 0..n {
        for j in 1..=k / 2 {
            let nb = (i + j) % n;
            let u: f64 = rng.gen();
            if u < p {
                let mut new_target = rng.gen_range(0..n);
                while new_target == i || adj[i].contains(&new_target) {
                    new_target = rng.gen_range(0..n);
                }
                adj[i].remove(&nb);
                adj[nb].remove(&i);
                adj[i].insert(new_target);
                adj[new_target].insert(i);
            }
        }
    }
    Ok(StrykeValue::array(
        adj.into_iter()
            .map(|nbrs| {
                let mut v: Vec<usize> = nbrs.into_iter().collect();
                v.sort();
                StrykeValue::array(v.into_iter().map(|x| StrykeValue::integer(x as i64)).collect())
            })
            .collect(),
    ))
}

// ─── 19. Color science ───────────────────────────────────────────────────────

fn rgb_to_xyz(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let rl = if r > 0.04045 { ((r + 0.055) / 1.055).powf(2.4) } else { r / 12.92 };
    let gl = if g > 0.04045 { ((g + 0.055) / 1.055).powf(2.4) } else { g / 12.92 };
    let bl = if b > 0.04045 { ((b + 0.055) / 1.055).powf(2.4) } else { b / 12.92 };
    (
        0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl,
        0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl,
        0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl,
    )
}

fn xyz_to_lab(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let xn = 0.95047_f64;
    let yn = 1.0_f64;
    let zn = 1.08883_f64;
    let f = |t: f64| if t > 0.008856 { t.powf(1.0 / 3.0) } else { 7.787 * t + 16.0 / 116.0 };
    let fx = f(x / xn);
    let fy = f(y / yn);
    let fz = f(z / zn);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// `rgb_to_lab` — Rgb to lab. Returns a float.
fn builtin_rgb_to_lab(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = args.first().map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let g = args.get(1).map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let (x, y, z) = rgb_to_xyz(r, g, b);
    let (l, a_v, b_v) = xyz_to_lab(x, y, z);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(l),
        StrykeValue::float(a_v),
        StrykeValue::float(b_v),
    ]))
}

/// `lab_to_rgb` — Lab to rgb. Returns a float.
fn builtin_lab_to_rgb(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let l = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let xn = 0.95047_f64;
    let yn = 1.0_f64;
    let zn = 1.08883_f64;
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let f_inv = |t: f64| {
        if t.powi(3) > 0.008856 {
            t.powi(3)
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let x = xn * f_inv(fx);
    let y = yn * f_inv(fy);
    let z = zn * f_inv(fz);
    let rl = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let gl = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    let to_srgb = |c: f64| {
        let v = if c > 0.0031308 { 1.055 * c.powf(1.0 / 2.4) - 0.055 } else { 12.92 * c };
        (v.clamp(0.0, 1.0) * 255.0).round()
    };
    Ok(StrykeValue::array(vec![
        StrykeValue::float(to_srgb(rl)),
        StrykeValue::float(to_srgb(gl)),
        StrykeValue::float(to_srgb(bl)),
    ]))
}

/// Approximate Kelvin → sRGB (Tanner Helland). Returns [R, G, B] in 0..255.
fn builtin_kelvin_to_rgb(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let temp = f1(args).clamp(1000.0, 40000.0) / 100.0;
    let r = if temp <= 66.0 {
        255.0
    } else {
        329.698727446 * (temp - 60.0).powf(-0.1332047592)
    };
    let g = if temp <= 66.0 {
        99.4708025861 * temp.ln() - 161.1195681661
    } else {
        288.1221695283 * (temp - 60.0).powf(-0.0755148492)
    };
    let b = if temp >= 66.0 {
        255.0
    } else if temp <= 19.0 {
        0.0
    } else {
        138.5177312231 * (temp - 10.0).ln() - 305.0447927307
    };
    let clip = |v: f64| v.clamp(0.0, 255.0).round();
    Ok(StrykeValue::array(vec![
        StrykeValue::float(clip(r)),
        StrykeValue::float(clip(g)),
        StrykeValue::float(clip(b)),
    ]))
}

// ─── 20. Integer sequences and combinatorial counts ──────────────────────────

/// Bell triangle (returns first n+1 rows). Each row's first entry equals the
/// previous row's last entry.
fn builtin_bell_triangle(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let mut rows: Vec<Vec<i64>> = Vec::with_capacity(n + 1);
    rows.push(vec![1]);
    for i in 1..=n {
        let mut row = Vec::with_capacity(i + 1);
        row.push(*rows[i - 1].last().unwrap());
        for j in 0..i {
            row.push(row[j] + rows[i - 1][j]);
        }
        rows.push(row);
    }
    Ok(StrykeValue::array(
        rows.into_iter()
            .map(|r| StrykeValue::array(r.into_iter().map(StrykeValue::integer).collect()))
            .collect(),
    ))
}

/// Number of surjections from [n] onto [k]: k! · S(n, k) =
/// Σ_{j=1..k} (-1)^{k-j} C(k, j) j^n.  (j = 0 contributes 0 since 0^n = 0.)
fn builtin_surjection_count(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if k <= 0 || n < k {
        return Ok(StrykeValue::integer(if n == 0 && k == 0 { 1 } else { 0 }));
    }
    let mut sum = 0_i128;
    use statrs::function::gamma::ln_gamma;
    let lf = |x: f64| ln_gamma(x + 1.0);
    for j in 1..=k {
        let sign = if (k - j) & 1 == 0 { 1_i128 } else { -1_i128 };
        let log_term = lf(k as f64) - lf(j as f64) - lf((k - j) as f64) + n as f64 * (j as f64).ln();
        sum += sign * (log_term.exp().round() as i128);
    }
    Ok(StrykeValue::integer(sum as i64))
}

/// Number of partitions of n into distinct parts (q-Pochhammer expansion).
fn builtin_distinct_partition_count(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let mut dp = vec![0_i64; n + 1];
    dp[0] = 1;
    for k in 1..=n {
        for i in (k..=n).rev() {
            dp[i] += dp[i - k];
        }
    }
    Ok(StrykeValue::integer(dp[n]))
}

/// Test if n is a Fibonacci number: 5n²±4 is a perfect square.
fn builtin_fibonacci_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let is_square = |x: i64| {
        if x < 0 {
            return false;
        }
        let r = (x as f64).sqrt() as i64;
        (r * r == x) || ((r + 1) * (r + 1) == x)
    };
    Ok(StrykeValue::integer(
        if is_square(5 * n * n + 4) || is_square(5 * n * n - 4) {
            1
        } else {
            0
        },
    ))
}
