// Batch 73 — micro/macro economics, mechanism design, game theory, auctions,
// econometric estimators.

fn b73_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b73_to_ints(v: &PerlValue) -> Vec<i64> {
    arg_to_vec(v).iter().map(|x| x.to_number() as i64).collect()
}

/// Cobb-Douglas Y = A · K^α · L^β.
fn builtin_cobb_douglas(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(a * k.powf(alpha) * l.powf(beta)))
}

/// CES Y = A · (αK^ρ + (1-α)L^ρ)^(1/ρ).
fn builtin_ces_production(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let rho = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let inner = alpha * k.powf(rho) + (1.0 - alpha) * l.powf(rho);
    Ok(PerlValue::float(a * inner.powf(1.0 / rho)))
}

/// Leontief input requirement: x_i = a_ij · y_j summed.
fn builtin_leontief_input(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let y = args.get(1).map(b73_to_floats).unwrap_or_default();
    let n = a.len().min(y.len());
    let s: f64 = (0..n).map(|i| a[i] * y[i]).sum();
    Ok(PerlValue::float(s))
}

/// Leontief output: y = (I - A)⁻¹ d (1-D scalar form).
fn builtin_leontief_output(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(d / (1.0 - a).max(1e-300)))
}

/// Slutsky decomposition: dq = SE + IE = ∂q/∂p|U + (-q · ∂q/∂I).
fn builtin_slutsky_decompose(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dq_dp_u = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dq_di = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dq_dp_u - q * dq_di))
}

/// Marshallian demand for Cobb-Douglas: q* = α·I/p.
fn builtin_marshallian_demand(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let income = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let price = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(alpha * income / price))
}

/// Hicksian demand for CES: h_i = α^σ · u · (p_j / Σ α^σ p_j^{1-σ})^σ. Simplified
/// 1-good marshallian-equivalent at u.
fn builtin_hicksian_demand(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let utility = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(utility / p))
}

/// Expenditure function E(p, u) = u · p (one-good).
fn builtin_expenditure_function(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(u * p))
}

/// Indirect utility V(p, I) = I/p (one-good).
fn builtin_indirect_utility(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let income = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(income / p))
}

/// Gale-Shapley single proposal step: returns 1 if accepted (better partner found).
fn builtin_gale_shapley_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let proposer_rank = i1(args);
    let cur_rank = args.get(1).map(|v| v.to_number() as i64).unwrap_or(i64::MAX);
    Ok(PerlValue::integer(if proposer_rank < cur_rank { 1 } else { 0 }))
}

/// Deferred acceptance round count: at most n rounds for n-by-n market.
fn builtin_deferred_acceptance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n))
}

/// Top trading cycles: length of the directed cycle found from `start` in the
/// functional graph `next[i]` (favourite-good pointers). Args: `next`, optional
/// `start` (default `0`).
fn builtin_top_trading_cycle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nxt = b73_to_ints(args.first().unwrap_or(&PerlValue::array(vec![])));
    if nxt.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    let n = nxt.len();
    let start_raw = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let start = start_raw.rem_euclid(n as i64) as usize;
    let adj: Vec<usize> = nxt
        .iter()
        .map(|&x| (x.rem_euclid(n as i64) as usize).min(n.saturating_sub(1)))
        .collect();
    let mut slow = start;
    let mut fast = start;
    let mut hops = 0_usize;
    loop {
        slow = adj[slow];
        fast = adj[adj[fast]];
        hops += 1;
        if slow == fast {
            break;
        }
        if hops > n + n {
            return Ok(PerlValue::integer(0));
        }
    }
    let mut len = 1_usize;
    let mut p = adj[slow];
    while p != slow {
        len += 1;
        p = adj[p];
        if len > n {
            return Ok(PerlValue::integer(n as i64));
        }
    }
    Ok(PerlValue::integer(len as i64))
}

/// VCG payment: bidder pays externality = max-without-i  -  total-without-i-portion.
fn builtin_vcg_payment(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let max_without_i = f1(args);
    let value_assigned = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((max_without_i - value_assigned).max(0.0)))
}

/// Myerson optimal reservation price for uniform [0,1]: r = (1 - F(r))/f(r) → 1/2.
fn builtin_myerson_optimal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = f1(args);
    Ok(PerlValue::float(((1.0 + cost) / 2.0).clamp(0.0, 1.0)))
}

/// Gini for market shares.
fn builtin_gini_market(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut shares = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = shares.len() as f64;
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    shares.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let total: f64 = shares.iter().sum();
    if total <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let mut cum = 0.0;
    let mut sum = 0.0;
    for (i, &s) in shares.iter().enumerate() {
        cum += s;
        sum += cum - s / 2.0;
        let _ = i;
    }
    let g = 1.0 - 2.0 * sum / (n * total);
    Ok(PerlValue::float(g))
}

/// Herfindahl-Hirschman index Σ s_i².
fn builtin_hhi_concentration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(s.iter().map(|x| x * x).sum::<f64>()))
}

/// Cournot equilibrium quantity per firm: q* = (a - c) / ((n+1) · b).
fn builtin_cournot_eq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((a - c) / ((n + 1.0) * b)))
}

/// Stackelberg leader q*_L = (a-c)/2b.
fn builtin_stackelberg_eq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float((a - c) / (2.0 * b)))
}

/// Bertrand: in symmetric homogeneous goods, p = MC.
fn builtin_bertrand_eq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mc = f1(args);
    Ok(PerlValue::float(mc))
}

/// Monopoly Lerner index: L = (P - MC)/P = -1/ε.
fn builtin_monopoly_lerner(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let mc = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if p.abs() < 1e-300 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((p - mc) / p))
}

/// Consumer surplus on linear demand: ½ · (a - p) · q.
fn builtin_consumer_surplus(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * (a - p).max(0.0) * q))
}

/// Producer surplus: ½ · (p - mc_min) · q.
fn builtin_producer_surplus(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let mc = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * (p - mc).max(0.0) * q))
}

/// Deadweight loss = ½ · (q* - q_t) · (p_t - p*).
fn builtin_deadweight_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dq = f1(args);
    let dp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * dq.abs() * dp.abs()))
}

/// Tax incidence on consumers: e_s / (e_s + |e_d|).
fn builtin_tax_incidence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let es = f1(args);
    let ed = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).abs();
    let denom = (es + ed).max(1e-300);
    Ok(PerlValue::float(es / denom))
}

/// Pareto efficiency check: is allocation a∈A weakly dominated by another?
fn builtin_pareto_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let utilities = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let other = args.get(1).map(b73_to_floats).unwrap_or_default();
    let n = utilities.len().min(other.len());
    let dom = (0..n).all(|i| other[i] >= utilities[i]) && (0..n).any(|i| other[i] > utilities[i]);
    Ok(PerlValue::integer(if !dom { 1 } else { 0 }))
}

/// Edgeworth box allocation: contract curve point with α-share.
fn builtin_edgeworth_box_alloc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let omega = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(alpha * omega))
}

/// Utilitarian SWF: Σ u_i.
fn builtin_social_welfare_utilitarian(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(u.iter().sum()))
}

/// Rawlsian SWF: min u_i.
fn builtin_social_welfare_rawls(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(u.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Nash SWF: Π u_i.
fn builtin_social_welfare_nash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(u.iter().product()))
}

/// **Local** Independence of Irrelevant Alternatives: `1` iff the strict
/// comparison of `a` vs `b` (encoded as `−1` / `0` / `+1` or caller convention)
/// is unchanged when alternative `c` is absent — pass full-set comparison then
/// `{a,b}`-only comparison.
fn builtin_arrow_independence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cmp_with = i1(args);
    let cmp_without = args.get(1).map(|v| v.to_number() as i64).unwrap_or(cmp_with);
    Ok(PerlValue::integer(if cmp_with == cmp_without { 1 } else { 0 }))
}

/// Vickrey (2nd-price) auction payment = 2nd-highest bid.
fn builtin_vickrey_auction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut bids = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    bids.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(bids.get(1).copied().unwrap_or(0.0)))
}

/// First-price sealed bid: optimal bid (1 - 1/n)·v for uniform value distrib.
fn builtin_first_price_seal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let n = args.get(1).map(|x| x.to_number()).unwrap_or(2.0).max(2.0);
    Ok(PerlValue::float((1.0 - 1.0 / n) * v))
}

/// English (ascending) auction final price ≈ 2nd-highest valuation.
fn builtin_english_auction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_vickrey_auction(args)
}

/// Dutch (descending) auction = first-price equivalent.
fn builtin_dutch_auction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_first_price_seal(args)
}

/// ** Nonempty coalition count** for a set of `n` players: `2^n − 1` (subsets
/// other than ∅). *Not* a cooperative-game **core** non-emptiness test.
fn builtin_core_coalition(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0) as u32;
    if n == 0 {
        return Ok(PerlValue::integer(0));
    }
    if n >= 63 {
        return Ok(PerlValue::integer(i64::MAX));
    }
    Ok(PerlValue::integer((1_i64 << n) - 1))
}

/// Loose **upper bound** on the number of distinct stable matchings in an
/// `n×n` marriage market: `n!`.
fn builtin_stable_matching_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let mut acc = 1_i64;
    for k in 1..=n {
        acc = acc.saturating_mul(k);
    }
    Ok(PerlValue::integer(acc))
}

/// Gale–Shapley (**men-optimal** stable matching). Args: `n`, arrayref of `n`
/// men's permutations (woman indices), arrayref of `n` women's ordered lists of
/// men. Returns array: wife index for each man, or `−1` if alone.
fn builtin_gale_optimal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0) as usize;
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let men_outer = arg_to_vec(args.get(1).unwrap_or(&PerlValue::UNDEF));
    let women_outer = arg_to_vec(args.get(2).unwrap_or(&PerlValue::UNDEF));
    if men_outer.len() < n || women_outer.len() < n {
        return Ok(PerlValue::array(vec![]));
    }
    let mut men: Vec<Vec<usize>> = Vec::with_capacity(n);
    for m in 0..n {
        let row = arg_to_vec(&men_outer[m]);
        men.push(row.iter().map(|x| x.to_number() as usize).collect());
    }
    let mut w_rank: Vec<Vec<usize>> = vec![vec![usize::MAX; n]; n];
    for w in 0..n {
        let row = arg_to_vec(&women_outer[w]);
        for (r, cell) in row.iter().enumerate().take(n) {
            let mm = cell.to_number() as usize;
            if mm < n {
                w_rank[w][mm] = r;
            }
        }
    }
    let mut wife: Vec<Option<usize>> = vec![None; n];
    let mut husband: Vec<Option<usize>> = vec![None; n];
    let mut next_p = vec![0_usize; n];
    let mut free: std::collections::VecDeque<usize> = (0..n).collect();
    while let Some(m) = free.pop_front() {
        if next_p[m] >= men[m].len() {
            continue;
        }
        let w = men[m][next_p[m]];
        next_p[m] += 1;
        if w >= n {
            continue;
        }
        match husband[w] {
            None => {
                husband[w] = Some(m);
                wife[m] = Some(w);
            }
            Some(m0) => {
                let rw = &w_rank[w];
                if rw[m] < rw[m0] {
                    husband[w] = Some(m);
                    wife[m] = Some(w);
                    wife[m0] = None;
                    free.push_back(m0);
                } else {
                    free.push_back(m);
                }
            }
        }
    }
    let out: Vec<PerlValue> = (0..n)
        .map(|m| PerlValue::integer(wife[m].map(|w| w as i64).unwrap_or(-1)))
        .collect();
    Ok(PerlValue::array(out))
}

/// Pareto dominance: 1 if a dominates b component-wise.
fn builtin_pareto_dominance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = b73_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let b = args.get(1).map(b73_to_floats).unwrap_or_default();
    let n = a.len().min(b.len());
    let geq = (0..n).all(|i| a[i] >= b[i]);
    let strict = (0..n).any(|i| a[i] > b[i]);
    Ok(PerlValue::integer(if geq && strict { 1 } else { 0 }))
}

/// Lerner index (alias of monopoly).
fn builtin_lerner_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_monopoly_lerner(args)
}

/// Price elasticity ε = (dQ/dP)·(P/Q).
fn builtin_price_elasticity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dq_dp = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(dq_dp * p / q))
}

/// Supply elasticity (same form, different sign convention).
fn builtin_supply_elasticity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_price_elasticity(args)
}

/// Income elasticity ε_I = (∂Q/∂I)·(I/Q).
fn builtin_income_elasticity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dq_di = f1(args);
    let income = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(dq_di * income / q))
}

/// Engel curve point Q(I): power form Q = A · I^β.
fn builtin_engel_curve(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let income = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a * income.powf(beta)))
}

/// Cross-elasticity ε_{xy} = (∂Q_x/∂P_y)·(P_y/Q_x).
fn builtin_cross_elasticity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dqx_dpy = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(dqx_dpy * py / qx))
}

/// Difference-in-differences estimator: (Y₁₁ - Y₁₀) - (Y₀₁ - Y₀₀).
fn builtin_diff_in_diff(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y11 = f1(args);
    let y10 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y01 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y00 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((y11 - y10) - (y01 - y00)))
}

/// DiD estimator alias.
fn builtin_did_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_diff_in_diff(args)
}

/// RDD: difference of estimated outcomes at the cutoff.
fn builtin_rdd_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_above = f1(args);
    let y_below = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(y_above - y_below))
}
