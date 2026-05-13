// sports analytics: rating systems (Elo, Glicko, TrueSkill), baseball
// sabermetric (wOBA, FIP, BABIP, WPA), hockey (Corsi, Fenwick), basketball/football
// summary metrics. Formulas from Glickman, Herbrich-Minka, FanGraphs guides.

fn b56_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Elo expected score: E = 1 / (1 + 10^((Rb − Ra) / 400)).
fn builtin_elo_expected(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_a = f1(args);
    let r_b = args.get(1).map(|v| v.to_number()).unwrap_or(r_a);
    Ok(StrykeValue::float(1.0 / (1.0 + 10f64.powf((r_b - r_a) / 400.0))))
}

/// Elo update: Ra' = Ra + K (S − E), where S ∈ {0, 0.5, 1}.
fn builtin_elo_update(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_a = f1(args);
    let r_b = args.get(1).map(|v| v.to_number()).unwrap_or(r_a);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(32.0);
    let e = 1.0 / (1.0 + 10f64.powf((r_b - r_a) / 400.0));
    Ok(StrykeValue::float(r_a + k * (s - e)))
}

/// Glicko-1 rating step. RD' = sqrt((1/RD² + 1/d²)⁻¹), where d² is the variance
/// estimate from opponents. Args: rating, RD, opp_rating, opp_RD, score.
fn builtin_glicko_rating(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let rd = args.get(1).map(|v| v.to_number()).unwrap_or(350.0);
    let r_op = args.get(2).map(|v| v.to_number()).unwrap_or(r);
    let rd_op = args.get(3).map(|v| v.to_number()).unwrap_or(350.0);
    let s = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let q = 10f64.ln() / 400.0;
    let g = 1.0 / (1.0 + 3.0 * q * q * rd_op * rd_op / (std::f64::consts::PI * std::f64::consts::PI)).sqrt();
    let e = 1.0 / (1.0 + 10f64.powf(-g * (r - r_op) / 400.0));
    let d2 = 1.0 / (q * q * g * g * e * (1.0 - e));
    let new_r = r + (q / (1.0 / (rd * rd) + 1.0 / d2)) * g * (s - e);
    Ok(StrykeValue::float(new_r))
}

/// TrueSkill rating update (1v1, no draw). μ' = μ + σ²/c · v(t, ε).
fn builtin_trueskill_update(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(8.333);
    let mu_op = args.get(2).map(|v| v.to_number()).unwrap_or(mu);
    let sigma_op = args.get(3).map(|v| v.to_number()).unwrap_or(8.333);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(4.166);
    let c = (2.0 * beta * beta + sigma * sigma + sigma_op * sigma_op).sqrt();
    let t = (mu - mu_op) / c;
    let v_pdf = (-t * t / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let v_cdf = 0.5 * (1.0 + libm::erf(t / std::f64::consts::SQRT_2));
    let v = if v_cdf > 1e-12 { v_pdf / v_cdf } else { 0.0 };
    Ok(StrykeValue::float(mu + sigma * sigma / c * v))
}

/// TrueSkill match quality q ≈ 2β / sqrt(c²) · exp(−(μ_a − μ_b)² / (2c²)).
fn builtin_trueskill_match_quality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu_a = f1(args);
    let sigma_a = args.get(1).map(|v| v.to_number()).unwrap_or(8.333);
    let mu_b = args.get(2).map(|v| v.to_number()).unwrap_or(mu_a);
    let sigma_b = args.get(3).map(|v| v.to_number()).unwrap_or(8.333);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(4.166);
    let c2 = 2.0 * beta * beta + sigma_a * sigma_a + sigma_b * sigma_b;
    let prefac = (2.0 * beta * beta / c2).sqrt();
    let kernel = (-(mu_a - mu_b).powi(2) / (2.0 * c2)).exp();
    Ok(StrykeValue::float(prefac * kernel))
}

/// Pythagorean expectation: W% = R^x / (R^x + RA^x). x = 1.83 for MLB.
fn builtin_pythagorean_expectation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let runs_for = f1(args).max(0.0);
    let runs_against = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let exponent = args.get(2).map(|v| v.to_number()).unwrap_or(1.83);
    let rf = runs_for.powf(exponent);
    let ra = runs_against.powf(exponent);
    Ok(StrykeValue::float(rf / (rf + ra)))
}

/// WAR scaffold: WAR = (player_runs - replacement_runs) / runs_per_win.
fn builtin_war_above_replacement(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let player_runs = f1(args);
    let repl_runs = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let runs_per_win = args.get(2).map(|v| v.to_number()).unwrap_or(10.0).max(1e-9);
    Ok(StrykeValue::float((player_runs - repl_runs) / runs_per_win))
}

/// wOBA (weighted on-base average): linear weight per offensive event.
/// Standard 2020 weights: BB=0.69, HBP=0.72, 1B=0.88, 2B=1.247, 3B=1.578, HR=2.031.
/// Args: array [BB, HBP, 1B, 2B, 3B, HR], plate appearances (PA).
fn builtin_woba_weight(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b56_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let pa = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1.0);
    let weights = [0.69, 0.72, 0.88, 1.247, 1.578, 2.031];
    let sum: f64 = v.iter().zip(weights.iter()).map(|(x, w)| x * w).sum();
    Ok(StrykeValue::float(sum / pa))
}

/// wRC+ = ((wRAA / PA + lgR/PA) + (lgR/PA - parkFactor·lgR/PA)) / lgwRC/PA · 100.
/// Simplified: 100·(wOBA / leagueWOBA) adjusted by park factor.
fn builtin_wrc_plus(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let woba = f1(args);
    let lg_woba = args.get(1).map(|v| v.to_number()).unwrap_or(0.32).max(1e-6);
    let park_factor = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(100.0 * (woba / lg_woba) / park_factor))
}

/// OPS+ = 100·(OBP/lgOBP + SLG/lgSLG − 1) / parkFactor.
fn builtin_ops_plus(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let obp = f1(args);
    let slg = args.get(1).map(|v| v.to_number()).unwrap_or(0.4);
    let lg_obp = args.get(2).map(|v| v.to_number()).unwrap_or(0.32).max(1e-6);
    let lg_slg = args.get(3).map(|v| v.to_number()).unwrap_or(0.4).max(1e-6);
    let pf = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    Ok(StrykeValue::float(100.0 * (obp / lg_obp + slg / lg_slg - 1.0) / pf))
}

/// ERA+ = 100·lgERA / (ERA · parkFactor).
fn builtin_era_plus(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let era = f1(args).max(1e-6);
    let lg_era = args.get(1).map(|v| v.to_number()).unwrap_or(4.0);
    let pf = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    Ok(StrykeValue::float(100.0 * lg_era / (era * pf)))
}

/// FIP = ((13·HR + 3·(BB+HBP) − 2·K) / IP) + lgFIPconst. Constants per FanGraphs.
fn builtin_fip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hr = f1(args);
    let bb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let hbp = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let ip = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    let fip_const = args.get(5).map(|v| v.to_number()).unwrap_or(3.10);
    Ok(StrykeValue::float((13.0 * hr + 3.0 * (bb + hbp) - 2.0 * k) / ip + fip_const))
}

/// xFIP: same as FIP but normalises HR rate to league-average HR/FB ratio.
/// Args: FB, BB, HBP, K, IP, lgHR/FB, fipConst.
fn builtin_xfip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let fb = f1(args);
    let bb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let hbp = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let ip = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    let lg_hr_fb = args.get(5).map(|v| v.to_number()).unwrap_or(0.105);
    let fip_const = args.get(6).map(|v| v.to_number()).unwrap_or(3.10);
    Ok(StrykeValue::float((13.0 * fb * lg_hr_fb + 3.0 * (bb + hbp) - 2.0 * k) / ip + fip_const))
}

/// SIERA (Skill-Interactive ERA): SIERA = a + b1 (SO/PA) + b2 (BB/PA)
/// + b3 (GB/(GB+FB)) + b4 cross-terms (Swartz). Coefficients for 2010+.
fn builtin_siera(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let so_pa = f1(args);
    let bb_pa = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gb_rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.4);
    let a = 6.157;
    Ok(StrykeValue::float(a - 16.5 * so_pa + 11.4 * bb_pa - 1.087 * gb_rate
        + 7.85 * so_pa * bb_pa - 1.62 * gb_rate * gb_rate))
}

/// BABIP = (H − HR) / (AB − K − HR + SF).
fn builtin_babip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let hr = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ab = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let sf = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = ab - k - hr + sf;
    if denom <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((h - hr) / denom))
}

/// WPA (Win Probability Added) = WP_after - WP_before.
fn builtin_wpa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let wp_after = f1(args);
    let wp_before = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(wp_after - wp_before))
}

/// Win probability via run-differential logistic model (simple).
fn builtin_win_probability(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lead = f1(args);
    let outs_remaining = args.get(1).map(|v| v.to_number()).unwrap_or(27.0);
    let scale = (outs_remaining / 27.0).sqrt() * 1.5;
    Ok(StrykeValue::float(1.0 / (1.0 + (-lead / scale).exp())))
}

/// Leverage Index per Tango: standard deviation of WPA in current state, divided
/// by average game-state σ. We compute the ratio given (sigma_state, sigma_avg).
fn builtin_leverage_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma_state = f1(args);
    let sigma_avg = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(sigma_state / sigma_avg))
}

/// Clutch score (FanGraphs): WPA/LI − WPA. Higher = clutch performer.
fn builtin_clutch_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let wpa = f1(args);
    let li_avg = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let wpa_per_li = args.get(2).map(|v| v.to_number()).unwrap_or(wpa);
    Ok(StrykeValue::float(wpa_per_li / li_avg - wpa))
}

/// Shooting percentage (basketball/hockey): made / attempted.
fn builtin_shooting_pct(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let made = f1(args);
    let att = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(made / att))
}

/// Save percentage (hockey): saves / shots.
fn builtin_save_pct(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let saves = f1(args);
    let shots = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(saves / shots))
}

/// Corsi-For: shot attempts (SOG + missed + blocked) per team. Args: sog, miss,
/// blocked.
fn builtin_corsi_for(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sog = f1(args);
    let miss = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let blocked = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(sog + miss + blocked))
}

/// Fenwick-For: shots-on-goal + missed (excludes blocked).
fn builtin_fenwick_for(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sog = f1(args);
    let miss = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(sog + miss))
}

/// Goals above average: skater goal-share above 50% of team's even-strength goals.
fn builtin_goals_above_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let player_goals = f1(args);
    let team_goals = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let exp_share = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    Ok(StrykeValue::float(player_goals - exp_share * team_goals))
}

/// Tackle efficiency: tackles / total_tackle_attempts.
fn builtin_tackle_efficiency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let made = f1(args);
    let att = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(made / att))
}

/// Yards per attempt (NFL passing): yards / attempts.
fn builtin_yards_per_attempt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let yards = f1(args);
    let att = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(yards / att))
}

/// QBR (ESPN composite, simplified). 0–100 scale.
fn builtin_qbr_metric(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let comp_pct = f1(args);
    let ypa = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let td_rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let int_rate = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let raw = (comp_pct - 0.30) * 5.0
        + (ypa - 3.0) * 0.25
        + (td_rate - 0.05) * 20.0
        - (int_rate - 0.05) * 25.0
        + 2.375;
    Ok(StrykeValue::float((raw * 100.0 / 6.0).clamp(0.0, 100.0)))
}

/// EPA per play: average expected points added across an array of EPA values.
fn builtin_epa_per_play(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b56_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}
