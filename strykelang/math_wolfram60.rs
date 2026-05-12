// Batch 60 — actuarial science (Bowers et al., "Actuarial Mathematics" 2nd ed.):
// life-table operations, annuity / insurance values, premiums, reserves,
// credibility, ruin theory, run-off triangles.

fn b60_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Curtate life expectancy e_x = (l_{x+1} + l_{x+2} + ...) / l_x. Given a
/// suffix [l_x, l_{x+1}, ...], compute e_x.
fn builtin_life_expectancy_e0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = b60_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if l.is_empty() || l[0] <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = l.iter().skip(1).sum();
    Ok(StrykeValue::float(s / l[0]))
}

/// Force of mortality μ(x) = -d ln l(x) / dx ≈ -(ln l_{x+1} − ln l_{x-1}) / 2.
fn builtin_force_of_mortality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l_minus = f1(args).max(1e-12);
    let l_plus = args.get(1).map(|v| v.to_number()).unwrap_or(l_minus).max(1e-12);
    Ok(StrykeValue::float(-(l_plus.ln() - l_minus.ln()) / 2.0))
}

/// Select-ultimate transition: l_{[x]+t} approaches l_{x+t} after the select
/// period. This returns the blend l_{[x]+t} = α·l_{select}(t) + (1−α)·l_{ult}(x+t)
/// with α decaying linearly in t/select_period.
fn builtin_select_ultimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l_select = f1(args);
    let l_ult = args.get(1).map(|v| v.to_number()).unwrap_or(l_select);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let period = args.get(3).map(|v| v.to_number()).unwrap_or(5.0).max(1e-9);
    let alpha = (1.0 - t / period).clamp(0.0, 1.0);
    Ok(StrykeValue::float(alpha * l_select + (1.0 - alpha) * l_ult))
}

/// Annuity-due ä_n = (1 − vⁿ) / d, with v = 1/(1+i), d = i/(1+i).
fn builtin_annuity_due_an(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if i == 0.0 { return Ok(StrykeValue::float(n)); }
    let v = 1.0 / (1.0 + i);
    let d = i / (1.0 + i);
    Ok(StrykeValue::float((1.0 - v.powf(n)) / d))
}

/// Annuity-immediate a_n = (1 − vⁿ) / i.
fn builtin_annuity_immediate_an(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if i == 0.0 { return Ok(StrykeValue::float(n)); }
    let v = 1.0 / (1.0 + i);
    Ok(StrykeValue::float((1.0 - v.powf(n)) / i))
}

/// n-year term life A^1_{x:n} = Σ_{k=0}^{n-1} v^{k+1} k_p_x q_{x+k}.
/// Args: i, q_x array of one-year mortality rates (length n).
fn builtin_term_life_a_n_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let q = b60_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let v = 1.0 / (1.0 + i);
    let mut p_x = 1.0_f64;
    let mut a = 0.0_f64;
    for (k, q_k) in q.iter().enumerate() {
        a += v.powi((k + 1) as i32) * p_x * q_k;
        p_x *= 1.0 - q_k;
    }
    Ok(StrykeValue::float(a))
}

/// Whole-life A_x = Σ_{k=0}^{ω-x} v^{k+1} k_p_x q_{x+k}. Same machinery.
fn builtin_whole_life_a(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_term_life_a_n_t(args)
}

/// Pure endowment _n E_x = vⁿ · n_p_x.
fn builtin_endowment_pure_e(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let q = b60_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let v = 1.0 / (1.0 + i);
    let n = q.len();
    let mut p_x = 1.0_f64;
    for &q_k in &q { p_x *= 1.0 - q_k; }
    Ok(StrykeValue::float(v.powi(n as i32) * p_x))
}

/// Endowment insurance A_{x:n} = A^1_{x:n} + _n E_x.
fn builtin_endowment_combined_a(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let term = builtin_term_life_a_n_t(args)?.to_number();
    let pure = builtin_endowment_pure_e(args)?.to_number();
    Ok(StrykeValue::float(term + pure))
}

/// Net level premium per unit benefit: P = A / a (equivalence principle).
fn builtin_premium_net(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_x = f1(args);
    let ann_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(a_x / ann_x))
}

/// Level premium for term insurance, given i and one-year q_x array.
fn builtin_level_premium(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let term_a = builtin_term_life_a_n_t(args)?.to_number();
    let i = f1(args);
    let q = b60_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let v = 1.0 / (1.0 + i);
    let mut p_x = 1.0_f64;
    let mut a = 0.0_f64;
    for (k, q_k) in q.iter().enumerate() {
        a += v.powi(k as i32) * p_x;
        p_x *= 1.0 - q_k;
    }
    if a <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(term_a / a))
}

/// Prospective reserve: V_t = A_{x+t:n-t} − P · ä_{x+t:n-t}.
fn builtin_reserve_prospective(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_xt = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ann_xt = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a_xt - p * ann_xt))
}

/// Retrospective reserve: V_t = (P · s_{x:t} − k · A^1_{x:t}) · (1+i)^t. Args:
/// premium P, accum annuity s_xt, accum insurance, t.
fn builtin_reserve_retrospective(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let s_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let acc_a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((p * s_x - acc_a) * (1.0 + i).powf(t)))
}

/// Gross premium with expense loading: G = P_net + e_α (initial) + e_β (renewal).
fn builtin_gross_premium_load(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_net = f1(args);
    let e_alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let e_beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p_net + e_alpha + e_beta))
}

/// Experience factor f = actual / expected (Bornhuetter-Ferguson seed).
fn builtin_experience_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let actual = f1(args);
    let expected = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(actual / expected))
}

/// Single-year mortality probability q_x = (l_x − l_{x+1}) / l_x.
fn builtin_mortality_table_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l_x = f1(args).max(1e-12);
    let l_xp1 = args.get(1).map(|v| v.to_number()).unwrap_or(l_x);
    Ok(StrykeValue::float(((l_x - l_xp1) / l_x).clamp(0.0, 1.0)))
}

/// Select period transition step: returns 1 if past the select period.
fn builtin_select_period_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let period = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    Ok(StrykeValue::integer(if t >= period { 1 } else { 0 }))
}

/// Multi-decrement total q'(j)_x: combine independent rates with associated
/// single-decrement formula. Args: array of independent q'_j.
fn builtin_multi_decrement_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let qs = b60_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let prod_one_minus: f64 = qs.iter().map(|q| 1.0 - q).product();
    Ok(StrykeValue::float((1.0 - prod_one_minus).clamp(0.0, 1.0)))
}

/// Multi-state transition probability via discrete-time Markov: p_ij = (P^t)_{ij}
/// computed by repeated squaring of a 2x2 transition matrix flat [p00, p01, p10, p11].
fn builtin_multi_state_pij(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = b60_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number() as u64).unwrap_or(1);
    let i = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let j = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    if m.len() != 4 { return Ok(StrykeValue::float(0.0)); }
    let mut a = [[m[0], m[1]], [m[2], m[3]]];
    let mut acc = [[1.0_f64, 0.0], [0.0, 1.0]];
    let mut e = t;
    while e > 0 {
        if e & 1 == 1 {
            let mut next = [[0.0_f64; 2]; 2];
            for r in 0..2 { for c in 0..2 { for k in 0..2 { next[r][c] += acc[r][k] * a[k][c]; } } }
            acc = next;
        }
        e >>= 1;
        if e > 0 {
            let mut sq = [[0.0_f64; 2]; 2];
            for r in 0..2 { for c in 0..2 { for k in 0..2 { sq[r][c] += a[r][k] * a[k][c]; } } }
            a = sq;
        }
    }
    if i >= 2 || j >= 2 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(acc[i][j]))
}

/// Bühlmann credibility factor Z = n / (n + k), k = E[Var(X|Θ)] / Var(E[X|Θ]).
fn builtin_credibility_buhlmann(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let v_within = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let v_between = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let k = v_within / v_between;
    Ok(StrykeValue::float(n / (n + k)))
}

/// Lognormal severity loss: density f(x) = (1/(xσ√2π)) exp(−(ln x − μ)²/(2σ²)).
fn builtin_loss_severity_lognormal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    if x <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let z = (x.ln() - mu) / sigma;
    Ok(StrykeValue::float((-z * z / 2.0).exp() / (x * sigma * (2.0 * std::f64::consts::PI).sqrt())))
}

/// Poisson loss-frequency PMF: P(N=k) = e^(-λ) λ^k / k!.
fn builtin_loss_frequency_poisson(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args).max(0.0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if k < 0 { return Ok(StrykeValue::float(0.0)); }
    let mut log_fact = 0.0_f64;
    for j in 1..=k { log_fact += (j as f64).ln(); }
    Ok(StrykeValue::float((-lambda + k as f64 * lambda.max(1e-12).ln() - log_fact).exp()))
}

/// Lundberg ruin upper bound: Ψ(u) ≤ exp(-R·u). Args: u (initial surplus), R.
fn builtin_ruin_probability_lundberg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(StrykeValue::float((-r * u).exp()))
}

/// Cramér-Lundberg surplus increment U(t+dt) = U(t) + (1+θ)λμ·dt − loss_increment.
fn builtin_cramer_lundberg_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mu_loss = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let loss = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(u + (1.0 + theta) * lambda * mu_loss * dt - loss))
}

/// Bornhuetter-Ferguson IBNR estimate: BF = expected_loss · (1 − f), where f
/// is the % reported (development factor).
fn builtin_bornhuetter_ferguson(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let expected = f1(args);
    let f_reported = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    Ok(StrykeValue::float(expected * (1.0 - f_reported)))
}

/// Chain-ladder development step: cumulative_t+1 = cumulative_t · age-to-age factor.
fn builtin_chain_ladder_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cum = f1(args);
    let factor = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(cum * factor))
}

/// IBNR estimate: ultimate − reported.
fn builtin_ibnr_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ultimate = f1(args);
    let reported = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((ultimate - reported).max(0.0)))
}

/// Run-off triangle step: project diagonal cell using age-to-age. Args:
/// last_cumulative, age_factor.
fn builtin_run_off_triangle_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_chain_ladder_step(args)
}
