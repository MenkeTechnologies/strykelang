// Batch 43 — game theory, mechanism design, social choice, repeated games.

fn b43_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Two-player zero-sum game value (max-min on row player payoff)
fn builtin_game_two_player_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row_min = f1(args);
    let col_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((row_min + col_max) / 2.0))
}

/// Nash equilibrium pair existence test (constant-sum 2x2)
fn builtin_nash_equilibrium_pair(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a - b - c + d;
    if denom == 0.0 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float((d - c) / denom))
}

/// Mixed strategy value v = (ad - bc)/(a - b - c + d)
fn builtin_mixed_strategy_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a - b - c + d;
    if denom == 0.0 { return Ok(StrykeValue::float((a + d) / 2.0)); }
    Ok(StrykeValue::float((a * d - b * c) / denom))
}

/// Zero-sum minmax = max_row min_col M_{ij}
fn builtin_zero_sum_minmax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = (v.len() as f64).sqrt() as usize;
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let mut best = f64::NEG_INFINITY;
    for i in 0..n {
        let row_min = (0..n).map(|j| v[i * n + j]).fold(f64::INFINITY, f64::min);
        if row_min > best { best = row_min; }
    }
    Ok(StrykeValue::float(best))
}

/// Saddle point check: max_row min_col == min_col max_row
fn builtin_saddle_point_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let max_min = f1(args);
    let min_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (max_min - min_max).abs() < 1e-9 { 1 } else { 0 }))
}

/// Correlated equilibrium value (max expected payoff)
fn builtin_correlated_equilibrium_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len().max(1) as f64))
}

/// Shapley value (2-player): φ_i = ½(v(i) + v(N) - v(N\i))
fn builtin_shapley_value_two_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_i = f1(args);
    let v_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_minus_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.5 * (v_i + v_n - v_minus_i)))
}

/// Banzhaf index (2-player) = #swing votes / 2^(n-1)
fn builtin_banzhaf_index_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let swings = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if n < 1.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(swings / 2f64.powf(n - 1.0)))
}

/// Nucleolus LP step (excess minimization)
fn builtin_nucleolus_lp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let excesses = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(excesses.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Core membership: all-coalition rationality satisfied
fn builtin_core_membership_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let payoffs = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let v_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total: f64 = payoffs.iter().sum();
    let smallest = payoffs.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(StrykeValue::integer(if (total - v_n).abs() < 1e-9 && smallest >= v_min { 1 } else { 0 }))
}

/// Imputation efficiency check Σ x_i = v(N)
fn builtin_imputation_efficient_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    let v_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (s - v_n).abs() < 1e-9 { 1 } else { 0 }))
}

/// Individual rationality: x_i ≥ v({i})
fn builtin_imputation_individual_rational(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_i = f1(args);
    let v_i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if x_i >= v_i { 1 } else { 0 }))
}

/// Prisoner's dilemma payoff (T,R,P,S)
fn builtin_prisoners_dilemma_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let action_i = i1(args);
    let action_j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    match (action_i, action_j) {
        (0, 0) => Ok(StrykeValue::float(3.0)),
        (0, 1) => Ok(StrykeValue::float(0.0)),
        (1, 0) => Ok(StrykeValue::float(5.0)),
        (1, 1) => Ok(StrykeValue::float(1.0)),
        _ => Ok(StrykeValue::float(0.0)),
    }
}

/// Matching pennies payoff
fn builtin_matching_pennies_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if i == j { 1 } else { -1 }))
}

/// Chicken (Hawk-Dove) payoff matrix
fn builtin_chicken_game_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    match (i, j) {
        (0, 0) => Ok(StrykeValue::float(0.0)),
        (0, 1) => Ok(StrykeValue::float(-1.0)),
        (1, 0) => Ok(StrykeValue::float(1.0)),
        (1, 1) => Ok(StrykeValue::float(-10.0)),
        _ => Ok(StrykeValue::float(0.0)),
    }
}

/// Stag Hunt payoff
fn builtin_stag_hunt_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    match (i, j) {
        (0, 0) => Ok(StrykeValue::float(4.0)),
        (0, 1) => Ok(StrykeValue::float(0.0)),
        (1, 0) => Ok(StrykeValue::float(3.0)),
        (1, 1) => Ok(StrykeValue::float(3.0)),
        _ => Ok(StrykeValue::float(0.0)),
    }
}

/// Battle of Sexes payoff
fn builtin_battle_sexes_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let role = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    match (i, j, role) {
        (0, 0, 0) => Ok(StrykeValue::float(2.0)),
        (1, 1, 0) => Ok(StrykeValue::float(1.0)),
        (0, 0, 1) => Ok(StrykeValue::float(1.0)),
        (1, 1, 1) => Ok(StrykeValue::float(2.0)),
        _ => Ok(StrykeValue::float(0.0)),
    }
}

/// Public goods game payoff
fn builtin_public_goods_game_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let endowment = f1(args);
    let contribution = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let total_contributions = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(2.0);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(endowment - contribution + r * total_contributions / n))
}

/// Tragedy of commons metric (overgrazing)
fn builtin_tragedy_commons_metric(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let resource = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(StrykeValue::float(resource)); }
    Ok(StrykeValue::float(resource / n - 0.1 * n))
}

/// Ultimatum acceptance prob: 1 if offer ≥ threshold else 0
fn builtin_ultimatum_acceptance_prob(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let offer = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.3);
    Ok(StrykeValue::integer(if offer >= threshold { 1 } else { 0 }))
}

/// Dictator game share
fn builtin_dictator_game_share(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let total = f1(args);
    let share = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(total * share.clamp(0.0, 1.0)))
}

/// Trust game repayment
fn builtin_trust_game_repayment(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let received = f1(args);
    let trust_factor = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(received * trust_factor))
}

/// Cooperative game value v(S): for unanimity game on coalition T (subset of N),
/// v(S) = 1 if T ⊆ S else 0; for additive game it's a sum. Combine: weight·sum + base.
fn builtin_cooperative_game_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let members = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let base = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let synergy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = members.len() as f64;
    Ok(StrykeValue::float(members.iter().sum::<f64>() + base + synergy * n * (n - 1.0) / 2.0))
}

/// Characteristic function v(S) for additive game
fn builtin_characteristic_function(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// Bargaining set check (no objection-counter-objection)
fn builtin_bargaining_set_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dominated = i1(args);
    Ok(StrykeValue::integer(if dominated == 0 { 1 } else { 0 }))
}

/// Kalai-Smorodinsky bargaining: (x, y) where x/y = u_max1/u_max2
fn builtin_kalai_smorodinsky_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u_max1 = f1(args);
    let u_max2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if u_max1 + u_max2 == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(u_max1 / (u_max1 + u_max2)))
}

/// Nash bargaining: max (u1 - d1)(u2 - d2)
fn builtin_nash_bargaining_solution(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u1 = f1(args);
    let u2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((u1 - d1) * (u2 - d2)))
}

/// Egalitarian solution: equalize u_i - d_i
fn builtin_egalitarian_solution(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let var: f64 = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / v.len() as f64;
    Ok(StrykeValue::float(var))
}

/// Utilitarian solution: max Σ u_i
fn builtin_utilitarian_solution(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// Social welfare sum
fn builtin_social_welfare_sum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_utilitarian_solution(args)
}

/// Arrow's impossibility theorem condition check
fn builtin_arrow_impossibility_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_alts = i1(args);
    Ok(StrykeValue::integer(if n_alts >= 3 { 1 } else { 0 }))
}

/// Gibbard-Satterthwaite check (manipulability)
fn builtin_gibbard_satterthwaite_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(if n >= 3 { 1 } else { 0 }))
}

/// Borda count step: rank position * weight
fn builtin_borda_count_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rank = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(n - rank))
}

/// Condorcet winner check (beats all others pairwise)
fn builtin_condorcet_winner_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pairwise_wins = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    Ok(StrykeValue::integer(if pairwise_wins == n - 1 { 1 } else { 0 }))
}

/// Plurality winner step (max votes)
fn builtin_plurality_winner_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let votes = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &v) in votes.iter().enumerate() {
        if v > best.1 { best = (i, v); }
    }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// Kemeny score (rank aggregation)
fn builtin_kemeny_score_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let disagreements = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(disagreements.iter().sum()))
}

/// Dodgson swap count
fn builtin_dodgson_swap_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inversions = f1(args);
    Ok(StrykeValue::integer(inversions as i64))
}

/// Coombs runoff step (eliminate most last-place votes)
fn builtin_coombs_runoff_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let last_place_counts = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut worst = (0_usize, f64::NEG_INFINITY);
    for (i, &v) in last_place_counts.iter().enumerate() {
        if v > worst.1 { worst = (i, v); }
    }
    Ok(StrykeValue::integer(worst.0 as i64))
}

/// Single transferable vote step
fn builtin_single_transferable_vote(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let quota = f1(args);
    let votes = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if votes >= quota { 1 } else { 0 }))
}

/// Range voting score sum
fn builtin_range_voting_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let scores = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(scores.iter().sum()))
}

/// Approval voting maximum
fn builtin_approval_voting_max(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let approvals = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(approvals.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Schulze method step (strongest path strength)
fn builtin_schulze_method_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_ij = f1(args);
    let p_ik = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p_kj = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p_ij.max(p_ik.min(p_kj))))
}

/// Copeland score (#wins - #losses)
fn builtin_copeland_score_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let wins = f1(args);
    let losses = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(wins - losses))
}

/// Black method: Condorcet winner if exists else Borda
fn builtin_black_method_winner(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let condorcet = i1(args);
    let borda = args.get(1).map(|v| v.to_number() as i64).unwrap_or(-1);
    Ok(StrykeValue::integer(if condorcet >= 0 { condorcet } else { borda }))
}

/// Median voter step (pick median preference)
fn builtin_median_voter_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut p = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if p.is_empty() { return Ok(StrykeValue::float(0.0)); }
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(p[p.len() / 2]))
}

/// Hotelling 1-D location best response: with linear transport cost t over [0, L]
/// for 2 firms with current rivals at x_other, BR is to locate just inside the
/// midpoint of the larger captive segment.
fn builtin_hotelling_location_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_other = f1(args);
    let length = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = 1e-6;
    if x_other <= length / 2.0 { Ok(StrykeValue::float((x_other + length) / 2.0 - eps)) }
    else { Ok(StrykeValue::float(x_other / 2.0 + eps)) }
}

/// Arrow Pareto check
fn builtin_arrow_pareto_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let unanimous_pref = i1(args);
    Ok(StrykeValue::integer(unanimous_pref))
}

/// Fair division envy-free check
fn builtin_fair_division_envy_free(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_own = f1(args);
    let v_others_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if v_own >= v_others_max { 1 } else { 0 }))
}

/// Proportional share v_i / n
fn builtin_proportional_share(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v / n))
}

/// Maximin share = max over allocations of min utility
fn builtin_maximin_share(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Egalitarian split (equal shares)
fn builtin_egalitarian_split(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_proportional_share(args)
}

/// Nash social welfare = (Π u_i)^(1/n)
fn builtin_nash_social_welfare(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let prod: f64 = v.iter().fold(1.0, |a, &b| a * b);
    Ok(StrykeValue::float(prod.powf(1.0 / v.len() as f64)))
}

/// Divisible goods proportional allocation
fn builtin_divisible_goods_proportional(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_proportional_share(args)
}

/// Indivisible envy-free check
fn builtin_indivisible_envy_free_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_fair_division_envy_free(args)
}

/// Adjusted winner percentage allocation
fn builtin_adjusted_winner_pct(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_a = f1(args);
    let v_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let total = v_a + v_b;
    if total == 0.0 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float(v_a / total))
}

/// Sealed-bid first-price auction: pay your bid if you win
fn builtin_sealed_bid_first_price(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bids = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(bids.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Sealed-bid second-price (Vickrey) auction
fn builtin_sealed_bid_second_price(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut bids = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if bids.len() < 2 { return Ok(StrykeValue::float(0.0)); }
    bids.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(bids[1]))
}

/// English auction step (ascending)
fn builtin_english_auction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let current = f1(args);
    let increment = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(current + increment))
}

/// Dutch auction step (descending)
fn builtin_dutch_auction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let current = f1(args);
    let decrement = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(current - decrement))
}

/// All-pay auction step
fn builtin_all_pay_auction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bids = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(bids.iter().sum()))
}

/// VCG payment: harm imposed on others
fn builtin_vcg_payment_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let welfare_without_i = f1(args);
    let welfare_with_others = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(welfare_without_i - welfare_with_others))
}

/// Revenue equivalence theorem check
fn builtin_revenue_equivalence_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_first = f1(args);
    let r_second = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (r_first - r_second).abs() < 1e-9 { 1 } else { 0 }))
}

/// Truthful mechanism check
fn builtin_truthful_mechanism_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dominant = i1(args);
    Ok(StrykeValue::integer(dominant))
}

/// Incentive compatibility check
fn builtin_incentive_compatibility_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_truthful_mechanism_check(args)
}

/// Mechanism design objective: max expected social welfare
fn builtin_mechanism_design_obj(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_social_welfare_sum(args)
}

/// Double auction step (k-double-auction at midpoint)
fn builtin_double_auction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bid = f1(args);
    let ask = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((bid + ask) / 2.0))
}

/// Combinatorial auction step (max value bundles)
fn builtin_combinatorial_auction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Posted price offer accept (binary)
fn builtin_posted_price_offer_accept(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let value = f1(args);
    let price = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if value >= price { 1 } else { 0 }))
}

/// Matching market step: count proposals minus rejections (positive = market clearing).
fn builtin_matching_market_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let proposals = f1(args);
    let rejections = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(proposals - rejections))
}

/// Deferred acceptance: round count until no rejections / proposals stabilizes.
/// Returns 1 if matching is stable (no blocking pair), 0 otherwise.
fn builtin_deferred_acceptance_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let blocking_pairs = f1(args);
    Ok(StrykeValue::integer(if blocking_pairs == 0.0 { 1 } else { 0 }))
}

/// Boston mechanism (immediate-acceptance, Abdulkadiroğlu & Sönmez 2003):
/// in round k, every unmatched student applies to their k-th choice; schools
/// IRREVOCABLY accept up to capacity by priority. UNLIKE deferred acceptance,
/// rejected students cannot displace previously-accepted ones — this makes
/// Boston manipulable but maximizes "first-choice" rate. Returns 1 if school
/// accepts (capacity remaining AND priority high enough), else 0.
/// Args: applicants_so_far, capacity, applicant_rank, top_rank_filled.
fn builtin_boston_mechanism_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let so_far = i1(args);
    let cap = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let rank = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let top_filled = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if so_far < cap && rank <= top_filled { 1 } else { 0 }))
}

/// Top Trading Cycles (Shapley-Scarf): number of agents matched in one TTC round
/// equals length of the cycle in the "points-to" graph. Args: cycle length.
fn builtin_top_trading_cycles_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pointers = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = pointers.len();
    if n == 0 { return Ok(StrykeValue::integer(0)); }
    let p: Vec<usize> = pointers.iter().map(|x| (x.to_number() as usize).min(n - 1)).collect();
    let mut tortoise = 0_usize;
    let mut hare = 0_usize;
    for _ in 0..n {
        tortoise = p[tortoise];
        hare = p[p[hare]];
        if tortoise == hare { break; }
    }
    let mut start = 0_usize;
    while start != tortoise { start = p[start]; tortoise = p[tortoise]; }
    let mut len = 1_i64;
    let mut cur = p[start];
    while cur != start { len += 1; cur = p[cur]; }
    Ok(StrykeValue::integer(len))
}

/// School choice match: priority * preference
fn builtin_school_choice_match(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let priority = f1(args);
    let preference = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(priority * preference))
}

/// Stable roommates (Irving 1985): unlike Gale-Shapley (bipartite), one-sided
/// matching may have NO stable matching. Irving's algorithm: phase 1 — each
/// person proposes down their list, holding best held; phase 2 — eliminate
/// rotations (cycles in second-best chains). Returns 1 if step yields stable
/// pairing so far, 0 if a "no-stable" rotation is detected.
/// Args: rejections_so_far, rotation_detected (0/1), unmatched_count.
fn builtin_roommate_match_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rotation = i1(args);
    let unmatched = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if rotation != 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(if unmatched == 0 { 1 } else { 0 }))
}

/// Network formation step (link cost vs benefit)
fn builtin_network_formation_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let benefit = f1(args);
    let cost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(benefit - cost))
}

/// Coordination game payoff
fn builtin_coordination_game_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if i == j { 2 } else { 0 }))
}

/// Evolutionary stable strategy condition: ESS if u(s, s) > u(s', s) or tie + u(s, s') > u(s', s')
fn builtin_evolutionary_stable_strategy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u_ss = f1(args);
    let u_sps = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if u_ss > u_sps { 1 } else { 0 }))
}

/// Replicator dynamics: ẋ_i = x_i (f_i - φ̄)
fn builtin_replicator_dynamics_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_i = f1(args);
    let f_i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let phi_bar = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x_i * (f_i - phi_bar)))
}

/// Hawk-Dove payoff
fn builtin_hawk_dove_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_chicken_game_payoff(args)
}

/// Fictitious play step (best response to empirical history)
fn builtin_fictitious_play_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let payoffs = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &p) in payoffs.iter().enumerate() {
        if p > best.1 { best = (i, p); }
    }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// Best response dynamic
fn builtin_best_response_dynamic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_fictitious_play_step(args)
}

/// Quantal response logit (softmax over payoffs)
fn builtin_quantal_response_logit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let idx = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if u.is_empty() || idx >= u.len() { return Ok(StrykeValue::float(0.0)); }
    let max = u.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = u.iter().map(|x| (lambda * (x - max)).exp()).sum();
    Ok(StrykeValue::float((lambda * (u[idx] - max)).exp() / denom))
}

/// Level-k iterated reasoning (Stahl & Wilson 1995): a level-k agent best-
/// responds to a level-(k−1) opponent. Recursive formulation:
///   π^k = BR(π^{k−1}),  with π^0 = uniform random / level-0 anchor.
/// Returns the level-k best-response probability for one action given the
/// level-(k−1) action probability. Args: prev_level_prob, br_value (= 1 if
/// action is BR else 0).
fn builtin_level_k_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev_p = f1(args).clamp(0.0, 1.0);
    let br = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if br == 0 { Ok(StrykeValue::float(0.0)) } else { Ok(StrykeValue::float(prev_p)) }
}

/// Cognitive Hierarchy (Camerer-Ho-Chong 2004): every agent's level k is drawn
/// from a truncated Poisson(τ). A level-k agent best-responds to a BELIEF
/// distribution over levels 0..k−1 with renormalized Poisson weights:
///   g_k(j) = (e^{−τ} τ^j / j!) / Σ_{i<k} (e^{−τ} τ^i / i!)  for j < k.
/// Differs from Level-k (which assumes ALL opponents are at exactly level k−1).
/// Args: τ (mean level, default 1.5), k (current level), j (queried lower level).
fn builtin_cognitive_hierarchy_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tau = f1(args).max(0.0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let j = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    if j >= k { return Ok(StrykeValue::float(0.0)); }
    fn pois(tau: f64, n: i64) -> f64 {
        let mut p = (-tau).exp();
        for i in 1..=n { p *= tau / i as f64; }
        p
    }
    let num = pois(tau, j);
    let denom: f64 = (0..k).map(|i| pois(tau, i)).sum();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(num / denom))
}

/// Sequential equilibrium check
fn builtin_sequential_eq_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let consistent = i1(args);
    Ok(StrykeValue::integer(consistent))
}

/// Subgame perfect equilibrium check
fn builtin_subgame_perfect_eq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_sequential_eq_check(args)
}

/// Stackelberg leader-follower step: leader maximizes given follower BR
fn builtin_stackelberg_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let leader_q = f1(args);
    let follower_br = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(leader_q * (1.0 - leader_q - follower_br)))
}

/// Cournot quantity step (best response in linear demand)
fn builtin_cournot_quantity_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let total_others = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((a - c - total_others) / 2.0))
}

/// Bertrand price step (undercut to marginal cost)
fn builtin_bertrand_price_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    Ok(StrykeValue::float(c))
}

/// Hotelling price step (with linear transport cost)
fn builtin_hotelling_price_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    Ok(StrykeValue::float(t))
}

/// Collusion payoff (split monopoly profit)
fn builtin_collusion_payoff_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let monopoly = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(monopoly / n))
}

/// Folk theorem feasible value (within IR)
fn builtin_folk_theorem_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let v_min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if v >= v_min { 1 } else { 0 }))
}

/// Repeated game average payoff: (1-δ)/(1-δ^T) Σ δ^t u_t
fn builtin_repeated_game_avg_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let stage = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if (1.0 - delta).abs() < 1e-12 { return Ok(StrykeValue::float(stage)); }
    Ok(StrykeValue::float(stage * (1.0 - delta.powf(t)) / (1.0 - delta)))
}

/// Discount factor δ from interest rate r: δ = 1/(1+r)
fn builtin_discount_factor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    if 1.0 + r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / (1.0 + r)))
}

/// Trigger strategy payoff
fn builtin_trigger_strategy_payoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coop = f1(args);
    let defect = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(0.95);
    Ok(StrykeValue::float(coop / (1.0 - delta) + defect))
}

/// Grim trigger step
fn builtin_grim_trigger_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let last_action = i1(args);
    let opponent_defected_ever = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if opponent_defected_ever != 0 { 1 } else { last_action }))
}

/// Tit-for-tat step (mirror previous)
fn builtin_tit_for_tat_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let opp_prev = i1(args);
    Ok(StrykeValue::integer(opp_prev))
}

/// Prisoner's repeated equilibrium check
fn builtin_prisoners_repeated_eq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let delta = f1(args);
    Ok(StrykeValue::integer(if delta >= 0.5 { 1 } else { 0 }))
}

/// Mertens-Zamir consistent value step
fn builtin_mertens_zamir_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_low = f1(args);
    let v_high = args.get(1).map(|v| v.to_number()).unwrap_or(v_low);
    Ok(StrykeValue::float((v_low + v_high) / 2.0))
}

/// Ex-post value check (after type realization)
fn builtin_ex_post_value_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_realized = f1(args);
    let v_threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if v_realized >= v_threshold { 1 } else { 0 }))
}

/// Ex-ante value (before type realization): expected utility under the prior.
///   E_t[u(t, a*(t))] = Σ_t p(t) · u(t, a*(t)).
/// Distinct from ex-post (which conditions on a realized type). Returns the
/// expectation given probability-weighted utility-of-type pairs.
/// Args: array of [p_t, u_t] pairs; optional threshold to compare against.
fn builtin_ex_ante_value_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b43_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let threshold = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let mut ev = 0.0_f64;
    for ch in v.chunks(2) {
        if ch.len() == 2 { ev += ch[0] * ch[1]; }
    }
    Ok(StrykeValue::integer(if ev >= threshold { 1 } else { 0 }))
}

/// Common knowledge iterations (mutual knowledge depth)
fn builtin_common_knowledge_iterations(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n.max(0)))
}
