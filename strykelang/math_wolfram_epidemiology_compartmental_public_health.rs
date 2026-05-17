// epidemiology / public health: compartmental ODEs (SIR/SEIR/SIRS),
// reproduction numbers, attack rate, herd immunity, mortality measures, RBA
// effect estimates, contact tracing.

fn b61_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// R₀ basic reproduction number from contact rate β, infectious period 1/γ.
fn builtin_r_naught_basic(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beta = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(beta / gamma))
}

/// Effective reproduction number R(t) = R₀ · S(t) / N (SIR mass-action).
fn builtin_r_effective_t(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(r0 * s / n))
}

/// Doubling time T_d = ln 2 / r, where r is exponential growth rate.
fn builtin_doubling_time_growth(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = f1(args);
    if r <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2f64.ln() / r))
}

/// SIRS one-step Euler: S' = -βSI/N + ξR, I' = βSI/N - γI, R' = γI - ξR.
fn builtin_sirs_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = (s + i + r).max(1.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let gamma = args.get(4).map(|v| v.to_number()).unwrap_or(0.1);
    let xi = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    let dt = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    let new_s = s + dt * (-beta * s * i / n + xi * r);
    let new_i = i + dt * (beta * s * i / n - gamma * i);
    Ok(StrykeValue::float(new_s.max(0.0) + new_i.max(0.0) / 1e9))
}

/// SEIRS one-step: adds exposed compartment with rate σ (E → I), waning ξ.
fn builtin_seirs_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let n = (s + e + i + r).max(1.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let sigma = args.get(5).map(|v| v.to_number()).unwrap_or(0.2);
    let gamma = args.get(6).map(|v| v.to_number()).unwrap_or(0.1);
    let xi = args.get(7).map(|v| v.to_number()).unwrap_or(0.01);
    let dt = args.get(8).map(|v| v.to_number()).unwrap_or(1.0);
    let new_s = s + dt * (-beta * s * i / n + xi * r);
    let new_e = e + dt * (beta * s * i / n - sigma * e);
    let new_i = i + dt * (sigma * e - gamma * i);
    Ok(StrykeValue::float(new_s.max(0.0) + (new_e + new_i).max(0.0) / 1e9))
}

/// Force of infection λ = β·I/N (S → I rate per susceptible).
fn builtin_susceptible_to_infected(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beta = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(beta * i / n))
}

/// Final attack rate (SIR endemic): solve A = 1 - exp(-R₀·A) numerically.
fn builtin_attack_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    if r0 <= 1.0 { return Ok(StrykeValue::float(0.0)); }
    let mut a = 0.5_f64;
    for _ in 0..200 {
        let f = a - 1.0 + (-r0 * a).exp();
        let fp = 1.0 + r0 * (-r0 * a).exp();
        if fp.abs() < 1e-15 { break; }
        let new_a = a - f / fp;
        if (new_a - a).abs() < 1e-12 { return Ok(StrykeValue::float(new_a.clamp(0.0, 1.0))); }
        a = new_a;
    }
    Ok(StrykeValue::float(a.clamp(0.0, 1.0)))
}

/// Vaccination coverage required: 1 − 1/R₀ for sterilising vaccine.
fn builtin_vaccination_coverage_required(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    if r0 <= 1.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((1.0 - 1.0 / r0).clamp(0.0, 1.0)))
}

/// Case fatality rate: deaths / cases.
fn builtin_cfr_case_fatality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let deaths = f1(args);
    let cases = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(deaths / cases))
}

/// Infection fatality rate: deaths / total infections (incl. asymptomatic).
fn builtin_ifr_infection_fatality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let deaths = f1(args);
    let infections = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(deaths / infections))
}

/// DALYs = YLL + YLD, weighted by disability weight d. Returns YLD increment.
fn builtin_dalys_disability_weight(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let prevalence = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let duration_years = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(prevalence * d * duration_years))
}

/// QALY remaining = years_remaining · health_utility (0..1).
fn builtin_qaly_lifetime(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let yrs = f1(args);
    let utility = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).clamp(0.0, 1.0);
    Ok(StrykeValue::float(yrs * utility))
}

/// YLL (years of life lost): sum over deaths of (life_expectancy − age_at_death).
fn builtin_ylll_pml(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let ages = b61_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let life_expectancy = args.get(1).map(|v| v.to_number()).unwrap_or(80.0);
    let s: f64 = ages.iter().map(|&a| (life_expectancy - a).max(0.0)).sum();
    Ok(StrykeValue::float(s))
}

/// Effective reproduction R_t from serial interval distribution (Wallinga-Teunis).
/// Args: incidence array, mean serial interval w.
fn builtin_rt_serial_interval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let inc = b61_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(5.0).max(1e-9);
    let n = inc.len();
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let cur = inc[n - 1];
    let prior = inc[..n - 1].iter().sum::<f64>() / (n - 1) as f64;
    if prior <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((cur / prior).powf(w)))
}

/// Generation time step (gamma-distributed): mean increment.
fn builtin_generation_time_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mean = f1(args);
    Ok(StrykeValue::float(mean))
}

/// Gini coefficient for health-inequality (Lorenz-curve area). Args: sorted shares.
fn builtin_gini_inequality_health(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut shares = b61_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if shares.is_empty() { return Ok(StrykeValue::float(0.0)); }
    shares.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = shares.len() as f64;
    let total: f64 = shares.iter().sum();
    if total <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let cumulative: f64 = shares.iter().enumerate().map(|(i, &x)| (i as f64 + 1.0) * x).sum();
    Ok(StrykeValue::float((2.0 * cumulative / (n * total)) - (n + 1.0) / n))
}

/// Standardised mortality ratio: observed / expected.
fn builtin_standardized_mortality_smr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let observed = f1(args);
    let expected = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(observed / expected))
}

/// Indirect age-adjusted rate: (SMR × standard rate).
fn builtin_indirect_age_adjusted(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let smr = f1(args);
    let std_rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(smr * std_rate))
}

/// Direct age-adjusted: Σ w_i · r_i with reference population weights.
fn builtin_direct_age_adjusted(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rates = b61_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let weights = b61_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = rates.len().min(weights.len());
    let total_w: f64 = weights.iter().take(n).sum();
    if total_w <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| rates[i] * weights[i]).sum();
    Ok(StrykeValue::float(s / total_w))
}

/// Odds ratio for 2×2 contingency: (a·d) / (b·c).
fn builtin_odds_ratio_2x2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = b * c;
    if denom <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(a * d / denom))
}

/// Risk ratio: incidence_exposed / incidence_unexposed = (a/(a+b)) / (c/(c+d)).
fn builtin_risk_ratio_2x2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let p_exp = if a + b > 0.0 { a / (a + b) } else { return Ok(StrykeValue::float(f64::INFINITY)); };
    let p_un = if c + d > 0.0 { c / (c + d) } else { return Ok(StrykeValue::float(f64::INFINITY)); };
    if p_un <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(p_exp / p_un))
}

/// Number Needed to Treat: 1 / (CER − EER).
fn builtin_number_needed_to_treat(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cer = f1(args);
    let eer = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let arr = cer - eer;
    if arr <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / arr))
}

/// Population-attributable fraction: PAF = (R − R₀) / R for the unexposed risk R₀.
fn builtin_attributable_fraction_pop(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = f1(args);
    let r0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if r <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(((r - r0) / r).clamp(0.0, 1.0)))
}

/// Preventive (preventable) fraction: PF = (R₀ − R) / R₀.
fn builtin_preventive_fraction(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if r0 <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(((r0 - r) / r0).clamp(0.0, 1.0)))
}

/// Contact tracing effectiveness: (cases prevented / total cases). Args: traced
/// secondary cases blocked, total secondary cases.
fn builtin_contact_tracing_eff(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let blocked = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float((blocked / total).clamp(0.0, 1.0)))
}

/// Cluster attack rate: cases / exposed within outbreak cluster.
fn builtin_cluster_attack_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cases = f1(args);
    let exposed = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(cases / exposed))
}

/// Transmission pair index for case (i, j): max p with j on i's onset trajectory.
fn builtin_transmission_pair_index(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let onset_i = f1(args);
    let onset_j = args.get(1).map(|v| v.to_number()).unwrap_or(onset_i);
    let serial_mean = args.get(2).map(|v| v.to_number()).unwrap_or(5.0);
    let serial_sd = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(1e-9);
    let dt = onset_j - onset_i;
    let z = (dt - serial_mean) / serial_sd;
    Ok(StrykeValue::float((-z * z / 2.0).exp()))
}
