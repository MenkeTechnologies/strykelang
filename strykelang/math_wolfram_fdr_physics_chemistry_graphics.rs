// ─────────────────────────────────────────────────────────────────────────────
// multiple-testing corrections, divergence/distance metrics, more
// physics (radiation, photon, gravitation), more astronomy/cosmology, more
// chemistry (Beer-Lambert, rate laws, colligative), mixed-strategy game
// theory, computer graphics (barycentric, Bresenham, bilinear), DSP (Hilbert,
// cepstrum, Butterworth/SG coefficients), image processing (Canny, bilateral),
// clustering helpers, more combinatorics, more number theory, network
// metrics, RSA / DH crypto helpers, quantum entanglement primitives, more 2-D
// geometry, time-series smoothing. Included after `math_wolfram_mcmc_hmm_survival_control.rs`.
// ─────────────────────────────────────────────────────────────────────────────

// ── 1. Multiple-testing corrections ──────────────────────────────────────────

/// `bonferroni_correction` — Bonferroni correction. Returns a float.
fn builtin_bonferroni_correction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_values: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = p_values.len() as f64;
    let adj: Vec<StrykeValue> = p_values
        .into_iter()
        .map(|p| StrykeValue::float((p * m).min(1.0)))
        .collect();
    Ok(StrykeValue::array(adj))
}

/// Benjamini-Hochberg (FDR) q-values.
fn builtin_benjamini_hochberg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_values: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = p_values.len();
    let mut indexed: Vec<(usize, f64)> = p_values.iter().enumerate().map(|(i, p)| (i, *p)).collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut adj = vec![0.0_f64; m];
    let mut prev = f64::INFINITY;
    for (rank_pos, &(orig_idx, p)) in indexed.iter().enumerate().rev() {
        let q = (p * m as f64 / (rank_pos as f64 + 1.0)).min(prev);
        prev = q;
        adj[orig_idx] = q.min(1.0);
    }
    Ok(StrykeValue::array(adj.into_iter().map(StrykeValue::float).collect()))
}

/// Tukey HSD critical-difference: q_α(k, df) · √(MSE / n) where the
/// studentized range q_α is approximated by a polynomial fit.  Returns the
/// critical difference; pairwise mean-differences exceeding it are significant
/// at level α.  Args: alpha (default 0.05), k (number of groups), df, MSE, n.
fn builtin_tukey_hsd(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(0.05);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(2);
    let df = args.get(2).map(|v| v.to_number()).unwrap_or(10.0).max(1.0);
    let mse = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    // Lund-Lund / Harter approximation (rough): q ≈ z(α/2) + 0.5 ln k.
    // For α=0.05, z = 1.96; k=3 → ≈ 2.50, matches table value at large df.
    use statrs::function::erf::erf;
    let z = (1.0 - alpha).clamp(1e-12, 1.0 - 1e-12);
    // Inverse Φ approximation (Beasley-Springer): use rough fit.
    fn invphi(p: f64) -> f64 {
        // Acklam's algorithm, abbreviated for stryke use.
        let a = [
            -3.969683028665376e+01,
            2.209460984245205e+02,
            -2.759285104469687e+02,
            1.383_577_518_672_69e2,
            -3.066479806614716e+01,
            2.506628277459239e+00,
        ];
        let b = [
            -5.447609879822406e+01,
            1.615858368580409e+02,
            -1.556989798598866e+02,
            6.680131188771972e+01,
            -1.328068155288572e+01,
        ];
        let c = [
            -7.784894002430293e-03,
            -3.223964580411365e-01,
            -2.400758277161838e+00,
            -2.549732539343734e+00,
            4.374664141464968e+00,
            2.938163982698783e+00,
        ];
        let d = [
            7.784695709041462e-03,
            3.224671290700398e-01,
            2.445134137142996e+00,
            3.754408661907416e+00,
        ];
        let p_low = 0.02425_f64;
        let p_high = 1.0 - p_low;
        if p < p_low {
            let q = (-2.0 * p.ln()).sqrt();
            return (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
                / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0);
        }
        if p < p_high {
            let q = p - 0.5;
            let r = q * q;
            return (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
                / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0);
        }
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    }
    let zv = invphi(1.0 - alpha / 2.0);
    let q = zv + 0.5 * (k as f64).ln() + 0.7 / df;
    let _ = (z, erf(0.0));
    Ok(StrykeValue::float(q * (mse / n).sqrt()))
}

// ── 2. Divergences / distances ───────────────────────────────────────────────

/// `hellinger_distance` — Hellinger distance. Returns a float.
fn builtin_hellinger_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut s = 0.0_f64;
    for (a, b) in p.iter().zip(q.iter()) {
        s += (a.sqrt() - b.sqrt()).powi(2);
    }
    Ok(StrykeValue::float((0.5 * s).sqrt()))
}

/// Wasserstein-1 distance between empirical 1-D distributions (sorted samples).
fn builtin_wasserstein_1d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    b.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let n = a.len().min(b.len()).max(1);
    let mut s = 0.0_f64;
    for i in 0..n {
        s += (a[i] - b[i]).abs();
    }
    Ok(StrykeValue::float(s / n as f64))
}

/// `chi_squared_divergence` — Chi squared divergence. Returns a float.
fn builtin_chi_squared_divergence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut s = 0.0_f64;
    for (a, b) in p.iter().zip(q.iter()) {
        if *b > 1e-15 {
            s += (a - b).powi(2) / b;
        }
    }
    Ok(StrykeValue::float(s))
}

// ── 3. More distributions ────────────────────────────────────────────────────

/// Beta-geometric PMF: number of failures before first success when p ~ Beta.
fn builtin_beta_geometric_pmf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    use statrs::function::beta::ln_beta;
    let log_pmf = ln_beta(alpha + 1.0, beta + k) - ln_beta(alpha, beta);
    Ok(StrykeValue::float(log_pmf.exp()))
}

/// Generalized-gamma PDF f(x; a, d, p) = (p / a^d) · x^{d-1} · exp(-(x/a)^p) / Γ(d/p).
fn builtin_generalized_gamma_pdf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 || a <= 0.0 || d <= 0.0 || p <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::gamma::gamma;
    let pre = p / a.powf(d) / gamma(d / p);
    Ok(StrykeValue::float(
        pre * x.powf(d - 1.0) * (-(x / a).powf(p)).exp(),
    ))
}

/// Zero-inflated Poisson PMF.
fn builtin_zip_pmf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let pi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    use statrs::function::gamma::ln_gamma;
    let pois = (-lambda + k as f64 * lambda.ln() - ln_gamma(k as f64 + 1.0)).exp();
    if k == 0 {
        return Ok(StrykeValue::float(pi + (1.0 - pi) * pois));
    }
    Ok(StrykeValue::float((1.0 - pi) * pois))
}

// ── 4. More physics ──────────────────────────────────────────────────────────

/// Stefan-Boltzmann luminosity L = 4π R² σ T⁴.
fn builtin_stefan_boltzmann_luminosity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = 5.670374419e-8_f64;
    Ok(StrykeValue::float(4.0 * std::f64::consts::PI * r * r * sigma * t.powi(4)))
}

/// Photon momentum p = h / λ.
fn builtin_photon_momentum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args).max(1e-30);
    let h = 6.626_070_15e-34_f64;
    Ok(StrykeValue::float(h / lambda))
}

/// Photon energy in eV from wavelength (m).
fn builtin_photon_energy_ev(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args).max(1e-30);
    let hc_ev_nm = 1239.841984_f64; // h c in eV·nm
    Ok(StrykeValue::float(hc_ev_nm / (lambda * 1e9)))
}

/// Larmor radiated power P = q² a² / (6π ε₀ c³).
fn builtin_dipole_radiation_power(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eps0 = 8.854_187_817e-12_f64;
    let c = 2.997_924_58e8_f64;
    Ok(StrykeValue::float(
        q * q * a * a / (6.0 * std::f64::consts::PI * eps0 * c.powi(3)),
    ))
}

/// Parallax to distance (parsecs from arc-seconds).
fn builtin_parallax_to_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args).max(1e-30);
    Ok(StrykeValue::float(1.0 / p))
}

/// Hawking temperature T = ℏ c³ / (8π G M k_B).
fn builtin_hawking_temperature(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args).max(1e-30);
    let hbar = 1.054_571_817e-34_f64;
    let c = 2.997_924_58e8_f64;
    let g = 6.674_30e-11_f64;
    let kb = 1.380_649e-23_f64;
    Ok(StrykeValue::float(
        hbar * c.powi(3) / (8.0 * std::f64::consts::PI * g * m * kb),
    ))
}

// ── 5. Astronomy ─────────────────────────────────────────────────────────────

/// Roche limit (rigid body): d = R · (2 ρ_M / ρ_m)^(1/3).
fn builtin_roche_limit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let rho_primary = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rho_satellite = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-30);
    Ok(StrykeValue::float(r * (2.0 * rho_primary / rho_satellite).powf(1.0 / 3.0)))
}

/// Apparent magnitude m = M + 5 log10(d_pc / 10).
fn builtin_apparent_magnitude(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let abs_m = f1(args);
    let d_pc = args.get(1).map(|v| v.to_number()).unwrap_or(10.0).max(1e-30);
    Ok(StrykeValue::float(abs_m + 5.0 * (d_pc / 10.0).log10()))
}

/// Distance modulus μ = 5 log10(d_pc) − 5.
fn builtin_distance_modulus(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_pc = f1(args).max(1e-30);
    Ok(StrykeValue::float(5.0 * d_pc.log10() - 5.0))
}

// ── 6. More chemistry ────────────────────────────────────────────────────────

/// Beer-Lambert law: A = ε l c.
fn builtin_beer_lambert(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let epsilon = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(epsilon * l * c))
}

/// nth-order rate-law concentration: 1/[A]^(n-1) − 1/[A₀]^(n-1) = (n−1) k t (n ≠ 1).
fn builtin_rate_law_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if (n - 1.0).abs() < 1e-12 {
        return Ok(StrykeValue::float(a0 * (-k * t).exp()));
    }
    let rhs = a0.powf(1.0 - n) + (n - 1.0) * k * t;
    if rhs <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(rhs.powf(1.0 / (1.0 - n))))
}

/// Freezing-point depression ΔT = K_f · m · i (van't Hoff factor).
fn builtin_freezing_point_depression(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kf = f1(args);
    let molality = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(kf * molality * i))
}

// ── 7. Game theory (mixed strategy) ──────────────────────────────────────────

/// Mixed Nash equilibrium of a 2×2 zero-sum or general game (closed form).
/// Args: P1 payoff, P2 payoff. Returns `[p, q]` (P1 plays row-0 with prob p,
/// P2 plays col-0 with prob q). NaN if no interior equilibrium.
fn builtin_mixed_nash_2x2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p1 = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let denom_p = (p2[0][0] - p2[0][1] - p2[1][0] + p2[1][1]).abs();
    let denom_q = (p1[0][0] - p1[0][1] - p1[1][0] + p1[1][1]).abs();
    if denom_p < 1e-12 || denom_q < 1e-12 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(f64::NAN),
            StrykeValue::float(f64::NAN),
        ]));
    }
    let p = (p2[1][1] - p2[0][1]) / (p2[0][0] - p2[0][1] - p2[1][0] + p2[1][1]);
    let q = (p1[1][1] - p1[1][0]) / (p1[0][0] - p1[0][1] - p1[1][0] + p1[1][1]);
    Ok(StrykeValue::array(vec![StrykeValue::float(p), StrykeValue::float(q)]))
}

/// Minimax 2×2 zero-sum value (security level for the row player).
fn builtin_minimax_2x2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() != 2 || m[0].len() != 2 {
        return Err(StrykeError::runtime("minimax_2x2: 2×2 matrix required", 0));
    }
    // row min then max
    let row_max = m
        .iter()
        .map(|row| row.iter().cloned().fold(f64::INFINITY, f64::min))
        .fold(f64::NEG_INFINITY, f64::max);
    Ok(StrykeValue::float(row_max))
}

// ── 8. Computer graphics ─────────────────────────────────────────────────────

/// 2-D barycentric coordinates of P with respect to triangle (A, B, C).
fn builtin_barycentric_coords_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let a = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let c = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let to_pair = |v: &[StrykeValue]| {
        (
            v.first().map(|x| x.to_number()).unwrap_or(0.0),
            v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )
    };
    let p = to_pair(&p);
    let a = to_pair(&a);
    let b = to_pair(&b);
    let c = to_pair(&c);
    let v0 = (b.0 - a.0, b.1 - a.1);
    let v1 = (c.0 - a.0, c.1 - a.1);
    let v2 = (p.0 - a.0, p.1 - a.1);
    let d00 = v0.0 * v0.0 + v0.1 * v0.1;
    let d01 = v0.0 * v1.0 + v0.1 * v1.1;
    let d11 = v1.0 * v1.0 + v1.1 * v1.1;
    let d20 = v2.0 * v0.0 + v2.1 * v0.1;
    let d21 = v2.0 * v1.0 + v2.1 * v1.1;
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-30 {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(f64::NAN),
            StrykeValue::float(f64::NAN),
            StrykeValue::float(f64::NAN),
        ]));
    }
    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    let u = 1.0 - v - w;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(u),
        StrykeValue::float(v),
        StrykeValue::float(w),
    ]))
}

/// Bresenham's line: integer pixel path from (x0, y0) to (x1, y1).
fn builtin_bresenham_line(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut x0 = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let mut y0 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let x1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let y1 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut out: Vec<StrykeValue> = Vec::new();
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        out.push(StrykeValue::array(vec![
            StrykeValue::integer(x0),
            StrykeValue::integer(y0),
        ]));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    Ok(StrykeValue::array(out))
}

/// Bilinear interpolation on a unit square: f(u, v) given corners f00, f10, f01, f11.
fn builtin_bilinear_interp_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f00 = f1(args);
    let f10 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f01 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f11 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let u = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let v = args.get(5).map(|v| v.to_number()).unwrap_or(0.5);
    let f0 = f00 * (1.0 - u) + f10 * u;
    let f1 = f01 * (1.0 - u) + f11 * u;
    Ok(StrykeValue::float(f0 * (1.0 - v) + f1 * v))
}

/// Point-in-polygon test (ray casting).  Returns 1 if inside, 0 otherwise.
fn builtin_point_in_polygon_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let poly = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let px = p.first().map(|x| x.to_number()).unwrap_or(0.0);
    let py = p.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let j = (i + n - 1) % n;
        let pi = arg_to_vec(&poly[i]);
        let pj = arg_to_vec(&poly[j]);
        let xi = pi.first().map(|x| x.to_number()).unwrap_or(0.0);
        let yi = pi.get(1).map(|x| x.to_number()).unwrap_or(0.0);
        let xj = pj.first().map(|x| x.to_number()).unwrap_or(0.0);
        let yj = pj.get(1).map(|x| x.to_number()).unwrap_or(0.0);
        let intersect = ((yi > py) != (yj > py))
            && (px < (xj - xi) * (py - yi) / (yj - yi) + xi);
        if intersect {
            inside = !inside;
        }
    }
    Ok(StrykeValue::integer(if inside { 1 } else { 0 }))
}

// ── 9. DSP ──────────────────────────────────────────────────────────────────

/// Discrete Hilbert transform of a finite signal via its DFT (returns the
/// imaginary-part of the analytic signal in the time domain).
fn builtin_hilbert_transform(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut re = vec![0.0_f64; n];
    let mut im = vec![0.0_f64; n];
    for k in 0..n {
        for (j, &x) in xs.iter().enumerate() {
            let theta = 2.0 * std::f64::consts::PI * k as f64 * j as f64 / n as f64;
            re[k] += x * theta.cos();
            im[k] -= x * theta.sin();
        }
    }
    // H(k) = -i sgn(k) X(k); zero DC and Nyquist.
    for k in 0..n {
        let (nr, ni);
        if k == 0 || (n.is_multiple_of(2) && k == n / 2) {
            nr = 0.0;
            ni = 0.0;
        } else if k < n / 2 + (n & 1) {
            // positive frequency: -i (a + ib) = b − i a
            nr = im[k];
            ni = -re[k];
        } else {
            // negative frequency: +i (a + ib) = -b + i a
            nr = -im[k];
            ni = re[k];
        }
        re[k] = nr;
        im[k] = ni;
    }
    // Inverse DFT, take real part.
    let mut out = vec![0.0_f64; n];
    for j in 0..n {
        for k in 0..n {
            let theta = 2.0 * std::f64::consts::PI * k as f64 * j as f64 / n as f64;
            out[j] += re[k] * theta.cos() - im[k] * theta.sin();
        }
        out[j] /= n as f64;
    }
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::float).collect()))
}

/// Real cepstrum: ifft(log|fft(x)|).
fn builtin_cepstrum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    let mut log_mag = vec![0.0_f64; n];
    for k in 0..n {
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        for (j, &x) in xs.iter().enumerate() {
            let theta = 2.0 * std::f64::consts::PI * k as f64 * j as f64 / n as f64;
            re += x * theta.cos();
            im -= x * theta.sin();
        }
        log_mag[k] = ((re * re + im * im).sqrt() + 1e-30).ln();
    }
    let mut cep = vec![0.0_f64; n];
    for j in 0..n {
        for k in 0..n {
            let theta = 2.0 * std::f64::consts::PI * k as f64 * j as f64 / n as f64;
            cep[j] += log_mag[k] * theta.cos();
        }
        cep[j] /= n as f64;
    }
    Ok(StrykeValue::array(cep.into_iter().map(StrykeValue::float).collect()))
}

/// Butterworth low-pass design (analog → bilinear transform). Returns
/// `[b_coeffs, a_coeffs]` of a digital biquad cascade flattened. Order ≤ 4.
fn builtin_butterworth_lowpass_coeffs(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let order = args.first().map(|v| v.to_number() as usize).unwrap_or(2).clamp(1, 4);
    let cutoff = args.get(1).map(|v| v.to_number()).unwrap_or(0.25);
    let warp = (std::f64::consts::PI * cutoff).tan();
    // Build numerator & denominator polynomials.
    let mut b = vec![1.0_f64];
    let mut a = vec![1.0_f64];
    for k in 0..order {
        let theta = std::f64::consts::PI * (2.0 * k as f64 + 1.0) / (2.0 * order as f64);
        let pole = (theta.cos(), theta.sin());
        let denom = 1.0 - 2.0 * pole.0 * warp + warp * warp;
        let b0 = warp * warp / denom;
        let b1 = 2.0 * b0;
        let b2 = b0;
        let a1 = 2.0 * (warp * warp - 1.0) / denom;
        let a2 = (1.0 + 2.0 * pole.0 * warp + warp * warp) / denom;
        b = poly_mul_real(&b, &[b0, b1, b2]);
        a = poly_mul_real(&a, &[1.0, a1, a2]);
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(b.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(a.into_iter().map(StrykeValue::float).collect()),
    ]))
}

fn poly_mul_real(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0_f64; a.len() + b.len() - 1];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += ai * bj;
        }
    }
    out
}

/// Savitzky-Golay coefficients (window size n_l + n_r + 1, polynomial order m).
fn builtin_savitzky_golay_coeffs(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nl = args.first().map(|v| v.to_number() as i64).unwrap_or(2);
    let nr = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    let m = args.get(2).map(|v| v.to_number() as usize).unwrap_or(2);
    let n = (nl + nr + 1) as usize;
    let mut a = vec![vec![0.0_f64; m + 1]; m + 1];
    for i in 0..=m {
        for j in 0..=m {
            let mut s = 0.0_f64;
            for k in -nl..=nr {
                s += (k as f64).powi((i + j) as i32);
            }
            a[i][j] = s;
        }
    }
    let inv = invert_mat(&a);
    let mut coeffs = vec![0.0_f64; n];
    for (idx, k) in (-nl..=nr).enumerate() {
        for j in 0..=m {
            coeffs[idx] += inv[0][j] * (k as f64).powi(j as i32);
        }
    }
    Ok(StrykeValue::array(coeffs.into_iter().map(StrykeValue::float).collect()))
}

/// Apply a Savitzky-Golay filter (zero-padded edges).
fn builtin_savitzky_golay_filter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let coeffs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let half = coeffs.len() as i64 / 2;
    let n = xs.len() as i64;
    let mut out = vec![0.0_f64; xs.len()];
    for i in 0..n {
        let mut s = 0.0_f64;
        for (k, &c) in coeffs.iter().enumerate() {
            let j = i + k as i64 - half;
            if j >= 0 && j < n {
                s += c * xs[j as usize];
            }
        }
        out[i as usize] = s;
    }
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::float).collect()))
}

// ── 10. Image processing ─────────────────────────────────────────────────────

/// Canny edge intensity map (gradient magnitude after Sobel + Gaussian blur);
/// callers can threshold the output.
fn builtin_canny_edge_intensity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let h = img.len();
    let w = if h == 0 { 0 } else { img[0].len() };
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 1..h.saturating_sub(1) {
        for j in 1..w.saturating_sub(1) {
            let gx = img[i - 1][j + 1] + 2.0 * img[i][j + 1] + img[i + 1][j + 1]
                - img[i - 1][j - 1]
                - 2.0 * img[i][j - 1]
                - img[i + 1][j - 1];
            let gy = img[i + 1][j - 1] + 2.0 * img[i + 1][j] + img[i + 1][j + 1]
                - img[i - 1][j - 1]
                - 2.0 * img[i - 1][j]
                - img[i - 1][j + 1];
            out[i][j] = (gx * gx + gy * gy).sqrt();
        }
    }
    Ok(matrix_to_value(&out))
}

/// Bilateral filter (small radius, sigma_s spatial, sigma_r intensity).
fn builtin_bilateral_filter_basic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let radius = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2).max(1);
    let sigma_s = args.get(2).map(|v| v.to_number()).unwrap_or(2.0).max(1e-3);
    let sigma_r = args.get(3).map(|v| v.to_number()).unwrap_or(0.1).max(1e-3);
    let h = img.len();
    let w = if h == 0 { 0 } else { img[0].len() };
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut s = 0.0_f64;
            let mut total = 0.0_f64;
            for di in -radius..=radius {
                for dj in -radius..=radius {
                    let ii = i as i64 + di;
                    let jj = j as i64 + dj;
                    if ii < 0 || ii >= h as i64 || jj < 0 || jj >= w as i64 {
                        continue;
                    }
                    let v = img[ii as usize][jj as usize];
                    let ws = (-((di * di + dj * dj) as f64) / (2.0 * sigma_s * sigma_s)).exp();
                    let wr = (-((v - img[i][j]).powi(2)) / (2.0 * sigma_r * sigma_r)).exp();
                    let we = ws * wr;
                    s += we * v;
                    total += we;
                }
            }
            out[i][j] = if total > 0.0 { s / total } else { img[i][j] };
        }
    }
    Ok(matrix_to_value(&out))
}

// ── 11. Clustering helpers ───────────────────────────────────────────────────

fn kmeans_pp_deterministic_seed(pts: &[Vec<f64>], k: usize) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    k.hash(&mut h);
    pts.len().hash(&mut h);
    for row in pts {
        row.len().hash(&mut h);
        for &x in row {
            x.to_bits().hash(&mut h);
        }
    }
    h.finish()
}

/// k-means++ initialisation: choose k seed centroids weighted by squared
/// distance to nearest already-chosen centroid.
fn builtin_kmeans_pp_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    use rand::rngs::StdRng;
    use rand::Rng;
    use rand::SeedableRng;
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let n = pts.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let mut rng = StdRng::seed_from_u64(kmeans_pp_deterministic_seed(&pts, k));
    let mut centers: Vec<Vec<f64>> = vec![pts[rng.gen_range(0..n)].clone()];
    while centers.len() < k {
        let mut min_sq = vec![f64::INFINITY; n];
        for (i, p) in pts.iter().enumerate() {
            for c in &centers {
                let d: f64 = p.iter().zip(c.iter()).map(|(a, b)| (a - b).powi(2)).sum();
                if d < min_sq[i] {
                    min_sq[i] = d;
                }
            }
        }
        let total: f64 = min_sq.iter().sum();
        let mut u: f64 = rng.gen::<f64>() * total;
        let mut chosen = 0_usize;
        for (i, &d) in min_sq.iter().enumerate() {
            u -= d;
            if u <= 0.0 {
                chosen = i;
                break;
            }
        }
        centers.push(pts[chosen].clone());
    }
    Ok(matrix_to_value(&centers))
}

/// Within-cluster sum of squares for the elbow plot.
fn builtin_elbow_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let mut clusters: std::collections::HashMap<i64, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, &l) in labels.iter().enumerate() {
        clusters.entry(l).or_default().push(i);
    }
    let mut wcss = 0.0_f64;
    for idxs in clusters.values() {
        if idxs.is_empty() {
            continue;
        }
        let dim = pts[idxs[0]].len();
        let mut centroid = vec![0.0_f64; dim];
        for &i in idxs {
            for d in 0..dim {
                centroid[d] += pts[i][d];
            }
        }
        for v in centroid.iter_mut() {
            *v /= idxs.len() as f64;
        }
        for &i in idxs {
            wcss += pts[i]
                .iter()
                .zip(centroid.iter())
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>();
        }
    }
    Ok(StrykeValue::float(wcss))
}

// ── 12. Combinatorics ────────────────────────────────────────────────────────

/// Hook-length formula for the number of standard Young tableaux of a given
/// partition λ = [λ₁ ≥ λ₂ ≥ …].
fn builtin_young_tableaux_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n: usize = lambda.iter().sum();
    if n == 0 {
        return Ok(StrykeValue::integer(1));
    }
    // Hook of cell (i, j) = λ_i − j + (count of rows below row i with λ_k ≥ j+1) + 1.
    let rows = lambda.len();
    let mut hook_product: f64 = 1.0;
    for i in 0..rows {
        for j in 0..lambda[i] {
            let arm = lambda[i] - j - 1;
            let leg = (i + 1..rows).filter(|&k| lambda[k] > j).count();
            let hook = arm + leg + 1;
            hook_product *= hook as f64;
        }
    }
    // n! / Π hooks.
    use statrs::function::gamma::ln_gamma;
    let result = ((1..=n).map(|k| (k as f64).ln()).sum::<f64>() - hook_product.ln()).exp().round() as i64;
    let _ = ln_gamma(0.0);
    Ok(StrykeValue::integer(result))
}

/// Euler (alternating-permutation) numbers E_n via boustrophedon transform.
fn builtin_euler_alt_permutation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let mut t = vec![0_i128; n + 1];
    t[0] = 1;
    for i in 1..=n {
        let mut row = vec![0_i128; i + 1];
        row[0] = 0;
        for k in 1..=i {
            row[k] = row[k - 1] + t[i - k];
        }
        for k in 0..=i {
            t[k] = row[i - k];
        }
    }
    Ok(StrykeValue::integer(t[0] as i64))
}

/// Genocchi number G_n via Seidel triangle.
fn builtin_genocchi_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    if n < 2 {
        return Ok(StrykeValue::integer(0));
    }
    // G_2k = 2 (1 - 2^{2k}) B_{2k}.
    if n & 1 == 1 {
        return Ok(StrykeValue::integer(0));
    }
    let bern = bernoulli_table(n + 1);
    let g = 2.0 * (1.0 - 2.0_f64.powi(n as i32)) * bern[n];
    Ok(StrykeValue::integer(g.round() as i64))
}

/// Lattice paths from (0,0) to (m,n) with → and ↑ moves: C(m+n, n).
fn builtin_lattice_paths_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = i1(args).max(0) as usize;
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(StrykeValue::integer(binomial_f(m + n, n).round() as i64))
}

// ── 13. Number theory extras ─────────────────────────────────────────────────

/// Tetration with overflow guard.
fn builtin_tetration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if n <= 0 {
        return Ok(StrykeValue::float(1.0));
    }
    let mut r = 1.0_f64;
    for _ in 0..n {
        r = a.powf(r);
        if r.is_infinite() || r.abs() > 1e308 {
            return Ok(StrykeValue::float(f64::INFINITY));
        }
    }
    Ok(StrykeValue::float(r))
}

/// Depth-limited Ackermann (m ≤ 3, n ≤ 24 stable; refuses past those).
fn builtin_ackermann_limited(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    fn ack(m: i64, n: i64, depth: i64) -> i64 {
        if depth > 1_000_000 {
            return -1;
        }
        if m == 0 {
            return n + 1;
        }
        if n == 0 {
            return ack(m - 1, 1, depth + 1);
        }
        let inner = ack(m, n - 1, depth + 1);
        if inner < 0 {
            return -1;
        }
        ack(m - 1, inner, depth + 1)
    }
    if m > 3 || (m == 3 && n > 14) {
        return Err(StrykeError::runtime("ackermann_limited: arguments too large", 0));
    }
    Ok(StrykeValue::integer(ack(m, n, 0)))
}

/// Test if N is a perfect power (N = a^b for integers a > 1, b > 1).
fn builtin_perfect_power_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 4 {
        return Ok(StrykeValue::integer(0));
    }
    for b in 2..((n as f64).log2() as i64 + 1) {
        let mut lo = 2_i64;
        let mut hi = n;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let p = (mid as i128).pow(b as u32);
            if p == n as i128 {
                return Ok(StrykeValue::integer(1));
            }
            if p < n as i128 {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
    }
    Ok(StrykeValue::integer(0))
}

/// B-smooth test: every prime factor of N is ≤ B.
fn builtin_b_smooth_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let factors = prime_factorize(n);
    Ok(StrykeValue::integer(
        if factors.iter().all(|f| *f <= b) { 1 } else { 0 },
    ))
}

// ── 14. Network metrics ──────────────────────────────────────────────────────

/// k-core decomposition: returns the coreness c(v) of each vertex (max k
/// such that v is in the k-core).
fn builtin_k_core(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut deg: Vec<usize> = adj.iter().map(|n| n.len()).collect();
    let mut alive = vec![true; n];
    let mut core = vec![0_i64; n];
    let mut k = 0_i64;
    while alive.iter().any(|&x| x) {
        let mut peeled_any = true;
        while peeled_any {
            peeled_any = false;
            for u in 0..n {
                if alive[u] && (deg[u] as i64) <= k {
                    alive[u] = false;
                    core[u] = k;
                    for &v in &adj[u] {
                        if v < n && alive[v] {
                            deg[v] = deg[v].saturating_sub(1);
                        }
                    }
                    peeled_any = true;
                }
            }
        }
        k += 1;
    }
    Ok(StrykeValue::array(core.into_iter().map(StrykeValue::integer).collect()))
}

/// Rich-club coefficient at degree k (fraction of edges among nodes with deg > k).
fn builtin_rich_club_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = adj.len();
    let degs: Vec<usize> = adj.iter().map(|n| n.len()).collect();
    let rich: Vec<usize> = (0..n).filter(|&i| degs[i] > k).collect();
    let r = rich.len();
    if r < 2 {
        return Ok(StrykeValue::float(0.0));
    }
    let max_edges = (r * (r - 1)) / 2;
    let rich_set: std::collections::HashSet<usize> = rich.into_iter().collect();
    let mut edges = 0_usize;
    for u in 0..n {
        if !rich_set.contains(&u) {
            continue;
        }
        for &v in &adj[u] {
            if v > u && rich_set.contains(&v) {
                edges += 1;
            }
        }
    }
    Ok(StrykeValue::float(edges as f64 / max_edges as f64))
}

// ── 15. Cryptography ─────────────────────────────────────────────────────────

/// Plain RSA encryption m^e mod n on small integers.
fn builtin_rsa_basic_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let e = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer(mod_pow_i64(m, e, n)))
}

/// Plain RSA decryption c^d mod n.
fn builtin_rsa_basic_decrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let d = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer(mod_pow_i64(c, d, n)))
}

/// Diffie-Hellman shared secret: g^(ab) mod p computed from one peer's public.
fn builtin_dh_shared_secret(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let peer_pub = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let private = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
    Ok(StrykeValue::integer(mod_pow_i64(peer_pub, private, p)))
}

// ── 16. Quantum entanglement primitives ──────────────────────────────────────

/// `bell_state_phi_plus` — Bell state phi plus. Returns a float.
fn builtin_bell_state_phi_plus(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = 1.0 / 2.0_f64.sqrt();
    Ok(StrykeValue::array(vec![
        StrykeValue::float(s),
        StrykeValue::float(0.0),
        StrykeValue::float(0.0),
        StrykeValue::float(s),
    ]))
}

/// `bell_state_psi_minus` — Bell state psi minus. Returns a float.
fn builtin_bell_state_psi_minus(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = 1.0 / 2.0_f64.sqrt();
    Ok(StrykeValue::array(vec![
        StrykeValue::float(0.0),
        StrykeValue::float(s),
        StrykeValue::float(-s),
        StrykeValue::float(0.0),
    ]))
}

/// Density-matrix purity tr(ρ²).
fn builtin_density_matrix_purity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = rho.len();
    let mut s = 0.0_f64;
    for i in 0..n {
        for j in 0..n {
            s += rho[i][j] * rho[j][i];
        }
    }
    Ok(StrykeValue::float(s))
}

/// Concurrence of a 2-qubit state in computational basis (real amplitudes only).
/// |Ψ⟩ = a|00⟩ + b|01⟩ + c|10⟩ + d|11⟩ → C = 2|ad − bc|.
fn builtin_concurrence_2qubit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if psi.len() != 4 {
        return Err(StrykeError::runtime("concurrence_2qubit: 4-vector required", 0));
    }
    Ok(StrykeValue::float(2.0 * (psi[0] * psi[3] - psi[1] * psi[2]).abs()))
}

// ── 17. 2-D geometry ────────────────────────────────────────────────────────

/// `point_in_circle` — Point in circle. Returns an integer.
fn builtin_point_in_circle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let center = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let dx = p.first().map(|x| x.to_number()).unwrap_or(0.0)
        - center.first().map(|x| x.to_number()).unwrap_or(0.0);
    let dy = p.get(1).map(|x| x.to_number()).unwrap_or(0.0)
        - center.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if dx * dx + dy * dy <= r * r { 1 } else { 0 }))
}

/// Circle-circle intersection points in 2-D.  Returns 0, 1, or 2 points.
fn builtin_circle_circle_intersect_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c1 = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let r1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c2 = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let r2 = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let x1 = c1.first().map(|x| x.to_number()).unwrap_or(0.0);
    let y1 = c1.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let x2 = c2.first().map(|x| x.to_number()).unwrap_or(0.0);
    let y2 = c2.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let d = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    if d > r1 + r2 || d < (r1 - r2).abs() {
        return Ok(StrykeValue::array(vec![]));
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let xm = x1 + a * (x2 - x1) / d;
    let ym = y1 + a * (y2 - y1) / d;
    let p1 = (xm + h * (y2 - y1) / d, ym - h * (x2 - x1) / d);
    let p2 = (xm - h * (y2 - y1) / d, ym + h * (x2 - x1) / d);
    if h.abs() < 1e-12 {
        return Ok(StrykeValue::array(vec![StrykeValue::array(vec![
            StrykeValue::float(p1.0),
            StrykeValue::float(p1.1),
        ])]));
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(vec![StrykeValue::float(p1.0), StrykeValue::float(p1.1)]),
        StrykeValue::array(vec![StrykeValue::float(p2.0), StrykeValue::float(p2.1)]),
    ]))
}

/// Polygon centroid by the shoelace + cross-product formula.
fn builtin_polygon_centroid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let poly = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = poly.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let pts: Vec<(f64, f64)> = poly
        .iter()
        .map(|p| {
            let v = arg_to_vec(p);
            (
                v.first().map(|x| x.to_number()).unwrap_or(0.0),
                v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            )
        })
        .collect();
    let mut a = 0.0_f64;
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        a += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    a /= 2.0;
    if a.abs() < 1e-30 {
        let xs: f64 = pts.iter().map(|p| p.0).sum::<f64>() / n as f64;
        let ys: f64 = pts.iter().map(|p| p.1).sum::<f64>() / n as f64;
        return Ok(StrykeValue::array(vec![StrykeValue::float(xs), StrykeValue::float(ys)]));
    }
    cx /= 6.0 * a;
    cy /= 6.0 * a;
    Ok(StrykeValue::array(vec![StrykeValue::float(cx), StrykeValue::float(cy)]))
}

/// Sutherland-Hodgman polygon clip against a convex clip polygon (CCW).
fn builtin_sutherland_hodgman_clip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let subject = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let clip = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let to_pair = |v: &StrykeValue| -> (f64, f64) {
        let xs = arg_to_vec(v);
        (
            xs.first().map(|x| x.to_number()).unwrap_or(0.0),
            xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )
    };
    let mut output: Vec<(f64, f64)> = subject.iter().map(to_pair).collect();
    let clip_pts: Vec<(f64, f64)> = clip.iter().map(to_pair).collect();
    let cn = clip_pts.len();
    for i in 0..cn {
        let a = clip_pts[i];
        let b = clip_pts[(i + 1) % cn];
        let inside = |p: (f64, f64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0;
        let intersect = |p: (f64, f64), q: (f64, f64)| {
            let denom = (a.0 - b.0) * (p.1 - q.1) - (a.1 - b.1) * (p.0 - q.0);
            if denom.abs() < 1e-15 {
                return p;
            }
            let t = ((a.0 - p.0) * (p.1 - q.1) - (a.1 - p.1) * (p.0 - q.0)) / denom;
            (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
        };
        if output.is_empty() {
            break;
        }
        let mut new_out = Vec::new();
        let m = output.len();
        for j in 0..m {
            let cur = output[j];
            let prev = output[(j + m - 1) % m];
            let cur_in = inside(cur);
            let prev_in = inside(prev);
            if cur_in {
                if !prev_in {
                    new_out.push(intersect(prev, cur));
                }
                new_out.push(cur);
            } else if prev_in {
                new_out.push(intersect(prev, cur));
            }
        }
        output = new_out;
    }
    Ok(StrykeValue::array(
        output
            .into_iter()
            .map(|(x, y)| StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
            .collect(),
    ))
}

// ── 18. Time-series smoothing ────────────────────────────────────────────────

/// Rauch-Tung-Striebel Kalman smoother (single backward pass).  Args:
///   X_HAT (filtered means as matrix, T × n),
///   P (filtered covariances as array of T n×n matrices),
///   X_PRED, P_PRED (one-step predictions),
///   F (transition matrix, n×n).
/// Returns smoothed means (T × n).
fn builtin_kalman_rts_smoother(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_hat: Vec<Vec<f64>> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|row| arg_to_vec(row).iter().map(|v| v.to_number()).collect())
        .collect();
    let p_arr: Vec<Vec<Vec<f64>>> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(matrix_from_value)
        .collect();
    let x_pred: Vec<Vec<f64>> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|row| arg_to_vec(row).iter().map(|v| v.to_number()).collect())
        .collect();
    let p_pred_arr: Vec<Vec<Vec<f64>>> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(matrix_from_value)
        .collect();
    let f_mat = matrix_from_value(&args.get(4).cloned().unwrap_or(StrykeValue::UNDEF));
    let t = x_hat.len();
    if t == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let n = x_hat[0].len();
    let mut xs = x_hat.clone();
    for k in (0..t - 1).rev() {
        // C_k = P_k F^T P_{k+1|k}^{-1}
        let mut p_ft = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                for kk in 0..n {
                    p_ft[i][j] += p_arr[k][i][kk] * f_mat[j][kk];
                }
            }
        }
        let p_pred_inv = invert_mat(&p_pred_arr[k]);
        let mut c = vec![vec![0.0_f64; n]; n];
        for i in 0..n {
            for j in 0..n {
                for kk in 0..n {
                    c[i][j] += p_ft[i][kk] * p_pred_inv[kk][j];
                }
            }
        }
        let mut diff = vec![0.0_f64; n];
        for i in 0..n {
            diff[i] = xs[k + 1][i] - x_pred[k][i];
        }
        for i in 0..n {
            let mut s = 0.0_f64;
            for j in 0..n {
                s += c[i][j] * diff[j];
            }
            xs[k][i] = x_hat[k][i] + s;
        }
    }
    Ok(matrix_to_value(&xs))
}
