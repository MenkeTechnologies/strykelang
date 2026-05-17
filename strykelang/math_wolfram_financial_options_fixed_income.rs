// financial pricing models, options, fixed income, exotics.

fn norm_cdf_b20(x: f64) -> f64 {
    0.5 * (1.0 + libm::erf(x / std::f64::consts::SQRT_2))
}
fn norm_pdf_b20(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

// Black-Scholes call
fn builtin_bs_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(s * norm_cdf_b20(d1) - k * (-r * t).exp() * norm_cdf_b20(d2)))
}
// Black-Scholes put
fn builtin_bs_put(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(k * (-r * t).exp() * norm_cdf_b20(-d2) - s * norm_cdf_b20(-d1)))
}
// BS vega
// BS theta call
fn builtin_bs_theta_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(
        -s * norm_pdf_b20(d1) * sigma / (2.0 * t.sqrt()) - r * k * (-r * t).exp() * norm_cdf_b20(d2),
    ))
}
// BS rho call
fn builtin_bs_rho_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(k * t * (-r * t).exp() * norm_cdf_b20(d2)))
}

// Implied vol via bisection

// Bachelier (normal) call
fn builtin_bachelier_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(f);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(20.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let d = (f - k) / (sigma * t.sqrt());
    Ok(StrykeValue::float((-r * t).exp() * ((f - k) * norm_cdf_b20(d) + sigma * t.sqrt() * norm_pdf_b20(d))))
}

// Black-76 (futures)
fn builtin_black76_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(f);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(0.05);
    let d1 = ((f / k).ln() + 0.5 * sigma * sigma * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float((-r * t).exp() * (f * norm_cdf_b20(d1) - k * norm_cdf_b20(d2))))
}

// CRR binomial American call
fn builtin_crr_american_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let u = (sigma * dt.sqrt()).exp();
    let d = 1.0 / u;
    let p = ((r * dt).exp() - d) / (u - d);
    let disc = (-r * dt).exp();
    let mut prices: Vec<f64> = (0..=n).map(|j| (s * u.powi(j as i32) * d.powi((n - j) as i32) - k).max(0.0)).collect();
    for i in (0..n).rev() {
        let mut next = vec![0.0; i + 1];
        for j in 0..=i {
            let cont = disc * (p * prices[j + 1] + (1.0 - p) * prices[j]);
            let intrinsic = (s * u.powi(j as i32) * d.powi((i - j) as i32) - k).max(0.0);
            next[j] = cont.max(intrinsic);
        }
        prices = next;
    }
    Ok(StrykeValue::float(prices[0]))
}

// CRR binomial American put
fn builtin_crr_american_put(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let u = (sigma * dt.sqrt()).exp();
    let d = 1.0 / u;
    let p = ((r * dt).exp() - d) / (u - d);
    let disc = (-r * dt).exp();
    let mut prices: Vec<f64> = (0..=n).map(|j| (k - s * u.powi(j as i32) * d.powi((n - j) as i32)).max(0.0)).collect();
    for i in (0..n).rev() {
        let mut next = vec![0.0; i + 1];
        for j in 0..=i {
            let cont = disc * (p * prices[j + 1] + (1.0 - p) * prices[j]);
            let intrinsic = (k - s * u.powi(j as i32) * d.powi((i - j) as i32)).max(0.0);
            next[j] = cont.max(intrinsic);
        }
        prices = next;
    }
    Ok(StrykeValue::float(prices[0]))
}

// JR binomial European call (Jarrow-Rudd)
fn builtin_jr_european_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let dt = t / n as f64;
    let nu = r - 0.5 * sigma * sigma;
    let u = (nu * dt + sigma * dt.sqrt()).exp();
    let d = (nu * dt - sigma * dt.sqrt()).exp();
    let p = 0.5_f64;
    let disc = (-r * dt).exp();
    let mut prices: Vec<f64> = (0..=n).map(|j| (s * u.powi(j as i32) * d.powi((n - j) as i32) - k).max(0.0)).collect();
    for _ in 0..n {
        let new_len = prices.len() - 1;
        let mut next = vec![0.0; new_len];
        for j in 0..new_len {
            next[j] = disc * (p * prices[j + 1] + (1.0 - p) * prices[j]);
        }
        prices = next;
    }
    Ok(StrykeValue::float(prices[0]))
}

// Trinomial tree (Boyle) European call
fn builtin_trinomial_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(50).max(1);
    let dt = t / n as f64;
    let u = (sigma * (3.0 * dt).sqrt()).exp();
    let _d = 1.0 / u;
    let pu = (((r * dt / 2.0).exp() - (-sigma * (dt / 2.0).sqrt()).exp())
        / ((sigma * (dt / 2.0).sqrt()).exp() - (-sigma * (dt / 2.0).sqrt()).exp()))
        .powi(2);
    let pd = (((sigma * (dt / 2.0).sqrt()).exp() - (r * dt / 2.0).exp())
        / ((sigma * (dt / 2.0).sqrt()).exp() - (-sigma * (dt / 2.0).sqrt()).exp()))
        .powi(2);
    let pm = 1.0 - pu - pd;
    let disc = (-r * dt).exp();
    let size = 2 * n + 1;
    let mut prices: Vec<f64> = (0..size).map(|j| {
        let exp = j as i32 - n as i32;
        (s * u.powi(exp) - k).max(0.0)
    }).collect();
    for _ in 0..n {
        let new_size = prices.len() - 2;
        if new_size == 0 { break; }
        let mut next = vec![0.0; new_size];
        for j in 0..new_size {
            next[j] = disc * (pd * prices[j] + pm * prices[j + 1] + pu * prices[j + 2]);
        }
        prices = next;
    }
    Ok(StrykeValue::float(prices[0]))
}

// Heston (1993) European call price by inversion of the characteristic function:
//   C = S₀·P₁ − K·e^(−rT)·P₂,   Pⱼ = ½ + (1/π) ∫₀^∞ Re[e^(−iu·lnK) fⱼ(u)/(iu)] du
// where f₁, f₂ are the Heston characteristic functions. Uses 64-point composite
// Simpson on truncated integration domain [ε, 200]. Args: S, K, r, v₀, κ, θ, σ_v, ρ, T.
fn builtin_heston_price_simple(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let v0 = args.get(3).map(|v| v.to_number()).unwrap_or(0.04);
    let kappa = args.get(4).map(|v| v.to_number()).unwrap_or(2.0);
    let theta = args.get(5).map(|v| v.to_number()).unwrap_or(0.04);
    let sigma_v = args.get(6).map(|v| v.to_number()).unwrap_or(0.3);
    let rho = args.get(7).map(|v| v.to_number()).unwrap_or(-0.7);
    let t = args.get(8).map(|v| v.to_number()).unwrap_or(1.0);
    if s <= 0.0 || k <= 0.0 || t <= 0.0 || sigma_v <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    fn cmul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) { (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0) }
    fn cdiv(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
        let d = b.0 * b.0 + b.1 * b.1;
        ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
    }
    fn cexp(z: (f64, f64)) -> (f64, f64) { let e = z.0.exp(); (e * z.1.cos(), e * z.1.sin()) }
    fn cln(z: (f64, f64)) -> (f64, f64) {
        let r = (z.0 * z.0 + z.1 * z.1).sqrt();
        (r.ln(), z.1.atan2(z.0))
    }
    fn csqrt(z: (f64, f64)) -> (f64, f64) {
        let r = (z.0 * z.0 + z.1 * z.1).sqrt();
        let re = ((r + z.0) / 2.0).max(0.0).sqrt();
        let im_sign = if z.1 >= 0.0 { 1.0 } else { -1.0 };
        let im = im_sign * ((r - z.0) / 2.0).max(0.0).sqrt();
        (re, im)
    }
    let phi_j = |u: f64, j: u8| -> (f64, f64) {
        let i_u = (0.0_f64, u);
        let (b, uj) = if j == 1 { (kappa - rho * sigma_v, 0.5_f64) } else { (kappa, -0.5_f64) };
        let a = kappa * theta;
        let xa = ((rho * sigma_v) * u, 0.0);
        let inner1 = (xa.0 - b, xa.1);
        let d_arg = cmul(inner1, inner1);
        let two_iu = (0.0, 2.0 * u);
        let two_uju = (2.0 * uj * u, 0.0);
        let plus = (two_uju.0 + 0.0 - u * u, two_uju.1 - two_iu.1);
        let _ = plus;
        let term2 = (sigma_v * sigma_v * (u * u), -sigma_v * sigma_v * 2.0 * uj * u);
        let inside = (d_arg.0 + term2.0, d_arg.1 + term2.1);
        let d = csqrt(inside);
        let g = cdiv((inner1.0 - d.0, inner1.1 - d.1), (inner1.0 + d.0, inner1.1 + d.1));
        let dt = (d.0 * t, d.1 * t);
        let exp_dt = cexp(dt);
        let one_minus_g_exp = (1.0 - cmul(g, exp_dt).0, -cmul(g, exp_dt).1);
        let one_minus_g = (1.0 - g.0, -g.1);
        let log_term = cln(cdiv(one_minus_g_exp, one_minus_g));
        let bd_t = (inner1.0 - d.0, inner1.1 - d.1);
        let bd_t_t = (bd_t.0 * t, bd_t.1 * t);
        let cap_c = (
            r * u * 0.0 + a / (sigma_v * sigma_v) * (bd_t_t.0 - 2.0 * log_term.0),
            r * u * t + a / (sigma_v * sigma_v) * (bd_t_t.1 - 2.0 * log_term.1),
        );
        let one_minus_exp = (1.0 - exp_dt.0, -exp_dt.1);
        let cap_d = cmul(cdiv(bd_t, (sigma_v * sigma_v, 0.0)),
            cdiv(one_minus_exp, one_minus_g_exp));
        let log_s = (s.ln() * 1.0, 0.0);
        let i_u_lns = cmul(i_u, log_s);
        let exponent = (cap_c.0 + cap_d.0 * v0 + i_u_lns.0, cap_c.1 + cap_d.1 * v0 + i_u_lns.1);
        cexp(exponent)
    };
    let integrand = |u: f64, j: u8| {
        let phi = phi_j(u, j);
        let factor = cdiv(cexp((0.0, -u * k.ln())), (0.0, u));
        let prod = cmul(factor, phi);
        prod.0
    };
    let simpson = |j: u8| -> f64 {
        let n = 256_usize;
        let lo = 1e-4_f64;
        let hi = 200.0_f64;
        let h = (hi - lo) / n as f64;
        let mut s = integrand(lo, j) + integrand(hi, j);
        for i in 1..n {
            let u = lo + i as f64 * h;
            let w = if i % 2 == 0 { 2.0 } else { 4.0 };
            s += w * integrand(u, j);
        }
        s * h / 3.0
    };
    let p1 = 0.5 + simpson(1) / std::f64::consts::PI;
    let p2 = 0.5 + simpson(2) / std::f64::consts::PI;
    Ok(StrykeValue::float(s * p1 - k * (-r * t).exp() * p2))
}

// SABR implied vol (Hagan 2002) approximation
fn builtin_sabr_implied_vol(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(f);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let rho = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let nu = args.get(6).map(|v| v.to_number()).unwrap_or(0.4);
    if (f - k).abs() < 1e-12 {
        return Ok(StrykeValue::float(
            alpha / f.powf(1.0 - beta)
                * (1.0
                    + ((1.0 - beta).powi(2) / 24.0 * alpha.powi(2) / f.powf(2.0 - 2.0 * beta)
                        + 0.25 * rho * beta * nu * alpha / f.powf(1.0 - beta)
                        + (2.0 - 3.0 * rho * rho) / 24.0 * nu * nu)
                        * t),
        ));
    }
    let z = (nu / alpha) * (f * k).powf((1.0 - beta) / 2.0) * (f / k).ln();
    let x_z = ((1.0 - 2.0 * rho * z + z * z).sqrt() + z - rho).ln() - (1.0 - rho).ln();
    let pre = alpha / ((f * k).powf((1.0 - beta) / 2.0)
        * (1.0
            + (1.0 - beta).powi(2) / 24.0 * (f / k).ln().powi(2)
            + (1.0 - beta).powi(4) / 1920.0 * (f / k).ln().powi(4)));
    let v = pre * z / x_z;
    let term = 1.0
        + ((1.0 - beta).powi(2) / 24.0 * alpha.powi(2) / (f * k).powf(1.0 - beta)
            + 0.25 * rho * beta * nu * alpha / (f * k).powf((1.0 - beta) / 2.0)
            + (2.0 - 3.0 * rho * rho) / 24.0 * nu * nu)
            * t;
    Ok(StrykeValue::float(v * term))
}

// Merton jump-diffusion call
fn builtin_merton_jump_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(5).map(|v| v.to_number()).unwrap_or(0.5);
    let mu_j = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma_j = args.get(7).map(|v| v.to_number()).unwrap_or(0.1);
    let kappa = (mu_j + 0.5 * sigma_j * sigma_j).exp() - 1.0;
    let lambda_p = lambda * (1.0 + kappa);
    let mut sum = 0.0;
    let mut fact = 1_f64;
    for n in 0..50 {
        if n > 0 { fact *= n as f64; }
        let r_n = r - lambda * kappa + n as f64 * (mu_j + 0.5 * sigma_j * sigma_j) / t;
        let sigma_n = (sigma * sigma + n as f64 * sigma_j * sigma_j / t).sqrt();
        let d1 = ((s / k).ln() + (r_n + 0.5 * sigma_n * sigma_n) * t) / (sigma_n * t.sqrt());
        let d2 = d1 - sigma_n * t.sqrt();
        let bs = s * norm_cdf_b20(d1) - k * (-r_n * t).exp() * norm_cdf_b20(d2);
        let weight = (-lambda_p * t).exp() * (lambda_p * t).powi(n) / fact;
        sum += weight * bs;
    }
    Ok(StrykeValue::float(sum))
}

// Asian arithmetic Monte Carlo (simplified, deterministic seed via xorshift)
fn builtin_asian_call_mc(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let steps = args.get(5).map(|v| v.to_number() as usize).unwrap_or(50).max(1);
    let paths = args.get(6).map(|v| v.to_number() as usize).unwrap_or(1000).max(1);
    let dt = t / steps as f64;
    let mut state: u64 = 0xDEAD_BEEF_DEAD_BEEF;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let u = ((state >> 11) as f64) / ((1u64 << 53) as f64);
        u.clamp(1e-12, 1.0 - 1e-12)
    };
    let mut sum_payoff = 0.0;
    for _ in 0..paths {
        let mut s_now = s;
        let mut accum = 0.0;
        for _ in 0..steps {
            let u1 = next();
            let u2 = next();
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            s_now *= ((r - 0.5 * sigma * sigma) * dt + sigma * dt.sqrt() * z).exp();
            accum += s_now;
        }
        let avg = accum / steps as f64;
        sum_payoff += (avg - k).max(0.0);
    }
    Ok(StrykeValue::float((-r * t).exp() * sum_payoff / paths as f64))
}

// Barrier up-and-out call (closed form)
fn builtin_barrier_up_out_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(120.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    if s >= h { return Ok(StrykeValue::float(0.0)); }
    let lambda = (r + sigma * sigma / 2.0) / (sigma * sigma);
    let x1 = (s / k).ln() / (sigma * t.sqrt()) + lambda * sigma * t.sqrt();
    let x2 = (s / h).ln() / (sigma * t.sqrt()) + lambda * sigma * t.sqrt();
    let y1 = (h * h / (s * k)).ln() / (sigma * t.sqrt()) + lambda * sigma * t.sqrt();
    let y2 = (h / s).ln() / (sigma * t.sqrt()) + lambda * sigma * t.sqrt();
    let bs = {
        let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
        let d2 = d1 - sigma * t.sqrt();
        s * norm_cdf_b20(d1) - k * (-r * t).exp() * norm_cdf_b20(d2)
    };
    let term1 = s * (norm_cdf_b20(x1) - norm_cdf_b20(x2))
        - k * (-r * t).exp() * (norm_cdf_b20(x1 - sigma * t.sqrt()) - norm_cdf_b20(x2 - sigma * t.sqrt()));
    let term2 = s * (h / s).powf(2.0 * lambda) * (norm_cdf_b20(-y1) - norm_cdf_b20(-y2));
    let term3 = k * (-r * t).exp() * (h / s).powf(2.0 * lambda - 2.0)
        * (norm_cdf_b20(-y1 + sigma * t.sqrt()) - norm_cdf_b20(-y2 + sigma * t.sqrt()));
    let _ = bs;
    Ok(StrykeValue::float((term1 - term2 + term3).max(0.0)))
}

// Digital cash-or-nothing call
fn builtin_digital_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let d2 = ((s / k).ln() + (r - 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    Ok(StrykeValue::float(q * (-r * t).exp() * norm_cdf_b20(d2)))
}

// Lookback fixed-strike call (analytic Conze-Viswanathan, simplified)
fn builtin_lookback_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let a1 = ((s / m).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let a2 = a1 - sigma * t.sqrt();
    let term1 = s * norm_cdf_b20(a1);
    let term2 = m * (-r * t).exp() * norm_cdf_b20(a2);
    let term3 = sigma * sigma / (2.0 * r) * s
        * (-((s / m).powf(-2.0 * r / (sigma * sigma)) * norm_cdf_b20(a1 - 2.0 * r * t.sqrt() / sigma))
            + (r * t).exp() * norm_cdf_b20(a1));
    Ok(StrykeValue::float(term1 - term2 + term3))
}

// Bond Macaulay duration
fn builtin_macaulay_duration(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut wsum = 0.0;
    let mut psum = 0.0;
    for (i, c) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = c / (1.0 + y).powf(t);
        wsum += t * pv;
        psum += pv;
    }
    if psum == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(wsum / psum))
}

// Bond convexity
#[allow(dead_code)]
fn builtin_bond_convexity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut wsum = 0.0;
    let mut psum = 0.0;
    for (i, c) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = c / (1.0 + y).powf(t);
        wsum += t * (t + 1.0) * pv;
        psum += pv;
    }
    if psum == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(wsum / (psum * (1.0 + y).powi(2))))
}

// Forward rate from 2 spot rates
fn builtin_forward_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r1 = f1(args);
    let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(r1);
    let t1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    if t2 == t1 { return Ok(StrykeValue::float(r1)); }
    Ok(StrykeValue::float((r2 * t2 - r1 * t1) / (t2 - t1)))
}

// Discount factor continuous
fn builtin_discount_continuous(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((-r * t).exp()))
}

// YTM via Newton
fn builtin_ytm_newton(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let price = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let mut y = 0.05_f64;
    for _ in 0..100 {
        let mut p = 0.0;
        let mut dp = 0.0;
        for (i, c) in cfs.iter().enumerate() {
            let t = (i + 1) as f64;
            let denom = (1.0 + y).powf(t);
            p += c / denom;
            dp -= t * c / (denom * (1.0 + y));
        }
        let f = p - price;
        if dp.abs() < 1e-15 { break; }
        let y1 = y - f / dp;
        if (y1 - y).abs() < 1e-10 { return Ok(StrykeValue::float(y1)); }
        y = y1;
    }
    Ok(StrykeValue::float(y))
}

// Vasicek bond price
fn builtin_vasicek_bond(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    let kappa = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.02);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let b = (1.0 - (-kappa * t).exp()) / kappa;
    let a = ((b - t) * (kappa * kappa * theta - 0.5 * sigma * sigma) / (kappa * kappa)
        - sigma * sigma * b * b / (4.0 * kappa))
        .exp();
    Ok(StrykeValue::float(a * (-b * r0).exp()))
}

// CIR bond price
fn builtin_cir_bond(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r0 = f1(args);
    let kappa = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(0.02);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let h = (kappa * kappa + 2.0 * sigma * sigma).sqrt();
    let denom = 2.0 * h + (kappa + h) * ((h * t).exp() - 1.0);
    let b = 2.0 * ((h * t).exp() - 1.0) / denom;
    let a = (2.0 * h * (((kappa + h) * t / 2.0).exp()) / denom).powf(2.0 * kappa * theta / (sigma * sigma));
    Ok(StrykeValue::float(a * (-b * r0).exp()))
}

// Hull-White short rate drift
fn builtin_hull_white_drift(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = f1(args);
    let theta_t = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let kappa = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(theta_t - kappa * r))
}

// CDS upfront simple (constant hazard)
fn builtin_cds_upfront(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let spread = f1(args);
    let hazard = args.get(1).map(|v| v.to_number()).unwrap_or(0.02);
    let recovery = args.get(2).map(|v| v.to_number()).unwrap_or(0.4);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.03);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(5.0);
    let protection = (1.0 - recovery) * hazard / (hazard + r) * (1.0 - (-(hazard + r) * t).exp());
    let premium = spread * (1.0 - (-(hazard + r) * t).exp()) / (hazard + r);
    Ok(StrykeValue::float(protection - premium))
}

// Black-Karasinski drift
fn builtin_black_karasinski_drift(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let log_r = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(-3.0);
    let kappa = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(kappa * (theta - log_r)))
}

// Quanto adjustment
fn builtin_quanto_adjustment(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let spot = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma_s = args.get(2).map(|v| v.to_number()).unwrap_or(0.2);
    let sigma_x = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(spot * (-rho * sigma_s * sigma_x * t).exp()))
}

// Forward FX
fn builtin_fx_forward(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let r_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let r_f = args.get(2).map(|v| v.to_number()).unwrap_or(0.02);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(s * ((r_d - r_f) * t).exp()))
}

// Garman-Kohlhagen FX option
fn builtin_garman_kohlhagen_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(s);
    let r_d = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let r_f = args.get(3).map(|v| v.to_number()).unwrap_or(0.02);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let d1 = ((s / k).ln() + (r_d - r_f + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(s * (-r_f * t).exp() * norm_cdf_b20(d1) - k * (-r_d * t).exp() * norm_cdf_b20(d2)))
}

// Margrabe exchange option
fn builtin_margrabe(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s1 = f1(args);
    let s2 = args.get(1).map(|v| v.to_number()).unwrap_or(s1);
    let sigma1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.2);
    let sigma2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let rho = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let t = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let sigma = (sigma1 * sigma1 - 2.0 * rho * sigma1 * sigma2 + sigma2 * sigma2).sqrt();
    let d1 = ((s1 / s2).ln() + 0.5 * sigma * sigma * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    Ok(StrykeValue::float(s1 * norm_cdf_b20(d1) - s2 * norm_cdf_b20(d2)))
}

// Stulz two-asset min option
fn builtin_stulz_min_call(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s1 = f1(args);
    let s2 = args.get(1).map(|v| v.to_number()).unwrap_or(s1);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma1 = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let sigma2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.2);
    let rho = args.get(6).map(|v| v.to_number()).unwrap_or(0.5);
    let t = args.get(7).map(|v| v.to_number()).unwrap_or(1.0);
    let sigma_min = (sigma1 * sigma1 + sigma2 * sigma2 - 2.0 * rho * sigma1 * sigma2).sqrt();
    let d_min = ((s1.min(s2) / k).ln() + (r + 0.5 * sigma_min * sigma_min) * t) / (sigma_min * t.sqrt());
    Ok(StrykeValue::float(s1.min(s2) * norm_cdf_b20(d_min) - k * (-r * t).exp() * norm_cdf_b20(d_min - sigma_min * t.sqrt())))
}

// Sharpe annualized
fn builtin_sharpe_annualized(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mean_r = f1(args);
    let std_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let rf = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let periods = args.get(3).map(|v| v.to_number()).unwrap_or(252.0);
    if std_r == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((mean_r - rf) / std_r * periods.sqrt()))
}

// Treynor ratio

// Information ratio

// Jensen's alpha
fn builtin_jensen_alpha(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r_p = f1(args);
    let r_f = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r_m = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r_p - (r_f + beta * (r_m - r_f))))
}

// Modified Sharpe (skew/kurtosis adjusted)
fn builtin_modified_sharpe(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let sharpe = f1(args);
    let skew = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kurt = args.get(2).map(|v| v.to_number()).unwrap_or(3.0);
    Ok(StrykeValue::float(sharpe / (1.0 + sharpe / 6.0 * skew - sharpe.powi(2) / 24.0 * (kurt - 3.0)).max(1e-12)))
}
