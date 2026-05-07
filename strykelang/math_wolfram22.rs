// Batch 22 — biology / ecology / population dynamics / epidemiology.

// Lotka-Volterra predator-prey step (returns next state [x, y])

// Logistic growth dN/dt = rN(1 - N/K)
fn builtin_logistic_growth_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(n + dt * r * n * (1.0 - n / k)))
}

// Logistic growth analytic solution N(t)
fn builtin_logistic_growth_analytic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n0 = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 1.0 + (k / n0 - 1.0) * (-r * t).exp();
    Ok(PerlValue::float(k / denom))
}

// Gompertz growth dN/dt = r*N*ln(K/N)
fn builtin_gompertz_growth_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args).max(1e-12);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(n + dt * r * n * (k / n).ln()))
}

// Allee effect dN/dt = rN(1-N/K)(N/A - 1)
fn builtin_allee_growth_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let a = args.get(3).map(|v| v.to_number()).unwrap_or(10.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    if a == 0.0 { return Ok(PerlValue::float(n)); }
    Ok(PerlValue::float(n + dt * r * n * (1.0 - n / k) * (n / a - 1.0)))
}

// Exponential growth N(t) = N0 e^(rt)

// Doubling time T = ln(2)/r

// Population doubling rate from N0/N1/Δt
fn builtin_growth_rate_from_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n0 = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(n0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n0 <= 0.0 || dt == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((n1 / n0).ln() / dt))
}

// SIR model step (returns [S, I, R])

// SEIR model step
fn builtin_seir_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.3);
    let sigma = args.get(5).map(|v| v.to_number()).unwrap_or(0.2);
    let gamma = args.get(6).map(|v| v.to_number()).unwrap_or(0.1);
    let dt = args.get(7).map(|v| v.to_number()).unwrap_or(0.01);
    let n = s + e + i + r;
    if n == 0.0 {
        return Ok(PerlValue::array(vec![
            PerlValue::float(s), PerlValue::float(e), PerlValue::float(i), PerlValue::float(r),
        ]));
    }
    let ds = -beta * s * i / n;
    let de = beta * s * i / n - sigma * e;
    let di = sigma * e - gamma * i;
    let dr = gamma * i;
    Ok(PerlValue::array(vec![
        PerlValue::float(s + dt * ds),
        PerlValue::float(e + dt * de),
        PerlValue::float(i + dt * di),
        PerlValue::float(r + dt * dr),
    ]))
}

// SEIRD step (with deaths)
fn builtin_seird_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(5).map(|v| v.to_number()).unwrap_or(0.3);
    let sigma = args.get(6).map(|v| v.to_number()).unwrap_or(0.2);
    let gamma = args.get(7).map(|v| v.to_number()).unwrap_or(0.1);
    let mu = args.get(8).map(|v| v.to_number()).unwrap_or(0.01);
    let dt = args.get(9).map(|v| v.to_number()).unwrap_or(0.01);
    let n = s + e + i + r + d;
    if n == 0.0 {
        return Ok(PerlValue::array(vec![
            PerlValue::float(s), PerlValue::float(e), PerlValue::float(i),
            PerlValue::float(r), PerlValue::float(d),
        ]));
    }
    let ds = -beta * s * i / n;
    let de = beta * s * i / n - sigma * e;
    let di = sigma * e - gamma * i - mu * i;
    let dr = gamma * i;
    let dd = mu * i;
    Ok(PerlValue::array(vec![
        PerlValue::float(s + dt * ds),
        PerlValue::float(e + dt * de),
        PerlValue::float(i + dt * di),
        PerlValue::float(r + dt * dr),
        PerlValue::float(d + dt * dd),
    ]))
}

// SIS step (no recovered, just S↔I)
fn builtin_sis_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.3);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let n = s + i;
    if n == 0.0 { return Ok(PerlValue::array(vec![PerlValue::float(s), PerlValue::float(i)])); }
    let ds = -beta * s * i / n + gamma * i;
    let di = beta * s * i / n - gamma * i;
    Ok(PerlValue::array(vec![PerlValue::float(s + dt * ds), PerlValue::float(i + dt * di)]))
}

// Basic reproduction number R0 = β/γ
fn builtin_r0_basic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let beta = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    if gamma == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(beta / gamma))
}

// Effective reproduction number Rt = R0 * S/N
fn builtin_rt_effective(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r0 = f1(args);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(r0 * s / n))
}

// Herd immunity threshold = 1 - 1/R0
fn builtin_herd_immunity_threshold(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r0 = f1(args);
    if r0 <= 1.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - 1.0 / r0))
}

// Generation time from serial interval (approx)
fn builtin_generation_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let serial = f1(args);
    let cv = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(serial * (1.0 - 0.5 * cv * cv)))
}

// Shannon diversity index H = -sum(p_i * ln p_i)
#[allow(dead_code)]
fn builtin_shannon_diversity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = counts.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let h: f64 = counts.iter().filter(|&&c| c > 0.0)
        .map(|&c| { let p = c / total; -p * p.ln() }).sum();
    Ok(PerlValue::float(h))
}

// Simpson diversity D = sum(p_i^2)
#[allow(dead_code)]
fn builtin_simpson_diversity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = counts.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let d: f64 = counts.iter().map(|&c| (c / total).powi(2)).sum();
    Ok(PerlValue::float(d))
}

// Inverse Simpson 1/D
fn builtin_inverse_simpson(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = counts.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let d: f64 = counts.iter().map(|&c| (c / total).powi(2)).sum();
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / d))
}

// Pielou evenness J = H / ln(S)
fn builtin_pielou_evenness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let nz: Vec<f64> = counts.iter().copied().filter(|&c| c > 0.0).collect();
    let s = nz.len() as f64;
    if s <= 1.0 { return Ok(PerlValue::float(0.0)); }
    let total: f64 = nz.iter().sum();
    let h: f64 = nz.iter().map(|&c| { let p = c / total; -p * p.ln() }).sum();
    Ok(PerlValue::float(h / s.ln()))
}

// Margalef richness D = (S - 1) / ln(N)
fn builtin_margalef_richness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let nz: usize = counts.iter().filter(|&&c| c > 0.0).count();
    let total: f64 = counts.iter().sum();
    if total <= 1.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((nz as f64 - 1.0) / total.ln()))
}

// Menhinick richness D = S / sqrt(N)
fn builtin_menhinick_richness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let nz: usize = counts.iter().filter(|&&c| c > 0.0).count();
    let total: f64 = counts.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(nz as f64 / total.sqrt()))
}

// Berger-Parker dominance = max(p_i)
fn builtin_berger_parker(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = counts.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let max_c = counts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Ok(PerlValue::float(max_c / total))
}

// Jaccard similarity = |A∩B| / |A∪B|

// Sorensen-Dice similarity 2|A∩B| / (|A|+|B|)
fn builtin_sorensen_dice(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: std::collections::HashSet<String> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_string()).collect();
    let b: std::collections::HashSet<String> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_string()).collect();
    let inter = a.intersection(&b).count();
    let total = a.len() + b.len();
    if total == 0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * inter as f64 / total as f64))
}

// Bray-Curtis dissimilarity
#[allow(dead_code)]
fn builtin_bray_curtis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..a.len().min(b.len()) {
        num += (a[i] - b[i]).abs();
        den += a[i] + b[i];
    }
    if den == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// Rao's quadratic entropy
fn builtin_rao_quadratic_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = p.iter().sum();
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    let normalized: Vec<f64> = p.iter().map(|&x| x / total).collect();
    let n = normalized.len();
    let mut q = 0.0;
    for i in 0..n {
        for j in 0..n {
            let d = ((i as f64) - (j as f64)).abs();
            q += d * normalized[i] * normalized[j];
        }
    }
    Ok(PerlValue::float(q))
}

// Hardy-Weinberg expected genotype freq (p², 2pq, q²)

// Selection coefficient → next allele freq
fn builtin_selection_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let q = 1.0 - p;
    let w_avg = p * p + 2.0 * p * q + q * q * (1.0 - s);
    if w_avg == 0.0 { return Ok(PerlValue::float(p)); }
    Ok(PerlValue::float((p * p + p * q) / w_avg))
}

// Fst from population freqs

// Nei's genetic distance D = -ln(I)
fn builtin_nei_genetic_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut num = 0.0;
    let mut den1 = 0.0;
    let mut den2 = 0.0;
    for i in 0..p1.len().min(p2.len()) {
        num += p1[i] * p2[i];
        den1 += p1[i] * p1[i];
        den2 += p2[i] * p2[i];
    }
    let denom = (den1 * den2).sqrt();
    if denom == 0.0 || num <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-(num / denom).ln()))
}

// Wright's effective population size: Ne_harmonic = N / (sum 1/N_i)
fn builtin_effective_pop_size(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if ns.is_empty() { return Ok(PerlValue::float(0.0)); }
    let inv_sum: f64 = ns.iter().filter(|&&n| n > 0.0).map(|&n| 1.0 / n).sum();
    if inv_sum == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(ns.len() as f64 / inv_sum))
}

// Carrying capacity from r and steady state
fn builtin_carrying_capacity_from_data(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if ns.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(ns.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

// Mark-recapture Petersen estimator: N = (M*C)/R
fn builtin_petersen_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(m * c / r))
}

// Lincoln-Petersen with Chapman correction: N = ((M+1)(C+1)/(R+1)) - 1
fn builtin_chapman_estimator(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((m + 1.0) * (c + 1.0) / (r + 1.0) - 1.0))
}

// Lotka–Volterra competition model — two species
fn builtin_lv_competition_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let r2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let k1 = args.get(4).map(|v| v.to_number()).unwrap_or(100.0);
    let k2 = args.get(5).map(|v| v.to_number()).unwrap_or(100.0);
    let alpha12 = args.get(6).map(|v| v.to_number()).unwrap_or(0.5);
    let alpha21 = args.get(7).map(|v| v.to_number()).unwrap_or(0.5);
    let dt = args.get(8).map(|v| v.to_number()).unwrap_or(0.01);
    let dn1 = r1 * n1 * (1.0 - (n1 + alpha12 * n2) / k1);
    let dn2 = r2 * n2 * (1.0 - (n2 + alpha21 * n1) / k2);
    Ok(PerlValue::array(vec![PerlValue::float(n1 + dt * dn1), PerlValue::float(n2 + dt * dn2)]))
}

// Holling type II functional response: f(N) = a*N/(1+a*h*N)
fn builtin_holling_type2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let denom = 1.0 + a * h * n;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(a * n / denom))
}

// Holling type III: a*N²/(1+a*h*N²)
fn builtin_holling_type3(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let denom = 1.0 + a * h * n * n;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(a * n * n / denom))
}

// Holling Type I functional response f(N) = a·N: prey-density-proportional
// consumption by a predator. Linear up to satiation; defining formula.
fn builtin_holling_type1(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a * n))
}

// Leslie matrix step (population vector × Leslie matrix)
fn builtin_leslie_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_vec: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let leslie = matrix_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    if n_vec.is_empty() || leslie.is_empty() { return Ok(PerlValue::array(vec![])); }
    let rows = leslie.len();
    let cols = leslie[0].len();
    if cols != n_vec.len() { return Ok(PerlValue::array(vec![])); }
    let mut out = vec![0.0; rows];
    for i in 0..rows {
        for j in 0..cols {
            out[i] += leslie[i][j] * n_vec[j];
        }
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Net reproductive rate R0 = sum(l_x * m_x)
fn builtin_net_reproductive_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lx: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mx: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let r0: f64 = lx.iter().zip(mx.iter()).map(|(&l, &m)| l * m).sum();
    Ok(PerlValue::float(r0))
}

// Generation time T = sum(x * l_x * m_x) / R0
fn builtin_generation_time_demo(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lx: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mx: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let r0: f64 = lx.iter().zip(mx.iter()).map(|(&l, &m)| l * m).sum();
    if r0 == 0.0 { return Ok(PerlValue::float(0.0)); }
    let t: f64 = lx.iter().zip(mx.iter()).enumerate()
        .map(|(x, (&l, &m))| x as f64 * l * m).sum();
    Ok(PerlValue::float(t / r0))
}

// Per-capita finite rate λ from R0 and T (approx)
fn builtin_finite_rate_lambda(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r0 = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if t == 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(r0.powf(1.0 / t)))
}

// Body mass to metabolic rate (Kleiber's): B = B0 * M^(3/4)
fn builtin_kleibers_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let b0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    Ok(PerlValue::float(b0 * m.powf(0.75)))
}

// Bergmann's rule: within a clade, body mass scales with latitude as a
// surface-to-volume thermoregulation response. Empirical fit (Meiri & Dayan 2003,
// J Biogeogr) for endotherms: log10(M) ≈ log10(M₀) + k · |lat°|, k ≈ 0.005..0.01.
// Args: equator-baseline mass M₀, |latitude° |, slope k.
fn builtin_bergmann_adjust(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m0 = f1(args);
    let lat_deg = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).abs();
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(0.0067);
    Ok(PerlValue::float(m0 * 10f64.powf(k * lat_deg)))
}

// Q10 temperature coefficient: rate2 = rate1 * Q10^((T2-T1)/10)
fn builtin_q10(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rate1 = f1(args);
    let q10 = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let t1 = args.get(2).map(|v| v.to_number()).unwrap_or(20.0);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(30.0);
    Ok(PerlValue::float(rate1 * q10.powf((t2 - t1) / 10.0)))
}

// Species-area curve S = c*A^z
fn builtin_species_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.25);
    Ok(PerlValue::float(c * a.powf(z)))
}

// Intrinsic rate r = b - d
fn builtin_intrinsic_growth_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(b - d))
}

// MacArthur-Wilson immigration rate I(S) = I_max(1 - S/P)
fn builtin_macarthur_wilson_immigration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let i_max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if p == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(i_max * (1.0 - s / p)))
}

// MacArthur-Wilson extinction rate E(S) = E_max * S/P
fn builtin_macarthur_wilson_extinction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let e_max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if p == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(e_max * s / p))
}

// Equilibrium species count S* (where I=E)
fn builtin_island_equilibrium(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let i_max = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let e_max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(p * i_max / (i_max + e_max)))
}
